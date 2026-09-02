//! iCloud: the signed-in account, Drive quota, sync status and logs from
//! `brctl`, and an eviction-aware listing of iCloud Drive that never
//! triggers a download (evicted files are `dataless` in `st_flags` on
//! current macOS, `.name.icloud` placeholders on older releases).
//!
//! `brctl` is Apple's CloudDocs control tool at `/usr/bin/brctl`. Its help
//! documents `status`, `log`, `quota`, `accounts`, `monitor`, `dump`, and
//! `diagnose`; `download` and `evict` are accepted operations that the help
//! omits (an unknown verb is refused with "No such operation", these are
//! not). Everything here is plain subprocess wrapping — no AppleEvents, so no
//! permission dialogs.

use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const BRCTL: &str = "/usr/bin/brctl";
const BRCTL_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// iCloud Drive's on-disk root, relative to `$HOME`.
pub const DRIVE_RELATIVE_ROOT: &str = "Library/Mobile Documents/com~apple~CloudDocs";

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct IcloudService {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct IcloudAccount {
    pub apple_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub services: Vec<IcloudService>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Quota {
    pub remaining_bytes: u64,
    /// Decimal gigabytes (the unit Apple sells storage in), one decimal.
    pub remaining_gb: f64,
    pub account_kind: String,
}

/// One line of `brctl status`: an `item: state` pair when the line has that
/// shape, otherwise the raw line.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StatusRow {
    Item { item: String, state: String },
    Line { line: String },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LogRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    pub message: String,
}

/// Whether an iCloud Drive item's bytes are on this Mac (`local`) or only a
/// `.<name>.icloud` placeholder is (`cloud`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriveState {
    Local,
    Cloud,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DriveEntry {
    /// The real name, with the placeholder's leading dot and `.icloud`
    /// suffix removed.
    pub name: String,
    /// Absolute path the item has (or will have once downloaded).
    pub path: String,
    pub is_dir: bool,
    /// Bytes on disk for local items; the cloud size recorded in the
    /// placeholder for evicted ones (absent when the placeholder does not
    /// say).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub state: DriveState,
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

/// `~/Library/Mobile Documents/com~apple~CloudDocs`.
pub fn drive_root() -> anyhow::Result<PathBuf> {
    Ok(home()?.join(DRIVE_RELATIVE_ROOT))
}

async fn brctl(args: &[&str], timeout: Duration) -> anyhow::Result<String> {
    run_command_with_timeout(BRCTL, args, timeout).await
}

/// The iCloud accounts signed in on this Mac, from the `MobileMeAccounts`
/// defaults domain. Errors with `not configured` when nobody is signed in.
pub async fn account() -> anyhow::Result<Vec<IcloudAccount>> {
    // Older macOS kept the signed-in account in the MobileMeAccounts
    // defaults domain; current macOS leaves that domain empty and the
    // Accounts framework store is the source of truth.
    let from_defaults = run_command_with_timeout(
        "/usr/bin/defaults",
        &["export", "MobileMeAccounts", "-"],
        BRCTL_TIMEOUT,
    )
    .await
    .ok()
    .and_then(|xml| parse_accounts(&xml).ok())
    .unwrap_or_default();
    if !from_defaults.is_empty() {
        return Ok(from_defaults);
    }
    let accounts = accounts_from_store().await?;
    if accounts.is_empty() {
        anyhow::bail!("iCloud is not configured: no account is signed in on this Mac");
    }
    Ok(accounts)
}

/// Apple accounts from the Accounts framework store
/// (`~/Library/Accounts/Accounts4.sqlite`), read-only.
///
/// The join table between accounts and enabled data classes carries
/// schema-generated column names (`Z_2ENABLEDACCOUNTS`,
/// `Z_7ENABLEDDATACLASSES`, numbered per macOS release), so both are
/// discovered from `pragma table_info` rather than hard-coded.
async fn accounts_from_store() -> anyhow::Result<Vec<IcloudAccount>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db = format!("{home}/Library/Accounts/Accounts4.sqlite");
    if tokio::fs::metadata(&db).await.is_err() {
        return Ok(Vec::new());
    }
    let uri = format!("file:{db}?mode=ro&immutable=1");
    let columns = run_command_with_timeout(
        "sqlite3",
        &[&uri, "pragma table_info(Z_2ENABLEDDATACLASSES);"],
        BRCTL_TIMEOUT,
    )
    .await?;
    let (account_col, class_col) = enabled_join_columns(&columns)
        .ok_or_else(|| anyhow::anyhow!("Accounts store has no enabled-dataclass join table"))?;
    let sql = format!(
        "SELECT a.ZUSERNAME AS apple_id, a.ZACCOUNTDESCRIPTION AS description, \
         a.ZACTIVE AS active, a.ZAUTHENTICATED AS authenticated, \
         (SELECT group_concat(hex(d.ZNAME), '|') FROM Z_2ENABLEDDATACLASSES e \
          JOIN ZDATACLASS d ON d.Z_PK = e.{class_col} \
          WHERE e.{account_col} = a.Z_PK) AS enabled \
         FROM ZACCOUNT a JOIN ZACCOUNTTYPE t ON a.ZACCOUNTTYPE = t.Z_PK \
         WHERE t.ZIDENTIFIER = 'com.apple.account.AppleAccount' \
         ORDER BY a.Z_PK;"
    );
    let json = run_command_with_timeout("sqlite3", &["-json", &uri, &sql], BRCTL_TIMEOUT).await?;
    parse_account_rows(&json)
}

/// Pick the account and data-class columns out of `pragma table_info`
/// output for the enabled-dataclasses join table.
pub fn enabled_join_columns(pragma: &str) -> Option<(String, String)> {
    let names: Vec<&str> = pragma
        .lines()
        .filter_map(|line| line.split('|').nth(1))
        .collect();
    let account = names.iter().find(|n| n.ends_with("ACCOUNTS"))?;
    let class = names.iter().find(|n| n.ends_with("DATACLASSES"))?;
    Some((account.to_string(), class.to_string()))
}

/// Map `sqlite3 -json` rows from the Accounts store into accounts. Only
/// active, authenticated Apple accounts count as signed in. Data-class
/// names are stored as NSKeyedArchiver blobs (an archived string such as
/// `com.apple.Dataclass.Calendars`), so the query hexes them and this
/// decodes each one; the last dotted component becomes the service name.
pub fn parse_account_rows(json: &str) -> anyhow::Result<Vec<IcloudAccount>> {
    parse_account_rows_with(json, decode_dataclass_name)
}

fn decode_dataclass_name(hex: &str) -> Option<String> {
    let bytes = super::keyed_archive::hex_decode(hex).ok()?;
    let value = super::keyed_archive::decode_bytes(&bytes).ok()?;
    value.as_str().map(str::to_string)
}

/// `parse_account_rows` with the blob decoder injected, so tests need no
/// real archives.
pub fn parse_account_rows_with(
    json: &str,
    decode: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Vec<IcloudAccount>> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<serde_json::Value> = serde_json::from_str(json)?;
    Ok(rows
        .iter()
        .filter(|r| {
            r["active"].as_i64().unwrap_or(0) == 1 && r["authenticated"].as_i64().unwrap_or(0) == 1
        })
        .filter_map(|r| {
            let apple_id = r["apple_id"].as_str()?.to_string();
            let display_name = r["description"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(str::to_string);
            let services = r["enabled"]
                .as_str()
                .unwrap_or("")
                .split('|')
                .filter(|s| !s.is_empty())
                .filter_map(&decode)
                .map(|id| IcloudService {
                    name: id.rsplit('.').next().unwrap_or(&id).to_string(),
                    enabled: true,
                })
                .collect();
            Some(IcloudAccount {
                apple_id,
                display_name,
                services,
            })
        })
        .collect())
}

/// Parse the XML plist `defaults export MobileMeAccounts -` prints.
pub fn parse_accounts(xml: &str) -> anyhow::Result<Vec<IcloudAccount>> {
    let value = plist::Value::from_reader_xml(xml.as_bytes())?;
    let Some(accounts) = value
        .as_dictionary()
        .and_then(|d| d.get("Accounts"))
        .and_then(|a| a.as_array())
    else {
        return Ok(Vec::new());
    };
    Ok(accounts
        .iter()
        .filter_map(|account| {
            let dict = account.as_dictionary()?;
            let string = |key: &str| {
                dict.get(key)
                    .and_then(|v| v.as_string())
                    .map(str::to_string)
            };
            let apple_id = string("AccountID")?;
            let display_name = string("DisplayName").or_else(|| string("AccountDescription"));
            let services = dict
                .get("Services")
                .and_then(|s| s.as_array())
                .into_iter()
                .flatten()
                .filter_map(|service| {
                    let service = service.as_dictionary()?;
                    let name = service.get("Name")?.as_string()?.to_string();
                    let enabled = service
                        .get("Enabled")
                        .and_then(|e| e.as_boolean())
                        .or_else(|| {
                            service
                                .get("status")
                                .and_then(|s| s.as_string())
                                .map(|s| s.eq_ignore_ascii_case("active"))
                        })
                        .unwrap_or(false);
                    Some(IcloudService { name, enabled })
                })
                .collect();
            Some(IcloudAccount {
                apple_id,
                display_name,
                services,
            })
        })
        .collect())
}

/// Remaining iCloud storage, from `brctl quota`.
pub async fn quota() -> anyhow::Result<Quota> {
    let output = brctl(&["quota"], BRCTL_TIMEOUT).await?;
    parse_quota(&output)
}

/// Parse `1984007400359 bytes of quota remaining in personal account`.
pub fn parse_quota(output: &str) -> anyhow::Result<Quota> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|l| l.contains("quota"))
        .unwrap_or_else(|| output.trim());
    let words: Vec<&str> = line.split_whitespace().collect();
    let remaining_bytes = words
        .iter()
        .find_map(|w| w.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("could not parse brctl quota output: {line:?}"))?;
    let account_kind = words
        .windows(2)
        .find(|pair| pair[1] == "account")
        .map(|pair| pair[0].to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok(Quota {
        remaining_bytes,
        remaining_gb: (remaining_bytes as f64 / 1e8).round() / 10.0,
        account_kind,
    })
}

/// Items `brctl status` reports as not fully synced, optionally for one
/// container (e.g. `com.apple.CloudDocs`).
pub async fn status(container: Option<&str>) -> anyhow::Result<Vec<StatusRow>> {
    let mut args = vec!["status"];
    if let Some(container) = container {
        args.push(container);
    }
    // `brctl status` prints its report and then stays attached to the
    // daemon instead of exiting, so it cannot be run to completion: capture
    // until the output goes quiet, then kill it.
    let output = capture_until_idle(&args, BRCTL_TIMEOUT, Duration::from_millis(750)).await?;
    Ok(parse_status(&output))
}

/// Run `brctl` and collect stdout until it has been silent for `idle` (after
/// producing something) or `overall` has elapsed, then kill it. For verbs
/// that never exit on their own.
async fn capture_until_idle(
    args: &[&str],
    overall: Duration,
    idle: Duration,
) -> anyhow::Result<String> {
    use tokio::io::AsyncReadExt;

    let mut child = tokio::process::Command::new(BRCTL)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("brctl stdout was not captured"))?;
    let deadline = tokio::time::Instant::now() + overall;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let wait = if buf.is_empty() {
            remaining
        } else {
            idle.min(remaining)
        };
        match tokio::time::timeout(wait, stdout.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => break,
        }
    }
    child.start_kill().ok();
    let _ = child.wait().await;
    if buf.is_empty() {
        anyhow::bail!("brctl {} produced no output within {overall:?}", args[0]);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Strip SGR escape sequences (`ESC [ ... m`) that brctl colors its output with.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse_status(output: &str) -> Vec<StatusRow> {
    output
        .lines()
        .map(|line| strip_ansi(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .map(|line| {
            // `<container-id state {details}>` container summary lines.
            if let Some(inner) = line.strip_prefix('<').and_then(|l| l.strip_suffix('>')) {
                if let Some((item, state)) = inner.split_once(' ') {
                    return StatusRow::Item {
                        item: item.to_string(),
                        state: state.trim().to_string(),
                    };
                }
            }
            if let Some((item, state)) = line.split_once(": ") {
                return StatusRow::Item {
                    item: item.to_string(),
                    state: state.trim().to_string(),
                };
            }
            StatusRow::Line { line }
        })
        .collect()
}

/// CloudDocs log lines from the last `minutes`, via `brctl log -b -z --last <n>m`.
pub async fn log(minutes: u64) -> anyhow::Result<Vec<LogRow>> {
    let last = format!("{}m", minutes.max(1));
    let output = brctl(&["log", "-b", "-z", "--last", &last], BRCTL_TIMEOUT).await?;
    Ok(parse_log(&output))
}

/// Parse `[note 2026-09-02 00:43:20.807-0700] bird[1252]  message` lines.
pub fn parse_log(output: &str) -> Vec<LogRow> {
    output
        .lines()
        .map(|line| strip_ansi(line).trim_end().to_string())
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let Some((header, rest)) = line.strip_prefix('[').and_then(|l| l.split_once("] "))
            else {
                return LogRow {
                    timestamp: None,
                    level: None,
                    process: None,
                    message: line.trim().to_string(),
                };
            };
            let mut header = header.split_whitespace();
            let level = header.next().map(str::to_string);
            let timestamp = header.collect::<Vec<_>>().join(" ");
            let timestamp = (!timestamp.is_empty()).then_some(timestamp);
            let rest = rest.trim_start();
            let (process, message) = match rest.split_once("  ") {
                Some((process, message)) => (Some(process.to_string()), message.trim().to_string()),
                None => (None, rest.to_string()),
            };
            LogRow {
                timestamp,
                level,
                process,
                message,
            }
        })
        .collect()
}

/// The real name behind a `.<name>.icloud` placeholder, if `file_name` is one.
pub fn placeholder_target(file_name: &str) -> Option<&str> {
    file_name
        .strip_prefix('.')
        .and_then(|n| n.strip_suffix(".icloud"))
        .filter(|n| !n.is_empty())
}

/// The placeholder path iCloud uses when `path` is evicted.
fn placeholder_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}.icloud")))
}

