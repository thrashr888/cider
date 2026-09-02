//! Client for the Cider Bridge helper app (`docs/RFC-swift-bridge.md`).
//!
//! HomeKit has no file, no script interface, and no cloud API; the only door
//! is `HomeKit.framework`, which loads only in a signed Mac Catalyst app. The
//! bridge is that app, built locally by the user with `cider bridge build`.
//! cider talks to it over a Unix socket with one JSON object per line each
//! way:
//!
//! ```text
//! → {"id": 1, "cmd": "home.scenes", "args": {"home": "Casa"}}
//! ← {"id": 1, "ok": true, "data": [...]}
//! ← {"id": 1, "ok": false, "error": {"code": "not_found", "message": "..."}}
//! ```
//!
//! Nothing here runs unless asked. [`Bridge::connect`] launches the app on
//! demand (it quits itself after ten idle minutes); [`ping`] and
//! [`Bridge::connect_running`] never launch anything, which is what `doctor`
//! and the cache-or-bridge choice in `cider home` rely on.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value as Json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::time::Instant;

use super::util::{run_command_with_timeout, ActionResult};

/// The app bundle's file name, wherever it is installed.
pub const APP_NAME: &str = "Cider Bridge.app";
/// Environment variable naming an app bundle outside the standard folders.
pub const APP_ENV: &str = "CIDER_BRIDGE_APP";
/// How long a running bridge gets to answer `ping` before cider decides it
/// is not there. Short on purpose: every cache-backed `cider home` call pays
/// it when the app is installed but idle.
pub const PING_TIMEOUT: Duration = Duration::from_millis(200);
/// How long a freshly launched app gets to open its socket.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Per-call ceiling once connected; a HomeKit round trip to a sleepy hub can
/// take seconds, an automation write a little longer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const LAUNCH_POLL: Duration = Duration::from_millis(250);
const LAUNCH_PING_TIMEOUT: Duration = Duration::from_secs(1);
const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_BUILD_DEPTH: usize = 8;

/// Why a bridge call did not produce data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// No `Cider Bridge.app` in `~/Applications`, `/Applications`, or `$CIDER_BRIDGE_APP`.
    NotInstalled,
    /// The socket did not connect, did not answer in time, or closed on us.
    Unreachable(String),
    /// The bridge answered with an error envelope; `code` is one of the RFC's
    /// codes (`not_found`, `invalid_args`, `homekit_denied`,
    /// `homekit_unavailable`, `timeout`, `internal`).
    Remote { code: String, message: String },
    /// The bridge answered with something that is not the protocol.
    Protocol(String),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NotInstalled => write!(
                f,
                "{APP_NAME} is not installed (looked in ~/Applications, /Applications, and \
                 ${APP_ENV}); build it from a cider checkout with `cider bridge build --install` \
                 — see docs/RFC-swift-bridge.md"
            ),
            BridgeError::Unreachable(detail) => write!(f, "Cider Bridge is unreachable: {detail}"),
            BridgeError::Remote { code, message } => write!(f, "Cider Bridge {code}: {message}"),
            BridgeError::Protocol(detail) => {
                write!(f, "Cider Bridge protocol error: {detail}")
            }
        }
    }
}

impl std::error::Error for BridgeError {}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// `$HOME/Library/Application Support/cider/bridge.sock`.
pub fn socket_path() -> PathBuf {
    home_dir().join("Library/Application Support/cider/bridge.sock")
}

/// The app bundle, searching `~/Applications`, `/Applications`, then
/// `$CIDER_BRIDGE_APP`.
pub fn app_path() -> Option<PathBuf> {
    app_path_from(app_candidates(
        &home_dir(),
        Path::new("/Applications"),
        std::env::var_os(APP_ENV).map(PathBuf::from),
    ))
}

