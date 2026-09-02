use serde::Serialize;
use std::path::{Path, PathBuf};

use super::bridge;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Missing,
    PermissionDenied,
    NotConfigured,
    NotProbed,
    /// Working today, but not for long: a signing profile inside its last
    /// thirty days.
    Expiring,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub required: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub platform: String,
    pub prompt_free: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainAuthorization {
    pub source: String,
    pub read_access: CheckStatus,
    pub write_automation: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct AuthorizationReport {
    pub prompt_free: bool,
    pub domains: Vec<DomainAuthorization>,
}

/// Inspect cider's local prerequisites without sending AppleEvents.
///
/// Sending a harmless-looking event to Calendar, Contacts, Mail, or
/// Reminders can itself trigger a macOS Automation prompt. Doctor therefore
/// verifies executables and local stores, but reports Automation permission as
/// `not_probed` instead of surprising the caller with UI.
pub async fn inspect() -> DoctorReport {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut checks = vec![
        check_executable("sqlite3", Path::new("/usr/bin/sqlite3"), true),
        check_executable("osascript", Path::new("/usr/bin/osascript"), true),
        check_path(
            "calendar_database",
            &home.join("Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb"),
            false,
        )
        .await,
        check_path(
            "contacts_database",
            &home.join("Library/Application Support/AddressBook/AddressBook-v22.abcddb"),
            false,
        )
        .await,
        check_path(
            "reminders_stores",
            &home.join("Library/Group Containers/group.com.apple.reminders/Container_v1/Stores"),
            false,
        )
        .await,
        check_mail_store(&home).await,
        check_home_cache(&home).await,
        check_bridge_app(),
        check_bridge_socket().await,
        check_bridge_cli(),
        check_bridge_profile().await,
        check_bridge_authorization().await,
        check_bridge_homekit().await,
        check_path(
            "shortcuts_database",
            &home.join("Library/Shortcuts/Shortcuts.sqlite"),
            false,
        )
        .await,
        check_path(
            "icloud_drive",
            &home.join(super::icloud::DRIVE_RELATIVE_ROOT),
            false,
        )
        .await,
    ];
    checks.push(DoctorCheck {
        name: "find_my".to_string(),
        status: CheckStatus::NotConfigured,
        required: false,
        detail: "Not supported: the Find My caches under ~/Library/Caches/com.apple.findmy.fmipcore are encrypted on current macOS, so cider has no find-my command".to_string(),
    });
    checks.push(DoctorCheck {
        name: "automation_permissions".to_string(),
        status: CheckStatus::NotProbed,
        required: false,
        detail: "Not probed because an AppleEvent can open a macOS permission dialog; write commands report permission failures directly".to_string(),
    });
    checks.push(permissions_check_from(&super::permissions::report().await));

    let ok = checks.iter().all(|check| {
        !check.required
            || matches!(
                check.status,
                CheckStatus::Ok | CheckStatus::NotConfigured | CheckStatus::NotProbed
            )
    });

    DoctorReport {
        ok,
        platform: std::env::consts::OS.to_string(),
        prompt_free: true,
        checks,
    }
}

/// The `permissions` summary check: one line over `cider permissions`.
/// `permission_denied` names what is denied (and what was never asked),
/// `not_configured` names what was never asked, `ok` otherwise; every
/// verdict points at `cider permissions` for the panes and the fixes. Pure.
pub fn permissions_check_from(report: &super::permissions::PermissionsReport) -> DoctorCheck {
    use super::permissions::PermissionStatus;
    let named = |wanted: PermissionStatus| -> Vec<String> {
        report
            .requirements
            .iter()
            .filter(|r| r.status == wanted)
            .map(|r| r.permission.label())
            .collect()
    };
    let mut denied = named(PermissionStatus::Denied);
    denied.extend(named(PermissionStatus::AddOnly));
    let not_determined = named(PermissionStatus::NotDetermined);
    let subject = report
        .responsible_process
        .as_deref()
        .unwrap_or("the app that launched cider");
    let (status, detail) = if !denied.is_empty() {
        let mut detail = format!("denied to {subject}: {}", denied.join(", "));
        if !not_determined.is_empty() {
            detail.push_str(&format!("; not yet asked: {}", not_determined.join(", ")));
        }
        (CheckStatus::PermissionDenied, detail)
    } else if !not_determined.is_empty() {
        (
            CheckStatus::NotConfigured,
            format!("not yet asked for {subject}: {}", not_determined.join(", ")),
        )
    } else {
        (
            CheckStatus::Ok,
            format!(
                "nothing denied to {subject} ({} ok, {} not probed)",
                report.summary.ok, report.summary.not_probed
            ),
        )
    };
    DoctorCheck {
        name: "permissions".to_string(),
        status,
        required: false,
        detail: format!(
            "{detail}; `cider permissions` lists every permission with the System Settings \
             pane and who to grant it to"
        ),
    }
}

/// Report the authorization state Cider can establish without triggering a
/// macOS privacy dialog. Local read access is observed directly; write-side
/// Automation stays `not_probed` until the user actually runs a write.
pub async fn auth_status() -> AuthorizationReport {
    let report = inspect().await;
    let mappings = [
        ("calendar", "calendar_database"),
        ("contacts", "contacts_database"),
        ("reminders", "reminders_stores"),
        ("mail", "mail_database"),
    ];
    let domains = mappings
        .into_iter()
        .map(|(source, check_name)| {
            let read_access = report
                .checks
                .iter()
                .find(|check| check.name == check_name)
                .map(|check| check.status.clone())
                .unwrap_or(CheckStatus::NotConfigured);
            DomainAuthorization {
                source: source.to_string(),
                read_access,
                write_automation: CheckStatus::NotProbed,
                detail: "Automation is not probed because an AppleEvent can open a macOS permission dialog".to_string(),
            }
        })
        .collect();
    AuthorizationReport {
        prompt_free: true,
        domains,
    }
}

/// The Home app keeps a decoded copy of the HomeKit configuration in its
/// container cache. It appears after the app has been opened once.
async fn check_home_cache(home: &Path) -> DoctorCheck {
    let dir = home.join(
        "Library/Containers/com.apple.Home/Data/Library/Caches/com.apple.HomeKit/com.apple.Home/com.apple.HomeKit.configurations",
    );
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return DoctorCheck {
                name: "home_cache".to_string(),
                status: CheckStatus::PermissionDenied,
                required: false,
                detail: format!("{} is not readable: {error}", dir.display()),
            };
        }
        Err(_) => {
            return DoctorCheck {
                name: "home_cache".to_string(),
                status: CheckStatus::NotConfigured,
                required: false,
                detail: format!(
                    "{} is not present; open the Home app once to create it",
                    dir.display()
                ),
            };
        }
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("homeData.") && name.ends_with(".config") {
            return DoctorCheck {
                name: "home_cache".to_string(),
                status: CheckStatus::Ok,
                required: false,
                detail: format!("{} is readable", entry.path().display()),
            };
        }
    }
    DoctorCheck {
        name: "home_cache".to_string(),
        status: CheckStatus::NotConfigured,
        required: false,
        detail: format!(
            "No homeData.*.config under {}; open the Home app once to create it",
            dir.display()
        ),
    }
}