/// Resolve a user-supplied path against iCloud Drive, refusing anything
/// outside it. Relative paths are taken from the Drive root; absolute (and
/// `~/`) paths must already lie under it. `..` is rejected outright rather
/// than normalized, so the check cannot be talked out of the root.
pub fn resolve_drive_path(root: &Path, input: &str) -> anyhow::Result<PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        anyhow::bail!("invalid path: empty");
    }
    let candidate = if let Some(rest) = input.strip_prefix("~/") {
        home()?.join(rest)
    } else {
        PathBuf::from(input)
    };
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        anyhow::bail!("invalid path: {input:?} uses `..`, which is not allowed");
    }
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    };
    if !resolved.starts_with(root) {
        anyhow::bail!(
            "invalid path: {input:?} is outside iCloud Drive ({})",
            root.display()
        );
    }
    Ok(resolved)
}

/// List one folder of iCloud Drive (or the whole tree with `recursive`),
/// reporting each item as `local` or `cloud` from its placeholder without
/// touching its contents, so nothing is downloaded.
pub async fn list(
    folder: Option<&str>,
    state: Option<DriveState>,
    recursive: bool,
) -> anyhow::Result<Vec<DriveEntry>> {
    let root = drive_root()?;
    if !root.is_dir() {
        anyhow::bail!(
            "iCloud Drive is not configured: {} is not present",
            root.display()
        );
    }
    let start = match folder {
        Some(folder) => resolve_drive_path(&root, folder)?,
        None => root,
    };
    if !start.is_dir() {
        anyhow::bail!("folder not found in iCloud Drive: {}", start.display());
    }
    let mut entries = list_dir(&start, recursive)?;
    if let Some(state) = state {
        entries.retain(|e| e.state == state);
    }
    Ok(entries)
}

