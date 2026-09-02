use std::io::Cursor;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plist::{Dictionary, Value};
use serde::Serialize;
use tokio::process::Command;

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

    #[test]
    fn apple_dates_convert_to_utc() {
        let dt = apple_date(0.0).unwrap();
        assert_eq!(dt.to_rfc3339(), "2001-01-01T00:00:00+00:00");
        let dt = apple_date(792538036.77).unwrap();
        assert_eq!(dt.timestamp(), 792538036 + APPLE_EPOCH);
    }
}
