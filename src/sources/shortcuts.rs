use std::io::Cursor;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use uuid::Uuid;

use super::home;
use super::keyed_archive::{hex_decode, plist_to_json};
use super::util::{run_command_with_timeout, ActionResult, APPLE_EPOCH};

#[derive(Debug, Serialize)]
pub struct Shortcut {
    pub name: String,
}

/// An installed shortcut with its action list decoded.
///
/// `actions` is the shortcut's `WFWorkflowActions` array converted from its
/// binary plist: each entry has `WFWorkflowActionIdentifier` and
/// `WFWorkflowActionParameters`. Data blobs appear as `{"$data_len", "$hex"}`
/// (hex only up to 256 bytes), and Home scene actions gain a `$decoded`
/// sibling next to their protobuf blob naming the scene and home UUIDs.
#[derive(Debug, Serialize)]
pub struct ShortcutExport {
    pub name: String,
    pub workflow_id: String,
    pub action_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    pub actions: serde_json::Value,
}

const HOME_ACTION: &str = "is.workflow.actions.homeaccessory";
const HEX_LIMIT: usize = 256;

/// Read one shortcut's action list out of the Shortcuts app's SQLite store.
pub async fn export(name: &str) -> anyhow::Result<ShortcutExport> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db = format!("{home}/Library/Shortcuts/Shortcuts.sqlite");
    if tokio::fs::metadata(&db).await.is_err() {
        anyhow::bail!("Shortcuts database not found (path: {db})");
    }
    let uri = format!("file:{db}?mode=ro");
    let query = format!(
        "select s.ZNAME as name, s.ZWORKFLOWID as workflow_id, s.ZACTIONCOUNT as action_count, \
         s.ZMODIFICATIONDATE as modified, hex(a.ZDATA) as actions_hex \
         from ZSHORTCUT s join ZSHORTCUTACTIONS a on a.Z_PK = s.ZACTIONS \
         where s.ZNAME = '{}' limit 1",
        name.replace('\'', "''")
    );
    let stdout =
        run_command_with_timeout("sqlite3", &["-json", &uri, &query], Duration::from_secs(10))
            .await?;
    let rows: Vec<serde_json::Value> = if stdout.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&stdout)?
    };
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("shortcut '{name}' not found"))?;

    let actions_hex = row["actions_hex"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("shortcut '{name}' has no action data"))?;
    let actions = decode_actions(&hex_decode(actions_hex)?)?;

    Ok(ShortcutExport {
        name: row["name"].as_str().unwrap_or(name).to_string(),
        workflow_id: row["workflow_id"].as_str().unwrap_or_default().to_string(),
        action_count: row["action_count"].as_i64().unwrap_or(0),
        modified_at: row["modified"].as_f64().and_then(apple_date),
        actions,
    })
}

fn apple_date(seconds: f64) -> Option<DateTime<Utc>> {
    let whole = seconds.trunc();
    let nanos = ((seconds - whole) * 1e9) as u32;
    DateTime::from_timestamp(whole as i64 + APPLE_EPOCH, nanos)
}

/// Binary-plist action bytes → JSON, with Home scene blobs decoded in place.
fn decode_actions(bytes: &[u8]) -> anyhow::Result<serde_json::Value> {
    let mut actions = Value::from_reader(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("action data is not a plist: {e}"))?;
    decorate_home_actions(&mut actions);
    Ok(plist_to_json(&actions, HEX_LIMIT))
}

/// Walk `WFWorkflowActions`; beside every `HMActionSetSerializedData` blob of a
/// Home action, add a `$decoded` dictionary with what the protobuf names.
fn decorate_home_actions(actions: &mut Value) {
    let Some(actions) = actions.as_array_mut() else {
        return;
    };
    for action in actions.iter_mut().filter_map(Value::as_dictionary_mut) {
        let identifier = action
            .get("WFWorkflowActionIdentifier")
            .and_then(Value::as_string);
        if identifier != Some(HOME_ACTION) {
            continue;
        }
        let sets = action
            .get_mut("WFWorkflowActionParameters")
            .and_then(Value::as_dictionary_mut)
            .and_then(|p| p.get_mut("WFHomeTriggerActionSets"))
            .and_then(Value::as_dictionary_mut)
            .and_then(|t| t.get_mut("WFHFTriggerActionSetsBuilderParameterStateActionSets"))
            .and_then(Value::as_array_mut);
        let Some(sets) = sets else {
            continue;
        };
        for set in sets.iter_mut().filter_map(Value::as_dictionary_mut) {
            let decoded = set
                .get("HMActionSetSerializedData")
                .and_then(Value::as_data)
                .and_then(decode_action_set_ref);
            if let Some(decoded) = decoded {
                set.insert("$decoded".to_string(), Value::Dictionary(decoded));
            }
        }
    }
}

/// One length-delimited or scalar protobuf field.
#[derive(Debug, PartialEq)]
enum ProtoField {
    Varint(u64),
    Fixed64(u64),
    Bytes(Vec<u8>),
    Fixed32(u32),
}

