//! Every macOS permission cider can need, with its state for this launcher.
//!
//! macOS attributes a command-line tool's privacy access to its *responsible
//! process*: the app that launched it (Terminal, iTerm, Claude, Alchemy…).
//! That app is what appears in System Settings › Privacy & Security, and its
//! Info.plist decides whether a prompt can appear at all. Everything in this
//! module is prompt-free: it opens files for reading, runs `ps`, and pings a
//! bridge that is already running. Opening a file never prompts; only
//! AppleEvents and framework APIs do, and none are sent here.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{bridge, bridge_cli};

/// Info.plist keys a host app must declare before macOS will show its
/// prompts for the access cider asks for on its behalf: (key, why). A host
/// that never declares them is never asked, never appears in System
/// Settings, and is silently denied forever.
pub const HOST_INFO_PLIST_KEYS: &[(&str, &str)] = &[
    (
        "NSCalendarsFullAccessUsageDescription",
        "Calendar reads and writes through EventKit (cider-bridge); Full Access, not Add Only",
    ),
    (
        "NSRemindersFullAccessUsageDescription",
        "Reminders reads and writes through EventKit (cider-bridge)",
    ),
    (
        "NSContactsUsageDescription",
        "Contacts reads and writes through the Contacts framework (cider-bridge)",
    ),
    (
        "NSAppleEventsUsageDescription",
        "Automation: every AppleScript/JXA call (Notes, Music, Mail, Messages, Safari tabs, \
         Calendar/Reminders/Contacts fallbacks, Shortcuts)",
    ),
    (
        "NSLocationWhenInUseUsageDescription",
        "Location Services, for a future `cider location` command; optional today",
    ),
];

/// One macOS privacy gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "name")]
pub enum Permission {
    /// Privacy & Security › Full Disk Access: the on-disk stores cider reads
    /// directly with sqlite3 or as plists.
    FullDiskAccess,
    Calendars,
    Reminders,
    Contacts,
    /// Privacy & Security › Automation: AppleEvents to one target app,
    /// granted per (launching app → target app) pair.
    Automation {
        target: String,
    },
    HomeKit,
    Location,
}

impl Permission {
    /// The `name` as it serializes: `full_disk_access`, `automation`, …
    pub fn name(&self) -> &'static str {
        match self {
            Permission::FullDiskAccess => "full_disk_access",
            Permission::Calendars => "calendars",
            Permission::Reminders => "reminders",
            Permission::Contacts => "contacts",
            Permission::Automation { .. } => "automation",
            Permission::HomeKit => "home_kit",
            Permission::Location => "location",
        }
    }

    /// One-cell label: the name, with the target for Automation.
    pub fn label(&self) -> String {
        match self {
            Permission::Automation { target } => format!("automation ({target})"),
            other => other.name().to_string(),
        }
    }
}

/// Who macOS records the grant against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum GrantedTo {
    /// The responsible process: the app that launched cider (Terminal, iTerm,
    /// an agent runner, the host app linking the crate).
    LaunchingApp,
    /// Cider Bridge.app itself; it is an app, so it is its own subject.
    CiderBridgeApp,
    /// An Automation pair: the launching app, allowed to control `target`.
    HostAppPair { target: String },
}

