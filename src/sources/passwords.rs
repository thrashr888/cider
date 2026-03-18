use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PasswordEntry {
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// List saved passwords (metadata only — no secrets).
///
/// Uses the macOS `security` CLI to read keychain items matching
/// generic-password and internet-password classes.
pub async fn list(search: Option<&str>) -> anyhow::Result<Vec<PasswordEntry>> {
    let timeout = std::time::Duration::from_secs(15);
    let output = run_command_with_timeout("security", &["dump-keychain"], timeout).await?;

    let mut items = parse_keychain_passwords(&output);

    if let Some(q) = search {
        let q = q.to_lowercase();
        items.retain(|item| {
            item.label.to_lowercase().contains(&q)
                || item
                    .service
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&q))
                || item
                    .server
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&q))
                || item
                    .account
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&q))
        });
    }

    Ok(items)
}

/// Get a password value by service name (generic password).
/// Triggers a macOS authorization dialog.
pub async fn get(service: &str, account: Option<&str>) -> anyhow::Result<PasswordEntry> {
    let timeout = std::time::Duration::from_secs(15);
    let output = run_command_with_timeout("security", &["dump-keychain"], timeout).await?;

    let items = parse_keychain_passwords(&output);
    let q = service.to_lowercase();

    let entry = items
        .into_iter()
        .find(|item| {
            let matches_service = item
                .service
                .as_deref()
                .is_some_and(|s| s.to_lowercase() == q);
            let matches_label = item.label.to_lowercase() == q;
            let matches_server = item
                .server
                .as_deref()
                .is_some_and(|s| s.to_lowercase() == q);

            let base_match = matches_service || matches_label || matches_server;
            match account {
                Some(acct) => {
                    base_match
                        && item
                            .account
                            .as_deref()
                            .is_some_and(|a| a.to_lowercase() == acct.to_lowercase())
                }
                None => base_match,
            }
        })
        .ok_or_else(|| anyhow::anyhow!("No password found for '{service}'"))?;

    Ok(entry)
}

/// Get the actual password value. Triggers macOS auth dialog.
pub async fn get_password(service: &str, account: Option<&str>) -> anyhow::Result<String> {
    // First try generic password
    let mut args = vec!["find-generic-password", "-s", service, "-w"];
    if let Some(acct) = account {
        args.insert(3, acct);
        args.insert(3, "-a");
    }

    let result =
        run_command_with_timeout("security", &args, std::time::Duration::from_secs(30)).await;

    if let Ok(pw) = result {
        return Ok(pw.trim().to_string());
    }

    // Fall back to internet password
    let mut args = vec!["find-internet-password", "-s", service, "-w"];
    if let Some(acct) = account {
        args.insert(3, acct);
        args.insert(3, "-a");
    }

    let pw =
        run_command_with_timeout("security", &args, std::time::Duration::from_secs(30)).await?;

    Ok(pw.trim().to_string())
}

/// Create a new password entry.
pub async fn create(
    service: &str,
    account: &str,
    password: &str,
    label: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let mut args = vec![
        "add-generic-password",
        "-s",
        service,
        "-a",
        account,
        "-w",
        password,
        "-U", // update if exists
    ];
    if let Some(l) = label {
        args.push("-l");
        args.push(l);
    }

    run_command_with_timeout("security", &args, std::time::Duration::from_secs(10)).await?;

    Ok(ActionResult::success_with_message(
        "created",
        &format!("Password saved for {service}/{account}"),
    ))
}

/// Update an existing password entry (upsert via -U flag).
pub async fn update(service: &str, account: &str, password: &str) -> anyhow::Result<ActionResult> {
    let args = vec![
        "add-generic-password",
        "-s",
        service,
        "-a",
        account,
        "-w",
        password,
        "-U",
    ];

    run_command_with_timeout("security", &args, std::time::Duration::from_secs(10)).await?;

    Ok(ActionResult::success_with_message(
        "updated",
        &format!("Password updated for {service}/{account}"),
    ))
}