fn read_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// Split a protobuf message into `(field_number, value)` pairs. Stops at the
/// first malformed byte rather than guessing.
fn protobuf_fields(bytes: &[u8]) -> Vec<(u32, ProtoField)> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(key) = read_varint(bytes, &mut pos) else {
            break;
        };
        let number = (key >> 3) as u32;
        let field = match key & 0x7 {
            0 => match read_varint(bytes, &mut pos) {
                Some(v) => ProtoField::Varint(v),
                None => break,
            },
            1 => match bytes.get(pos..pos + 8) {
                Some(b) => {
                    pos += 8;
                    ProtoField::Fixed64(u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
                }
                None => break,
            },
            2 => {
                let Some(len) = read_varint(bytes, &mut pos) else {
                    break;
                };
                let Some(end) = usize::try_from(len)
                    .ok()
                    .and_then(|len| pos.checked_add(len))
                else {
                    break;
                };
                match bytes.get(pos..end) {
                    Some(b) => {
                        pos = end;
                        ProtoField::Bytes(b.to_vec())
                    }
                    None => break,
                }
            }
            5 => match bytes.get(pos..pos + 4) {
                Some(b) => {
                    pos += 4;
                    ProtoField::Fixed32(u32::from_le_bytes(b.try_into().unwrap_or([0; 4])))
                }
                None => break,
            },
            _ => break,
        };
        fields.push((number, field));
    }
    fields
}

/// What a Home action's `HMActionSetSerializedData` protobuf points at.
///
/// A scene run carries field 4 = scene (action set) UUID and field 5 = home
/// UUID, both 16 raw bytes. A per-accessory write instead carries field 1 =
/// accessory UUID as ASCII and field 2 = the characteristic state to apply.
fn decode_action_set_ref(bytes: &[u8]) -> Option<Dictionary> {
    let mut out = Dictionary::new();
    for (number, field) in protobuf_fields(bytes) {
        let ProtoField::Bytes(data) = field else {
            continue;
        };
        match number {
            4 | 5 => {
                if let Ok(id) = uuid::Uuid::from_slice(&data) {
                    let key = if number == 4 { "scene_id" } else { "home_id" };
                    out.insert(
                        key.into(),
                        Value::String(id.hyphenated().to_string().to_uppercase()),
                    );
                }
            }
            1 => {
                if let Ok(id) = String::from_utf8(data) {
                    out.insert("accessory_id".into(), Value::String(id));
                }
            }
            2 => {
                out.insert(
                    "state_len".into(),
                    Value::Integer((data.len() as u64).into()),
                );
            }
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

pub async fn list() -> anyhow::Result<Vec<Shortcut>> {
    let output =
        run_command_with_timeout("shortcuts", &["list"], std::time::Duration::from_secs(15))
            .await?;

    Ok(parse_output(&output))
}

pub async fn run(name: &str, input: Option<&str>) -> anyhow::Result<ActionResult> {
    let timeout = Duration::from_secs(120);

    let output = if let Some(input_text) = input {
        // Pipe input via stdin
        let mut command = Command::new("shortcuts");
        command
            .args(["run", name])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(input_text.as_bytes()).await?;
            // Drop stdin to close it so the shortcut can proceed
        }

        tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("shortcuts timed out after {timeout:?}"))??
    } else {
        let mut command = Command::new("shortcuts");
        command
            .args(["run", name])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn()?;

        tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("shortcuts timed out after {timeout:?}"))??
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("shortcuts run failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)?.trim().to_string();
    if stdout.is_empty() {
        Ok(ActionResult::success_with_message(
            "run",
            &format!("Ran shortcut '{name}'"),
        ))
    } else {
        Ok(ActionResult::success_with_message("run", &stdout))
    }
}

pub async fn view(name: &str) -> anyhow::Result<ActionResult> {
    run_command_with_timeout(
        "shortcuts",
        &["view", name],
        std::time::Duration::from_secs(15),
    )
    .await?;

    Ok(ActionResult::success_with_message(
        "view",
        &format!("Opened shortcut '{name}' in Shortcuts"),
    ))
}

pub async fn sign(input: &str, output: &str, mode: Option<&str>) -> anyhow::Result<ActionResult> {
    let mut args = vec!["sign"];
    if let Some(m) = mode {
        args.push("--mode");
        args.push(m);
    }
    args.push("--input");
    args.push(input);
    args.push("--output");
    args.push(output);

    run_command_with_timeout("shortcuts", &args, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "sign",
        &format!("Signed shortcut file to '{output}'"),
    ))
}

/// A shortcut to generate: a name and an ordered list of steps.
///
/// ```json
/// {"name": "River: Night", "steps": [
///   {"scene": {"home": "2183 26th Ave", "scene": "Good Night"}},
///   {"delay_seconds": 600},
///   {"speak": "Good night"},
///   {"open_url": "https://example.com"},
///   {"ssh": {"host": "mac.tail.ts.net", "user": "paul", "script": "~/bin/announce hi"}}
/// ]}
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct ShortcutSpec {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Run a HomeKit scene; `home` and `scene` are names or UUIDs.
    Scene {
        home: String,
        scene: String,
    },
    DelaySeconds(f64),
    Speak(String),
    OpenUrl(String),
    /// Run Script Over SSH. The password is asked for in Shortcuts on the
    /// first run; it is never part of the file.
    Ssh {
        host: String,
        user: String,
        script: String,
        #[serde(default)]
        port: Option<u16>,
    },
}

pub fn parse_spec(json: &str) -> anyhow::Result<ShortcutSpec> {
    let spec: ShortcutSpec =
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("invalid shortcut spec: {e}"))?;
    if spec.name.trim().is_empty() {
        anyhow::bail!("shortcut spec needs a non-empty name");
    }
    if spec.steps.is_empty() {
        anyhow::bail!("shortcut spec '{}' has no steps", spec.name);
    }
    Ok(spec)
}