impl GrantedTo {
    pub fn label(&self) -> String {
        match self {
            GrantedTo::LaunchingApp => "launching app".to_string(),
            GrantedTo::CiderBridgeApp => "Cider Bridge.app".to_string(),
            GrantedTo::HostAppPair { target } => format!("launching app → {target}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Ok,
    Denied,
    NotDetermined,
    /// Calendar granted as "Add Only" (EventKit `write_only`): writes land,
    /// every read comes back empty.
    AddOnly,
    /// Cannot be observed without risking a prompt, or the thing that would
    /// report it (a bridge) is not running.
    NotProbed,
    /// Nothing on this Mac needs it: the store does not exist, or no command
    /// uses the permission yet.
    NotApplicable,
}

impl PermissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionStatus::Ok => "ok",
            PermissionStatus::Denied => "denied",
            PermissionStatus::NotDetermined => "not_determined",
            PermissionStatus::AddOnly => "add_only",
            PermissionStatus::NotProbed => "not_probed",
            PermissionStatus::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub permission: Permission,
    /// Source names, optionally with the verb that needs it: `messages`,
    /// `shortcuts run`, `calendar (JXA fallback)`.
    pub needed_by: Vec<String>,
    pub granted_to: GrantedTo,
    pub status: PermissionStatus,
    /// What was observed and how.
    pub detail: String,
    /// The exact System Settings pane and who to grant it to.
    pub how_to_grant: String,
    /// Info.plist keys the launching or host app must declare before macOS
    /// will show the prompt. Empty when no prompt exists (Full Disk Access)
    /// or the prompt belongs to another app (HomeKit → Cider Bridge.app).
    pub host_app_requirements: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub ok: usize,
    /// `denied` plus `add_only`: both refuse what cider asks for.
    pub denied: usize,
    pub not_determined: usize,
    pub not_probed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsReport {
    /// Always true: nothing in [`report`] can open a dialog.
    pub prompt_free: bool,
    /// Best effort: the first `.app` up the parent-process chain, else
    /// `$TERM_PROGRAM`. This is the app the grants belong to.
    pub responsible_process: Option<String>,
    pub requirements: Vec<Requirement>,
    pub summary: Summary,
}

/// One flat row per requirement, for a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub permission: String,
    pub status: &'static str,
    pub granted_to: String,
    pub needed_by: String,
    pub detail: String,
}

impl PermissionsReport {
    /// Only the requirements `source` needs (`calendar`, `messages`,
    /// `shortcuts`…), matched on the first word of each `needed_by` entry.
    pub fn for_source(&self, source: &str) -> PermissionsReport {
        let source = source.trim().to_ascii_lowercase().replace('_', "-");
        let requirements: Vec<Requirement> = self
            .requirements
            .iter()
            .filter(|requirement| {
                requirement
                    .needed_by
                    .iter()
                    .any(|entry| entry.split_whitespace().next() == Some(source.as_str()))
            })
            .cloned()
            .collect();
        PermissionsReport {
            prompt_free: self.prompt_free,
            responsible_process: self.responsible_process.clone(),
            summary: summarize(&requirements),
            requirements,
        }
    }

    /// The requirements flattened to one row each, for `--pretty`.
    pub fn rows(&self) -> Vec<Row> {
        self.requirements
            .iter()
            .map(|requirement| Row {
                permission: requirement.permission.label(),
                status: requirement.status.as_str(),
                granted_to: requirement.granted_to.label(),
                needed_by: requirement.needed_by.join(", "),
                detail: requirement.detail.clone(),
            })
            .collect()
    }
}

pub fn summarize(requirements: &[Requirement]) -> Summary {
    let mut summary = Summary::default();
    for requirement in requirements {
        match requirement.status {
            PermissionStatus::Ok => summary.ok += 1,
            PermissionStatus::Denied | PermissionStatus::AddOnly => summary.denied += 1,
            PermissionStatus::NotDetermined => summary.not_determined += 1,
            PermissionStatus::NotProbed => summary.not_probed += 1,
            PermissionStatus::NotApplicable => {}
        }
    }
    summary
}

/// Every permission cider can need, with its state for the app that
/// launched this process. Prompt-free.
///
/// macOS attributes a command-line tool's privacy access to its
/// **responsible process** — the app that launched it: Terminal, iTerm, an
/// agent runner, or the host app that links this crate. The grant is
/// recorded against that app, the prompt is shown only if that app's
/// Info.plist carries the matching usage string ([`HOST_INFO_PLIST_KEYS`]),
/// and an app that never asked never appears in System Settings, so the
/// user cannot pre-grant it. A host app embedding cider therefore has to
/// ship those keys; without them Calendar, Reminders, Contacts, and
/// Automation are silently denied. Full Disk Access has no prompt at all:
/// the user adds the app by hand.
///
/// What is probed: Full Disk Access by opening the Messages and Safari
/// stores for reading (opening never prompts); Calendars, Reminders, and
/// Contacts from `cider-bridge ping` when the CLI is installed (it reads
/// authorization status, never requests it), else by opening each store's
/// database; HomeKit from a Cider Bridge that is already running (never
/// launched). Automation cannot be observed without sending an AppleEvent,
/// which can itself prompt, so it is always `not_probed`.
pub async fn report() -> PermissionsReport {
    let home = home_dir();
    let responsible_process = responsible_process().await;
    let store_auth = store_authorization().await;

    let mut requirements = vec![full_disk_access(&home).await];
    requirements.extend(event_stores(&home, store_auth.as_ref()).await);
    requirements.extend(automation());
    requirements.push(home_kit().await);
    requirements.push(location());

    PermissionsReport {
        prompt_free: true,
        responsible_process,
        summary: summarize(&requirements),
        requirements,
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

const LAUNCHER_RULE: &str = "grant it to the app that launches cider (e.g. Terminal, iTerm, or \
                             the host app such as Alchemy), not to cider itself";

// ---------------------------------------------------------------- probes

/// Open `path` for reading without prompting. A directory is listed instead
/// of opened. `metadata` succeeds on a TCC-protected file even when `open`
/// fails with EPERM, so this must actually open.
pub fn probe_read(path: &Path) -> PermissionStatus {
    let result = match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::read_dir(path).map(|_| ()),
        Ok(_) => std::fs::File::open(path).map(|_| ()),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => PermissionStatus::Ok,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            PermissionStatus::Denied
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            PermissionStatus::NotApplicable
        }
        Err(_) => PermissionStatus::NotProbed,
    }
}

/// EventKit/Contacts status words as `cider-bridge ping` reports them.
pub fn status_from_ping(word: &str) -> PermissionStatus {
    match word {
        "full_access" | "authorized" => PermissionStatus::Ok,
        "write_only" | "add_only" => PermissionStatus::AddOnly,
        "denied" | "restricted" | "limited" => PermissionStatus::Denied,
        "not_determined" => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotProbed,
    }
}

/// The app the grants belong to: the first `.app` bundle up the
/// parent-process chain, else `$TERM_PROGRAM`. Pure.
pub fn responsible_process_from<'a>(
    chain: impl IntoIterator<Item = &'a str>,
    term_program: Option<&str>,
) -> Option<String> {
    for executable in chain {
        if let Some(app) = app_name(executable) {
            return Some(app);
        }
    }
    term_program
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| match name {
            "Apple_Terminal" => "Terminal".to_string(),
            other => other.strip_suffix(".app").unwrap_or(other).to_string(),
        })
}

