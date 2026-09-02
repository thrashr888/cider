//! Client for the native `cider-bridge` CLI (`docs/RFC-swift-bridge.md`).
//!
//! Where [`super::bridge`] talks to the Catalyst app over a socket (HomeKit,
//! WeatherKit), this module runs the plain Swift executable that wraps
//! EventKit and Contacts: `cider-bridge <cmd> [json-args]`, one envelope
//! line on stdout, exit 0/1. cider prefers it for Reminders and Calendar
//! writes when it is installed — EventKit commits in milliseconds where an
//! AppleScript `whose` scan takes seconds — and keeps the SQLite fast path
//! for bulk reads. `watch` is the exception to one-line-and-exit: it streams
//! one envelope per store change until killed.
//!
//! The executable is found via `$CIDER_BRIDGE_CLI`, then inside the
//! personal `~/Applications/Cider Bridge.app`, then next to the `cider`
//! binary (where Homebrew puts the packaged copy, `opt/cider/bin`), then
//! `~/.local/bin`, then `$PATH`. `CIDER_BRIDGE_CLI=off` disables it, which
//! forces every write back onto the AppleScript/JXA path.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value as Json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::bridge::{incompatible_if_unknown, parse_reply, ping_version, BridgeError, APP_NAME};

/// The executable's file name.
pub const CLI_NAME: &str = "cider-bridge";
/// Environment variable naming the executable, or `off` to disable it.
pub const CLI_ENV: &str = "CIDER_BRIDGE_CLI";
/// Per-call ceiling. A first call can wait on a TCC consent dialog, and an
/// EventKit commit against a slow CalDAV account is not instant either.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// `ping` never touches a store, so it is quick or it is broken.
pub const PING_TIMEOUT: Duration = Duration::from_secs(5);
/// The CLI answers every request with this id.
const CLI_REQUEST_ID: u64 = 0;

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Where the executable would be looked for, in order, given the inputs
/// that decide it. `None` means the CLI is switched off (`CIDER_BRIDGE_CLI=off`).
///
/// Pure, so the search order is testable without touching the real home
/// folder or the process environment.
pub fn cli_candidates(
    home: &Path,
    env_override: Option<&OsStr>,
    brew_bin: &[PathBuf],
    path_var: Option<&OsStr>,
) -> Option<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    if let Some(value) = env_override {
        if value.to_string_lossy().trim().eq_ignore_ascii_case("off") {
            return None;
        }
        if !value.is_empty() {
            candidates.push(PathBuf::from(value));
        }
    }
    candidates.push(
        home.join("Applications")
            .join(APP_NAME)
            .join("Contents/MacOS")
            .join(CLI_NAME),
    );
    candidates.extend(brew_bin.iter().map(|dir| dir.join(CLI_NAME)));
    candidates.push(home.join(".local/bin").join(CLI_NAME));
    if let Some(path_var) = path_var {
        candidates.extend(std::env::split_paths(path_var).map(|dir| dir.join(CLI_NAME)));
    }
    Some(candidates)
}

/// The first candidate that is a file on disk.
pub fn cli_path_from(candidates: Option<Vec<PathBuf>>) -> Option<PathBuf> {
    candidates?.into_iter().find(|path| path.is_file())
}

/// The executable, or `None` when it is not installed or switched off.
pub fn cli_path() -> Option<PathBuf> {
    let env_override = std::env::var_os(CLI_ENV);
    let path_var = std::env::var_os("PATH");
    cli_path_from(cli_candidates(
        &home_dir(),
        env_override.as_deref(),
        &super::bridge::brew_bin_dirs(std::env::current_exe().ok().as_deref()),
        path_var.as_deref(),
    ))
}

/// Whether `$CIDER_BRIDGE_CLI` is `off`.
pub fn is_disabled() -> bool {
    std::env::var_os(CLI_ENV)
        .is_some_and(|value| value.to_string_lossy().trim().eq_ignore_ascii_case("off"))
}

/// Installed and not switched off: the condition under which writes route
/// through the CLI.
pub fn is_installed() -> bool {
    cli_path().is_some()
}