/// `./<name>.shortcut`, with path separators in the name flattened.
/// Shortcuts names an imported shortcut after the file, not after
/// `WFWorkflowName`, so the default file name is the shortcut name itself.
pub fn default_output(name: &str) -> String {
    format!("./{}.shortcut", name.replace(['/', ':'], "-"))
}

/// True when importing `output` would give the shortcut a different name
/// than the spec asked for.
pub fn output_renames(name: &str, output: &str) -> bool {
    let stem = std::path::Path::new(output)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    stem != name.replace(['/', ':'], "-")
}

/// How long an ssh probe waits for `host:port` to accept a connection.
const SSH_PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// Where macOS turns `sshd` on; named in the error so the fix is one step.
const REMOTE_LOGIN_SETTING: &str = "System Settings › General › Sharing › Remote Login";

/// True when `host` names the Mac this is running on: `localhost`, a
/// loopback address, or one of `own_names` (see [`own_host_names`]).
pub fn is_this_mac(host: &str, own_names: &[String]) -> bool {
    let host = host.trim().trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || own_names
            .iter()
            .any(|name| name.trim_end_matches('.').eq_ignore_ascii_case(host))
}

/// This Mac's own names from `hostname`: the full name (`mac.local`) and
/// its first label (`mac`). Empty when the command fails.
pub fn own_host_names() -> Vec<String> {
    let Ok(output) = std::process::Command::new("hostname").output() else {
        return Vec::new();
    };
    let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if full.is_empty() {
        return Vec::new();
    }
    let short = full.split('.').next().map(str::to_string);
    std::iter::once(full)
        .chain(short.filter(|s| !s.is_empty()))
        .collect::<Vec<_>>()
}

/// Whether something accepts TCP connections at `host:port` within
/// [`SSH_PROBE_TIMEOUT`]. A name that does not resolve is "closed".
pub fn ssh_port_open(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    (host, port)
        .to_socket_addrs()
        .ok()
        .into_iter()
        .flatten()
        .any(|addr| TcpStream::connect_timeout(&addr, SSH_PROBE_TIMEOUT).is_ok())
}

/// Check every `ssh` step before building. Returns the warnings to print
/// on stderr; fails when a step targets this Mac and nothing listens on
/// its port — Remote Login is off, and the shortcut would hang silently
/// in the Shortcuts app — unless `allow_unreachable` is set. A remote host
/// that does not answer only warns: it may be off or unreachable from
/// here at build time. `probe(host, port)` is injected so the decision is
/// testable without a network.
pub fn check_ssh_steps(
    spec: &ShortcutSpec,
    allow_unreachable: bool,
    own_names: &[String],
    probe: impl Fn(&str, u16) -> bool,
) -> anyhow::Result<Vec<String>> {
    let mut warnings = Vec::new();
    for step in &spec.steps {
        let Step::Ssh {
            host, user, port, ..
        } = step
        else {
            continue;
        };
        let port = port.unwrap_or(22);
        warnings.push(format!(
            "ssh step to {user}@{host} uses password authentication; the Shortcuts app will ask for the password on the first run"
        ));
        if probe(host, port) {
            continue;
        }
        if is_this_mac(host, own_names) {
            let problem = format!(
                "ssh step targets this Mac but Remote Login is off (port {port} closed); enable it in {REMOTE_LOGIN_SETTING}"
            );
            if !allow_unreachable {
                anyhow::bail!("invalid shortcut spec: {problem}, or pass --allow-unreachable-ssh");
            }
            warnings.push(format!(
                "{problem}; generating anyway (--allow-unreachable-ssh)"
            ));
        } else {
            warnings.push(format!(
                "ssh step to {host}:{port} is not reachable from here right now; the shortcut will hang in the Shortcuts app if it still is not when run"
            ));
        }
    }
    Ok(warnings)
}

/// A step with every name turned into the identifier the file needs.
#[derive(Debug, Clone, PartialEq)]
enum ResolvedStep {
    Scene {
        home_id: Uuid,
        scene_id: Uuid,
    },
    Delay(f64),
    Speak(String),
    OpenUrl(String),
    Ssh {
        host: String,
        user: String,
        script: String,
        port: u16,
    },
}

