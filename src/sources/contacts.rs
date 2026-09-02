use super::util::{escape_jxa, run_command_with_timeout, run_jxa, slug, ActionResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LabeledValue {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PostalAddress {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emails: Vec<LabeledValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phones: Vec<LabeledValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<LabeledValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub postal_addresses: Vec<PostalAddress>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContactGroup {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct NewContact<'a> {
    pub first: &'a str,
    pub last: &'a str,
    pub middle: Option<&'a str>,
    pub nickname: Option<&'a str>,
    pub organization: Option<&'a str>,
    pub job_title: Option<&'a str>,
    pub department: Option<&'a str>,
    pub birthday: Option<&'a str>,
    pub note: Option<&'a str>,
    pub emails: &'a [String],
    pub phones: &'a [String],
}

#[derive(Debug, Default)]
pub struct ContactUpdates<'a> {
    pub first: Option<&'a str>,
    pub last: Option<&'a str>,
    pub middle: Option<&'a str>,
    pub nickname: Option<&'a str>,
    pub organization: Option<&'a str>,
    pub job_title: Option<&'a str>,
    pub department: Option<&'a str>,
    pub birthday: Option<&'a str>,
    pub note: Option<&'a str>,
    pub email: Option<&'a str>,
    pub phone: Option<&'a str>,
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
                || c.middle_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.nickname
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.organization
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.job_title
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.department
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || c.email.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || c.phone.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || c.emails
                    .iter()
                    .chain(c.phones.iter())
                    .chain(c.urls.iter())
                    .any(|value| value.value.to_lowercase().contains(&q))
        });
    }

    Ok(records)
}

/// Get a single contact by ID with full details.
pub async fn get(id: &str) -> anyhow::Result<Contact> {
    query_contact_dbs()
        .await?
        .into_iter()
        .find(|contact| contact.id.eq_ignore_ascii_case(id.trim()))
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
    let emails = email.map(String::from).into_iter().collect::<Vec<_>>();
    let phones = phone.map(String::from).into_iter().collect::<Vec<_>>();
    create_detailed(&NewContact {
        first,
        last,
        organization: org,
        emails: &emails,
        phones: &phones,
        ..Default::default()
    })
    .await
}

pub async fn create_detailed(contact: &NewContact<'_>) -> anyhow::Result<ActionResult> {
    if contact.first.trim().is_empty()
        && contact.last.trim().is_empty()
        && contact.organization.unwrap_or("").trim().is_empty()
    {
        anyhow::bail!("A first name, last name, or organization is required");
    }
    let jxa_script = build_create_script(contact);
    let raw = run_jxa(&jxa_script).await?;
    let new_id = raw.trim().to_string();
    Ok(ActionResult::success_with_id("create", &new_id))
}

fn push_prop(props: &mut Vec<String>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        props.push(format!("{name}: \"{}\"", escape_jxa(value)));
    }
}

fn build_create_script(contact: &NewContact<'_>) -> String {
    let mut props = vec![
        format!("firstName: \"{}\"", escape_jxa(contact.first)),
        format!("lastName: \"{}\"", escape_jxa(contact.last)),
    ];
    push_prop(&mut props, "middleName", contact.middle);
    push_prop(&mut props, "nickname", contact.nickname);
    push_prop(&mut props, "organization", contact.organization);
    push_prop(&mut props, "jobTitle", contact.job_title);
    push_prop(&mut props, "department", contact.department);
    push_prop(&mut props, "note", contact.note);
    if let Some(birthday) = contact.birthday {
        props.push(format!("birthDate: new Date(\"{}\")", escape_jxa(birthday)));
    }

    let mut extra = String::new();
    for email in contact.emails {
        extra.push_str(&format!(
            "p.emails.push(app.Email({{value: \"{}\", label: \"work\"}}));\n",
            escape_jxa(email)
        ));
    }
    for phone in contact.phones {
        extra.push_str(&format!(
            "p.phones.push(app.Phone({{value: \"{}\", label: \"mobile\"}}));\n",
            escape_jxa(phone)
        ));
    }

    format!(
        r#"
const app = Application("Contacts");
const p = app.Person({{{}}});
app.people.push(p);
{}app.save();
p.id()
"#,
        props.join(", "),
        extra
    )
}