/// Run one command and return its `data`, or the mapped error.
pub async fn call(cmd: &str, args: Json) -> Result<Json, BridgeError> {
    call_with_timeout(cmd, args, CALL_TIMEOUT).await
}

pub async fn call_with_timeout(
    cmd: &str,
    args: Json,
    timeout: Duration,
) -> Result<Json, BridgeError> {
    let cli = cli_path().ok_or(BridgeError::CliNotInstalled)?;
    call_at(&cli, cmd, args, timeout).await
}

/// [`call_with_timeout`] against an explicit executable, so tests can point
/// at a stub without touching the process environment.
pub async fn call_at(
    cli: &Path,
    cmd: &str,
    args: Json,
    timeout: Duration,
) -> Result<Json, BridgeError> {
    match call_raw(cli, cmd, args, timeout).await {
        // A CLI that predates `cmd` answers `unknown command`; name its
        // version in the error, which costs one more `ping` and only on
        // this path.
        Err(error) if cmd != "ping" && super::bridge::is_unknown_command(&error) => {
            let have = ping_at(cli).await.as_ref().map(ping_version);
            Err(incompatible_if_unknown(error, || {
                have.unwrap_or_else(|| "unknown".to_string())
            }))
        }
        other => other,
    }
}

/// One `cider-bridge <cmd> <json>` run, its envelope parsed and nothing more.
async fn call_raw(
    cli: &Path,
    cmd: &str,
    args: Json,
    timeout: Duration,
) -> Result<Json, BridgeError> {
    let encoded = serde_json::to_string(&args)
        .map_err(|error| BridgeError::Protocol(format!("could not encode {cmd} args: {error}")))?;
    let mut command = Command::new(cli);
    command
        .arg(cmd)
        .arg(&encoded)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().map_err(|error| {
        BridgeError::Unreachable(format!("could not run {}: {error}", cli.display()))
    })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            BridgeError::Unreachable(format!("{CLI_NAME} {cmd} timed out after {timeout:?}"))
        })?
        .map_err(|error| BridgeError::Unreachable(format!("{CLI_NAME} {cmd} failed: {error}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    match stdout.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(line) => parse_reply(CLI_REQUEST_ID, line),
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(BridgeError::Protocol(format!(
                "{CLI_NAME} {cmd} printed no envelope (exit {}){}",
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                }
            )))
        }
    }
}

/// `cider-bridge ping`: version and per-store authorization, without
/// touching any store or opening a dialog. `None` when it is not installed
/// or did not answer.
pub async fn ping() -> Option<Json> {
    ping_at(&cli_path()?).await
}

/// What `cider-bridge ping` says about TCC, verbatim, plus the fix for
/// each store that is not fully granted. Shown by `cider bridge status`
/// and `cider doctor` (`bridge_authorization`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoreAuthorization {
    /// EventKit's status for events: `full_access`, `write_only`,
    /// `denied`, `restricted`, `not_determined`.
    pub calendar: String,
    /// EventKit's status for reminders; same vocabulary.
    pub reminders: String,
    /// Contacts' status: `authorized`, `limited`, `denied`, `restricted`,
    /// `not_determined`.
    pub contacts: String,
    /// `launcher` when TCC attributes the CLI's requests to the app that
    /// launched cider (Terminal, an agent runner), `cider-bridge` when it
    /// disclaims that responsibility and is prompted for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcc_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// One line per store that needs attention, naming the System Settings
    /// pane and what to grant.
    pub fixes: Vec<String>,
}

