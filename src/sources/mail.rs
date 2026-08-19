use super::util::{
    escape_jxa, run_command_with_timeout, run_jxa, run_osascript_with_timeout, ActionResult,
    SUBPROCESS_TIMEOUT,
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
    let script = build_mutation_script(&action);

    let output = run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }
    Ok(())
}

/// Wrap a Mail mutation in error reporting and an AppleEvent timeout.
///
/// The `with timeout` wrapper matters: a `whose id is` scan over a large
/// inbox takes longer than the default AppleEvent timeout, which gives up
/// with -1712 partway through — so both the AppleScript side and the Rust
/// side (`SUBPROCESS_TIMEOUT`) need generous limits.
fn build_mutation_script(action: &str) -> String {
    format!(
        r#"
with timeout of 600 seconds
tell application "Mail"
    try
        {action}
        return "done"
    on error errMsg
        return "ERROR: " & errMsg
    end try
end tell
end timeout
"#
    )
}

async fn query_inbox_messages(limit: usize) -> anyhow::Result<Vec<MailMessageRecord>> {
    let db_path = mail_db_path()?;
    let query = format!(
        r#"
SELECT
    m.ROWID AS rowid,
    COALESCE(s.subject, '') AS subject,
    COALESCE(a.address, '') AS sender,
    datetime(m.date_received, 'unixepoch') AS date_received,
    m.read AS is_read,
    COALESCE(mb.url, 'INBOX') AS mailbox_url,
    COALESCE(sm.summary, '') AS summary
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
        &["-json", &db_path, query.trim()],
        std::time::Duration::from_secs(20),
    )
    .await?;

    Ok(parse_json_rows(&output))
}

/// A JSON field that sqlite3 may emit as a number or a string, read as i64.
fn json_i64(value: &serde_json::Value) -> i64 {
    match value {
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Parse `sqlite3 -json` output into inbox records. JSON is the point:
/// `sm.summary` is prose that routinely contains newlines, and the old
/// tab-separated line-per-record read sheared any multiline summary into
/// broken rows that were silently dropped or misattributed.
fn parse_json_rows(output: &str) -> Vec<MailMessageRecord> {
    let output = output.trim();
    if output.is_empty() {
        return Vec::new();
    }

    let rows: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Skipping unparseable mail output: {e}");
            return Vec::new();
        }
    };

    let mut records = Vec::new();
    for row in &rows {
        let apple_mail_id = json_i64(&row["rowid"]);
        if apple_mail_id == 0 {
            continue;
        }
        let subject = row["subject"].as_str().unwrap_or("").trim().to_string();
        let sender = row["sender"].as_str().unwrap_or("").trim().to_string();
        let date_received = row["date_received"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        let is_read = json_i64(&row["is_read"]) == 1;
        let mailbox = row["mailbox_url"].as_str().unwrap_or("").trim().to_string();
        let body_preview = row["summary"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
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
    records
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_rows() {
        let output = r#"[
            {"rowid":42,"subject":"Meeting tomorrow","sender":"alice@example.com","date_received":"2026-02-07 12:00:00","is_read":0,"mailbox_url":"imap://user@mail.example.com/INBOX","summary":"Can we move it to 3pm?"},
            {"rowid":43,"subject":"Re: Meeting tomorrow","sender":"bob@example.com","date_received":"2026-02-07 12:05:00","is_read":1,"mailbox_url":"imap://user@mail.example.com/INBOX","summary":""}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].apple_mail_id, 42);
        assert_eq!(records[0].detail.subject, "Meeting tomorrow");
        assert_eq!(records[0].detail.sender, "alice@example.com");
        assert_eq!(records[0].detail.mailbox, "INBOX");
        assert!(!records[0].detail.is_read);
        assert_eq!(
            records[0].detail.body_preview.as_deref(),
            Some("Can we move it to 3pm?")
        );
        assert!(records[1].detail.is_read);
        assert!(
            records[1].detail.body_preview.is_none(),
            "empty summary must be None"
        );
    }

    #[test]
    fn test_parse_json_rows_empty() {
        assert!(parse_json_rows("").is_empty());
        assert!(parse_json_rows("[]").is_empty());
    }

    /// The reason for `-json`: a multiline summary used to shear one message
    /// into several broken TSV rows. The full summary must come back intact
    /// on a single record.
    #[test]
    fn test_parse_json_rows_multiline_summary_survives() {
        let summary = "First line of the preview.\nSecond line\twith a tab.\n\nFourth line.";
        let output = serde_json::json!([{
            "rowid": 7,
            "subject": "Newsletter",
            "sender": "news@example.com",
            "date_received": "2026-02-07 09:00:00",
            "is_read": 0,
            "mailbox_url": "imap://user@mail.example.com/INBOX",
            "summary": summary,
        }])
        .to_string();

        let records = parse_json_rows(&output);
        assert_eq!(records.len(), 1, "one message must parse as one record");
        assert_eq!(records[0].detail.body_preview.as_deref(), Some(summary));
    }

    /// Without the AppleEvent timeout wrapper a `whose id is` scan over a
    /// big inbox dies with -1712 before it finds the message.
    #[test]
    fn test_mutation_script_wraps_applescript_timeout() {
        let script = build_mutation_script("delete (first message of inbox whose id is 42)");
        assert!(script
            .trim_start()
            .starts_with("with timeout of 600 seconds"));
        assert!(script.trim_end().ends_with("end timeout"));
        assert!(script.contains("delete (first message of inbox whose id is 42)"));
        assert!(script.contains("on error errMsg"));
    }
}