fn resolve_steps(spec: &ShortcutSpec, homes: &[home::Home]) -> anyhow::Result<Vec<ResolvedStep>> {
    spec.steps
        .iter()
        .map(|step| {
            Ok(match step {
                Step::Scene { home, scene } => {
                    let home = home::find_home(homes, home)?;
                    let scene = home::find_scene(home, scene)?;
                    ResolvedStep::Scene {
                        home_id: Uuid::parse_str(&home.id).map_err(|e| {
                            anyhow::anyhow!("home '{}' id {}: {e}", home.name, home.id)
                        })?,
                        scene_id: Uuid::parse_str(&scene.id).map_err(|e| {
                            anyhow::anyhow!("scene '{}' id {}: {e}", scene.name, scene.id)
                        })?,
                    }
                }
                Step::DelaySeconds(seconds) => ResolvedStep::Delay(*seconds),
                Step::Speak(text) => ResolvedStep::Speak(text.clone()),
                Step::OpenUrl(url) => ResolvedStep::OpenUrl(url.clone()),
                Step::Ssh {
                    host,
                    user,
                    script,
                    port,
                } => ResolvedStep::Ssh {
                    host: host.clone(),
                    user: user.clone(),
                    script: script.clone(),
                    port: port.unwrap_or(22),
                },
            })
        })
        .collect()
}

/// The `HMActionSetSerializedData` protobuf for running a scene: field 4 =
/// scene UUID bytes, field 5 = home UUID bytes. Inverse of
/// `decode_action_set_ref`.
fn encode_scene_ref(scene_id: &Uuid, home_id: &Uuid) -> Vec<u8> {
    let mut out = Vec::with_capacity(36);
    out.push(0x22);
    out.push(16);
    out.extend_from_slice(scene_id.as_bytes());
    out.push(0x2a);
    out.push(16);
    out.extend_from_slice(home_id.as_bytes());
    out
}

fn uppercase(id: &Uuid) -> String {
    id.hyphenated().to_string().to_uppercase()
}

fn string_value(s: &str) -> Value {
    Value::String(s.to_string())
}

fn action(identifier: &str, params: Dictionary) -> Value {
    let mut out = Dictionary::new();
    out.insert(
        "WFWorkflowActionIdentifier".into(),
        string_value(identifier),
    );
    out.insert(
        "WFWorkflowActionParameters".into(),
        Value::Dictionary(params),
    );
    Value::Dictionary(out)
}

fn build_action(step: &ResolvedStep) -> Value {
    let mut params = Dictionary::new();
    match step {
        ResolvedStep::Scene { home_id, scene_id } => {
            let mut set = Dictionary::new();
            set.insert(
                "HMActionSetSerializedData".into(),
                Value::Data(encode_scene_ref(scene_id, home_id)),
            );
            set.insert(
                "HMActionSetSerializedDictionaryProtocol".into(),
                string_value("ProtoBuf"),
            );
            set.insert(
                "HMActionSetSerializedDictionaryVersion".into(),
                string_value("1.0"),
            );
            let mut trigger = Dictionary::new();
            trigger.insert(
                "WFHFTriggerActionSetsBuilderParameterStateActionSets".into(),
                Value::Array(vec![Value::Dictionary(set)]),
            );
            trigger.insert(
                "WFHFTriggerActionSetsBuilderParameterStateHome".into(),
                string_value(&uppercase(home_id)),
            );
            params.insert("UUID".into(), string_value(&uppercase(&Uuid::new_v4())));
            params.insert("WFHomeTriggerActionSets".into(), Value::Dictionary(trigger));
            action(HOME_ACTION, params)
        }
        ResolvedStep::Delay(seconds) => {
            params.insert("WFDelayTime".into(), Value::Real(*seconds));
            action("is.workflow.actions.delay", params)
        }
        ResolvedStep::Speak(text) => {
            params.insert("WFText".into(), string_value(text));
            action("is.workflow.actions.speaktext", params)
        }
        ResolvedStep::OpenUrl(url) => {
            params.insert("WFInput".into(), string_value(url));
            action("is.workflow.actions.openurl", params)
        }
        ResolvedStep::Ssh {
            host,
            user,
            script,
            port,
        } => {
            params.insert("WFSSHHost".into(), string_value(host));
            params.insert("WFSSHPort".into(), string_value(&port.to_string()));
            params.insert("WFSSHUser".into(), string_value(user));
            params.insert("WFSSHScript".into(), string_value(script));
            params.insert("WFSSHAuthenticationType".into(), string_value("Password"));
            action("is.workflow.actions.runsshscript", params)
        }
    }
}

/// The complete `.shortcut` plist for a name and its resolved steps.
fn build_workflow(name: &str, steps: &[ResolvedStep]) -> Value {
    let mut icon = Dictionary::new();
    icon.insert(
        "WFWorkflowIconStartColor".into(),
        Value::Integer(4_282_601_983u64.into()),
    );
    icon.insert(
        "WFWorkflowIconGlyphNumber".into(),
        Value::Integer(59_511u64.into()),
    );

    let mut out = Dictionary::new();
    out.insert(
        "WFWorkflowActions".into(),
        Value::Array(steps.iter().map(build_action).collect()),
    );
    out.insert("WFWorkflowName".into(), string_value(name));
    out.insert("WFWorkflowClientVersion".into(), string_value("2607.0.3"));
    out.insert(
        "WFWorkflowMinimumClientVersion".into(),
        Value::Integer(900u64.into()),
    );
    out.insert(
        "WFWorkflowMinimumClientVersionString".into(),
        string_value("900"),
    );
    out.insert("WFWorkflowIcon".into(), Value::Dictionary(icon));
    out.insert(
        "WFWorkflowTypes".into(),
        Value::Array(vec![string_value("NCWidget"), string_value("WatchKit")]),
    );
    out.insert(
        "WFWorkflowInputContentItemClasses".into(),
        Value::Array(Vec::new()),
    );
    out.insert("WFWorkflowImportQuestions".into(), Value::Array(Vec::new()));
    out.insert(
        "WFWorkflowHasShortcutInputVariables".into(),
        Value::Boolean(false),
    );
    out.insert("WFWorkflowHasOutputFallback".into(), Value::Boolean(false));
    out.insert(
        "WFWorkflowOutputContentItemClasses".into(),
        Value::Array(Vec::new()),
    );
    Value::Dictionary(out)
}