impl StoreAuthorization {
    pub fn from_ping(pong: &Json) -> Self {
        let field = |key: &str| pong.get(key).and_then(Json::as_str).map(str::to_string);
        let calendar = field("calendar").unwrap_or_else(|| "unknown".into());
        let reminders = field("reminders").unwrap_or_else(|| "unknown".into());
        let contacts = field("contacts").unwrap_or_else(|| "unknown".into());
        let tcc_subject = field("tcc_subject");
        let grantee = match tcc_subject.as_deref() {
            Some("cider-bridge") => "cider-bridge".to_string(),
            _ => "the app that launches cider (Terminal, iTerm, or your agent runner)".to_string(),
        };
        let mut fixes = Vec::new();
        for (store, status, pane, want) in [
            ("calendar", &calendar, "Calendars", "Full Access"),
            ("reminders", &reminders, "Reminders", "Full Access"),
            ("contacts", &contacts, "Contacts", "access"),
        ] {
            let fix = match status.as_str() {
                "full_access" | "authorized" | "unknown" => continue,
                "not_determined" => format!(
                    "{store} is not_determined: the first `cider {store}` call through \
                     cider-bridge will prompt; grant {want} to {grantee}"
                ),
                other => format!(
                    "{store} is {other}: System Settings › Privacy & Security › {pane}, grant \
                     {want} to {grantee}"
                ),
            };
            fixes.push(fix);
        }
        Self {
            calendar,
            reminders,
            contacts,
            tcc_subject,
            executable: field("executable"),
            fixes,
        }
    }

    /// Every store fully granted.
    pub fn all_granted(&self) -> bool {
        self.fixes.is_empty()
    }

    /// Any store outright refused (`denied`, `restricted`, `add_only`,
    /// `write_only`, `limited`), as opposed to merely not asked yet.
    pub fn any_denied(&self) -> bool {
        [&self.calendar, &self.reminders, &self.contacts]
            .into_iter()
            .any(|s| {
                !matches!(
                    s.as_str(),
                    "full_access" | "authorized" | "not_determined" | "unknown"
                )
            })
    }
}

/// [`ping`] against an explicit executable.
pub async fn ping_at(cli: &Path) -> Option<Json> {
    call_raw(cli, "ping", Json::Object(Default::default()), PING_TIMEOUT)
        .await
        .ok()
}

/// Run `watch` for `sources` and hand each change's `data` object to
/// `on_line` until the CLI exits. An error envelope ends the stream as that
/// error; a line that is not an envelope is skipped with a note on stderr.
pub async fn stream_watch(sources: &[&str], on_line: impl FnMut(Json)) -> Result<(), BridgeError> {
    let cli = cli_path().ok_or(BridgeError::CliNotInstalled)?;
    stream_watch_at(&cli, sources, on_line).await
}

/// [`stream_watch`] against an explicit executable.
pub async fn stream_watch_at(
    cli: &Path,
    sources: &[&str],
    mut on_line: impl FnMut(Json),
) -> Result<(), BridgeError> {
    let args = serde_json::json!({ "sources": sources });
    let mut command = Command::new(cli);
    // The CLI streams until its stdin closes, so hold a pipe open for the
    // life of the stream; dropping this future closes it (and kills the
    // child), which is how the caller stops watching.
    command
        .arg("watch")
        .arg(args.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        BridgeError::Unreachable(format!("could not run {}: {error}", cli.display()))
    })?;
    let _stdin_kept_open = child.stdin.take();
    let stdout = child.stdout.take().ok_or_else(|| {
        BridgeError::Unreachable(format!("{CLI_NAME} watch stdout was not captured"))
    })?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await.map_err(|error| {
        BridgeError::Unreachable(format!("{CLI_NAME} watch read failed: {error}"))
    })? {
        if line.trim().is_empty() {
            continue;
        }
        match parse_reply(CLI_REQUEST_ID, &line) {
            Ok(data) => on_line(data),
            Err(BridgeError::Protocol(detail)) => log::warn!("cider watch: {CLI_NAME}: {detail}"),
            Err(error) => return Err(error),
        }
    }
    // Reaching here means the CLI ended on its own: a watch is meant to run
    // until the caller drops it, so even a clean exit is a failure worth
    // surfacing rather than a silent end of the stream.
    let status = child
        .wait()
        .await
        .map_err(|error| BridgeError::Unreachable(format!("{CLI_NAME} watch failed: {error}")))?;
    Err(BridgeError::Unreachable(format!(
        "{CLI_NAME} watch ended early ({status}); it should stream until cider stops it"
    )))
}