/// Update an existing contact by ID.
pub async fn update(
    id: &str,
    first: Option<&str>,
    last: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
) -> anyhow::Result<ActionResult> {
    update_detailed(
        id,
        &ContactUpdates {
            first,
            last,
            email,
            phone,
            ..Default::default()
        },
    )
    .await
}

pub async fn update_detailed(
    id: &str,
    fields: &ContactUpdates<'_>,
) -> anyhow::Result<ActionResult> {
    let mut updates = String::new();

    if let Some(f) = fields.first {
        let f_esc = escape_jxa(f);
        updates.push_str(&format!("p.firstName = \"{f_esc}\";\n"));
    }
    if let Some(l) = fields.last {
        let l_esc = escape_jxa(l);
        updates.push_str(&format!("p.lastName = \"{l_esc}\";\n"));
    }
    for (property, value) in [
        ("middleName", fields.middle),
        ("nickname", fields.nickname),
        ("organization", fields.organization),
        ("jobTitle", fields.job_title),
        ("department", fields.department),
        ("note", fields.note),
    ] {
        if let Some(value) = value {
            updates.push_str(&format!("p.{property} = \"{}\";\n", escape_jxa(value)));
        }
    }
    if let Some(birthday) = fields.birthday {
        updates.push_str(&format!(
            "p.birthDate = new Date(\"{}\");\n",
            escape_jxa(birthday)
        ));
    }
    if let Some(e) = fields.email {
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
    if let Some(ph) = fields.phone {
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
        anyhow::bail!("Nothing to update");
    }

    let canonical_id = canonical_id(id).await;
    let escaped_id = escape_jxa(&canonical_id);
    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const p = app.people.byId("{escaped_id}");
{updates}app.save();
p.id()
"#
    );

    run_jxa(&jxa_script).await?;
    Ok(ActionResult::success_with_id("update", &canonical_id))
}

/// Delete a contact by ID.
pub async fn delete(id: &str) -> anyhow::Result<ActionResult> {
    let canonical_id = canonical_id(id).await;
    let escaped_id = escape_jxa(&canonical_id);
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
    Ok(ActionResult::success_with_id("delete", &canonical_id))
}

async fn canonical_id(id: &str) -> String {
    match query_contact_dbs().await {
        Ok(contacts) => contacts
            .into_iter()
            .find(|contact| contact.id.eq_ignore_ascii_case(id.trim()))
            .map(|contact| contact.id)
            .unwrap_or_else(|| id.trim().to_string()),
        Err(_) => id.trim().to_string(),
    }
}

