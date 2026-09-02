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
                "{} is not installed (looked in ~/Applications, /Applications, ${}); HomeKit \
                 live commands need it — build it with `cider bridge build --install`",
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
                    "{} is not installed (looked at ${}, {}/Contents/MacOS, ~/.local/bin, and \
                     $PATH); writes fall back to AppleScript/JXA — build it with `cider bridge \
                     build --install`",
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