// ---------------------------------------------------------------------------
// Reminders and Calendar writes.
//
// Same verbs as the AppleScript/JXA paths in `reminders` and `calendar`, same
// `ActionResult` shape (`ok`, `action`, `id`, `message`) so callers need not
// care which path ran, plus the row EventKit handed back as `record`.
// ---------------------------------------------------------------------------

/// Which path a Reminders or Calendar write takes. Decided once per
/// command from [`is_installed`]; `--envelope` reports it as `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteBackend {
    /// `cider-bridge` (EventKit).
    Cli,
    /// AppleScript (Reminders) or JXA (Calendar).
    Native,
}

impl WriteBackend {
    pub fn detect() -> Self {
        if is_installed() {
            Self::Cli
        } else {
            Self::Native
        }
    }

    pub fn is_cli(self) -> bool {
        self == Self::Cli
    }

    /// The `source` value in an `--envelope`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Native => "native",
        }
    }

    /// How a dry-run message names the path.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "via cider-bridge",
            Self::Native => "via AppleScript/JXA",
        }
    }
}

/// An `ActionResult`-shaped object with the CLI's row attached as `record`.
pub fn action_with_record(
    action: &str,
    id: Option<&str>,
    message: Option<String>,
    record: Json,
) -> Json {
    let mut out = serde_json::Map::new();
    out.insert("ok".into(), Json::Bool(true));
    out.insert("action".into(), Json::String(action.to_string()));
    let id = id
        .map(str::to_string)
        .or_else(|| record.get("id").and_then(Json::as_str).map(str::to_string));
    if let Some(id) = id {
        out.insert("id".into(), Json::String(id));
    }
    if let Some(message) = message {
        out.insert("message".into(), Json::String(message));
    }
    if record.is_object() {
        out.insert("record".into(), record);
    }
    Json::Object(out)
}

/// The id a `Target` names: an id as given, a title resolved through the
/// SQLite read path (the CLI addresses reminders by id only).
pub async fn reminder_id(
    target: super::reminders::Target<'_>,
    list: Option<&str>,
) -> anyhow::Result<String> {
    match target {
        super::reminders::Target::Id(id) => Ok(id.trim().to_string()),
        super::reminders::Target::Title(_) => Ok(super::reminders::get(target, list).await?.id),
    }
}

fn optional_args(pairs: &[(&str, Option<Json>)]) -> serde_json::Map<String, Json> {
    pairs
        .iter()
        .filter_map(|(key, value)| value.clone().map(|v| ((*key).to_string(), v)))
        .collect()
}

pub async fn reminders_create(
    title: &str,
    list: Option<&str>,
    due: Option<&str>,
    priority: Option<i32>,
    notes: Option<&str>,
) -> Result<Json, BridgeError> {
    let mut args = optional_args(&[
        ("list", list.map(Json::from)),
        ("due", due.map(Json::from)),
        ("priority", priority.map(Json::from)),
        ("notes", notes.map(Json::from)),
    ]);
    args.insert("title".into(), Json::from(title));
    let row = call("reminders.create", Json::Object(args)).await?;
    Ok(action_with_record("created", None, None, row))
}

/// `reminders.update`; `append_notes` is folded into `notes` after reading
/// the current notes from the store, since EventKit has no append.
pub async fn reminders_update(
    id: &str,
    fields: &super::reminders::UpdateFields<'_>,
) -> anyhow::Result<Json> {
    let notes = match (fields.notes, fields.append_notes) {
        (Some(notes), _) => Some(notes.to_string()),
        (None, Some(extra)) => {
            let current = super::reminders::get(super::reminders::Target::Id(id), None)
                .await?
                .notes
                .unwrap_or_default();
            Some(if current.is_empty() {
                extra.to_string()
            } else {
                format!("{current}\n{extra}")
            })
        }
        (None, None) => None,
    };
    let mut args = optional_args(&[
        ("title", fields.title.map(Json::from)),
        ("notes", notes.map(Json::from)),
        ("priority", fields.priority.map(Json::from)),
        ("due", fields.due.map(Json::from)),
    ]);
    if args.is_empty() {
        anyhow::bail!("Nothing to update");
    }
    args.insert("id".into(), Json::from(id));
    let row = call("reminders.update", Json::Object(args)).await?;
    Ok(action_with_record(
        "updated",
        Some(id),
        Some(format!("Updated id {id}")),
        row,
    ))
}