/// The search list behind [`app_path`], with its roots injectable so the
/// order is testable without touching the real home folder.
pub fn app_candidates(
    home: &Path,
    system_apps: &Path,
    env_override: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = vec![
        home.join("Applications").join(APP_NAME),
        system_apps.join(APP_NAME),
    ];
    candidates.extend(env_override);
    candidates
}

/// The first candidate that is an app bundle (a directory) on disk.
pub fn app_path_from(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_dir())
}

pub fn is_installed() -> bool {
    app_path().is_some()
}

/// `ping` a running bridge without launching one. `None` when nothing
/// answers within [`PING_TIMEOUT`].
pub async fn ping() -> Option<Json> {
    Bridge::open(&socket_path(), PING_TIMEOUT)
        .await
        .ok()
        .map(|(_, pong)| pong)
}

/// One connection to the bridge. Requests carry increasing ids and replies
/// are matched to them; the protocol is strictly one reply per request.
pub struct Bridge {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
}

impl fmt::Debug for Bridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bridge")
            .field("next_id", &self.next_id)
            .finish()
    }
}

impl Bridge {
    /// Connect, launching the app if it is installed but not running.
    ///
    /// Tries the socket first (200 ms); if nothing answers and the app is
    /// installed, runs `open -gj -a <app>` and polls for up to ten seconds.
    /// Without an app this is [`BridgeError::NotInstalled`].
    pub async fn connect() -> Result<Bridge, BridgeError> {
        connect_with(&socket_path(), app_path()).await
    }

    /// Connect only if the bridge is already running; never launches it.
    pub async fn connect_running() -> Result<Bridge, BridgeError> {
        Self::open(&socket_path(), PING_TIMEOUT)
            .await
            .map(|(bridge, _)| bridge)
    }

    /// Connect to the socket at `path` and require a `ping` answer within
    /// `timeout`. Returns the connection and the ping data.
    pub async fn open(path: &Path, timeout: Duration) -> Result<(Bridge, Json), BridgeError> {
        let stream = match tokio::time::timeout(timeout, UnixStream::connect(path)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                return Err(BridgeError::Unreachable(format!(
                    "{}: {error}",
                    path.display()
                )))
            }
            Err(_) => {
                return Err(BridgeError::Unreachable(format!(
                    "{}: connect timed out after {timeout:?}",
                    path.display()
                )))
            }
        };
        let (reader, writer) = stream.into_split();
        let mut bridge = Bridge {
            reader: BufReader::new(reader),
            writer,
            next_id: 0,
        };
        let pong = bridge.call_with_timeout("ping", json!({}), timeout).await?;
        Ok((bridge, pong))
    }

    /// Send one command and return its `data`, or the mapped error.
    pub async fn call(&mut self, cmd: &str, args: Json) -> Result<Json, BridgeError> {
        self.call_with_timeout(cmd, args, CALL_TIMEOUT).await
    }

    pub async fn call_with_timeout(
        &mut self,
        cmd: &str,
        args: Json,
        timeout: Duration,
    ) -> Result<Json, BridgeError> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({"id": id, "cmd": cmd, "args": args});
        let mut line = serde_json::to_string(&request)
            .map_err(|error| BridgeError::Protocol(format!("could not encode {cmd}: {error}")))?;
        line.push('\n');
        tokio::time::timeout(timeout, self.exchange(id, cmd, &line))
            .await
            .map_err(|_| {
                BridgeError::Unreachable(format!("no reply to {cmd} within {timeout:?}"))
            })?
    }

    async fn exchange(&mut self, id: u64, cmd: &str, line: &str) -> Result<Json, BridgeError> {
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|error| BridgeError::Unreachable(format!("write of {cmd} failed: {error}")))?;
        let mut reply = String::new();
        let read = self.reader.read_line(&mut reply).await.map_err(|error| {
            BridgeError::Unreachable(format!("read after {cmd} failed: {error}"))
        })?;
        if read == 0 {
            return Err(BridgeError::Unreachable(format!(
                "connection closed before {cmd} was answered"
            )));
        }
        parse_reply(id, &reply)
    }
}