/// Write `spec` as a `.shortcut` file at `output`, optionally signed for
/// anyone via `shortcuts sign`. Scene steps are resolved against the Home
/// app cache, so those need `cider home` to work first. An `ssh` step to
/// this Mac is refused while its port is closed (see [`check_ssh_steps`])
/// unless `allow_unreachable_ssh` is set.
pub async fn gen(
    spec: &ShortcutSpec,
    output: &str,
    sign_it: bool,
    allow_unreachable_ssh: bool,
) -> anyhow::Result<ActionResult> {
    if output_renames(&spec.name, output) {
        eprintln!(
            "cider shortcuts gen: Shortcuts will name the imported shortcut after the file, not '{}'",
            spec.name
        );
    }
    for warning in check_ssh_steps(
        spec,
        allow_unreachable_ssh,
        &own_host_names(),
        ssh_port_open,
    )? {
        eprintln!("cider shortcuts gen: {warning}");
    }
    let needs_homes = spec.steps.iter().any(|s| matches!(s, Step::Scene { .. }));
    let homes = if needs_homes {
        home::list().await?
    } else {
        Vec::new()
    };
    let steps = resolve_steps(spec, &homes)?;
    let workflow = build_workflow(&spec.name, &steps);

    let mut bytes = Vec::new();
    workflow
        .to_writer_binary(&mut bytes)
        .map_err(|e| anyhow::anyhow!("failed to encode shortcut plist: {e}"))?;

    if sign_it {
        let unsigned = std::env::temp_dir().join(format!("cider-{}.shortcut", Uuid::new_v4()));
        let unsigned_path = unsigned.to_string_lossy().into_owned();
        tokio::fs::write(&unsigned, &bytes).await?;
        let signed = sign(&unsigned_path, output, Some("anyone")).await;
        let _ = tokio::fs::remove_file(&unsigned).await;
        signed?;
        Ok(ActionResult::success_with_message(
            "gen",
            &format!(
                "Generated and signed shortcut '{}' ({} actions) at '{output}'",
                spec.name,
                steps.len()
            ),
        ))
    } else {
        tokio::fs::write(output, &bytes).await?;
        Ok(ActionResult::success_with_message(
            "gen",
            &format!(
                "Generated unsigned shortcut '{}' ({} actions) at '{output}'; sign it with --sign or `cider shortcuts sign` before importing",
                spec.name,
                steps.len()
            ),
        ))
    }
}

/// Hand a `.shortcut` file to the Shortcuts app, which asks the user to add it.
pub async fn install(input: &str) -> anyhow::Result<ActionResult> {
    if tokio::fs::metadata(input).await.is_err() {
        anyhow::bail!("shortcut file not found: {input}");
    }
    run_command_with_timeout("open", &[input], Duration::from_secs(15)).await?;
    Ok(ActionResult::success_with_message(
        "install",
        &format!("Opened '{input}' in Shortcuts; it is added under the file's name (confirm the prompt if one appears)"),
    ))
}