/// `reminders.complete` / `reminders.reopen`.
pub async fn reminders_set_completed(id: &str, completed: bool) -> Result<Json, BridgeError> {
    let (cmd, action, past) = if completed {
        ("reminders.complete", "completed", "Marked")
    } else {
        ("reminders.reopen", "reopened", "Reopened")
    };
    let row = call(cmd, serde_json::json!({"id": id})).await?;
    Ok(action_with_record(
        action,
        Some(id),
        Some(format!("{past} id {id}")),
        row,
    ))
}

pub async fn reminders_delete(id: &str) -> Result<Json, BridgeError> {
    let row = call("reminders.delete", serde_json::json!({"id": id})).await?;
    Ok(action_with_record(
        "deleted",
        Some(id),
        Some(format!("Deleted id {id}")),
        row,
    ))
}

/// `batch-complete` / `batch-reopen` / `batch-delete`: one CLI call per id,
/// every outcome recorded, so a caller retries only the failed ids.
pub async fn reminders_batch(
    verb: &str,
    ids: &[String],
) -> anyhow::Result<super::util::BatchActionResult> {
    use super::util::{BatchActionResult, BatchItemResult};
    let cmd = match verb {
        "complete" => "reminders.complete",
        "reopen" => "reminders.reopen",
        "delete" => "reminders.delete",
        other => anyhow::bail!("unknown batch verb {other:?}"),
    };
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        results.push(match call(cmd, serde_json::json!({"id": id})).await {
            Ok(_) => BatchItemResult::success(id.clone()),
            Err(error) => BatchItemResult::failure(id.clone(), error.to_string()),
        });
    }
    Ok(BatchActionResult::new(&format!("batch-{verb}"), results))
}

#[allow(clippy::too_many_arguments)]
pub async fn calendar_create(
    title: &str,
    start: &str,
    end: &str,
    calendar: Option<&str>,
    location: Option<&str>,
    notes: Option<&str>,
    all_day: bool,
) -> Result<Json, BridgeError> {
    let mut args = optional_args(&[
        ("calendar", calendar.map(Json::from)),
        ("location", location.map(Json::from)),
        ("notes", notes.map(Json::from)),
        ("all_day", all_day.then_some(Json::Bool(true))),
    ]);
    args.insert("title".into(), Json::from(title));
    args.insert("start".into(), Json::from(start));
    args.insert("end".into(), Json::from(end));
    let row = call("calendar.create", Json::Object(args)).await?;
    Ok(action_with_record("created", None, None, row))
}

pub async fn calendar_update(
    id: &str,
    fields: &super::calendar::UpdateFields<'_>,
) -> anyhow::Result<Json> {
    let mut args = optional_args(&[
        ("title", fields.title.map(Json::from)),
        ("start", fields.start.map(Json::from)),
        ("end", fields.end.map(Json::from)),
        ("location", fields.location.map(Json::from)),
        ("notes", fields.notes.map(Json::from)),
        ("all_day", fields.all_day.map(Json::from)),
    ]);
    if args.is_empty() {
        anyhow::bail!("Nothing to update");
    }
    args.insert("id".into(), Json::from(id));
    let row = call("calendar.update", Json::Object(args)).await?;
    Ok(action_with_record(
        "updated",
        Some(id),
        Some(format!("Updated id {id}")),
        row,
    ))
}