/// Walk `dir` with `std::fs`; the tree is local metadata only. With
/// "Desktop & Documents" sync on, the Drive root holds `Desktop` and
/// `Documents` as symlinks into `$HOME`, so directory symlinks count as
/// folders and are followed when recursing (each real directory once).
pub fn list_dir(dir: &Path, recursive: bool) -> anyhow::Result<Vec<DriveEntry>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = pending.pop() {
        if let Ok(canonical) = std::fs::canonicalize(&dir) {
            if !visited.insert(canonical) {
                continue;
            }
        }
        let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        names.sort();
        for path in names {
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if let Some(real) = placeholder_target(file_name) {
                let real_path = path.with_file_name(real);
                out.push(DriveEntry {
                    name: real.to_string(),
                    path: real_path.to_string_lossy().into_owned(),
                    is_dir: false,
                    size: placeholder_size(&path),
                    state: DriveState::Cloud,
                });
                continue;
            }
            if file_name.starts_with('.') {
                continue;
            }
            let is_dir = meta.is_dir() || (meta.file_type().is_symlink() && path.is_dir());
            out.push(DriveEntry {
                name: file_name.to_string(),
                path: path.to_string_lossy().into_owned(),
                is_dir,
                size: meta.is_file().then_some(meta.len()),
                state: state_from_flags(meta_flags(&meta)),
            });
            if is_dir && recursive {
                pending.push(path);
            }
        }
    }
    Ok(out)
}