/// The Cider Bridge app bundle (HomeKit live commands). It is built locally,
/// never distributed, so absence is `not_configured` rather than `missing`.
fn check_bridge_app() -> DoctorCheck {
    let (status, detail) = match bridge::app_path() {
        Some(path) => (CheckStatus::Ok, format!("{} is installed", path.display())),
        None => (
            CheckStatus::NotConfigured,
            format!(
                "{} is not installed (looked at ${}, ~/Applications, /Applications, and the \
                 Homebrew libexec); `brew install cider` includes a bridge for WeatherKit and \
                 EventKit/Contacts, and HomeKit live commands need a personal build — `cider \
                 bridge build --install`",
                bridge::APP_NAME,
                bridge::APP_ENV
            ),
        ),
    };
    DoctorCheck {
        name: "bridge_app".to_string(),
        status,
        required: false,
        detail,
    }
}

/// Whether a bridge is answering right now. Only pings; the app is launched
/// on demand by `cider home`, never by doctor, so idle is the normal state.
async fn check_bridge_socket() -> DoctorCheck {
    let socket = bridge::socket_path();
    let (status, detail) = match bridge::ping().await {
        Some(pong) if bridge::check_version(&pong).is_err() => (
            CheckStatus::NotConfigured,
            format!(
                "Cider Bridge answers ping on {} but {}",
                socket.display(),
                bridge::check_version(&pong).unwrap_err()
            ),
        ),
        Some(pong) => (
            CheckStatus::Ok,
            format!(
                "Cider Bridge answers ping on {} (version {}, homekit_authorized {}, {} homes)",
                socket.display(),
                bridge::ping_version(&pong),
                pong.get("homekit_authorized")
                    .and_then(|v| v.as_bool())
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                pong.get("homes")
                    .and_then(|v| v.as_u64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string()),
            ),
        ),
        None => (
            CheckStatus::NotConfigured,
            format!(
                "{} is not answering; cider launches Cider Bridge on demand, so this is normal \
                 while it is idle{}",
                socket.display(),
                if bridge::is_installed() {
                    ""
                } else {
                    " (and the app is not installed)"
                }
            ),
        ),
    };
    DoctorCheck {
        name: "bridge_socket".to_string(),
        status,
        required: false,
        detail,
    }
}