/// List all contact groups.
pub async fn groups() -> anyhow::Result<Vec<ContactGroup>> {
    let jxa_script = r#"
const app = Application("Contacts");
const groups = app.groups();
const results = [];
for (let i = 0; i < groups.length; i++) {
    results.push({id: groups[i].id(), name: groups[i].name()});
}
JSON.stringify(results)
"#;

    let raw = run_jxa(jxa_script).await?;
    Ok(serde_json::from_str(&raw)?)
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
    COALESCE(r.ZUNIQUEID, '') AS id,
    COALESCE(r.ZFIRSTNAME, '') AS first_name,
    COALESCE(r.ZLASTNAME, '') AS last_name,
    COALESCE(r.ZMIDDLENAME, '') AS middle_name,
    COALESCE(r.ZNICKNAME, '') AS nickname,
    COALESCE(r.ZORGANIZATION, '') AS organization,
    COALESCE(r.ZJOBTITLE, '') AS job_title,
    COALESCE(r.ZDEPARTMENT, '') AS department,
    COALESCE(datetime(r.ZBIRTHDAY + 978307200, 'unixepoch'), '') AS birthday,
    COALESCE((
        SELECT e.ZADDRESS
        FROM ZABCDEMAILADDRESS e
        WHERE e.ZOWNER = r.Z_PK
        ORDER BY e.ZISPRIMARY DESC, e.ZORDERINGINDEX ASC, e.Z_PK ASC
        LIMIT 1
    ), '') AS email,
    COALESCE((
        SELECT p.ZFULLNUMBER
        FROM ZABCDPHONENUMBER p
        WHERE p.ZOWNER = r.Z_PK
        ORDER BY p.ZISPRIMARY DESC, p.ZORDERINGINDEX ASC, p.Z_PK ASC
        LIMIT 1
    ), '') AS phone,
    COALESCE((SELECT n.ZTEXT FROM ZABCDNOTE n WHERE n.ZCONTACT = r.Z_PK LIMIT 1), '') AS note,
    COALESCE((
        SELECT json_group_array(json_object('label', COALESCE(e.ZLABEL, ''), 'value', e.ZADDRESS))
        FROM ZABCDEMAILADDRESS e WHERE e.ZOWNER = r.Z_PK
        ORDER BY e.ZISPRIMARY DESC, e.ZORDERINGINDEX ASC, e.Z_PK ASC
    ), '[]') AS emails,
    COALESCE((
        SELECT json_group_array(json_object('label', COALESCE(p.ZLABEL, ''), 'value', p.ZFULLNUMBER))
        FROM ZABCDPHONENUMBER p WHERE p.ZOWNER = r.Z_PK
        ORDER BY p.ZISPRIMARY DESC, p.ZORDERINGINDEX ASC, p.Z_PK ASC
    ), '[]') AS phones,
    COALESCE((
        SELECT json_group_array(json_object('label', COALESCE(u.ZLABEL, ''), 'value', u.ZURL))
        FROM ZABCDURLADDRESS u WHERE u.ZOWNER = r.Z_PK
        ORDER BY u.ZISPRIMARY DESC, u.ZORDERINGINDEX ASC, u.Z_PK ASC
    ), '[]') AS urls,
    COALESCE((
        SELECT json_group_array(json_object(
            'label', COALESCE(address.ZLABEL, ''),
            'street', COALESCE(address.ZSTREET, ''),
            'city', COALESCE(address.ZCITY, ''),
            'state', COALESCE(address.ZSTATE, ''),
            'postal_code', COALESCE(address.ZZIPCODE, ''),
            'country', COALESCE(address.ZCOUNTRYNAME, ''),
            'country_code', COALESCE(address.ZCOUNTRYCODE, '')
        ))
        FROM ZABCDPOSTALADDRESS address WHERE address.ZOWNER = r.Z_PK
        ORDER BY address.ZISPRIMARY DESC, address.ZORDERINGINDEX ASC, address.Z_PK ASC
    ), '[]') AS postal_addresses
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
            &["-json", &db_path, query.trim()],
            std::time::Duration::from_secs(20),
        )
        .await
        {
            Ok(s) => s,
            Err(_) => continue,
        };
        all.extend(parse_json_rows(&stdout));
    }

    all.sort_by_key(|contact| contact.name.to_lowercase());
    all.dedup_by(|a, b| a.id == b.id);
    Ok(all)
}