/// `SF_DATALESS` in `st_flags`: the file's bytes live only in iCloud.
/// Current macOS evicts by setting this flag and keeping the name and size
/// (no `.name.icloud` placeholder); reading the file would trigger a
/// download, `stat` does not.
const SF_DATALESS: u32 = 0x4000_0000;

fn meta_flags(meta: &std::fs::Metadata) -> u32 {
    use std::os::macos::fs::MetadataExt;
    meta.st_flags()
}

/// Evicted (dataless) files are `cloud`; everything else is `local`.
pub fn state_from_flags(flags: u32) -> DriveState {
    if flags & SF_DATALESS != 0 {
        DriveState::Cloud
    } else {
        DriveState::Local
    }
}

/// The cloud size a placeholder records (`NSURLFileSizeKey`), when readable.
fn placeholder_size(placeholder: &Path) -> Option<u64> {
    let value = plist::Value::from_file(placeholder).ok()?;
    value
        .as_dictionary()?
        .get("NSURLFileSizeKey")
        .and_then(|v| {
            v.as_unsigned_integer()
                .or_else(|| v.as_signed_integer().map(|i| i as u64))
        })
}

/// Ask iCloud to bring `path` onto this Mac (`brctl download`).
pub async fn download(path: &str) -> anyhow::Result<ActionResult> {
    let target = resolve_drive_path(&drive_root()?, path)?;
    let present = target.exists() || placeholder_path(&target).is_some_and(|p| p.exists());
    if !present {
        anyhow::bail!("{} not found in iCloud Drive", target.display());
    }
    let target_str = target.to_string_lossy().into_owned();
    brctl(&["download", &target_str], DOWNLOAD_TIMEOUT).await?;
    Ok(ActionResult::success_with_message(
        "download",
        &format!("Requested download of {target_str}"),
    ))
}