/// The native `cider-bridge` CLI (EventKit/Contacts writes and `watch`).
/// Optional: without it those commands use AppleScript/JXA and FSEvents.
fn check_bridge_cli() -> DoctorCheck {
    use super::bridge_cli;
    let (status, detail) = if bridge_cli::is_disabled() {
        (
            CheckStatus::NotConfigured,
            format!(
                "{}=off: Reminders and Calendar writes use AppleScript/JXA and `cider watch` \
                 uses FSEvents",
                bridge_cli::CLI_ENV
            ),
        )
    } else {
        match bridge_cli::cli_path() {
            Some(path) => (
                CheckStatus::Ok,
                format!(
                    "{} is installed; Reminders and Calendar writes and `cider watch` use it",
                    path.display()
                ),
            ),
            None => (
                CheckStatus::NotConfigured,
                format!(
                    "{} is not installed (looked at ${}, ~/Applications/{}/Contents/MacOS, next \
                     to the cider binary, ~/.local/bin, and $PATH); writes fall back to \
                     AppleScript/JXA — `brew install cider` includes it, or build it with \
                     `cider bridge build --install`",
                    bridge_cli::CLI_NAME,
                    bridge_cli::CLI_ENV,
                    bridge::APP_NAME
                ),
            ),
        }
    };
    DoctorCheck {
        name: "bridge_cli".to_string(),
        status,
        required: false,
        detail,
    }
}

/// Days before a profile expires at which `bridge_profile` turns `expiring`.
pub const PROFILE_WARNING_DAYS: i64 = 30;

/// What the installed app's embedded provisioning profile says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub expires: chrono::DateTime<chrono::Utc>,
    /// `com.apple.developer.homekit` in the profile's entitlements, when
    /// the profile lists entitlements at all.
    pub homekit: Option<bool>,
}

/// Decode a provisioning profile's plist (the CMS payload as `security cms
/// -D` prints it, XML or binary). Pure, so the shape is testable without
/// a signed profile.
pub fn parse_profile_plist(bytes: &[u8]) -> anyhow::Result<ProfileInfo> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("profile plist root is not a dictionary"))?;
    let expires = dict
        .get("ExpirationDate")
        .and_then(plist::Value::as_date)
        .map(|date| chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::from(date)))
        .ok_or_else(|| anyhow::anyhow!("profile plist has no ExpirationDate"))?;
    let homekit = dict
        .get("Entitlements")
        .and_then(plist::Value::as_dictionary)
        .map(|entitlements| {
            entitlements
                .get("com.apple.developer.homekit")
                .and_then(plist::Value::as_boolean)
                .unwrap_or(false)
        });
    Ok(ProfileInfo { expires, homekit })
}

