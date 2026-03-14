use super::util::{escape_jxa, run_command_with_timeout, run_jxa, slug, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ContactGroup {
    pub name: String,
}

/// List all contacts, optionally filtering by name search.
pub async fn list(search: Option<&str>) -> anyhow::Result<Vec<Contact>> {
    let query_filter = search.map(|q| q.to_lowercase());
    let mut records = query_contact_dbs().await?;

    if let Some(q) = query_filter {
        records.retain(|c| {
            c.name.to_lowercase().contains(&q)
                || c.first_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.last_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.organization
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.email.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || c.phone.as_deref().unwrap_or("").to_lowercase().contains(&q)
        });
    }

    Ok(records)
}

/// Get a single contact by ID with full details.
pub async fn get(id: &str) -> anyhow::Result<Contact> {
    let needle = slug(id);
    query_contact_dbs()
        .await?
        .into_iter()
        .find(|c| c.id == needle)
        .ok_or_else(|| anyhow::anyhow!("Contact not found: {id}"))
}

/// Create a new contact.
pub async fn create(
    first: &str,
    last: &str,
    email: Option<&str>,
    phone: Option<&str>,
    org: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let first_esc = escape_jxa(first);
    let last_esc = escape_jxa(last);

    let mut props = format!("firstName: \"{first_esc}\", lastName: \"{last_esc}\"");
    if let Some(o) = org {
        let o_esc = escape_jxa(o);
        props.push_str(&format!(", organization: \"{o_esc}\""));
    }

    let mut extra = String::new();
    if let Some(e) = email {
        let e_esc = escape_jxa(e);
        extra.push_str(&format!(
            "p.emails.push(app.Email({{value: \"{e_esc}\", label: \"work\"}}));\n"
        ));
    }
    if let Some(ph) = phone {
        let ph_esc = escape_jxa(ph);
        extra.push_str(&format!(
            "p.phones.push(app.Phone({{value: \"{ph_esc}\", label: \"mobile\"}}));\n"
        ));
    }

    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const p = app.Person({{{props}}});
app.people.push(p);
{extra}app.save();
p.id()
"#
    );

    let raw = run_jxa(&jxa_script).await?;
    let new_id = raw.trim().to_string();
    Ok(ActionResult::success_with_id("create", &new_id))
}

/// Update an existing contact by ID.
pub async fn update(
    id: &str,
    first: Option<&str>,
    last: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let escaped_id = escape_jxa(id);
    let mut updates = String::new();

    if let Some(f) = first {
        let f_esc = escape_jxa(f);
        updates.push_str(&format!("p.firstName = \"{f_esc}\";\n"));
    }
    if let Some(l) = last {
        let l_esc = escape_jxa(l);
        updates.push_str(&format!("p.lastName = \"{l_esc}\";\n"));
    }
    if let Some(e) = email {
        let e_esc = escape_jxa(e);
        updates.push_str(&format!(
            r#"
try {{
    const emails = p.emails();
    if (emails.length > 0) {{
        emails[0].value = "{e_esc}";
    }} else {{
        p.emails.push(app.Email({{value: "{e_esc}", label: "work"}}));
    }}
}} catch(e) {{
    p.emails.push(app.Email({{value: "{e_esc}", label: "work"}}));
}}
"#
        ));
    }
    if let Some(ph) = phone {
        let ph_esc = escape_jxa(ph);
        updates.push_str(&format!(
            r#"
try {{
    const phones = p.phones();
    if (phones.length > 0) {{
        phones[0].value = "{ph_esc}";
    }} else {{
        p.phones.push(app.Phone({{value: "{ph_esc}", label: "mobile"}}));
    }}
}} catch(e) {{
    p.phones.push(app.Phone({{value: "{ph_esc}", label: "mobile"}}));
}}
"#
        ));
    }

    if updates.is_empty() {
        return Ok(ActionResult::success_with_message(
            "update",
            "no fields to update",
        ));
    }

    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const p = app.people.byId("{escaped_id}");
{updates}app.save();
p.id()
"#
    );

    run_jxa(&jxa_script).await?;
    Ok(ActionResult::success_with_id("update", id))
}

/// Delete a contact by ID.
pub async fn delete(id: &str) -> anyhow::Result<ActionResult> {
    let escaped_id = escape_jxa(id);
    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const p = app.people.byId("{escaped_id}");
app.delete(p);
app.save();
"true"
"#
    );

    run_jxa(&jxa_script).await?;
    Ok(ActionResult::success_with_id("delete", id))
}

/// List all contact groups.
pub async fn groups() -> anyhow::Result<Vec<ContactGroup>> {
    let jxa_script = r#"
const app = Application("Contacts");
const groups = app.groups();
const results = [];
for (let i = 0; i < groups.length; i++) {
    results.push(groups[i].name());
}
results.join("\n")
"#;

    let raw = run_jxa(jxa_script).await?;
    let groups = raw
        .lines()
        .filter(|l| !l.is_empty())
        .map(|name| ContactGroup {
            name: name.to_string(),
        })
        .collect();
    Ok(groups)
}

