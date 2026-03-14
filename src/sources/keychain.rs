use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct KeychainItem {
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
    pub keychain: String,
}

/// List keychain items (metadata only — no passwords).
pub async fn list(kind: Option<&str>) -> anyhow::Result<Vec<KeychainItem>> {
    let timeout = std::time::Duration::from_secs(15);

    // dump-keychain lists all items without passwords
    let output = run_command_with_timeout("security", &["dump-keychain"], timeout).await?;

    Ok(parse_dump(&output, kind))
}

/// Search for a specific keychain item by service or server name.
pub async fn search(query: &str, kind: Option<&str>) -> anyhow::Result<Vec<KeychainItem>> {
    let items = list(None).await?;
    let q = query.to_lowercase();
    let filtered: Vec<KeychainItem> = items
        .into_iter()
        .filter(|item| {
            if let Some(k) = kind {
                if item.kind != k {
                    return false;
                }
            }
            item.label.to_lowercase().contains(&q)
                || item
                    .service
                    .as_deref()
                    .map(|s| s.to_lowercase().contains(&q))
                    .unwrap_or(false)
                || item
                    .server
                    .as_deref()
                    .map(|s| s.to_lowercase().contains(&q))
                    .unwrap_or(false)
                || item
                    .account
                    .as_deref()
                    .map(|s| s.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();
    Ok(filtered)
}

/// Get the password for a generic (app) password by service and account.
/// Returns the password as a string. Requires user approval via macOS security dialog.
pub async fn get_password(service: &str, account: Option<&str>) -> anyhow::Result<String> {
    let mut args = vec!["find-generic-password", "-s", service, "-w"];
    if let Some(acct) = account {
        args.insert(3, acct);
        args.insert(3, "-a");
    }

    let password =
        run_command_with_timeout("security", &args, std::time::Duration::from_secs(30)).await?;

    Ok(password.trim().to_string())
}

/// Get the password for an internet password by server and account.
pub async fn get_internet_password(server: &str, account: Option<&str>) -> anyhow::Result<String> {
    let mut args = vec!["find-internet-password", "-s", server, "-w"];
    if let Some(acct) = account {
        args.insert(3, acct);
        args.insert(3, "-a");
    }

    let password =
        run_command_with_timeout("security", &args, std::time::Duration::from_secs(30)).await?;

    Ok(password.trim().to_string())
}

/// Add a generic password to the keychain.
pub async fn add(
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
        "add",
        &format!("Added password for {service}/{account}"),
    ))
}

/// Delete a generic password from the keychain.
pub async fn delete(service: &str, account: Option<&str>) -> anyhow::Result<ActionResult> {
    let mut args = vec!["delete-generic-password", "-s", service];
    if let Some(acct) = account {
        args.push("-a");
        args.push(acct);
    }

    run_command_with_timeout("security", &args, std::time::Duration::from_secs(10)).await?;

    Ok(ActionResult::success_with_message(
        "delete",
        &format!("Deleted password for {service}"),
    ))
}

/// List all keychains.
pub async fn keychains() -> anyhow::Result<Vec<String>> {
    let output = run_command_with_timeout(
        "security",
        &["list-keychains"],
        std::time::Duration::from_secs(5),
    )
    .await?;

    Ok(output
        .lines()
        .map(|l| l.trim().trim_matches('"').to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn parse_dump(output: &str, kind_filter: Option<&str>) -> Vec<KeychainItem> {
    let mut items = Vec::new();
    let mut current_kind = String::new();
    let mut current_keychain = String::new();
    let mut attrs: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // New item block: "keychain: "/path/to/keychain""
        if trimmed.starts_with("keychain: ") {
            // Flush previous item
            if !current_kind.is_empty() {
                if let Some(item) = build_item(&current_kind, &current_keychain, &attrs) {
                    if kind_filter.is_none() || kind_filter.map(|k| k == item.kind).unwrap_or(true)
                    {
                        items.push(item);
                    }
                }
            }
            current_keychain = trimmed
                .strip_prefix("keychain: ")
                .unwrap_or("")
                .trim_matches('"')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            attrs.clear();
            current_kind.clear();
        }
        // Class line: "class: "genp"" or "class: "inet""
        else if trimmed.starts_with("class: ") {
            let class = trimmed
                .strip_prefix("class: ")
                .unwrap_or("")
                .trim_matches('"');
            current_kind = match class {
                "genp" => "generic-password",
                "inet" => "internet-password",
                "cert" => "certificate",
                "keys" => "key",
                _ => class,
            }
            .to_string();
        }
        // Attribute line: "    "key"<type>="value""
        else if let Some(eq_pos) = trimmed.find('=') {
            let key_part = &trimmed[..eq_pos];
            let value = trimmed[eq_pos + 1..].trim().trim_matches('"');

            // Extract the 4-char attribute name
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

    // Flush last item
    if !current_kind.is_empty() {
        if let Some(item) = build_item(&current_kind, &current_keychain, &attrs) {
            if kind_filter.is_none() || kind_filter.map(|k| k == item.kind).unwrap_or(true) {
                items.push(item);
            }
        }
    }

    items
}

fn build_item(
    kind: &str,
    keychain: &str,
    attrs: &std::collections::HashMap<String, String>,
) -> Option<KeychainItem> {
    let label = attrs
        .get("labl")
        .or(attrs.get("svce"))
        .or(attrs.get("srvr"))
        .cloned()
        .unwrap_or_default();

    if label.is_empty() {
        return None;
    }

    Some(KeychainItem {
        kind: kind.to_string(),
        label,
        account: attrs.get("acct").cloned().filter(|s| !s.is_empty()),
        service: attrs.get("svce").cloned().filter(|s| !s.is_empty()),
        server: attrs.get("srvr").cloned().filter(|s| !s.is_empty()),
        protocol: attrs.get("ptcl").cloned().filter(|s| !s.is_empty()),
        keychain: keychain.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dump() {
        let output = r#"keychain: "/Users/test/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    "acct"<blob>="testuser"
    "labl"<blob>="My App"
    "svce"<blob>="com.example.myapp"
keychain: "/Users/test/Library/Keychains/login.keychain-db"
version: 512
class: "inet"
attributes:
    "acct"<blob>="admin"
    "labl"<blob>="example.com"
    "srvr"<blob>="example.com"
    "ptcl"<uint32>="htps"
"#;
        let items = parse_dump(output, None);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "generic-password");
        assert_eq!(items[0].label, "My App");
        assert_eq!(items[0].account.as_deref(), Some("testuser"));
        assert_eq!(items[0].service.as_deref(), Some("com.example.myapp"));
        assert_eq!(items[1].kind, "internet-password");
        assert_eq!(items[1].server.as_deref(), Some("example.com"));
    }

    #[test]
    fn test_parse_dump_filter() {
        let output = r#"keychain: "/path/login.keychain-db"
class: "genp"
attributes:
    "labl"<blob>="App1"
    "svce"<blob>="svc1"
keychain: "/path/login.keychain-db"
class: "inet"
attributes:
    "labl"<blob>="Web1"
    "srvr"<blob>="web1.com"
"#;
        let items = parse_dump(output, Some("internet-password"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Web1");
    }

    #[test]
    fn test_parse_dump_empty() {
        assert!(parse_dump("", None).is_empty());
    }
}
