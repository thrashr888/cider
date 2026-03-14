use super::util::{
    escape_jxa, run_command_with_timeout, run_jxa, run_osascript_with_timeout, ActionResult,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MailMessage {
    pub id: String,
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
    pub mailbox_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailMessageDetail {
    pub id: String,
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
    pub mailbox_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

#[derive(Debug, Clone)]
struct MailMessageRecord {
    pub apple_mail_id: i64,
    pub detail: MailMessageDetail,
}

#[derive(Debug, Serialize)]
pub struct Mailbox {
    pub name: String,
    pub url: String,
}

pub async fn list() -> anyhow::Result<Vec<MailMessage>> {
    let records = query_inbox_messages(50).await?;
    if records.is_empty() {
        anyhow::bail!("Mail inbox is empty or Mail.app is not configured");
    }
    Ok(records
        .into_iter()
        .map(|m| MailMessage {
            id: m.apple_mail_id.to_string(),
            subject: m.detail.subject,
            sender: m.detail.sender,
            date_received: m.detail.date_received,
            is_read: m.detail.is_read,
            mailbox: m.detail.mailbox,
            mailbox_url: m.detail.mailbox_url,
        })
        .collect())
}

pub async fn get(idx: usize) -> anyhow::Result<MailMessageDetail> {
    Ok(inbox_message_for_index(idx, 50).await?.detail)
}

pub async fn read(idx: usize) -> anyhow::Result<ActionResult> {
    let record = inbox_message_for_index(idx, 50).await?;
    mutate_inbox_message_by_id(record.apple_mail_id, |target| {
        format!("set read status of ({target}) to true")
    })
    .await?;
    Ok(ActionResult::success("read"))
}

pub async fn unread(idx: usize) -> anyhow::Result<ActionResult> {
    let record = inbox_message_for_index(idx, 50).await?;
    mutate_inbox_message_by_id(record.apple_mail_id, |target| {
        format!("set read status of ({target}) to false")
    })
    .await?;
    Ok(ActionResult::success("unread"))
}

pub async fn trash(idx: usize) -> anyhow::Result<ActionResult> {
    let record = inbox_message_for_index(idx, 50).await?;
    mutate_inbox_message_by_id(record.apple_mail_id, |target| format!("delete ({target})")).await?;
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
            name: mailbox_display_name(l),
            url: l.to_string(),
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

async fn inbox_message_for_index(idx: usize, limit: usize) -> anyhow::Result<MailMessageRecord> {
    if idx == 0 {
        anyhow::bail!("Message index out of range");
    }

    let records = query_inbox_messages(limit).await?;
    records
        .into_iter()
        .nth(idx - 1)
        .ok_or_else(|| anyhow::anyhow!("Message index out of range"))
}

async fn mutate_inbox_message_by_id<F>(apple_mail_id: i64, build_action: F) -> anyhow::Result<()>
where
    F: FnOnce(&str) -> String,
{
    let target = format!("first message of inbox whose id is {apple_mail_id}");
    let action = build_action(&target);
    let script = format!(
        r#"
tell application "Mail"
    try
        {action}
        return "done"
    on error errMsg
        return "ERROR: " & errMsg
    end try
end tell
"#
    );

    let output = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }
    Ok(())
}

async fn query_inbox_messages(limit: usize) -> anyhow::Result<Vec<MailMessageRecord>> {
    let db_path = mail_db_path()?;
    let query = format!(
        r#"
SELECT
    m.ROWID,
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
        if parts.len() < 6 {
            continue;
        }
        let apple_mail_id: i64 = match parts[0].trim().parse() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let subject = parts[1].trim().to_string();
        let sender = parts[2].trim().to_string();
        let date_received = parts[3].trim().to_string();
        let is_read = parts[4].trim() == "1";
        let mailbox = parts[5].trim().to_string();
        let body_preview = parts
            .get(6)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let id = apple_mail_id.to_string();
        let mailbox_url = mailbox.clone();
        records.push(MailMessageRecord {
            apple_mail_id,
            detail: MailMessageDetail {
                id,
                subject,
                sender,
                date_received,
                is_read,
                mailbox: mailbox_display_name(&mailbox),
                mailbox_url,
                body_preview,
            },
        });
    }
    Ok(records)
}

fn mailbox_display_name(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(url)
        .to_string()
}