fn optional_string(row: &serde_json::Value, key: &str) -> Option<String> {
    row[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

fn friendly_label(label: &str) -> String {
    label
        .strip_prefix("_!<$!>")
        .and_then(|label| label.strip_suffix("!<$_!>"))
        .unwrap_or(label)
        .to_lowercase()
}

fn labeled_values(row: &serde_json::Value, key: &str) -> Vec<LabeledValue> {
    let Some(raw) = row[key].as_str() else {
        return Vec::new();
    };
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    values
        .into_iter()
        .filter_map(|value| {
            let item = value["value"].as_str()?.trim();
            if item.is_empty() {
                return None;
            }
            Some(LabeledValue {
                label: friendly_label(value["label"].as_str().unwrap_or("")),
                value: item.to_string(),
            })
        })
        .collect()
}

fn postal_addresses(row: &serde_json::Value) -> Vec<PostalAddress> {
    let Some(raw) = row["postal_addresses"].as_str() else {
        return Vec::new();
    };
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    values
        .into_iter()
        .map(|value| PostalAddress {
            label: friendly_label(value["label"].as_str().unwrap_or("")),
            street: optional_string(&value, "street"),
            city: optional_string(&value, "city"),
            state: optional_string(&value, "state"),
            postal_code: optional_string(&value, "postal_code"),
            country: optional_string(&value, "country"),
            country_code: optional_string(&value, "country_code"),
        })
        .collect()
}

fn parse_json_rows(output: &str) -> Vec<Contact> {
    let rows: Vec<serde_json::Value> = match serde_json::from_str(output.trim()) {
        Ok(rows) => rows,
        Err(_) if output.trim().is_empty() => return Vec::new(),
        Err(error) => {
            log::warn!("Skipping unparseable contacts output: {error}");
            return Vec::new();
        }
    };
    let mut records = Vec::new();
    for row in &rows {
        let contact_id = row["id"].as_str().unwrap_or("").trim();
        let first_name = row["first_name"].as_str().unwrap_or("").trim();
        let last_name = row["last_name"].as_str().unwrap_or("").trim();
        let org = row["organization"].as_str().unwrap_or("").trim();

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
            contact_id.to_string()
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
            middle_name: optional_string(row, "middle_name"),
            nickname: optional_string(row, "nickname"),
            organization: if org.is_empty() {
                None
            } else {
                Some(org.to_string())
            },
            job_title: optional_string(row, "job_title"),
            department: optional_string(row, "department"),
            birthday: optional_string(row, "birthday"),
            note: optional_string(row, "note"),
            email: optional_string(row, "email"),
            phone: optional_string(row, "phone"),
            emails: labeled_values(row, "emails"),
            phones: labeled_values(row, "phones"),
            urls: labeled_values(row, "urls"),
            postal_addresses: postal_addresses(row),
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = r#"[
          {"id":"ABC123","first_name":"Alice","last_name":"Smith","middle_name":"Q","nickname":"Al","organization":"Acme Corp","job_title":"CEO","department":"Ops","birthday":"1990-01-02 00:00:00","email":"alice@example.com","phone":"+15551234567","note":"hello","emails":"[{\"label\":\"_!<$!>Work!<$_!>\",\"value\":\"alice@example.com\"}]","phones":"[{\"label\":\"mobile\",\"value\":\"+15551234567\"}]","urls":"[]","postal_addresses":"[{\"label\":\"home\",\"street\":\"1 Main St\",\"city\":\"Seattle\",\"state\":\"WA\",\"postal_code\":\"98101\",\"country\":\"United States\",\"country_code\":\"US\"}]"},
          {"id":"DEF456","first_name":"Bob","last_name":"Jones","middle_name":"","nickname":"","organization":"","job_title":"","department":"","birthday":"","email":"bob@test.com","phone":"","note":"","emails":"[]","phones":"[]","urls":"[]"}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Alice Smith");
        assert_eq!(records[0].first_name.as_deref(), Some("Alice"));
        assert_eq!(records[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(records[0].email.as_deref(), Some("alice@example.com"));
        assert_eq!(records[0].phone.as_deref(), Some("+15551234567"));
        assert_eq!(records[0].organization.as_deref(), Some("Acme Corp"));
        assert_eq!(records[0].middle_name.as_deref(), Some("Q"));
        assert_eq!(records[0].emails[0].label, "work");
        assert_eq!(
            records[0].postal_addresses[0].city.as_deref(),
            Some("Seattle")
        );
        assert_eq!(records[1].name, "Bob Jones");
        assert!(records[1].phone.is_none());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_json_rows("").is_empty());
    }

    #[test]
    fn test_parse_output_org_only() {
        let records = parse_json_rows(
            r#"[{"id":"","first_name":"","last_name":"","organization":"Acme Corp","emails":"[]","phones":"[]","urls":"[]"}]"#,
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Acme Corp");
    }

    #[test]
    fn test_parse_output_skips_empty() {
        let records = parse_json_rows(
            r#"[{"id":"","first_name":"","last_name":"","organization":"","emails":"[]","phones":"[]","urls":"[]"}]"#,
        );
        assert!(records.is_empty());
    }

    #[test]
    fn test_parse_output_first_last_fields() {
        let records = parse_json_rows(
            r#"[{"id":"ID1","first_name":"Jane","last_name":"Doe","organization":"","email":"jane@test.com","emails":"[]","phones":"[]","urls":"[]"}]"#,
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].first_name.as_deref(), Some("Jane"));
        assert_eq!(records[0].last_name.as_deref(), Some("Doe"));
        assert_eq!(records[0].name, "Jane Doe");
    }

    #[test]
    fn detailed_create_includes_richer_fields_and_multiple_values() {
        let emails = vec!["one@example.com".to_string(), "two@example.com".to_string()];
        let script = build_create_script(&NewContact {
            first: "Jane",
            last: "Doe",
            job_title: Some("Engineer"),
            birthday: Some("1990-01-02"),
            emails: &emails,
            ..Default::default()
        });
        assert!(script.contains("jobTitle: \"Engineer\""));
        assert!(script.contains("birthDate: new Date(\"1990-01-02\")"));
        assert_eq!(script.matches("app.Email").count(), 2);
    }

    #[tokio::test]
    async fn empty_create_and_update_fail_before_automation() {
        assert!(create_detailed(&NewContact::default()).await.is_err());
        assert!(update_detailed("unused", &ContactUpdates::default())
            .await
            .is_err());
    }
}