async fn query_contact_dbs() -> anyhow::Result<Vec<Contact>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut db_paths = vec![format!(
        "{home}/Library/Application Support/AddressBook/AddressBook-v22.abcddb"
    )];

    if let Ok(source_paths) = run_command_with_timeout(
        "sh",
        &["-c", "find \"$HOME/Library/Application Support/AddressBook/Sources\" -name 'AddressBook-v22.abcddb' 2>/dev/null | sort"],
        std::time::Duration::from_secs(10),
    )
    .await
    {
        db_paths.extend(source_paths.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }

    db_paths.sort();
    db_paths.dedup();

    let query = r#"
SELECT
    lower(COALESCE(r.ZUNIQUEID, '')),
    COALESCE(r.ZFIRSTNAME, ''),
    COALESCE(r.ZLASTNAME, ''),
    COALESCE(r.ZORGANIZATION, ''),
    COALESCE((
        SELECT e.ZADDRESS
        FROM ZABCDEMAILADDRESS e
        WHERE e.ZOWNER = r.Z_PK
        ORDER BY e.ZISPRIMARY DESC, e.ZORDERINGINDEX ASC, e.Z_PK ASC
        LIMIT 1
    ), ''),
    COALESCE((
        SELECT p.ZFULLNUMBER
        FROM ZABCDPHONENUMBER p
        WHERE p.ZOWNER = r.Z_PK
        ORDER BY p.ZISPRIMARY DESC, p.ZORDERINGINDEX ASC, p.Z_PK ASC
        LIMIT 1
    ), '')
FROM ZABCDRECORD r
WHERE r.Z_ENT = 22
  AND (r.ZFIRSTNAME IS NOT NULL OR r.ZLASTNAME IS NOT NULL OR r.ZORGANIZATION IS NOT NULL)
ORDER BY r.ZLASTNAME, r.ZFIRSTNAME, r.ZORGANIZATION;
"#;

    let mut all = Vec::new();
    for db_path in db_paths {
        if tokio::fs::metadata(&db_path).await.is_err() {
            continue;
        }
        let stdout = match run_command_with_timeout(
            "sqlite3",
            &["-separator", "\t", &db_path, query.trim()],
            std::time::Duration::from_secs(20),
        )
        .await
        {
            Ok(s) => s,
            Err(_) => continue,
        };
        let lines: Vec<String> = stdout.lines().map(String::from).collect();
        all.extend(parse_output(&lines));
    }

    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    all.dedup_by(|a, b| a.id == b.id);
    Ok(all)
}

fn parse_output(lines: &[String]) -> Vec<Contact> {
    let mut records = Vec::new();

    for line in lines {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let contact_id = parts[0].trim();
        let first_name = parts.get(1).copied().unwrap_or("").trim();
        let last_name = parts.get(2).copied().unwrap_or("").trim();
        let org = parts.get(3).copied().unwrap_or("").trim();
        let email = parts.get(4).copied().unwrap_or("").trim();
        let phone = parts.get(5).copied().unwrap_or("").trim();

        let name = format!("{first_name} {last_name}").trim().to_string();
        let name = if name.is_empty() {
            if !org.is_empty() {
                org.to_string()
            } else {
                continue;
            }
        } else {
            name
        };

        let id = if contact_id.is_empty() {
            slug(&name)
        } else {
            slug(contact_id)
        };

        records.push(Contact {
            id,
            name,
            first_name: if first_name.is_empty() {
                None
            } else {
                Some(first_name.to_string())
            },
            last_name: if last_name.is_empty() {
                None
            } else {
                Some(last_name.to_string())
            },
            organization: if org.is_empty() {
                None
            } else {
                Some(org.to_string())
            },
            email: if email.is_empty() {
                None
            } else {
                Some(email.to_string())
            },
            phone: if phone.is_empty() {
                None
            } else {
                Some(phone.to_string())
            },
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let lines = vec![
            "ABC123\tAlice\tSmith\tAcme Corp\talice@example.com\t+15551234567".to_string(),
            "DEF456\tBob\tJones\t\tbob@test.com\t".to_string(),
        ];
        let records = parse_output(&lines);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Alice Smith");
        assert_eq!(records[0].first_name.as_deref(), Some("Alice"));
        assert_eq!(records[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(records[0].email.as_deref(), Some("alice@example.com"));
        assert_eq!(records[0].phone.as_deref(), Some("+15551234567"));
        assert_eq!(records[0].organization.as_deref(), Some("Acme Corp"));
        assert_eq!(records[1].name, "Bob Jones");
        assert!(records[1].phone.is_none());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output(&[]).is_empty());
    }

    #[test]
    fn test_parse_output_org_only() {
        let lines = vec!["\t\t\tAcme Corp\t\t".to_string()];
        let records = parse_output(&lines);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Acme Corp");
    }

    #[test]
    fn test_parse_output_skips_empty() {
        let lines = vec!["\t\t\t\t\t".to_string()];
        let records = parse_output(&lines);
        assert!(records.is_empty());
    }

    #[test]
    fn test_parse_output_first_last_fields() {
        let lines = vec!["ID1\tJane\tDoe\t\tjane@test.com\t".to_string()];
        let records = parse_output(&lines);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].first_name.as_deref(), Some("Jane"));
        assert_eq!(records[0].last_name.as_deref(), Some("Doe"));
        assert_eq!(records[0].name, "Jane Doe");
    }
}
