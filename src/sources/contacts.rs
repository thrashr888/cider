use super::util::{escape_jxa, run_jxa, run_jxa_with_timeout, slug, ActionResult};
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
    if let Some(query) = search {
        return search_contacts(query).await;
    }

    let count_str = run_jxa(r#"Application("Contacts").people().length"#).await?;
    let total: usize = count_str.trim().parse().unwrap_or(0);
    if total == 0 {
        return Ok(vec![]);
    }

    let chunk_size = 200usize;
    let mut all_lines = Vec::new();

    for start in (0..total).step_by(chunk_size) {
        let end = (start + chunk_size).min(total);

        let jxa_script = format!(
            r#"
const app = Application("Contacts");
const results = [];
const people = app.people();
const end = Math.min({end}, people.length);

for (let i = {start}; i < end; i++) {{
    const p = people[i];
    let firstName = "", lastName = "", org = "", email = "", phone = "";
    try {{ firstName = (p.firstName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    try {{ lastName = (p.lastName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    try {{ org = (p.organization() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    try {{
        const emails = p.emails();
        if (emails.length > 0) email = emails[0].value() || "";
    }} catch(e) {{}}
    try {{
        const phones = p.phones();
        if (phones.length > 0) phone = phones[0].value() || "";
    }} catch(e) {{}}
    results.push([p.id(), firstName, lastName, org, email, phone].join("\t"));
}}
results.join("\n")
"#,
            start = start,
            end = end,
        );

        match run_jxa_with_timeout(&jxa_script, std::time::Duration::from_secs(120)).await {
            Ok(chunk) => {
                if !chunk.is_empty() {
                    all_lines.extend(chunk.lines().map(String::from));
                }
            }
            Err(e) => return Err(e),
        }
    }

    Ok(parse_output(&all_lines))
}

/// Search contacts by name using `whose` clause.
async fn search_contacts(query: &str) -> anyhow::Result<Vec<Contact>> {
    let escaped = escape_jxa(query);
    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const results = [];
const q = "{escaped}".toLowerCase();
const people = app.people();
for (let i = 0; i < people.length; i++) {{
    const p = people[i];
    let firstName = "", lastName = "", org = "", email = "", phone = "";
    try {{ firstName = (p.firstName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    try {{ lastName = (p.lastName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    try {{ org = (p.organization() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    const fullName = (firstName + " " + lastName).toLowerCase();
    if (fullName.indexOf(q) === -1 && org.toLowerCase().indexOf(q) === -1) continue;
    try {{
        const emails = p.emails();
        if (emails.length > 0) email = emails[0].value() || "";
    }} catch(e) {{}}
    try {{
        const phones = p.phones();
        if (phones.length > 0) phone = phones[0].value() || "";
    }} catch(e) {{}}
    results.push([p.id(), firstName, lastName, org, email, phone].join("\t"));
}}
results.join("\n")
"#
    );

    let raw = run_jxa_with_timeout(&jxa_script, std::time::Duration::from_secs(120)).await?;
    if raw.is_empty() {
        return Ok(vec![]);
    }
    let lines: Vec<String> = raw.lines().map(String::from).collect();
    Ok(parse_output(&lines))
}

/// Get a single contact by ID with full details.
pub async fn get(id: &str) -> anyhow::Result<Contact> {
    let escaped_id = escape_jxa(id);
    let jxa_script = format!(
        r#"
const app = Application("Contacts");
const p = app.people.byId("{escaped_id}");
let firstName = "", lastName = "", org = "", email = "", phone = "";
try {{ firstName = (p.firstName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
try {{ lastName = (p.lastName() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
try {{ org = (p.organization() || "").replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
try {{
    const emails = p.emails();
    if (emails.length > 0) email = emails[0].value() || "";
}} catch(e) {{}}
try {{
    const phones = p.phones();
    if (phones.length > 0) phone = phones[0].value() || "";
}} catch(e) {{}}
[p.id(), firstName, lastName, org, email, phone].join("\t")
"#
    );

    let raw = run_jxa(&jxa_script).await?;
    let lines: Vec<String> = raw.lines().map(String::from).collect();
    let contacts = parse_output(&lines);
    contacts
        .into_iter()
        .next()
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