fn parse_output(output: &str) -> Vec<Shortcut> {
    output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| Shortcut {
            name: l.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "Morning Routine\nOpen Apps\nSend ETA\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].name, "Morning Routine");
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }

    const GOOD_NIGHT_BLOB: &str =
        "2210e8ae569af8725352b27126871610859d2a10cb31865c3cae44e487feac8f9c9bd81a";

    #[test]
    fn protobuf_scene_ref_decodes_scene_and_home() {
        let bytes = hex_decode(GOOD_NIGHT_BLOB).unwrap();
        let fields = protobuf_fields(&bytes);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, 4);
        assert_eq!(fields[1].0, 5);

        let decoded = decode_action_set_ref(&bytes).unwrap();
        assert_eq!(
            decoded.get("scene_id").and_then(Value::as_string),
            Some("E8AE569A-F872-5352-B271-26871610859D")
        );
        assert_eq!(
            decoded.get("home_id").and_then(Value::as_string),
            Some("CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A")
        );
    }

    #[test]
    fn protobuf_accessory_write_decodes_id_and_state_len() {
        // field 1 (bytes) "ACC-1", field 2 (bytes) 3 bytes of state
        let bytes = [0x0a, 5, b'A', b'C', b'C', b'-', b'1', 0x12, 3, 1, 2, 3];
        let decoded = decode_action_set_ref(&bytes).unwrap();
        assert_eq!(
            decoded.get("accessory_id").and_then(Value::as_string),
            Some("ACC-1")
        );
        assert_eq!(
            decoded
                .get("state_len")
                .and_then(Value::as_unsigned_integer),
            Some(3)
        );
        assert!(decode_action_set_ref(&[]).is_none());
        assert!(decode_action_set_ref(&[0x22, 0x40, 1]).is_none());
    }

    #[test]
    fn decode_actions_converts_plist_and_decorates_home_actions() {
        let mut set = Dictionary::new();
        set.insert(
            "HMActionSetSerializedData".into(),
            Value::Data(hex_decode(GOOD_NIGHT_BLOB).unwrap()),
        );
        set.insert(
            "HMActionSetSerializedDictionaryProtocol".into(),
            Value::String("ProtoBuf".into()),
        );
        let mut trigger = Dictionary::new();
        trigger.insert(
            "WFHFTriggerActionSetsBuilderParameterStateActionSets".into(),
            Value::Array(vec![Value::Dictionary(set)]),
        );
        let mut params = Dictionary::new();
        params.insert("WFHomeTriggerActionSets".into(), Value::Dictionary(trigger));
        let mut action = Dictionary::new();
        action.insert(
            "WFWorkflowActionIdentifier".into(),
            Value::String(HOME_ACTION.into()),
        );
        action.insert(
            "WFWorkflowActionParameters".into(),
            Value::Dictionary(params),
        );

        let mut delay = Dictionary::new();
        delay.insert(
            "WFWorkflowActionIdentifier".into(),
            Value::String("is.workflow.actions.delay".into()),
        );
        let mut delay_params = Dictionary::new();
        delay_params.insert("WFDelayTime".into(), Value::Real(600.0));
        delay.insert(
            "WFWorkflowActionParameters".into(),
            Value::Dictionary(delay_params),
        );

        let mut bytes = Vec::new();
        Value::Array(vec![Value::Dictionary(action), Value::Dictionary(delay)])
            .to_writer_binary(&mut bytes)
            .unwrap();

        let json = decode_actions(&bytes).unwrap();
        let set = &json[0]["WFWorkflowActionParameters"]["WFHomeTriggerActionSets"]
            ["WFHFTriggerActionSetsBuilderParameterStateActionSets"][0];
        assert_eq!(set["HMActionSetSerializedData"]["$data_len"], 36);
        assert_eq!(set["HMActionSetSerializedData"]["$hex"], GOOD_NIGHT_BLOB);
        assert_eq!(
            set["$decoded"]["scene_id"],
            "E8AE569A-F872-5352-B271-26871610859D"
        );
        assert_eq!(
            set["$decoded"]["home_id"],
            "CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A"
        );
        assert_eq!(json[1]["WFWorkflowActionParameters"]["WFDelayTime"], 600.0);
        assert!(decode_actions(b"not a plist").is_err());
    }

    fn sample_homes() -> Vec<home::Home> {
        home::map_cache(&serde_json::json!({
            "kHomeDataKey": [{
                "homeName": "2183 26th Ave",
                "homeUUID": "CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A",
                "builtinActionSets": [{
                    "actionSetName": "Good Night",
                    "actionSetUUID": "E8AE569A-F872-5352-B271-26871610859D",
                    "actionSetActions": [{}]
                }]
            }]
        }))
    }

    #[test]
    fn output_renames_detects_a_mismatched_file_stem() {
        assert!(!output_renames(
            "Cider Good Night",
            "./Cider Good Night.shortcut"
        ));
        assert!(!output_renames(
            "River: Night",
            "/tmp/River- Night.shortcut"
        ));
        assert!(output_renames(
            "Cider Good Night",
            "/tmp/cider-good-night.shortcut"
        ));
    }

    #[test]
    fn spec_parses_every_step_kind() {
        let spec = parse_spec(
            r#"{"name": "River: Night", "steps": [
                {"scene": {"home": "2183 26th Ave", "scene": "Good Night"}},
                {"delay_seconds": 600},
                {"speak": "Good night"},
                {"open_url": "https://example.com"},
                {"ssh": {"host": "mac.tail.ts.net", "user": "paul", "script": "~/bin/announce hi"}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(spec.name, "River: Night");
        assert_eq!(spec.steps.len(), 5);
        assert!(
            matches!(&spec.steps[0], Step::Scene { home, scene } if home == "2183 26th Ave" && scene == "Good Night")
        );
        assert!(matches!(spec.steps[1], Step::DelaySeconds(s) if s == 600.0));
        assert!(matches!(&spec.steps[2], Step::Speak(t) if t == "Good night"));
        assert!(matches!(&spec.steps[3], Step::OpenUrl(u) if u == "https://example.com"));
        assert!(matches!(&spec.steps[4], Step::Ssh { port: None, .. }));

        assert!(parse_spec(r#"{"name": "x", "steps": []}"#).is_err());
        assert!(parse_spec(r#"{"name": "", "steps": [{"speak": "hi"}]}"#).is_err());
        assert!(parse_spec(r#"{"name": "x", "steps": [{"bogus": 1}]}"#).is_err());
        assert_eq!(default_output("River: Night"), "./River- Night.shortcut");
    }

    #[test]
    fn scene_ref_encoder_round_trips_with_decoder() {
        let scene = Uuid::parse_str("E8AE569A-F872-5352-B271-26871610859D").unwrap();
        let home = Uuid::parse_str("CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A").unwrap();
        let bytes = encode_scene_ref(&scene, &home);
        assert_eq!(bytes, hex_decode(GOOD_NIGHT_BLOB).unwrap());
        let decoded = decode_action_set_ref(&bytes).unwrap();
        assert_eq!(
            decoded.get("scene_id").and_then(Value::as_string),
            Some("E8AE569A-F872-5352-B271-26871610859D")
        );
        assert_eq!(
            decoded.get("home_id").and_then(Value::as_string),
            Some("CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A")
        );
    }

    #[test]
    fn scene_action_has_the_exact_key_set() {
        let spec = parse_spec(
            r#"{"name": "n", "steps": [{"scene": {"home": "2183 26th ave", "scene": "good night"}}]}"#,
        )
        .unwrap();
        let steps = resolve_steps(&spec, &sample_homes()).unwrap();
        let action = build_action(&steps[0]);
        let action = action.as_dictionary().unwrap();
        let mut keys: Vec<&str> = action.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            ["WFWorkflowActionIdentifier", "WFWorkflowActionParameters"]
        );
        assert_eq!(
            action
                .get("WFWorkflowActionIdentifier")
                .and_then(Value::as_string),
            Some(HOME_ACTION)
        );

        let params = action
            .get("WFWorkflowActionParameters")
            .and_then(Value::as_dictionary)
            .unwrap();
        let mut keys: Vec<&str> = params.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["UUID", "WFHomeTriggerActionSets"]);
        let uuid = params.get("UUID").and_then(Value::as_string).unwrap();
        assert_eq!(uuid, uuid.to_uppercase());
        assert!(Uuid::parse_str(uuid).is_ok());

        let trigger = params
            .get("WFHomeTriggerActionSets")
            .and_then(Value::as_dictionary)
            .unwrap();
        let mut keys: Vec<&str> = trigger.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "WFHFTriggerActionSetsBuilderParameterStateActionSets",
                "WFHFTriggerActionSetsBuilderParameterStateHome"
            ]
        );
        assert_eq!(
            trigger
                .get("WFHFTriggerActionSetsBuilderParameterStateHome")
                .and_then(Value::as_string),
            Some("CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A")
        );

        let sets = trigger
            .get("WFHFTriggerActionSetsBuilderParameterStateActionSets")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(sets.len(), 1);
        let set = sets[0].as_dictionary().unwrap();
        let mut keys: Vec<&str> = set.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "HMActionSetSerializedData",
                "HMActionSetSerializedDictionaryProtocol",
                "HMActionSetSerializedDictionaryVersion"
            ]
        );
        assert_eq!(
            set.get("HMActionSetSerializedData")
                .and_then(Value::as_data),
            Some(hex_decode(GOOD_NIGHT_BLOB).unwrap().as_slice())
        );
        assert_eq!(
            set.get("HMActionSetSerializedDictionaryProtocol")
                .and_then(Value::as_string),
            Some("ProtoBuf")
        );
        assert_eq!(
            set.get("HMActionSetSerializedDictionaryVersion")
                .and_then(Value::as_string),
            Some("1.0")
        );
    }

    fn ssh_spec(host: &str, port: Option<u16>) -> ShortcutSpec {
        let port = port.map(|p| format!(", \"port\": {p}")).unwrap_or_default();
        parse_spec(&format!(
            r#"{{"name": "n", "steps": [{{"speak": "hi"}}, {{"ssh": {{"host": "{host}", "user": "u", "script": "s"{port}}}}}]}}"#
        ))
        .unwrap()
    }

    fn own_names() -> Vec<String> {
        vec!["Studio.local".to_string(), "Studio".to_string()]
    }

    #[test]
    fn this_mac_is_localhost_loopback_or_its_own_hostname() {
        let names = own_names();
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "::1",
            "studio.local",
            "Studio",
            "Studio.local.",
        ] {
            assert!(is_this_mac(host, &names), "{host}");
        }
        for host in ["mac.tail.ts.net", "studio.example.com", "10.0.0.5", ""] {
            assert!(!is_this_mac(host, &names), "{host}");
        }
    }

    #[test]
    fn closed_local_ssh_is_refused_as_invalid_input() {
        for host in ["localhost", "127.0.0.1", "Studio.local"] {
            let err = check_ssh_steps(&ssh_spec(host, None), false, &own_names(), |_, _| false)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("Remote Login is off (port 22 closed)"),
                "{err}"
            );
            assert!(err.contains("Sharing › Remote Login"), "{err}");
            assert!(err.contains("--allow-unreachable-ssh"), "{err}");
            // `invalid` is what main.rs classifies as `invalid_input`.
            assert!(err.starts_with("invalid shortcut spec"), "{err}");
        }
    }

    #[test]
    fn allow_unreachable_turns_the_local_refusal_into_a_warning() {
        let warnings = check_ssh_steps(&ssh_spec("localhost", None), true, &own_names(), |_, _| {
            false
        })
        .unwrap();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("password"), "{warnings:?}");
        assert!(
            warnings[1].contains("Remote Login is off")
                && warnings[1].contains("generating anyway"),
            "{warnings:?}"
        );
    }

    #[test]
    fn open_port_only_warns_about_the_password_prompt() {
        let warnings =
            check_ssh_steps(&ssh_spec("localhost", None), false, &own_names(), |_, _| {
                true
            })
            .unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("u@localhost"), "{warnings:?}");
        assert!(warnings[0].contains("first run"), "{warnings:?}");
    }

    #[test]
    fn closed_remote_host_warns_but_generates() {
        let warnings = check_ssh_steps(
            &ssh_spec("mac.tail.ts.net", Some(2222)),
            false,
            &own_names(),
            |_, _| false,
        )
        .unwrap();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[1].contains("mac.tail.ts.net:2222 is not reachable"),
            "{warnings:?}"
        );
        assert!(!warnings[1].contains("Remote Login"), "{warnings:?}");
    }

    #[test]
    fn probe_sees_the_step_port_and_is_skipped_without_ssh_steps() {
        let probed = std::cell::RefCell::new(Vec::new());
        let spec = ssh_spec("Studio", Some(2222));
        check_ssh_steps(&spec, false, &own_names(), |host, port| {
            probed.borrow_mut().push((host.to_string(), port));
            true
        })
        .unwrap();
        assert_eq!(probed.borrow().as_slice(), [("Studio".to_string(), 2222)]);

        let spec = parse_spec(r#"{"name": "n", "steps": [{"speak": "hi"}]}"#).unwrap();
        let warnings = check_ssh_steps(&spec, false, &own_names(), |_, _| {
            panic!("no ssh step, nothing to probe")
        })
        .unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_scene_or_home_is_an_error() {
        let spec =
            parse_spec(r#"{"name": "n", "steps": [{"scene": {"home": "Nope", "scene": "x"}}]}"#)
                .unwrap();
        let err = resolve_steps(&spec, &sample_homes()).unwrap_err();
        assert!(err.to_string().contains("no home matches 'Nope'"));
        let spec = parse_spec(
            r#"{"name": "n", "steps": [{"scene": {"home": "2183 26th Ave", "scene": "x"}}]}"#,
        )
        .unwrap();
        let err = resolve_steps(&spec, &sample_homes()).unwrap_err();
        assert!(err.to_string().contains("no scene matches 'x'"));
    }

    #[test]
    fn workflow_has_standard_envelope_and_one_action_per_step() {
        let spec = parse_spec(
            r#"{"name": "Night", "steps": [
                {"delay_seconds": 1.5}, {"speak": "hi"}, {"open_url": "https://x"},
                {"ssh": {"host": "h", "user": "u", "script": "s", "port": 2222}}
            ]}"#,
        )
        .unwrap();
        let steps = resolve_steps(&spec, &[]).unwrap();
        let workflow = build_workflow(&spec.name, &steps);
        let dict = workflow.as_dictionary().unwrap();
        let mut keys: Vec<&str> = dict.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "WFWorkflowActions",
                "WFWorkflowClientVersion",
                "WFWorkflowHasOutputFallback",
                "WFWorkflowHasShortcutInputVariables",
                "WFWorkflowIcon",
                "WFWorkflowImportQuestions",
                "WFWorkflowInputContentItemClasses",
                "WFWorkflowMinimumClientVersion",
                "WFWorkflowMinimumClientVersionString",
                "WFWorkflowName",
                "WFWorkflowOutputContentItemClasses",
                "WFWorkflowTypes"
            ]
        );
        assert_eq!(
            dict.get("WFWorkflowName").and_then(Value::as_string),
            Some("Night")
        );
        assert_eq!(
            dict.get("WFWorkflowMinimumClientVersion")
                .and_then(Value::as_unsigned_integer),
            Some(900)
        );

        let actions = dict
            .get("WFWorkflowActions")
            .and_then(Value::as_array)
            .unwrap();
        let ids: Vec<&str> = actions
            .iter()
            .filter_map(|a| {
                a.as_dictionary()?
                    .get("WFWorkflowActionIdentifier")?
                    .as_string()
            })
            .collect();
        assert_eq!(
            ids,
            [
                "is.workflow.actions.delay",
                "is.workflow.actions.speaktext",
                "is.workflow.actions.openurl",
                "is.workflow.actions.runsshscript"
            ]
        );
        let param = |i: usize, key: &str| {
            actions[i]
                .as_dictionary()?
                .get("WFWorkflowActionParameters")?
                .as_dictionary()?
                .get(key)
                .cloned()
        };
        assert_eq!(param(0, "WFDelayTime"), Some(Value::Real(1.5)));
        assert_eq!(param(1, "WFText"), Some(string_value("hi")));
        assert_eq!(param(2, "WFInput"), Some(string_value("https://x")));
        assert_eq!(param(3, "WFSSHPort"), Some(string_value("2222")));
        assert_eq!(
            param(3, "WFSSHAuthenticationType"),
            Some(string_value("Password"))
        );

        // It must survive a binary round trip.
        let mut bytes = Vec::new();
        workflow.to_writer_binary(&mut bytes).unwrap();
        let back = Value::from_reader(Cursor::new(bytes.as_slice())).unwrap();
        assert_eq!(back, workflow);
    }

    #[test]
    fn apple_dates_convert_to_utc() {
        let dt = apple_date(0.0).unwrap();
        assert_eq!(dt.to_rfc3339(), "2001-01-01T00:00:00+00:00");
        let dt = apple_date(792538036.77).unwrap();
        assert_eq!(dt.timestamp(), 792538036 + APPLE_EPOCH);
    }
}