pub async fn calendar_delete(id: &str) -> Result<Json, BridgeError> {
    let row = call("calendar.delete", serde_json::json!({"id": id})).await?;
    Ok(action_with_record(
        "deleted",
        Some(id),
        Some(format!("Deleted id {id}")),
        row,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn action_with_record_keeps_the_action_result_contract() {
        let created =
            action_with_record("created", None, None, json!({"id": "R-1", "title": "Milk"}));
        assert_eq!(created["ok"], true);
        assert_eq!(created["action"], "created");
        assert_eq!(created["id"], "R-1", "id is lifted from the row");
        assert_eq!(created["record"]["title"], "Milk");
        assert!(created.get("message").is_none());

        let deleted = action_with_record(
            "deleted",
            Some("R-1"),
            Some("Deleted id R-1".into()),
            json!({"deleted": true}),
        );
        assert_eq!(deleted["id"], "R-1");
        assert_eq!(deleted["message"], "Deleted id R-1");
        assert_eq!(deleted["record"]["deleted"], true);

        let bare = action_with_record("deleted", Some("R-2"), None, Json::Null);
        assert!(bare.get("record").is_none(), "no record for a null reply");
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cider-bridge-cli-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for `cider-bridge`: canned envelopes keyed on the command,
    /// echoing its JSON argument back for `echo` so the wire shape is
    /// checked, and a two-line stream for `watch`.
    fn stub_cli(dir: &Path) -> PathBuf {
        let script = dir.join(CLI_NAME);
        std::fs::write(
            &script,
            r#"#!/bin/sh
case "$1" in
  reminders.create) echo '{"id":0,"ok":true,"data":{"id":"R-1","title":"Milk","list":"Shopping","completed":false}}' ;;
  ping) echo '{"id":0,"ok":true,"data":{"version":"0.7.0","calendar":"full_access","reminders":"denied","contacts":"not_determined"}}' ;;
  unknown) echo "{\"id\":0,\"ok\":false,\"error\":{\"code\":\"invalid_args\",\"message\":\"unknown command '$1'\"}}"; exit 1 ;;
  echo) printf '{"id":0,"ok":true,"data":%s}\n' "$2" ;;
  denied) echo '{"id":0,"ok":false,"error":{"code":"permission_denied","message":"Reminders access is denied"}}'; exit 1 ;;
  slow) sleep 3; echo '{"id":0,"ok":true,"data":null}' ;;
  garbage) echo 'not an envelope' ;;
  silent) echo 'boom' >&2; exit 3 ;;
  watch)
    echo '{"id":0,"ok":true,"data":{"source":"reminders","at":"2026-09-02T10:00:00Z","kind":"store_changed"}}'
    echo 'noise'
    echo '{"id":0,"ok":true,"data":{"source":"calendar","at":"2026-09-02T10:00:01Z","kind":"store_changed"}}'
    ;;
  watch-fail) echo '{"id":0,"ok":false,"error":{"code":"invalid_args","message":"unknown source"}}'; exit 1 ;;
  *) echo "{\"id\":0,\"ok\":false,\"error\":{\"code\":\"not_found\",\"message\":\"no such command $1\"}}"; exit 1 ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[tokio::test]
    async fn call_parses_data_and_passes_args_as_one_json_argument() {
        let dir = temp_dir("call");
        let cli = stub_cli(&dir);

        let row = call_at(
            &cli,
            "reminders.create",
            json!({"title": "Milk"}),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(row["id"], "R-1");
        assert_eq!(row["list"], "Shopping");

        let args = json!({"id": "R-1", "notes": "two\nlines", "priority": 5, "due": null});
        let echoed = call_at(&cli, "echo", args.clone(), CALL_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(echoed, args, "args arrive as a single JSON object");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn error_envelopes_map_to_typed_errors() {
        let dir = temp_dir("errors");
        let cli = stub_cli(&dir);

        let denied = call_at(&cli, "denied", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(
            denied,
            BridgeError::Remote {
                code: "permission_denied".into(),
                message: "Reminders access is denied".into()
            }
        );
        let missing = call_at(&cli, "nope", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(
            matches!(missing, BridgeError::Remote { ref code, .. } if code == "not_found"),
            "{missing:?}"
        );
        let garbage = call_at(&cli, "garbage", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(matches!(garbage, BridgeError::Protocol(_)), "{garbage:?}");
        let silent = call_at(&cli, "silent", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(
            matches!(silent, BridgeError::Protocol(ref d) if d.contains("exit 3") && d.contains("boom")),
            "{silent:?}"
        );
        let absent = call_at(&dir.join("missing"), "ping", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(matches!(absent, BridgeError::Unreachable(_)), "{absent:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn slow_cli_times_out_and_is_killed() {
        let dir = temp_dir("timeout");
        let cli = stub_cli(&dir);
        let started = std::time::Instant::now();
        let error = call_at(&cli, "slow", json!({}), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            matches!(error, BridgeError::Unreachable(ref d) if d.contains("timed out after 1s")),
            "{error:?}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "the 3 s stub must not be waited for: {:?}",
            started.elapsed()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn watch_streams_each_data_object_and_skips_noise() {
        let dir = temp_dir("watch");
        let cli = stub_cli(&dir);
        let mut seen = Vec::new();
        // The stub ignores the sources; the shape of the argument is what
        // matters, and `echo` covered that above.
        let stub = cli.clone();
        stream_watch_at(&stub, &["reminders", "calendar"], |data| seen.push(data))
            .await
            .unwrap_err();
        assert_eq!(
            seen.len(),
            2,
            "both lines are delivered before the early end"
        );
        assert_eq!(seen[0]["source"], "reminders");
        assert_eq!(seen[0]["kind"], "store_changed");
        assert_eq!(seen[1]["source"], "calendar");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn watch_error_envelope_ends_the_stream_as_that_error() {
        let dir = temp_dir("watch-fail");
        let cli = stub_cli(&dir);
        // A wrapper maps the `watch` the client sends onto the stub's
        // failing branch.
        let mut count = 0;
        let wrapper = dir.join("wrapper.sh");
        std::fs::write(
            &wrapper,
            format!("#!/bin/sh\nexec \"{}\" watch-fail \"$2\"\n", cli.display()),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = stream_watch_at(&wrapper, &["mail"], |_| count += 1)
            .await
            .unwrap_err();
        assert_eq!(count, 0);
        assert!(
            matches!(error, BridgeError::Remote { ref code, .. } if code == "invalid_args"),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn ping_reports_version_and_unknown_command_is_incompatible() {
        let dir = temp_dir("version");
        let cli = stub_cli(&dir);

        let pong = ping_at(&cli).await.unwrap();
        assert_eq!(ping_version(&pong), "0.7.0");
        assert_eq!(pong["reminders"], "denied");
        assert!(ping_at(&dir.join("missing")).await.is_none());

        let stale = call_at(&cli, "unknown", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(
            stale,
            BridgeError::Incompatible {
                have: "0.7.0".into(),
                want: super::super::bridge::BRIDGE_PROTOCOL_VERSION.into()
            },
            "the version comes from a follow-up ping"
        );
        assert!(stale.to_string().contains("Cider Bridge 0.7.0"));

        // A CLI so old that even `ping` is unknown must not loop on itself.
        let ancient = dir.join("ancient");
        std::fs::write(
            &ancient,
            "#!/bin/sh\necho \"{\\\"id\\\":0,\\\"ok\\\":false,\\\"error\\\":{\\\"code\\\":\\\"invalid_args\\\",\\\"message\\\":\\\"unknown command '$1'\\\"}}\"; exit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&ancient, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ping_at(&ancient).await.is_none());
        let error = call_at(&ancient, "reminders.create", json!({}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(
            matches!(error, BridgeError::Incompatible { ref have, .. } if have == "unknown"),
            "{error:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_authorization_summarizes_ping_verbatim_with_fixes() {
        let auth = StoreAuthorization::from_ping(&json!({
            "version": "0.1.0", "calendar": "not_determined", "reminders": "full_access",
            "contacts": "denied", "executable": "/x/cider-bridge", "tcc_subject": "launcher"
        }));
        assert_eq!(auth.calendar, "not_determined");
        assert_eq!(auth.reminders, "full_access");
        assert_eq!(auth.contacts, "denied");
        assert!(!auth.all_granted());
        assert!(auth.any_denied());
        assert_eq!(auth.fixes.len(), 2);
        assert!(
            auth.fixes[0].starts_with("calendar is not_determined"),
            "{:?}",
            auth.fixes
        );
        assert!(
            auth.fixes[1].contains("Privacy & Security › Contacts"),
            "{:?}",
            auth.fixes
        );
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["tcc_subject"], "launcher");
        assert_eq!(json["fixes"].as_array().unwrap().len(), 2);

        let granted = StoreAuthorization::from_ping(&json!({
            "calendar": "full_access", "reminders": "full_access", "contacts": "authorized"
        }));
        assert!(granted.all_granted());
        assert!(!granted.any_denied());
        assert!(serde_json::to_value(&granted)
            .unwrap()
            .get("executable")
            .is_none());

        // A ping without the fields (an older CLI) is unknown, not a problem.
        let unknown = StoreAuthorization::from_ping(&json!({"version": "0.0.1"}));
        assert_eq!(unknown.calendar, "unknown");
        assert!(unknown.all_granted());
    }

    #[test]
    fn search_order_is_env_then_app_then_local_bin_then_path() {
        let root = temp_dir("search");
        let home = root.join("home");
        let app_cli = home
            .join("Applications")
            .join(APP_NAME)
            .join("Contents/MacOS")
            .join(CLI_NAME);
        let local_cli = home.join(".local/bin").join(CLI_NAME);
        let path_dir = root.join("path-bin");
        let path_cli = path_dir.join(CLI_NAME);
        let env_cli = root.join("elsewhere").join(CLI_NAME);
        for cli in [&app_cli, &local_cli, &path_cli, &env_cli] {
            std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
            std::fs::write(cli, "#!/bin/sh\n").unwrap();
        }
        let brew_dir = root.join("brew/opt/cider/bin");
        let brew_cli = brew_dir.join(CLI_NAME);
        std::fs::create_dir_all(&brew_dir).unwrap();
        std::fs::write(&brew_cli, "#!/bin/sh\n").unwrap();
        let brew = vec![brew_dir.clone()];
        let path_var = std::env::join_paths([root.join("nowhere"), path_dir.clone()]).unwrap();
        let candidates = || {
            cli_candidates(
                &home,
                Some(env_cli.as_os_str()),
                &brew,
                Some(path_var.as_os_str()),
            )
        };

        assert_eq!(
            candidates().unwrap(),
            vec![
                env_cli.clone(),
                app_cli.clone(),
                brew_cli.clone(),
                local_cli.clone(),
                root.join("nowhere").join(CLI_NAME),
                path_cli.clone(),
            ]
        );
        assert_eq!(cli_path_from(candidates()), Some(env_cli.clone()));
        std::fs::remove_file(&env_cli).unwrap();
        assert_eq!(cli_path_from(candidates()), Some(app_cli.clone()));
        std::fs::remove_file(&app_cli).unwrap();
        assert_eq!(
            cli_path_from(candidates()),
            Some(brew_cli.clone()),
            "the packaged CLI next to the cider binary comes before ~/.local/bin"
        );
        std::fs::remove_file(&brew_cli).unwrap();
        assert_eq!(cli_path_from(candidates()), Some(local_cli.clone()));
        std::fs::remove_file(&local_cli).unwrap();
        assert_eq!(cli_path_from(candidates()), Some(path_cli.clone()));
        std::fs::remove_file(&path_cli).unwrap();
        assert_eq!(cli_path_from(candidates()), None);

        // `off` switches the CLI off even when it is installed.
        std::fs::write(&app_cli, "#!/bin/sh\n").unwrap();
        assert_eq!(
            cli_candidates(&home, Some(OsStr::new("off")), &[], None),
            None
        );
        assert_eq!(
            cli_candidates(&home, Some(OsStr::new(" OFF ")), &[], None),
            None
        );
        assert_eq!(
            cli_path_from(cli_candidates(&home, Some(OsStr::new("off")), &[], None)),
            None
        );
        // An empty override is the same as none.
        assert_eq!(
            cli_path_from(cli_candidates(&home, Some(OsStr::new("")), &[], None)),
            Some(app_cli.clone())
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn not_installed_error_says_how_to_build_it() {
        let message = BridgeError::CliNotInstalled.to_string();
        assert!(
            message.contains("cider-bridge is not installed"),
            "{message}"
        );
        assert!(
            message.contains("cider bridge build --install"),
            "{message}"
        );
    }
}