/// Decode one reply line for request `id` into its data or typed error.
pub fn parse_reply(id: u64, line: &str) -> Result<Json, BridgeError> {
    let reply: Json = serde_json::from_str(line.trim())
        .map_err(|error| BridgeError::Protocol(format!("unparseable reply ({error}): {line:?}")))?;
    let reply_id = reply.get("id").and_then(Json::as_u64);
    if reply_id != Some(id) {
        return Err(BridgeError::Protocol(format!(
            "reply id {reply_id:?} does not match request {id}"
        )));
    }
    match reply.get("ok").and_then(Json::as_bool) {
        Some(true) => Ok(reply.get("data").cloned().unwrap_or(Json::Null)),
        Some(false) => {
            let error = reply.get("error");
            let field = |key: &str| {
                error
                    .and_then(|e| e.get(key))
                    .and_then(Json::as_str)
                    .map(str::to_string)
            };
            Err(BridgeError::Remote {
                code: field("code").unwrap_or_else(|| "internal".to_string()),
                message: field("message").unwrap_or_else(|| {
                    "the bridge reported an error without a message".to_string()
                }),
            })
        }
        None => Err(BridgeError::Protocol(format!(
            "reply has no boolean ok field: {line:?}"
        ))),
    }
}

/// [`Bridge::connect`] with the socket and app injectable, so the
/// not-installed path is testable without ever running `open`.
pub async fn connect_with(socket: &Path, app: Option<PathBuf>) -> Result<Bridge, BridgeError> {
    if let Ok((bridge, _)) = Bridge::open(socket, PING_TIMEOUT).await {
        return Ok(bridge);
    }
    let app = app.ok_or(BridgeError::NotInstalled)?;
    launch(&app).await?;
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    let mut last_error = String::from("never answered");
    while Instant::now() < deadline {
        tokio::time::sleep(LAUNCH_POLL).await;
        match Bridge::open(socket, LAUNCH_PING_TIMEOUT).await {
            Ok((bridge, _)) => return Ok(bridge),
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(BridgeError::Unreachable(format!(
        "{} was launched but {} did not answer within {LAUNCH_TIMEOUT:?} ({last_error})",
        app.display(),
        socket.display()
    )))
}

/// `open -gj -a <app>`: launch in the background without activating it.
async fn launch(app: &Path) -> Result<(), BridgeError> {
    let path = app.to_string_lossy();
    run_command_with_timeout(
        "/usr/bin/open",
        &["-gj", "-a", &path],
        Duration::from_secs(15),
    )
    .await
    .map(|_| ())
    .map_err(|error| {
        BridgeError::Unreachable(format!("could not launch {}: {error}", app.display()))
    })
}

/// What `cider bridge status` prints. Never launches the app.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
    pub socket_path: String,
    /// Whether the socket answered `ping` within [`PING_TIMEOUT`].
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<Json>,
    /// `bridge/scripts/build.sh` from the checkout this binary was built in,
    /// when it exists; `cider bridge build` needs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_script: Option<String>,
}

pub async fn status() -> BridgeStatus {
    let app = app_path();
    let pong = ping().await;
    BridgeStatus {
        installed: app.is_some(),
        app_path: app.map(|p| p.to_string_lossy().into_owned()),
        socket_path: socket_path().to_string_lossy().into_owned(),
        running: pong.is_some(),
        ping: pong,
        build_script: build_script_path().map(|p| p.to_string_lossy().into_owned()),
    }
}

/// Ask a running bridge to exit. Not running is success: there is nothing
/// to quit, and this never launches the app just to stop it.
pub async fn quit() -> Result<ActionResult, BridgeError> {
    match Bridge::connect_running().await {
        Ok(mut bridge) => {
            bridge.call("quit", json!({})).await?;
            Ok(ActionResult::success_with_message(
                "bridge.quit",
                "asked Cider Bridge to quit",
            ))
        }
        Err(BridgeError::Unreachable(_)) => Ok(ActionResult::success_with_message(
            "bridge.quit",
            "Cider Bridge is not running",
        )),
        Err(error) => Err(error),
    }
}

/// `bridge/scripts/build.sh` from the checkout this binary was compiled in,
/// or from the current directory. The Swift sources are not part of the
/// crate, so a `cargo install`ed binary has neither.
pub fn build_script_path() -> Option<PathBuf> {
    let roots = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_dir().unwrap_or_default(),
    ];
    roots
        .iter()
        .map(|root| root.join("bridge/scripts/build.sh"))
        .find(|path| path.is_file())
}