/// `security cms -D -i <profile>`: strip the CMS signature and print the
/// plist inside. `security` is part of macOS; doctor already shells out.
async fn read_profile(path: &Path) -> anyhow::Result<ProfileInfo> {
    let path = path.to_string_lossy();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new("/usr/bin/security")
            .args(["cms", "-D", "-i", &path])
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("security cms timed out"))??;
    if !output.status.success() {
        anyhow::bail!(
            "security cms failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_profile_plist(&output.stdout)
}

/// The `bridge_profile` verdict for a decoded profile at `now`. Pure.
pub fn profile_check_from(
    path: &Path,
    profile: &ProfileInfo,
    now: chrono::DateTime<chrono::Utc>,
) -> DoctorCheck {
    let days = (profile.expires - now).num_days();
    let homekit = match profile.homekit {
        Some(true) => "with the HomeKit entitlement",
        Some(false) => "without the HomeKit entitlement",
        None => "entitlements not listed",
    };
    let rebuild = "rebuild with `cider bridge build --install` (Xcode renews the profile)";
    let (status, detail) = if days < 0 {
        (
            CheckStatus::Missing,
            format!(
                "{} expired {} day(s) ago ({}); the app will not launch until you {rebuild}",
                path.display(),
                -days,
                profile.expires.format("%Y-%m-%d")
            ),
        )
    } else if days <= PROFILE_WARNING_DAYS {
        (
            CheckStatus::Expiring,
            format!(
                "{} expires in {days} day(s) on {}; {rebuild} before then",
                path.display(),
                profile.expires.format("%Y-%m-%d")
            ),
        )
    } else {
        (
            CheckStatus::Ok,
            format!(
                "{} is valid for {days} more days (until {}), {homekit}",
                path.display(),
                profile.expires.format("%Y-%m-%d")
            ),
        )
    };
    DoctorCheck {
        name: "bridge_profile".to_string(),
        status,
        required: false,
        detail,
    }
}

/// A development-signed bridge (the only kind that can carry HomeKit) is
/// tied to a provisioning profile that expires a year after it was made;
/// when it does, the app silently stops launching. A Developer ID build,
/// such as the Homebrew-packaged one, has no embedded profile and needs
/// none for EventKit/Contacts/WeatherKit, but cannot do HomeKit.
async fn check_bridge_profile() -> DoctorCheck {
    let not_configured = |detail: String| DoctorCheck {
        name: "bridge_profile".to_string(),
        status: CheckStatus::NotConfigured,
        required: false,
        detail,
    };
    let Some(app) = bridge::app_path() else {
        return not_configured(format!("{} is not installed", bridge::APP_NAME));
    };
    let profile = app.join("Contents/embedded.provisionprofile");
    if !profile.is_file() {
        return not_configured(format!(
            "{} has no embedded profile, HomeKit unavailable (a packaged Developer ID build; \
             EventKit, Contacts, and WeatherKit still work) — build a personal bridge with \
             `cider bridge build --install` for HomeKit",
            app.display()
        ));
    }
    match read_profile(&profile).await {
        Ok(info) => profile_check_from(&profile, &info, chrono::Utc::now()),
        Err(error) => not_configured(format!(
            "{} could not be decoded: {error}",
            profile.display()
        )),
    }
}

/// The `bridge_authorization` verdict for a `cider-bridge ping` reply. Pure.
pub fn authorization_check_from(pong: &serde_json::Value) -> DoctorCheck {
    let auth = super::bridge_cli::StoreAuthorization::from_ping(pong);
    let summary = format!(
        "calendar {}, reminders {}, contacts {} (TCC subject: {})",
        auth.calendar,
        auth.reminders,
        auth.contacts,
        auth.tcc_subject.as_deref().unwrap_or("unknown")
    );
    let (status, detail) = if auth.all_granted() {
        (CheckStatus::Ok, summary)
    } else {
        let status = if auth.any_denied() {
            CheckStatus::PermissionDenied
        } else {
            CheckStatus::NotConfigured
        };
        (status, format!("{summary}; {}", auth.fixes.join("; ")))
    };
    DoctorCheck {
        name: "bridge_authorization".to_string(),
        status,
        required: false,
        detail,
    }
}

/// Per-store TCC state as `cider-bridge ping` reports it. The CLI's ping
/// reads authorization status only; it never requests access, so this
/// opens no dialog. The point is to make the Calendar "Full Access"
/// requirement visible: EventKit hides every event from an app that was
/// granted less, and the grant belongs to whichever app launched cider.
async fn check_bridge_authorization() -> DoctorCheck {
    use super::bridge_cli;
    let not_configured = |detail: String| DoctorCheck {
        name: "bridge_authorization".to_string(),
        status: CheckStatus::NotConfigured,
        required: false,
        detail,
    };
    if bridge_cli::is_disabled() {
        return not_configured(format!("{}=off", bridge_cli::CLI_ENV));
    }
    let Some(cli) = bridge_cli::cli_path() else {
        return not_configured(format!(
            "{} is not installed; Reminders and Calendar writes use AppleScript/JXA, whose \
             Automation permission is not probed",
            bridge_cli::CLI_NAME
        ));
    };
    match bridge_cli::ping_at(&cli).await {
        Some(pong) => authorization_check_from(&pong),
        None => not_configured(format!("{} did not answer ping", cli.display())),
    }
}

/// The `bridge_homekit` verdict for a running app's `ping` reply. Pure.
/// `homekit_entitled` is newer than `homekit_authorized`; a bridge that
/// omits it is reported as unknown rather than assumed either way.
pub fn homekit_check_from(pong: &serde_json::Value) -> DoctorCheck {
    let entitled = pong.get("homekit_entitled").and_then(|v| v.as_bool());
    let authorized = pong.get("homekit_authorized").and_then(|v| v.as_bool());
    let homes = pong.get("homes").and_then(|v| v.as_u64());
    let (status, detail) = match (entitled, authorized) {
        (Some(false), _) => (
            CheckStatus::NotConfigured,
            format!(
                "homekit_entitled false: {}",
                bridge::HOMEKIT_UNAVAILABLE_MESSAGE
            ),
        ),
        (_, Some(false)) => (
            CheckStatus::PermissionDenied,
            format!(
                "homekit_entitled {}, homekit_authorized false: System Settings › Privacy & \
                 Security › HomeKit, allow Cider Bridge (or answer its prompt on the next \
                 `cider home` live command)",
                entitled
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ),
        ),
        (_, Some(true)) => (
            CheckStatus::Ok,
            format!(
                "homekit_entitled {}, homekit_authorized true, {} home(s)",
                entitled
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                homes.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
            ),
        ),
        (_, None) => (
            CheckStatus::NotProbed,
            "ping reported neither homekit_entitled nor homekit_authorized".to_string(),
        ),
    };
    DoctorCheck {
        name: "bridge_homekit".to_string(),
        status,
        required: false,
        detail,
    }
}

/// HomeKit entitlement and authorization from the app's `ping`, only when
/// it is already running: doctor never launches the bridge.
async fn check_bridge_homekit() -> DoctorCheck {
    match bridge::ping().await {
        Some(pong) => homekit_check_from(&pong),
        None => DoctorCheck {
            name: "bridge_homekit".to_string(),
            status: CheckStatus::NotProbed,
            required: false,
            detail: format!(
                "Cider Bridge is not running and doctor never launches it; run a live command \
                 such as `cider home homes --live` and then `cider doctor` again{}",
                if bridge::is_installed() {
                    ""
                } else {
                    " (the app is not installed)"
                }
            ),
        },
    }
}

fn check_executable(name: &str, path: &Path, required: bool) -> DoctorCheck {
    let (status, detail) = if path.is_file() {
        (CheckStatus::Ok, format!("{} is available", path.display()))
    } else {
        (
            CheckStatus::Missing,
            format!("{} was not found", path.display()),
        )
    };
    DoctorCheck {
        name: name.to_string(),
        status,
        required,
        detail,
    }
}

async fn check_path(name: &str, path: &Path, required: bool) -> DoctorCheck {
    match tokio::fs::metadata(path).await {
        Ok(_) => DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::Ok,
            required,
            detail: format!("{} is readable", path.display()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::PermissionDenied,
            required,
            detail: format!("{} is not readable: {error}", path.display()),
        },
        Err(_) => DoctorCheck {
            name: name.to_string(),
            status: CheckStatus::NotConfigured,
            required,
            detail: format!("{} is not present", path.display()),
        },
    }
}

async fn check_mail_store(home: &Path) -> DoctorCheck {
    let mail = home.join("Library/Mail");
    let mut versions = match tokio::fs::read_dir(&mail).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return DoctorCheck {
                name: "mail_database".to_string(),
                status: CheckStatus::PermissionDenied,
                required: false,
                detail: format!("{} is not readable: {error}", mail.display()),
            };
        }
        Err(_) => {
            return DoctorCheck {
                name: "mail_database".to_string(),
                status: CheckStatus::NotConfigured,
                required: false,
                detail: format!("{} is not present", mail.display()),
            };
        }
    };

    while let Ok(Some(entry)) = versions.next_entry().await {
        let path = entry.path().join("MailData/Envelope Index");
        if tokio::fs::metadata(&path).await.is_ok() {
            return DoctorCheck {
                name: "mail_database".to_string(),
                status: CheckStatus::Ok,
                required: false,
                detail: format!("{} is readable", path.display()),
            };
        }
    }

    DoctorCheck {
        name: "mail_database".to_string(),
        status: CheckStatus::NotConfigured,
        required: false,
        detail: "No Mail Envelope Index was found under ~/Library/Mail/V*".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_check_is_deterministic() {
        let check = check_executable("definitely-missing", Path::new("/no/such/cider-tool"), true);
        assert_eq!(check.status, CheckStatus::Missing);
        assert!(check.required);
    }

    #[tokio::test]
    async fn home_cache_check_reports_missing_cache_without_failing() {
        let check = check_home_cache(Path::new("/no/such/home")).await;
        assert_eq!(check.status, CheckStatus::NotConfigured);
        assert!(!check.required);
        assert!(check.detail.contains("open the Home app"));
    }

    #[tokio::test]
    async fn bridge_checks_are_optional_and_never_launch_the_app() {
        // `inspect` only pings the socket: a fast refusal when nothing runs,
        // a ping reply when the bridge is up, never a launch. Both checks must
        // be present and optional whichever state the machine is in.
        let report = inspect().await;
        for name in ["bridge_app", "bridge_socket"] {
            let check = report
                .checks
                .iter()
                .find(|check| check.name == name)
                .unwrap_or_else(|| panic!("{name} check missing"));
            assert!(!check.required, "{name} must not gate doctor's ok");
            assert!(check.detail.contains("Cider Bridge"), "{}", check.detail);
        }
        let cli = report
            .checks
            .iter()
            .find(|check| check.name == "bridge_cli")
            .expect("bridge_cli check missing");
        assert!(!cli.required);
        assert!(
            matches!(cli.status, CheckStatus::Ok | CheckStatus::NotConfigured),
            "{:?}",
            cli.status
        );
        assert!(cli.detail.contains("cider-bridge"), "{}", cli.detail);
    }

    fn profile_xml(expires: &str, homekit: Option<bool>) -> Vec<u8> {
        let entitlements = match homekit {
            Some(b) => format!(
                "<key>Entitlements</key><dict><key>com.apple.developer.homekit</key><{}/></dict>",
                if b { "true" } else { "false" }
            ),
            None => String::new(),
        };
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Name</key><string>iOS Team Provisioning Profile: dev.thrasher.cider.bridge</string>
<key>ExpirationDate</key><date>{expires}</date>
{entitlements}
</dict></plist>"#
        )
        .into_bytes()
    }

    #[test]
    fn profile_plist_yields_expiry_and_homekit_entitlement() {
        let info = parse_profile_plist(&profile_xml("2027-09-02T07:15:13Z", Some(true))).unwrap();
        assert_eq!(info.expires.to_rfc3339(), "2027-09-02T07:15:13+00:00");
        assert_eq!(info.homekit, Some(true));
        let bare = parse_profile_plist(&profile_xml("2027-09-02T07:15:13Z", None)).unwrap();
        assert_eq!(bare.homekit, None);
        assert!(parse_profile_plist(b"<plist version=\"1.0\"><dict/></plist>").is_err());
        assert!(parse_profile_plist(b"not a plist").is_err());
    }

    #[test]
    fn profile_check_is_ok_then_expiring_then_missing() {
        let path = Path::new("/x/embedded.provisionprofile");
        let expires = chrono::DateTime::parse_from_rfc3339("2027-09-02T07:15:13Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let info = ProfileInfo {
            expires,
            homekit: Some(true),
        };
        let at = |days_before: i64| expires - chrono::Duration::days(days_before);

        let fresh = profile_check_from(path, &info, at(365));
        assert_eq!(fresh.status, CheckStatus::Ok);
        assert!(!fresh.required);
        assert!(fresh.detail.contains("365 more days"), "{}", fresh.detail);
        assert!(fresh.detail.contains("with the HomeKit entitlement"));

        let soon = profile_check_from(path, &info, at(30));
        assert_eq!(soon.status, CheckStatus::Expiring);
        assert!(
            soon.detail.contains("expires in 30 day(s)"),
            "{}",
            soon.detail
        );
        assert!(soon.detail.contains("cider bridge build --install"));
        assert_eq!(
            profile_check_from(path, &info, at(31)).status,
            CheckStatus::Ok
        );

        let gone = profile_check_from(path, &info, at(-2));
        assert_eq!(gone.status, CheckStatus::Missing);
        assert!(
            gone.detail.contains("expired 2 day(s) ago"),
            "{}",
            gone.detail
        );
        assert!(gone.detail.contains("will not launch"));

        assert_eq!(
            serde_json::to_value(CheckStatus::Expiring).unwrap(),
            serde_json::json!("expiring")
        );
    }

    #[test]
    fn authorization_check_reports_each_store_and_names_the_fix() {
        let granted = serde_json::json!({
            "calendar": "full_access", "reminders": "full_access", "contacts": "authorized",
            "tcc_subject": "launcher", "executable": "/x/cider-bridge"
        });
        let ok = authorization_check_from(&granted);
        assert_eq!(ok.status, CheckStatus::Ok);
        assert!(!ok.required);
        assert!(ok.detail.contains("calendar full_access"), "{}", ok.detail);

        let pending = serde_json::json!({
            "calendar": "not_determined", "reminders": "full_access",
            "contacts": "not_determined", "tcc_subject": "launcher"
        });
        let check = authorization_check_from(&pending);
        assert_eq!(
            check.status,
            CheckStatus::NotConfigured,
            "not asked is not denied"
        );
        assert!(
            check.detail.contains("calendar is not_determined"),
            "{}",
            check.detail
        );
        assert!(check.detail.contains("the app that launches cider"));

        let denied = serde_json::json!({
            "calendar": "write_only", "reminders": "denied", "contacts": "authorized",
            "tcc_subject": "cider-bridge"
        });
        let check = authorization_check_from(&denied);
        assert_eq!(check.status, CheckStatus::PermissionDenied);
        assert!(
            check
                .detail
                .contains("Privacy & Security › Calendars, grant Full Access to cider-bridge"),
            "{}",
            check.detail
        );
        assert!(
            check.detail.contains("reminders is denied"),
            "{}",
            check.detail
        );
        assert!(!check.detail.contains("contacts is"), "{}", check.detail);
    }

    #[test]
    fn homekit_check_reads_entitlement_and_authorization() {
        let packaged = homekit_check_from(&serde_json::json!({
            "version": "0.1.0", "homekit_entitled": false, "homekit_authorized": false, "homes": 0
        }));
        assert_eq!(packaged.status, CheckStatus::NotConfigured);
        assert!(
            packaged.detail.contains("no HomeKit entitlement"),
            "{}",
            packaged.detail
        );
        assert!(packaged.detail.contains("cider bridge build --install"));

        let denied = homekit_check_from(&serde_json::json!({
            "homekit_entitled": true, "homekit_authorized": false, "homes": 0
        }));
        assert_eq!(denied.status, CheckStatus::PermissionDenied);
        assert!(
            denied.detail.contains("Privacy & Security › HomeKit"),
            "{}",
            denied.detail
        );

        let old_bridge = homekit_check_from(&serde_json::json!({
            "version": "0.1.0", "homekit_authorized": true, "homes": 2
        }));
        assert_eq!(old_bridge.status, CheckStatus::Ok);
        assert!(
            old_bridge.detail.contains("homekit_entitled unknown"),
            "{}",
            old_bridge.detail
        );
        assert!(old_bridge.detail.contains("2 home(s)"));

        let cli_pong = homekit_check_from(&serde_json::json!({"version": "0.1.0"}));
        assert_eq!(cli_pong.status, CheckStatus::NotProbed);
    }

    #[tokio::test]
    async fn new_bridge_checks_are_optional_and_never_launch_the_app() {
        let report = inspect().await;
        for name in ["bridge_profile", "bridge_authorization", "bridge_homekit"] {
            let check = report
                .checks
                .iter()
                .find(|check| check.name == name)
                .unwrap_or_else(|| panic!("{name} check missing"));
            assert!(!check.required, "{name} must not gate doctor's ok");
            assert!(!check.detail.is_empty(), "{name} has no detail");
        }
    }

    #[tokio::test]
    async fn auth_status_never_claims_unprobed_write_access() {
        let report = auth_status().await;
        assert_eq!(report.domains.len(), 4);
        assert!(report
            .domains
            .iter()
            .all(|domain| domain.write_automation == CheckStatus::NotProbed));
    }
}
