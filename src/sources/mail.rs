use super::util::run_jxa_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MailMessage {
    pub subject: String,
    pub sender: String,
    pub date_received: String,
    pub is_read: bool,
    pub mailbox: String,
}

pub async fn fetch() -> anyhow::Result<Vec<MailMessage>> {
    // Use JXA instead of AppleScript — avoids `read` keyword conflicts
    // and is faster for bulk property access.
    let script = r#"
const app = Application("Mail");
const inbox = app.inbox();
const msgSpec = inbox.messages;
const count = Math.min(msgSpec.length, 50);
if (count === 0) { ""; } else {
    const subjects = msgSpec.subject().slice(0, count);
    const senders = msgSpec.sender().slice(0, count);
    const dates = msgSpec.dateReceived().slice(0, count);
    const readStatuses = msgSpec.readStatus().slice(0, count);
    const results = [];
    for (let i = 0; i < count; i++) {
        const subj = (subjects[i] || "").replace(/[\t\n\r]/g, " ");
        const sndr = (senders[i] || "").replace(/[\t\n\r]/g, " ");
        const dt = dates[i] ? dates[i].toISOString() : "";
        const rd = readStatuses[i] ? "1" : "0";
        results.push([subj, sndr, dt, rd].join("\t"));
    }
    results.join("\n");
}
"#;

    let output = run_jxa_with_timeout(script, std::time::Duration::from_secs(60)).await?;
    if output.is_empty() {
        anyhow::bail!("Mail inbox is empty or Mail.app is not configured");
    }
    Ok(parse_output(&output))
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