/// Run the bridge build script with the user's team, streaming its output
/// to stderr so stdout stays JSON. `team` falls back to `$CIDER_TEAM_ID`
/// (the script also reads `bridge/.env.local`).
pub async fn build(team: Option<&str>, install_after: bool) -> anyhow::Result<ActionResult> {
    let script = build_script_path().ok_or_else(|| {
        anyhow::anyhow!(
            "bridge/scripts/build.sh not found under {} or the current directory; the bridge \
             is built from source, so clone https://github.com/thrashr888/cider and run \
             `cider bridge build` from that checkout (docs/RFC-swift-bridge.md)",
            env!("CARGO_MANIFEST_DIR")
        )
    })?;
    let bridge_dir = script
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let team = team
        .map(str::to_string)
        .or_else(|| std::env::var("CIDER_TEAM_ID").ok());

    let mut command = tokio::process::Command::new("/bin/bash");
    command
        .arg(&script)
        .current_dir(&bridge_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if let Some(team) = &team {
        command.env("CIDER_TEAM_ID", team);
    }
    let mut child = command.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("build script stdout was not captured"))?;
    let mut stderr = tokio::io::stderr();
    let status = tokio::time::timeout(BUILD_TIMEOUT, async {
        tokio::io::copy(&mut stdout, &mut stderr).await?;
        child.wait().await
    })
    .await
    .map_err(|_| anyhow::anyhow!("bridge build timed out after {BUILD_TIMEOUT:?}"))??;
    if !status.success() {
        anyhow::bail!("bridge build failed ({status}); see the output above");
    }
    if install_after {
        return install(None).await;
    }
    Ok(ActionResult::success_with_message(
        "bridge.build",
        &format!(
            "built with {}{}",
            script.display(),
            team.map(|t| format!(" (team {t})")).unwrap_or_default()
        ),
    ))
}

/// Copy a built app bundle into `~/Applications`, replacing any older copy.
/// Without `from`, uses the newest `Cider Bridge.app` under `bridge/.build`.
pub async fn install(from: Option<&Path>) -> anyhow::Result<ActionResult> {
    let source = match from {
        Some(path) => path.to_path_buf(),
        None => built_app_path().ok_or_else(|| {
            anyhow::anyhow!(
                "no built {APP_NAME} under bridge/.build; run `cider bridge build` first or pass \
                 --from <path-to-app>"
            )
        })?,
    };
    if !source.is_dir() {
        anyhow::bail!("{} is not an app bundle", source.display());
    }
    let destination_dir = home_dir().join("Applications");
    tokio::fs::create_dir_all(&destination_dir).await?;
    let destination = destination_dir.join(APP_NAME);
    if destination.exists() {
        tokio::fs::remove_dir_all(&destination).await?;
    }
    let (source_arg, destination_arg) = (source.to_string_lossy(), destination.to_string_lossy());
    run_command_with_timeout(
        "/usr/bin/ditto",
        &[&source_arg, &destination_arg],
        Duration::from_secs(60),
    )
    .await?;
    Ok(ActionResult::success_with_id(
        "bridge.install",
        &destination_arg,
    ))
}

