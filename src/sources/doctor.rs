use serde::Serialize;
use std::path::{Path, PathBuf};

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
    ];
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
    async fn auth_status_never_claims_unprobed_write_access() {
        let report = auth_status().await;
        assert_eq!(report.domains.len(), 4);
        assert!(report
            .domains
            .iter()
            .all(|domain| domain.write_automation == CheckStatus::NotProbed));
    }
}