fn app_name(executable: &str) -> Option<String> {
    Path::new(executable)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .find_map(|component| component.strip_suffix(".app").map(str::to_string))
}

async fn responsible_process() -> Option<String> {
    let chain = process_chain().await;
    responsible_process_from(
        chain.iter().map(String::as_str),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
    )
}

/// Executables of this process's ancestors, nearest first, via `ps`.
async fn process_chain() -> Vec<String> {
    let mut chain = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..24 {
        let Some((parent, executable)) = parent_of(pid).await else {
            break;
        };
        chain.push(executable);
        if parent <= 1 {
            break;
        }
        pid = parent;
    }
    chain
}

async fn parent_of(pid: u32) -> Option<(u32, String)> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::process::Command::new("/bin/ps")
            .args(["-o", "ppid=,comm=", "-p", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ps_line(&String::from_utf8_lossy(&output.stdout))
}

/// `"  512 /Applications/iTerm.app/Contents/MacOS/iTerm2"` → (512, path).
/// The executable may contain spaces (`Cider Bridge.app`). Pure.
pub fn parse_ps_line(line: &str) -> Option<(u32, String)> {
    let line = line.trim();
    let (ppid, executable) = line.split_once(char::is_whitespace)?;
    Some((ppid.parse().ok()?, executable.trim().to_string()))
}

// ---------------------------------------------------------- requirements

/// (source, path under $HOME) for the stores cider reads directly.
/// Every one is under ~/Library, which Full Disk Access gates.
pub const FULL_DISK_ACCESS_STORES: &[(&str, &str)] = &[
    ("messages", "Library/Messages/chat.db"),
    ("mail", "Library/Mail/V*/MailData/Envelope Index"),
    ("safari", "Library/Safari/History.db"),
    ("safari", "Library/Safari/Bookmarks.plist"),
    ("reading-list", "Library/Safari/Bookmarks.plist"),
    (
        "photos",
        "Pictures/Photos Library.photoslibrary/database/Photos.sqlite",
    ),
    (
        "books",
        "Library/Containers/com.apple.iBooksX/Data/Documents/BKLibrary",
    ),
    (
        "voice-memos",
        "Library/Group Containers/group.com.apple.VoiceMemos*/Recordings/CloudRecordings.db",
    ),
    (
        "facetime",
        "Library/Application Support/CallHistoryDB/CallHistory.storedata",
    ),
    ("icloud account", "Library/Accounts/Accounts4.sqlite"),
    ("stocks", "Library/Group Containers/group.com.apple.stocks"),
    (
        "calendar (SQLite reads)",
        "Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb",
    ),
    (
        "reminders (SQLite reads)",
        "Library/Group Containers/group.com.apple.reminders/Container_v1/Stores",
    ),
    (
        "contacts (SQLite reads)",
        "Library/Application Support/AddressBook/AddressBook-v22.abcddb",
    ),
    (
        "home (cache reads)",
        "Library/Containers/com.apple.Home/Data/Library/Caches/com.apple.HomeKit",
    ),
    ("shortcuts", "Library/Shortcuts/Shortcuts.sqlite"),
    ("watch", "the stores above, via FSEvents"),
];

async fn full_disk_access(home: &Path) -> Requirement {
    let probes = [
        ("Messages", home.join("Library/Messages/chat.db")),
        ("Safari History", home.join("Library/Safari/History.db")),
    ];
    let mut observed = Vec::new();
    let mut status = PermissionStatus::NotApplicable;
    for (label, path) in &probes {
        let result = probe_read(path);
        observed.push(format!(
            "{label} ({}) {}",
            path.display(),
            match result {
                PermissionStatus::Ok => "opened for reading",
                PermissionStatus::Denied =>
                    "refused (EPERM: Full Disk Access is not granted \
                                             to the launching app)",
                PermissionStatus::NotApplicable => "does not exist",
                _ => "could not be opened",
            }
        ));
        status = worst(status, result);
    }
    let mut needed_by: Vec<String> = FULL_DISK_ACCESS_STORES
        .iter()
        .map(|(source, _)| source.to_string())
        .collect();
    needed_by.dedup();
    Requirement {
        permission: Permission::FullDiskAccess,
        needed_by,
        granted_to: GrantedTo::LaunchingApp,
        status,
        detail: format!(
            "Probed by opening two stores for reading, which never prompts: {}. One grant \
             covers every store cider reads from disk ({} of them under ~/Library)",
            observed.join("; "),
            FULL_DISK_ACCESS_STORES.len() - 1
        ),
        how_to_grant: format!(
            "System Settings › Privacy & Security › Full Disk Access: add the app and turn it \
             on, then relaunch it — {LAUNCHER_RULE}. There is no prompt and no Info.plist key; \
             `sudo` does not bypass it"
        ),
        host_app_requirements: vec![],
    }
}

fn worst(a: PermissionStatus, b: PermissionStatus) -> PermissionStatus {
    fn rank(status: PermissionStatus) -> u8 {
        match status {
            PermissionStatus::Denied | PermissionStatus::AddOnly => 5,
            PermissionStatus::NotDetermined => 4,
            PermissionStatus::NotProbed => 3,
            PermissionStatus::Ok => 2,
            PermissionStatus::NotApplicable => 1,
        }
    }
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// `cider-bridge ping`, when the CLI is installed and not switched off.
/// The ping reads authorization status and never requests it.
async fn store_authorization() -> Option<bridge_cli::StoreAuthorization> {
    if bridge_cli::is_disabled() {
        return None;
    }
    let cli = bridge_cli::cli_path()?;
    bridge_cli::ping_at(&cli)
        .await
        .map(|pong| bridge_cli::StoreAuthorization::from_ping(&pong))
}

struct EventStore {
    permission: Permission,
    pane: &'static str,
    want: &'static str,
    plist_key: &'static str,
    needed_by: &'static [&'static str],
    store: &'static str,
    /// Whether macOS shows a prompt to a command-line requester at all.
    prompts_from_terminal: bool,
}

const EVENT_STORES: [EventStore; 3] = [
    EventStore {
        permission: Permission::Calendars,
        pane: "Calendars",
        want: "Full Access (not Add Only: with Add Only EventKit hides every event)",
        plist_key: "NSCalendarsFullAccessUsageDescription",
        needed_by: &["calendar (EventKit writes and reads through cider-bridge)"],
        store: "Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb",
        prompts_from_terminal: false,
    },
    EventStore {
        permission: Permission::Reminders,
        pane: "Reminders",
        want: "Full Access",
        plist_key: "NSRemindersFullAccessUsageDescription",
        needed_by: &["reminders (EventKit writes and reads through cider-bridge)"],
        store: "Library/Group Containers/group.com.apple.reminders/Container_v1/Stores",
        prompts_from_terminal: true,
    },
    EventStore {
        permission: Permission::Contacts,
        pane: "Contacts",
        want: "access",
        plist_key: "NSContactsUsageDescription",
        needed_by: &["contacts (Contacts framework writes and reads through cider-bridge)"],
        store: "Library/Application Support/AddressBook/AddressBook-v22.abcddb",
        prompts_from_terminal: false,
    },
];

async fn event_stores(
    home: &Path,
    auth: Option<&bridge_cli::StoreAuthorization>,
) -> Vec<Requirement> {
    EVENT_STORES
        .iter()
        .map(|store| event_store_requirement(home, store, auth))
        .collect()
}

fn event_store_requirement(
    home: &Path,
    store: &EventStore,
    auth: Option<&bridge_cli::StoreAuthorization>,
) -> Requirement {
    let (status, detail) = match auth {
        Some(auth) => {
            let word = match store.permission {
                Permission::Calendars => &auth.calendar,
                Permission::Reminders => &auth.reminders,
                _ => &auth.contacts,
            };
            (
                status_from_ping(word),
                format!(
                    "`cider-bridge ping` reports {word} for {} (TCC subject: {}); the ping \
                     reads status and never requests it",
                    store.pane.to_ascii_lowercase(),
                    auth.tcc_subject.as_deref().unwrap_or("unknown")
                ),
            )
        }
        None => {
            let path = home.join(store.store);
            let probe = probe_read(&path);
            let observed = match probe {
                PermissionStatus::Ok => "opened for reading, so SQLite reads work",
                PermissionStatus::Denied => {
                    "refused (EPERM): macOS denies this store to the \
                                             launching app"
                }
                PermissionStatus::NotApplicable => "does not exist",
                _ => "could not be opened",
            };
            (
                probe,
                format!(
                    "cider-bridge is not installed, so the framework grant is not probed; {} \
                     {observed}. Writes use AppleScript/JXA (see automation). Install the \
                     bridge (`brew install cider`) for a definitive answer from `cider-bridge \
                     ping`",
                    path.display()
                ),
            )
        }
    };
    let prompt = if store.prompts_from_terminal {
        "The first call through cider-bridge prompts the launching app".to_string()
    } else {
        format!(
            "Current macOS shows no {} prompt to a command-line requester: the first call \
             through cider-bridge registers the launching app in the pane and is denied, and \
             you then set it by hand",
            store.pane
        )
    };
    Requirement {
        permission: store.permission.clone(),
        needed_by: store.needed_by.iter().map(|s| s.to_string()).collect(),
        granted_to: GrantedTo::LaunchingApp,
        status,
        detail,
        how_to_grant: format!(
            "System Settings › Privacy & Security › {}: grant {} — {LAUNCHER_RULE}. {prompt}. \
             A host app appears in the pane only if it declares {} and has asked once; without \
             the key the access is silently denied",
            store.pane, store.want, store.plist_key
        ),
        host_app_requirements: vec![store.plist_key.to_string()],
    }
}

/// (target app, what sends it AppleEvents).
pub const AUTOMATION_TARGETS: &[(&str, &[&str])] = &[
    ("Notes", &["notes (every read and write)"]),
    ("Music", &["music (every read and control)"]),
    (
        "Mail",
        &[
            "mail send",
            "mail read",
            "mail unread",
            "mail trash",
            "mail get",
        ],
    ),
    ("Messages", &["messages send"]),
    ("Safari", &["safari tabs"]),
    (
        "Calendar",
        &["calendar (JXA fallback when cider-bridge is absent or the SQLite read fails)"],
    ),
    (
        "Reminders",
        &["reminders (AppleScript fallback when cider-bridge is absent)"],
    ),
    (
        "Contacts",
        &["contacts (JXA writes when cider-bridge is absent)"],
    ),
    ("Shortcuts", &["shortcuts run", "shortcuts view"]),
];

fn automation() -> Vec<Requirement> {
    AUTOMATION_TARGETS
        .iter()
        .map(|(target, needed_by)| Requirement {
            permission: Permission::Automation {
                target: target.to_string(),
            },
            needed_by: needed_by.iter().map(|s| s.to_string()).collect(),
            granted_to: GrantedTo::HostAppPair {
                target: target.to_string(),
            },
            status: PermissionStatus::NotProbed,
            detail: format!(
                "Not probed: the only way to observe Automation is to send {target} an \
                 AppleEvent, which can itself open the dialog. Granted per (launching app → \
                 {target}) pair; a real `cider` call surfaces any denial as an error"
            ),
            how_to_grant: format!(
                "System Settings › Privacy & Security › Automation: under the launching app, \
                 enable {target} — {LAUNCHER_RULE}. The pair appears only after the first \
                 AppleEvent, which prompts; a host app must declare \
                 NSAppleEventsUsageDescription or the event is refused with no prompt"
            ),
            host_app_requirements: vec!["NSAppleEventsUsageDescription".to_string()],
        })
        .collect()
}

/// HomeKit from a running bridge's ping (never launched). Pure.
pub fn home_kit_from(pong: Option<&serde_json::Value>) -> Requirement {
    let entitled = pong
        .and_then(|p| p.get("homekit_entitled"))
        .and_then(|v| v.as_bool());
    let authorized = pong
        .and_then(|p| p.get("homekit_authorized"))
        .and_then(|v| v.as_bool());
    let (status, detail) = match (pong, entitled, authorized) {
        (None, _, _) => (
            PermissionStatus::NotProbed,
            format!(
                "Cider Bridge is not running and this report never launches it{}; run a live \
                 command such as `cider home homes --live`, then `cider permissions` again",
                if bridge::is_installed() {
                    ""
                } else {
                    " (the app is not installed)"
                }
            ),
        ),
        (_, Some(false), _) => (
            PermissionStatus::NotApplicable,
            format!(
                "homekit_entitled false: {}",
                bridge::HOMEKIT_UNAVAILABLE_MESSAGE
            ),
        ),
        (_, _, Some(true)) => (
            PermissionStatus::Ok,
            "Cider Bridge reports homekit_authorized true".to_string(),
        ),
        (_, _, Some(false)) => (
            PermissionStatus::Denied,
            "Cider Bridge reports homekit_authorized false".to_string(),
        ),
        (_, _, None) => (
            PermissionStatus::NotProbed,
            "Cider Bridge answered ping without a HomeKit status".to_string(),
        ),
    };
    Requirement {
        permission: Permission::HomeKit,
        needed_by: vec![
            "home state".into(),
            "home run".into(),
            "home set".into(),
            "home triggers".into(),
            "home --live".into(),
        ],
        granted_to: GrantedTo::CiderBridgeApp,
        status,
        detail,
        how_to_grant: "System Settings › Privacy & Security › HomeKit: allow Cider Bridge. The \
                       grant belongs to the bridge app itself, not to the app that launches \
                       cider: it is an app, declares NSHomeKitUsageDescription, and prompts on \
                       its first HomeKit call. Needs a personal build (`cider bridge build \
                       --install`); the packaged bridge has no HomeKit entitlement"
            .to_string(),
        host_app_requirements: vec![],
    }
}

async fn home_kit() -> Requirement {
    home_kit_from(bridge::ping().await.as_ref())
}

fn location() -> Requirement {
    Requirement {
        permission: Permission::Location,
        needed_by: vec![],
        granted_to: GrantedTo::LaunchingApp,
        status: PermissionStatus::NotApplicable,
        detail: "No command needs it yet; reserved for a future `cider location` (weather \
                 takes --lat/--lon or a home's address instead)"
            .to_string(),
        how_to_grant: format!(
            "System Settings › Privacy & Security › Location Services: enable the app — \
             {LAUNCHER_RULE}. A host app must declare NSLocationWhenInUseUsageDescription"
        ),
        host_app_requirements: vec!["NSLocationWhenInUseUsageDescription".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cider-permissions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ping_words_map_to_statuses() {
        assert_eq!(status_from_ping("full_access"), PermissionStatus::Ok);
        assert_eq!(status_from_ping("authorized"), PermissionStatus::Ok);
        assert_eq!(status_from_ping("write_only"), PermissionStatus::AddOnly);
        assert_eq!(status_from_ping("add_only"), PermissionStatus::AddOnly);
        assert_eq!(status_from_ping("denied"), PermissionStatus::Denied);
        assert_eq!(status_from_ping("restricted"), PermissionStatus::Denied);
        assert_eq!(status_from_ping("limited"), PermissionStatus::Denied);
        assert_eq!(
            status_from_ping("not_determined"),
            PermissionStatus::NotDetermined
        );
        assert_eq!(status_from_ping("unknown"), PermissionStatus::NotProbed);
        assert_eq!(status_from_ping(""), PermissionStatus::NotProbed);
    }

    #[test]
    fn probe_read_distinguishes_readable_unreadable_and_missing() {
        let dir = temp_dir("probe");
        let readable = dir.join("readable.db");
        std::fs::write(&readable, b"x").unwrap();
        assert_eq!(probe_read(&readable), PermissionStatus::Ok);

        let unreadable = dir.join("unreadable.db");
        std::fs::write(&unreadable, b"x").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(probe_read(&unreadable), PermissionStatus::Denied);

        assert_eq!(
            probe_read(&dir.join("missing.db")),
            PermissionStatus::NotApplicable
        );

        // A store directory (Reminders' `Stores`) is listed, not opened.
        let stores = dir.join("Stores");
        std::fs::create_dir(&stores).unwrap();
        assert_eq!(probe_read(&stores), PermissionStatus::Ok);
        std::fs::set_permissions(&stores, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(probe_read(&stores), PermissionStatus::Denied);

        std::fs::set_permissions(&stores, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn responsible_process_prefers_the_first_app_in_the_chain() {
        let chain = [
            "/bin/zsh",
            "/Users/me/.local/bin/claude",
            "/Applications/Alchemy.app/Contents/MacOS/Alchemy",
            "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
        ];
        assert_eq!(
            responsible_process_from(chain, Some("Apple_Terminal")).as_deref(),
            Some("Alchemy")
        );
        assert_eq!(
            responsible_process_from(
                ["/Applications/Cider Bridge.app/Contents/MacOS/Cider Bridge"],
                None
            )
            .as_deref(),
            Some("Cider Bridge")
        );
    }

    #[test]
    fn responsible_process_falls_back_to_term_program() {
        let no_app = ["/bin/zsh", "/usr/bin/login"];
        assert_eq!(
            responsible_process_from(no_app, Some("Apple_Terminal")).as_deref(),
            Some("Terminal")
        );
        assert_eq!(
            responsible_process_from(no_app, Some("iTerm.app")).as_deref(),
            Some("iTerm")
        );
        assert_eq!(
            responsible_process_from(no_app, Some("Alchemy")).as_deref(),
            Some("Alchemy")
        );
        assert_eq!(responsible_process_from(no_app, Some("  ")), None);
        assert_eq!(responsible_process_from(no_app, None), None);
    }

    #[test]
    fn ps_line_parses_a_ppid_and_an_executable_with_spaces() {
        assert_eq!(
            parse_ps_line("  512 /Applications/Cider Bridge.app/Contents/MacOS/Cider Bridge\n"),
            Some((
                512,
                "/Applications/Cider Bridge.app/Contents/MacOS/Cider Bridge".to_string()
            ))
        );
        assert_eq!(parse_ps_line("1 launchd"), Some((1, "launchd".to_string())));
        assert_eq!(parse_ps_line("garbage"), None);
        assert_eq!(parse_ps_line(""), None);
    }

    #[test]
    fn event_store_from_ping_and_from_probe() {
        let home = temp_dir("stores");
        let auth = bridge_cli::StoreAuthorization::from_ping(&serde_json::json!({
            "calendar": "write_only", "reminders": "not_determined", "contacts": "authorized",
            "tcc_subject": "launcher"
        }));
        let calendar = event_store_requirement(&home, &EVENT_STORES[0], Some(&auth));
        assert_eq!(calendar.status, PermissionStatus::AddOnly);
        assert!(
            calendar.detail.contains("write_only"),
            "{}",
            calendar.detail
        );
        assert!(calendar
            .how_to_grant
            .contains("Privacy & Security › Calendars"));
        assert!(calendar.how_to_grant.contains("Full Access"));
        assert!(calendar.how_to_grant.contains("no Calendars prompt"));
        assert_eq!(
            calendar.host_app_requirements,
            vec!["NSCalendarsFullAccessUsageDescription"]
        );
        let reminders = event_store_requirement(&home, &EVENT_STORES[1], Some(&auth));
        assert_eq!(reminders.status, PermissionStatus::NotDetermined);
        assert!(reminders.how_to_grant.contains("prompts the launching app"));
        let contacts = event_store_requirement(&home, &EVENT_STORES[2], Some(&auth));
        assert_eq!(contacts.status, PermissionStatus::Ok);

        // Without the bridge CLI the store itself is opened.
        let missing = event_store_requirement(&home, &EVENT_STORES[2], None);
        assert_eq!(missing.status, PermissionStatus::NotApplicable);
        assert!(
            missing.detail.contains("not installed"),
            "{}",
            missing.detail
        );
        let db = home.join(EVENT_STORES[2].store);
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, b"x").unwrap();
        assert_eq!(
            event_store_requirement(&home, &EVENT_STORES[2], None).status,
            PermissionStatus::Ok
        );
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = event_store_requirement(&home, &EVENT_STORES[2], None);
        assert_eq!(denied.status, PermissionStatus::Denied);
        assert!(denied.detail.contains("EPERM"), "{}", denied.detail);
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn home_kit_reads_the_bridge_ping_or_stays_unprobed() {
        assert_eq!(home_kit_from(None).status, PermissionStatus::NotProbed);
        let packaged = home_kit_from(Some(&serde_json::json!({
            "homekit_entitled": false, "homekit_authorized": false
        })));
        assert_eq!(packaged.status, PermissionStatus::NotApplicable);
        assert_eq!(
            home_kit_from(Some(&serde_json::json!({"homekit_authorized": false}))).status,
            PermissionStatus::Denied
        );
        let ok = home_kit_from(Some(&serde_json::json!({
            "homekit_entitled": true, "homekit_authorized": true, "homes": 1
        })));
        assert_eq!(ok.status, PermissionStatus::Ok);
        assert_eq!(ok.granted_to, GrantedTo::CiderBridgeApp);
        assert!(ok.host_app_requirements.is_empty());
        assert!(ok.how_to_grant.contains("Privacy & Security › HomeKit"));
    }

    #[test]
    fn permission_and_granted_to_serialize_flat_with_a_tag() {
        assert_eq!(
            serde_json::to_value(Permission::FullDiskAccess).unwrap(),
            serde_json::json!({"name": "full_disk_access"})
        );
        assert_eq!(
            serde_json::to_value(Permission::Automation {
                target: "Notes".into()
            })
            .unwrap(),
            serde_json::json!({"name": "automation", "target": "Notes"})
        );
        assert_eq!(
            serde_json::to_value(GrantedTo::HostAppPair {
                target: "Mail".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "host_app_pair", "target": "Mail"})
        );
        assert_eq!(
            serde_json::to_value(PermissionStatus::AddOnly).unwrap(),
            serde_json::json!("add_only")
        );
        let back: Permission =
            serde_json::from_value(serde_json::json!({"name": "automation", "target": "Notes"}))
                .unwrap();
        assert_eq!(back.label(), "automation (Notes)");
    }

    #[tokio::test]
    async fn report_has_every_permission_with_a_fix_and_host_keys() {
        let report = report().await;
        assert!(report.prompt_free);
        let names: Vec<&str> = report
            .requirements
            .iter()
            .map(|r| r.permission.name())
            .collect();
        for expected in [
            "full_disk_access",
            "calendars",
            "reminders",
            "contacts",
            "automation",
            "home_kit",
            "location",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        assert_eq!(
            report
                .requirements
                .iter()
                .filter(|r| r.permission.name() == "automation")
                .count(),
            AUTOMATION_TARGETS.len()
        );
        for requirement in &report.requirements {
            let label = requirement.permission.label();
            assert!(!requirement.how_to_grant.is_empty(), "{label}");
            assert!(
                requirement
                    .how_to_grant
                    .contains("System Settings › Privacy & Security"),
                "{label}: {}",
                requirement.how_to_grant
            );
            assert!(!requirement.detail.is_empty(), "{label}");
            let expects_host_key = !matches!(
                requirement.permission,
                Permission::FullDiskAccess | Permission::HomeKit
            );
            assert_eq!(
                !requirement.host_app_requirements.is_empty(),
                expects_host_key,
                "{label}: {:?}",
                requirement.host_app_requirements
            );
            for key in &requirement.host_app_requirements {
                assert!(
                    HOST_INFO_PLIST_KEYS.iter().any(|(k, _)| k == key),
                    "{label}: {key} is not in HOST_INFO_PLIST_KEYS"
                );
            }
            if requirement.granted_to == GrantedTo::LaunchingApp {
                assert!(
                    requirement
                        .how_to_grant
                        .contains("the app that launches cider"),
                    "{label}: {}",
                    requirement.how_to_grant
                );
            }
        }
        // Automation is never probed; Location has no source yet.
        assert!(report
            .requirements
            .iter()
            .filter(|r| r.permission.name() == "automation")
            .all(|r| r.status == PermissionStatus::NotProbed));
        assert_eq!(
            report.summary,
            summarize(&report.requirements),
            "summary matches"
        );
        assert_eq!(report.rows().len(), report.requirements.len());
    }

    #[tokio::test]
    async fn for_source_keeps_only_what_that_source_needs() {
        let report = report().await;
        let calendar = report.for_source("calendar");
        let names: Vec<String> = calendar
            .requirements
            .iter()
            .map(|r| r.permission.label())
            .collect();
        assert_eq!(
            names,
            vec!["full_disk_access", "calendars", "automation (Calendar)"],
            "{names:?}"
        );
        assert_eq!(calendar.summary, summarize(&calendar.requirements));

        let messages = report.for_source("messages");
        let names: Vec<String> = messages
            .requirements
            .iter()
            .map(|r| r.permission.label())
            .collect();
        assert_eq!(names, vec!["full_disk_access", "automation (Messages)"]);

        let shortcuts = report.for_source("shortcuts");
        assert!(shortcuts
            .requirements
            .iter()
            .any(|r| r.permission.label() == "automation (Shortcuts)"));
        assert!(report
            .for_source("voice_memos")
            .requirements
            .iter()
            .any(|r| r.permission == Permission::FullDiskAccess));
        assert!(report.for_source("nothing").requirements.is_empty());
        assert_eq!(report.for_source("nothing").summary, Summary::default());
    }
}