/// Delete a password entry.
pub async fn delete(service: &str, account: Option<&str>) -> anyhow::Result<ActionResult> {
    let mut args = vec!["delete-generic-password", "-s", service];
    if let Some(acct) = account {
        args.push("-a");
        args.push(acct);
    }

    run_command_with_timeout("security", &args, std::time::Duration::from_secs(10)).await?;

    Ok(ActionResult::success_with_message(
        "deleted",
        &format!("Password deleted for {service}"),
    ))
}

fn parse_keychain_passwords(output: &str) -> Vec<PasswordEntry> {
    let mut items = Vec::new();
    let mut current_kind = String::new();
    let mut attrs: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for line in output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("keychain: ") {
            // Flush previous
            if is_password_class(&current_kind) {
                if let Some(item) = build_entry(&current_kind, &attrs) {
                    items.push(item);
                }
            }
            attrs.clear();
            current_kind.clear();
        } else if trimmed.starts_with("class: ") {
            let class = trimmed
                .strip_prefix("class: ")
                .unwrap_or("")
                .trim_matches('"');
            current_kind = class.to_string();
        } else if let Some(eq_pos) = trimmed.find('=') {
            let key_part = &trimmed[..eq_pos];
            let value = trimmed[eq_pos + 1..].trim().trim_matches('"');

            let attr_name = key_part
                .trim_matches('"')
                .split('<')
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .trim();

            if !attr_name.is_empty() && value != "<NULL>" {
                attrs.insert(attr_name.to_string(), value.to_string());
            }
        }
    }

    // Flush last
    if is_password_class(&current_kind) {
        if let Some(item) = build_entry(&current_kind, &attrs) {
            items.push(item);
        }
    }

    items
}

fn is_password_class(class: &str) -> bool {
    matches!(class, "genp" | "inet")
}

fn build_entry(
    class: &str,
    attrs: &std::collections::HashMap<String, String>,
) -> Option<PasswordEntry> {
    let label = attrs
        .get("labl")
        .or(attrs.get("svce"))
        .or(attrs.get("srvr"))
        .cloned()
        .unwrap_or_default();

    if label.is_empty() {
        return None;
    }

    let kind = match class {
        "genp" => "generic-password",
        "inet" => "internet-password",
        _ => return None,
    };

    Some(PasswordEntry {
        kind: kind.to_string(),
        label,
        account: attrs.get("acct").cloned().filter(|s| !s.is_empty()),
        service: attrs.get("svce").cloned().filter(|s| !s.is_empty()),
        server: attrs.get("srvr").cloned().filter(|s| !s.is_empty()),
        protocol: attrs.get("ptcl").cloned().filter(|s| !s.is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_keychain_passwords() {
        let output = r#"keychain: "/Users/test/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    "acct"<blob>="user@example.com"
    "labl"<blob>="Example App"
    "svce"<blob>="com.example.app"
keychain: "/Users/test/Library/Keychains/login.keychain-db"
version: 512
class: "inet"
attributes:
    "acct"<blob>="admin"
    "labl"<blob>="example.com"
    "srvr"<blob>="example.com"
    "ptcl"<uint32>="htps"
keychain: "/Users/test/Library/Keychains/login.keychain-db"
version: 512
class: "cert"
attributes:
    "labl"<blob>="Some Certificate"
"#;
        let items = parse_keychain_passwords(output);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "generic-password");
        assert_eq!(items[0].label, "Example App");
        assert_eq!(items[0].account.as_deref(), Some("user@example.com"));
        assert_eq!(items[1].kind, "internet-password");
        assert_eq!(items[1].server.as_deref(), Some("example.com"));
    }

    #[test]
    fn test_filters_non_password_classes() {
        let output = r#"keychain: "/path/login.keychain-db"
class: "cert"
attributes:
    "labl"<blob>="My Certificate"
keychain: "/path/login.keychain-db"
class: "keys"
attributes:
    "labl"<blob>="My Key"
"#;
        let items = parse_keychain_passwords(output);
        assert!(items.is_empty());
    }

    #[test]
    fn test_empty_input() {
        assert!(parse_keychain_passwords("").is_empty());
    }
}