/// The newest `Cider Bridge.app` under `bridge/.build`, wherever Xcode or
/// SwiftPM put it.
pub fn built_app_path() -> Option<PathBuf> {
    let build_dir = build_script_path()?.parent()?.parent()?.join(".build");
    newest_app_under(&build_dir)
}

fn newest_app_under(root: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    find_apps(root, 0, &mut found);
    found.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    });
    found.pop()
}

fn find_apps(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_BUILD_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name() == Some(OsStr::new(APP_NAME)) {
            out.push(path);
        } else {
            find_apps(&path, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::UnixListener;

    /// Unix socket paths are capped near 104 bytes on macOS, so these live in
    /// the system temp dir rather than a deep test folder.
    fn temp_socket() -> PathBuf {
        std::env::temp_dir().join(format!(
            "cider-bridge-{}.sock",
            uuid::Uuid::new_v4().simple()
        ))
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cider-bridge-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for Cider Bridge.app: answers `ping`, one `home.scenes`
    /// fixture, and an error envelope for anything else, recording every
    /// request it received so tests can assert the wire shape.
    async fn stub_server(path: &Path) -> (Arc<Mutex<Vec<Json>>>, tokio::task::JoinHandle<()>) {
        let listener = UnixListener::bind(path).unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let request: Json = serde_json::from_str(&line).unwrap();
                log.lock().unwrap().push(request.clone());
                let id = request["id"].clone();
                let reply = match request["cmd"].as_str() {
                    Some("ping") => json!({
                        "id": id, "ok": true,
                        "data": {"version": "0.0.0-stub", "homekit_authorized": true, "homes": 1}
                    }),
                    Some("home.scenes") => json!({
                        "id": id, "ok": true,
                        "data": [{"id": "SCENE-1", "name": "Good Night", "home": "Casa",
                                  "kind": "builtin", "actions": 2}]
                    }),
                    Some("stale") => json!({"id": 999, "ok": true, "data": null}),
                    Some(other) => json!({
                        "id": id, "ok": false,
                        "error": {"code": "not_found", "message": format!("no such command {other}")}
                    }),
                    None => json!({"id": id, "ok": false,
                                   "error": {"code": "invalid_args", "message": "cmd missing"}}),
                };
                writer
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        (received, server)
    }

    #[tokio::test]
    async fn call_round_trips_data_and_sends_id_cmd_args() {
        let path = temp_socket();
        let (received, server) = stub_server(&path).await;

        let (mut bridge, pong) = Bridge::open(&path, Duration::from_secs(2)).await.unwrap();
        assert_eq!(pong["homekit_authorized"], true);
        assert_eq!(pong["homes"], 1);

        let scenes = bridge
            .call("home.scenes", json!({"home": "Casa"}))
            .await
            .unwrap();
        assert_eq!(scenes[0]["name"], "Good Night");
        assert_eq!(scenes[0]["actions"], 2);

        let requests = received.lock().unwrap().clone();
        assert_eq!(
            requests,
            vec![
                json!({"id": 1, "cmd": "ping", "args": {}}),
                json!({"id": 2, "cmd": "home.scenes", "args": {"home": "Casa"}}),
            ]
        );

        drop(bridge);
        server.abort();
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn error_envelope_maps_to_remote_error_with_code() {
        let path = temp_socket();
        let (_received, server) = stub_server(&path).await;
        let (mut bridge, _) = Bridge::open(&path, Duration::from_secs(2)).await.unwrap();

        let error = bridge.call("home.nope", json!({})).await.unwrap_err();
        assert_eq!(
            error,
            BridgeError::Remote {
                code: "not_found".to_string(),
                message: "no such command home.nope".to_string(),
            }
        );
        assert!(error.to_string().contains("not_found"));

        // A reply for some other request is a protocol error, not data.
        let stale = bridge.call("stale", json!({})).await.unwrap_err();
        assert!(
            matches!(stale, BridgeError::Protocol(ref d) if d.contains("999")),
            "{stale:?}"
        );

        drop(bridge);
        server.abort();
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn silent_server_is_unreachable_after_the_ping_timeout() {
        let path = temp_socket();
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let error = Bridge::open(&path, Duration::from_millis(100))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            matches!(error, BridgeError::Unreachable(ref d) if d.contains("no reply to ping")),
            "{error:?}"
        );

        server.abort();
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn missing_socket_without_an_app_is_not_installed_and_never_launches() {
        let path = temp_socket();
        let error = connect_with(&path, None).await.map(|_| ()).unwrap_err();
        assert_eq!(error, BridgeError::NotInstalled);
        let message = error.to_string();
        assert!(message.contains("cider bridge build"), "{message}");
        assert!(message.contains("RFC-swift-bridge"), "{message}");

        let open = Bridge::open(&path, PING_TIMEOUT)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(open, BridgeError::Unreachable(_)), "{open:?}");
    }

    #[test]
    fn parse_reply_handles_every_envelope_shape() {
        assert_eq!(
            parse_reply(1, r#"{"id":1,"ok":true,"data":{"ran":true}}"#).unwrap(),
            json!({"ran": true})
        );
        assert_eq!(parse_reply(1, r#"{"id":1,"ok":true}"#).unwrap(), Json::Null);
        assert_eq!(
            parse_reply(
                7,
                r#"{"id":7,"ok":false,"error":{"code":"homekit_denied","message":"no"}}"#
            )
            .unwrap_err(),
            BridgeError::Remote {
                code: "homekit_denied".to_string(),
                message: "no".to_string()
            }
        );
        assert!(matches!(
            parse_reply(1, r#"{"id":1,"ok":false}"#).unwrap_err(),
            BridgeError::Remote { ref code, .. } if code == "internal"
        ));
        assert!(matches!(
            parse_reply(1, r#"{"id":2,"ok":true}"#).unwrap_err(),
            BridgeError::Protocol(_)
        ));
        assert!(matches!(
            parse_reply(1, r#"{"id":1}"#).unwrap_err(),
            BridgeError::Protocol(_)
        ));
        assert!(matches!(
            parse_reply(1, "not json").unwrap_err(),
            BridgeError::Protocol(_)
        ));
    }

    #[test]
    fn app_path_prefers_home_then_system_then_env_override() {
        let root = temp_dir("apps");
        let home = root.join("home");
        let system = root.join("system");
        let env_app = root.join("elsewhere").join(APP_NAME);
        let home_app = home.join("Applications").join(APP_NAME);
        let system_app = system.join(APP_NAME);
        for app in [&home_app, &system_app, &env_app] {
            std::fs::create_dir_all(app).unwrap();
        }
        let candidates = || app_candidates(&home, &system, Some(env_app.clone()));

        assert_eq!(
            candidates(),
            vec![home_app.clone(), system_app.clone(), env_app.clone()]
        );
        assert_eq!(app_path_from(candidates()), Some(home_app.clone()));
        std::fs::remove_dir_all(&home_app).unwrap();
        assert_eq!(app_path_from(candidates()), Some(system_app.clone()));
        std::fs::remove_dir_all(&system_app).unwrap();
        assert_eq!(app_path_from(candidates()), Some(env_app.clone()));
        std::fs::remove_dir_all(&env_app).unwrap();
        assert_eq!(app_path_from(candidates()), None);
        // No override: the list is just the two folders.
        assert_eq!(app_candidates(&home, &system, None).len(), 2);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn built_app_is_found_wherever_xcode_nested_it() {
        let root = temp_dir("build");
        let app = root.join("Build/Products/Debug-maccatalyst").join(APP_NAME);
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::create_dir_all(root.join("SourcePackages/checkouts")).unwrap();

        assert_eq!(newest_app_under(&root), Some(app));
        assert_eq!(newest_app_under(&root.join("missing")), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