/// Remove the local copy of `path`, keeping it in iCloud (`brctl evict`).
pub async fn evict(path: &str) -> anyhow::Result<ActionResult> {
    let target = resolve_drive_path(&drive_root()?, path)?;
    if !target.exists() {
        anyhow::bail!("{} not found in iCloud Drive", target.display());
    }
    let target_str = target.to_string_lossy().into_owned();
    brctl(&["evict", &target_str], BRCTL_TIMEOUT).await?;
    Ok(ActionResult::success_with_message(
        "evict",
        &format!("Evicted local copy of {target_str}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataless_flag_means_cloud() {
        assert_eq!(state_from_flags(0), DriveState::Local);
        assert_eq!(state_from_flags(SF_DATALESS), DriveState::Cloud);
        // compressed + dataless, as seen on a real evicted file
        assert_eq!(state_from_flags(0x20 | SF_DATALESS), DriveState::Cloud);
    }

    #[test]
    fn enabled_join_columns_are_discovered_from_pragma() {
        let pragma = "0|Z_2ENABLEDACCOUNTS|INTEGER|0||0\n1|Z_7ENABLEDDATACLASSES|INTEGER|0||0\n";
        assert_eq!(
            enabled_join_columns(pragma),
            Some(("Z_2ENABLEDACCOUNTS".into(), "Z_7ENABLEDDATACLASSES".into()))
        );
        assert_eq!(enabled_join_columns(""), None);
    }

    #[test]
    fn account_rows_keep_only_signed_in_apple_accounts() {
        let json = r#"[
          {"apple_id":"a@example.com","description":"iCloud","active":1,"authenticated":1,
           "enabled":"CAFE|BEEF"},
          {"apple_id":"old@example.com","description":"","active":0,"authenticated":1,"enabled":null}
        ]"#;
        let decode = |hex: &str| match hex {
            "CAFE" => Some("com.apple.Dataclass.Calendars".to_string()),
            "BEEF" => Some("com.apple.Dataclass.Reminders".to_string()),
            _ => None,
        };
        let accounts = parse_account_rows_with(json, decode).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].apple_id, "a@example.com");
        assert_eq!(accounts[0].display_name.as_deref(), Some("iCloud"));
        let names: Vec<&str> = accounts[0]
            .services
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["Calendars", "Reminders"]);
        assert!(accounts[0].services.iter().all(|s| s.enabled));
        assert!(parse_account_rows("").unwrap().is_empty());
    }

    #[test]
    fn dataclass_blob_decodes_to_its_identifier() {
        // bplist00 NSKeyedArchiver with a single string root, as the store holds it.
        let mut value = plist::Dictionary::new();
        value.insert(
            "$archiver".into(),
            plist::Value::String("NSKeyedArchiver".into()),
        );
        value.insert(
            "$objects".into(),
            plist::Value::Array(vec![
                plist::Value::String("$null".into()),
                plist::Value::String("com.apple.Dataclass.Calendars".into()),
            ]),
        );
        let mut top = plist::Dictionary::new();
        top.insert("root".into(), plist::Value::Uid(plist::Uid::new(1)));
        value.insert("$top".into(), plist::Value::Dictionary(top));
        value.insert("$version".into(), plist::Value::Integer(100000.into()));
        let mut bytes = Vec::new();
        plist::to_writer_binary(&mut bytes, &plist::Value::Dictionary(value)).unwrap();
        let hex = super::super::keyed_archive::hex_encode(&bytes);
        assert_eq!(
            decode_dataclass_name(&hex).as_deref(),
            Some("com.apple.Dataclass.Calendars")
        );
    }

    #[test]
    fn quota_line_parses_bytes_gb_and_kind() {
        let quota =
            parse_quota("1984007400359 bytes of quota remaining in personal account\n").unwrap();
        assert_eq!(quota.remaining_bytes, 1_984_007_400_359);
        assert_eq!(quota.remaining_gb, 1984.0);
        assert_eq!(quota.account_kind, "personal");

        let small = parse_quota("4550000000 bytes of quota remaining in family account").unwrap();
        assert_eq!(small.remaining_gb, 4.6);
        assert_eq!(small.account_kind, "family");

        assert!(parse_quota("no quota here").is_err());
    }

    #[test]
    fn status_lines_split_into_item_state_or_raw() {
        let fixture = "1 containers matching '*'\n\
            <c{1}m.a{3}e.C{7}s[1] \u{1b}[0;1;32mforeground\u{1b}[0m {client:idle server:full-sync}>\n\
            \n\
            Desktop & Documents: current=YES lastEnabled=(never) lastDisabled=(never)\n";
        let rows = parse_status(fixture);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            StatusRow::Line {
                line: "1 containers matching '*'".into()
            }
        );
        assert_eq!(
            rows[1],
            StatusRow::Item {
                item: "c{1}m.a{3}e.C{7}s[1]".into(),
                state: "foreground {client:idle server:full-sync}".into()
            }
        );
        assert_eq!(
            rows[2],
            StatusRow::Item {
                item: "Desktop & Documents".into(),
                state: "current=YES lastEnabled=(never) lastDisabled=(never)".into()
            }
        );
        let json = serde_json::to_value(&rows).unwrap();
        assert_eq!(json[2]["item"], "Desktop & Documents");
        assert_eq!(json[0]["line"], "1 containers matching '*'");
    }

    #[test]
    fn log_lines_split_into_level_timestamp_process_message() {
        let fixture = "[WARN 2026-09-02 00:43:20.787-0700] bird[1252]  UploadV1 tracker has no available spots\n\
            [note 2026-09-02 00:43:21.938-0700] bird[1252]  received a push for client zone <private>\n\
            plain continuation line\n";
        let rows = parse_log(fixture);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            LogRow {
                timestamp: Some("2026-09-02 00:43:20.787-0700".into()),
                level: Some("WARN".into()),
                process: Some("bird[1252]".into()),
                message: "UploadV1 tracker has no available spots".into(),
            }
        );
        assert_eq!(rows[1].level.as_deref(), Some("note"));
        assert_eq!(
            rows[2],
            LogRow {
                timestamp: None,
                level: None,
                process: None,
                message: "plain continuation line".into(),
            }
        );
    }

    #[test]
    fn placeholder_names_map_to_real_names() {
        assert_eq!(placeholder_target(".foo.pdf.icloud"), Some("foo.pdf"));
        assert_eq!(
            placeholder_target(".archive.tar.gz.icloud"),
            Some("archive.tar.gz")
        );
        assert_eq!(placeholder_target("foo.pdf"), None);
        assert_eq!(placeholder_target(".DS_Store"), None);
        assert_eq!(placeholder_target("foo.icloud"), None, "no leading dot");
        assert_eq!(placeholder_target("..icloud"), None, "empty name");
    }

    #[test]
    fn listing_reports_cloud_placeholders_by_real_name_without_reading_them() {
        let dir = std::env::temp_dir().join(format!("cider-icloud-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::write(dir.join("local.txt"), b"hello").unwrap();
        std::fs::write(dir.join(".evicted.pdf.icloud"), b"not a plist").unwrap();
        std::fs::write(dir.join(".DS_Store"), b"").unwrap();
        std::fs::write(dir.join("Sub/nested.txt"), b"hi").unwrap();
        // The Drive root links Desktop/Documents into $HOME the same way.
        std::os::unix::fs::symlink(dir.join("Sub"), dir.join("Linked")).unwrap();

        let shallow = list_dir(&dir, false).unwrap();
        let names: Vec<&str> = shallow.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["evicted.pdf", "Linked", "Sub", "local.txt"]);
        let linked = shallow.iter().find(|e| e.name == "Linked").unwrap();
        assert!(linked.is_dir, "a symlink to a directory is a folder");
        assert!(linked.size.is_none());
        let shallow: Vec<DriveEntry> = shallow.into_iter().filter(|e| e.name != "Linked").collect();
        let evicted = &shallow[0];
        assert_eq!(evicted.state, DriveState::Cloud);
        assert!(!evicted.is_dir);
        assert!(evicted.size.is_none(), "unreadable placeholder has no size");
        assert!(evicted.path.ends_with("/evicted.pdf"), "{}", evicted.path);
        assert_eq!(shallow[1].state, DriveState::Local);
        assert!(shallow[1].is_dir);
        assert_eq!(shallow[2].size, Some(5));
        assert_eq!(shallow[2].state, DriveState::Local);

        let deep = list_dir(&dir, true).unwrap();
        assert_eq!(
            deep.iter().filter(|e| e.name == "nested.txt").count(),
            1,
            "a directory reached through a symlink is walked once"
        );
        assert_eq!(
            serde_json::to_value(&deep[0]).unwrap()["state"],
            "cloud",
            "state serializes lowercase"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn placeholder_size_comes_from_the_plist() {
        let dir = std::env::temp_dir().join(format!("cider-icloud-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let placeholder = dir.join(".big.mov.icloud");
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "NSURLFileSizeKey".into(),
            plist::Value::Integer(123_456.into()),
        );
        dict.insert(
            "NSURLNameKey".into(),
            plist::Value::String("big.mov".into()),
        );
        plist::Value::Dictionary(dict)
            .to_file_binary(&placeholder)
            .unwrap();
        let rows = list_dir(&dir, false).unwrap();
        assert_eq!(rows[0].name, "big.mov");
        assert_eq!(rows[0].size, Some(123_456));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn path_guard_keeps_targets_inside_icloud_drive() {
        let root = Path::new("/Users/me/Library/Mobile Documents/com~apple~CloudDocs");
        assert_eq!(
            resolve_drive_path(root, "Documents/a.txt").unwrap(),
            root.join("Documents/a.txt")
        );
        assert_eq!(
            resolve_drive_path(
                root,
                "/Users/me/Library/Mobile Documents/com~apple~CloudDocs/Desktop"
            )
            .unwrap(),
            root.join("Desktop")
        );
        for bad in [
            "/Users/me/Documents/a.txt",
            "../../Documents/a.txt",
            "Documents/../../x",
            "/Users/me/Library/Mobile Documents/com~apple~CloudDocs/../other",
            "",
        ] {
            let err = resolve_drive_path(root, bad).expect_err(bad).to_string();
            assert!(err.starts_with("invalid path"), "{bad}: {err}");
        }
        // A sibling that merely shares the root as a string prefix is outside.
        let err = resolve_drive_path(
            root,
            "/Users/me/Library/Mobile Documents/com~apple~CloudDocs-evil/x",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("outside iCloud Drive"));
    }

    #[test]
    fn accounts_parse_from_the_defaults_plist() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Accounts</key><array><dict>
    <key>AccountID</key><string>someone@icloud.com</string>
    <key>DisplayName</key><string>Someone</string>
    <key>Services</key><array>
      <dict><key>Name</key><string>MOBILE_DOCUMENTS</string><key>Enabled</key><true/></dict>
      <dict><key>Name</key><string>CLOUDDESKTOP</string><key>status</key><string>active</string></dict>
      <dict><key>Name</key><string>PHOTO_STREAM</string><key>Enabled</key><false/></dict>
    </array>
  </dict></array>
</dict></plist>"#;
        let accounts = parse_accounts(xml).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].apple_id, "someone@icloud.com");
        assert_eq!(accounts[0].display_name.as_deref(), Some("Someone"));
        assert_eq!(
            accounts[0].services,
            vec![
                IcloudService {
                    name: "MOBILE_DOCUMENTS".into(),
                    enabled: true
                },
                IcloudService {
                    name: "CLOUDDESKTOP".into(),
                    enabled: true
                },
                IcloudService {
                    name: "PHOTO_STREAM".into(),
                    enabled: false
                },
            ]
        );

        let empty = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict/></plist>"#;
        assert!(parse_accounts(empty).unwrap().is_empty());
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[0;1;32mok\u{1b}[0m done"), "ok done");
        assert_eq!(strip_ansi("plain"), "plain");
    }
}
