use super::util::{run_jxa, run_jxa_with_timeout, slug};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<Contact>> {
    let count_str = run_jxa(r#"Application("Contacts").people().length"#).await?;
    let total: usize = count_str.trim().parse().unwrap_or(0);
    if total == 0 {
        return Ok(vec![]);
    }

    eprintln!("Contacts: found {total}, fetching in chunks...");

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
            Err(e) => eprintln!("Contacts: error fetching chunk {start}..{end}: {e}"),
        }
    }

    Ok(parse_output(&all_lines))
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
}
