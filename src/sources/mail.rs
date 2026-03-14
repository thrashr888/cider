use super::util::{escape_jxa, run_command_with_timeout, run_jxa, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MailMessage {
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailMessageDetail {
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Mailbox {
    pub name: String,
}

pub async fn list() -> anyhow::Result<Vec<MailMessage>> {
    let records = query_inbox_messages(50).await?;
    if records.is_empty() {
        anyhow::bail!("Mail inbox is empty or Mail.app is not configured");
    }
    Ok(records
        .into_iter()
        .map(|m| MailMessage {
            subject: m.subject,
            sender: m.sender,
            date_received: m.date_received,
            is_read: m.is_read,
            mailbox: m.mailbox,
        })
        .collect())
}

pub async fn get(idx: usize) -> anyhow::Result<MailMessageDetail> {
    let records = query_inbox_messages(50).await?;
    if idx == 0 || idx > records.len() {
        anyhow::bail!("Message index out of range");
    }
    Ok(records[idx - 1].clone())
}

pub async fn read(idx: usize) -> anyhow::Result<ActionResult> {
    let script = format!(
        r#"
const app = Application("Mail");
const inbox = app.inbox();
const msgs = inbox.messages;
const total = msgs.length;
const i = {} - 1;
if (i < 0 || i >= total) {{
    "ERROR: Message index out of range";
}} else {{
    const dates = msgs.dateReceived();
    const rows = [];
    for (let j = 0; j < total; j++) {{
        rows.push({{ msg: msgs[j], date: dates[j] || null }});
    }}
    rows.sort((a, b) => {{
        const aTime = a.date ? a.date.getTime() : 0;
        const bTime = b.date ? b.date.getTime() : 0;
        return bTime - aTime;
    }});
    rows[i].msg.readStatus = true;
    "done";
}}
"#,
        idx
    );

    let output = run_jxa(&script).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }
    Ok(ActionResult::success("read"))
}

pub async fn unread(idx: usize) -> anyhow::Result<ActionResult> {
    let script = format!(
        r#"
const app = Application("Mail");
const inbox = app.inbox();
const msgs = inbox.messages;
const total = msgs.length;
const i = {} - 1;
if (i < 0 || i >= total) {{
    "ERROR: Message index out of range";
}} else {{
    const dates = msgs.dateReceived();
    const rows = [];
    for (let j = 0; j < total; j++) {{
        rows.push({{ msg: msgs[j], date: dates[j] || null }});
    }}
    rows.sort((a, b) => {{
        const aTime = a.date ? a.date.getTime() : 0;
        const bTime = b.date ? b.date.getTime() : 0;
        return bTime - aTime;
    }});
    rows[i].msg.readStatus = false;
    "done";
}}
"#,
        idx
    );

    let output = run_jxa(&script).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }
    Ok(ActionResult::success("unread"))
}

pub async fn trash(idx: usize) -> anyhow::Result<ActionResult> {
    let script = format!(
        r#"
const app = Application("Mail");
const inbox = app.inbox();
const msgs = inbox.messages;
const total = msgs.length;
const i = {} - 1;
if (i < 0 || i >= total) {{
    "ERROR: Message index out of range";
}} else {{
    const dates = msgs.dateReceived();
    const rows = [];
    for (let j = 0; j < total; j++) {{
        rows.push({{ msg: msgs[j], date: dates[j] || null }});
    }}
    rows.sort((a, b) => {{
        const aTime = a.date ? a.date.getTime() : 0;
        const bTime = b.date ? b.date.getTime() : 0;
        return bTime - aTime;
    }});
    app.delete(rows[i].msg);
    "done";
}}
"#,
        idx
    );

    let output = run_jxa(&script).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }
    Ok(ActionResult::success("trash"))
}

pub async fn mailboxes() -> anyhow::Result<Vec<Mailbox>> {
    let db_path = mail_db_path()?;
    let query = r#"
SELECT url FROM mailboxes ORDER BY ROWID ASC;
"#;
    let output = run_command_with_timeout(
        "sqlite3",
        &[&db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| Mailbox {
            name: l.to_string(),
        })
        .collect())
}

pub async fn send(to: &str, subject: &str, body: &str) -> anyhow::Result<ActionResult> {
    let script = format!(
        r#"
const app = Application("Mail");
const msg = app.OutgoingMessage({{
    subject: "{}",
    content: "{}"
}});
app.outgoingMessages.push(msg);
msg.toRecipients.push(app.Recipient({{address: "{}"}}));
msg.send();
"done";
"#,
        escape_jxa(subject),
        escape_jxa(body),
        escape_jxa(to)
    );

    run_jxa(&script).await?;
    Ok(ActionResult::success_with_message(
        "send",
        &format!("Sent to {to}"),
    ))
}

fn mail_db_path() -> anyhow::Result<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/Library/Mail/V10/MailData/Envelope Index");
    if std::path::Path::new(&path).exists() {
        Ok(path)
    } else {
        anyhow::bail!("Mail envelope index not found")
    }
}

async fn query_inbox_messages(limit: usize) -> anyhow::Result<Vec<MailMessageDetail>> {
    let db_path = mail_db_path()?;
    let query = format!(
        r#"
SELECT
    COALESCE(s.subject, ''),
    COALESCE(a.address, ''),
    datetime(m.date_received, 'unixepoch'),
    m.read,
    COALESCE(mb.url, 'INBOX'),
    COALESCE(sm.summary, '')
FROM messages m
LEFT JOIN addresses a ON m.sender = a.ROWID
LEFT JOIN subjects s ON m.subject = s.ROWID
LEFT JOIN summaries sm ON m.summary = sm.ROWID
LEFT JOIN mailboxes mb ON m.mailbox = mb.ROWID
WHERE m.mailbox IN (SELECT ROWID FROM mailboxes WHERE url LIKE '%/INBOX')
  AND m.deleted = 0
ORDER BY m.date_received DESC
LIMIT {limit};
"#
    );
    let output = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", &db_path, query.trim()],
        std::time::Duration::from_secs(20),
    )
    .await?;

    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let subject = parts[0].trim().to_string();
        let sender = parts[1].trim().to_string();
        let date_received = parts[2].trim().to_string();
        let is_read = parts[3].trim() == "1";
        let mailbox = parts[4].trim().to_string();
        let body_preview = parts
            .get(5)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        records.push(MailMessageDetail {
            subject,
            sender,
            date_received,
            is_read,
            mailbox,
            body_preview,
        });
    }
    Ok(records)
}
