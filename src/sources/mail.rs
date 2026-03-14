use super::util::{escape_jxa, run_jxa, run_jxa_with_timeout, ActionResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MailMessage {
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
}

#[derive(Debug, Serialize)]
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
    // Use JXA instead of AppleScript — avoids `read` keyword conflicts
    // and is faster for bulk property access.
    let script = r#"
const app = Application("Mail");
const inbox = app.inbox();
const msgSpec = inbox.messages;
const total = msgSpec.length;
if (total === 0) { ""; } else {
    const subjects = msgSpec.subject();
    const senders = msgSpec.sender();
    const dates = msgSpec.dateReceived();
    const readStatuses = msgSpec.readStatus();

    const rows = [];
    for (let i = 0; i < total; i++) {
        rows.push({
            subject: subjects[i] || "",
            sender: senders[i] || "",
            date: dates[i] || null,
            isRead: !!readStatuses[i],
        });
    }

    rows.sort((a, b) => {
        const aTime = a.date ? a.date.getTime() : 0;
        const bTime = b.date ? b.date.getTime() : 0;
        return bTime - aTime;
    });

    rows.slice(0, 50).map((row) => {
        const subj = row.subject.replace(/[\t\n\r]/g, " ");
        const sndr = row.sender.replace(/[\t\n\r]/g, " ");
        const dt = row.date ? row.date.toISOString() : "";
        const rd = row.isRead ? "1" : "0";
        return [subj, sndr, dt, rd].join("\t");
    }).join("\n");
}
"#;

    let output = run_jxa_with_timeout(script, std::time::Duration::from_secs(60)).await?;
    if output.is_empty() {
        anyhow::bail!("Mail inbox is empty or Mail.app is not configured");
    }
    Ok(parse_output(&output))
}

pub async fn get(idx: usize) -> anyhow::Result<MailMessageDetail> {
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
    const subjects = msgs.subject();
    const senders = msgs.sender();
    const dates = msgs.dateReceived();
    const readStatuses = msgs.readStatus();
    const rows = [];
    for (let j = 0; j < total; j++) {{
        rows.push({{
            msg: msgs[j],
            subject: subjects[j] || "",
            sender: senders[j] || "",
            date: dates[j] || null,
            isRead: !!readStatuses[j],
        }});
    }}
    rows.sort((a, b) => {{
        const aTime = a.date ? a.date.getTime() : 0;
        const bTime = b.date ? b.date.getTime() : 0;
        return bTime - aTime;
    }});
    const m = rows[i].msg;
    const subj = (rows[i].subject || "").replace(/[\t\n\r]/g, " ");
    const sndr = (rows[i].sender || "").replace(/[\t\n\r]/g, " ");
    const dt = rows[i].date ? rows[i].date.toISOString() : "";
    const rd = rows[i].isRead ? "1" : "0";
    let body = "";
    try {{ body = (m.content() || "").substring(0, 500).replace(/[\t\n\r]/g, " "); }} catch(e) {{}}
    [subj, sndr, dt, rd, body].join("\t");
}}
"#,
        idx
    );

    let output = run_jxa(&script).await?;
    if output.starts_with("ERROR:") {
        anyhow::bail!("{}", output);
    }

    let parts: Vec<&str> = output.split('\t').collect();
    if parts.len() < 4 {
        anyhow::bail!("Unexpected output from Mail.app");
    }

    let body = parts.get(4).map(|s| s.trim().to_string());

    Ok(MailMessageDetail {
        subject: parts[0].trim().to_string(),
        sender: parts[1].trim().to_string(),
        date_received: parts[2].trim().to_string(),
        is_read: parts[3].trim() == "1",
        mailbox: "INBOX".to_string(),
        body_preview: body.filter(|s| !s.is_empty()),
    })
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
    let script = r#"
const app = Application("Mail");
const names = app.mailboxes.name();
names.join("\n");
"#;

    let output = run_jxa(script).await?;
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

fn parse_output(output: &str) -> Vec<MailMessage> {
    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let subject = parts[0].trim();
        if subject.is_empty() {
            continue;
        }
        let sender = parts[1].trim();
        let date_received = parts[2].trim();
        let is_read = parts[3].trim() == "1";

        records.push(MailMessage {
            subject: subject.to_string(),
            sender: sender.to_string(),
            date_received: date_received.to_string(),
            is_read,
            mailbox: "INBOX".to_string(),
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "Hello World\tjohn@example.com\t2026-03-14T10:30:00.000Z\t1\n\
                       Meeting invite\tboss@work.com\t2026-03-13T15:00:00.000Z\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].subject, "Hello World");
        assert_eq!(records[0].sender, "john@example.com");
        assert!(records[0].is_read);
        assert!(!records[1].is_read);
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
