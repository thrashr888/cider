use super::util::{run_command_with_timeout, truncate_for_title};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Message {
    pub id: String,
    pub text: String,
    pub sender: String,
    pub is_from_me: bool,
    pub service: String,
    pub conversation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<chrono::Utc>>,
}

pub async fn fetch(days: u32) -> anyhow::Result<Vec<Message>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{home}/Library/Messages/chat.db");

    if tokio::fs::metadata(&db_path).await.is_err() {
        anyhow::bail!("Messages database not accessible at {db_path}");
    }

    let lookback = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
    let apple_ns = (lookback.timestamp() - 978_307_200) * 1_000_000_000;

    let query = format!(
        r#"
SELECT
    m.ROWID,
    COALESCE(m.text, '') as msg_text,
    m.date / 1000000000 + 978307200 as unix_ts,
    m.is_from_me,
    COALESCE(h.id, '') as handle_id,
    COALESCE(c.display_name, '') as chat_name,
    COALESCE(c.chat_identifier, '') as chat_identifier,
    CASE WHEN m.text IS NULL AND m.attributedBody IS NOT NULL THEN 1 ELSE 0 END as has_attributed
FROM message m
LEFT JOIN handle h ON m.handle_id = h.ROWID
LEFT JOIN chat_message_join cmj ON m.ROWID = cmj.message_id
LEFT JOIN chat c ON cmj.chat_id = c.ROWID
WHERE m.date > {apple_ns}
  AND (m.text IS NOT NULL AND m.text != '' OR m.attributedBody IS NOT NULL)
ORDER BY m.date DESC
LIMIT 200;
"#
    );

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", &db_path, query.trim()],
        std::time::Duration::from_secs(30),
    )
    .await?;

    Ok(parse_output(&stdout))
}

fn parse_output(output: &str) -> Vec<Message> {
    let mut records = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }

        let rowid = parts[0].trim();
        let text = parts[1].trim();
        let has_attributed = parts.get(7).map(|s| s.trim() == "1").unwrap_or(false);

        if text.is_empty() && !has_attributed {
            continue;
        }

        let body_text = if text.is_empty() {
            "[Attachment or rich text]".to_string()
        } else {
            text.to_string()
        };

        let unix_ts: i64 = parts[2].trim().parse().unwrap_or(0);
        let is_from_me: bool = parts[3].trim() == "1";
        let handle_id = parts.get(4).copied().unwrap_or("").trim();
        let chat_name = parts.get(5).copied().unwrap_or("").trim();
        let chat_identifier = parts.get(6).copied().unwrap_or("").trim();

        let timestamp = if unix_ts > 0 {
            DateTime::from_timestamp(unix_ts, 0)
        } else {
            None
        };

        let sender = if is_from_me {
            "Me".to_string()
        } else if !handle_id.is_empty() {
            handle_id.to_string()
        } else {
            "Unknown".to_string()
        };

        let conversation = if !chat_name.is_empty() {
            chat_name.to_string()
        } else if !handle_id.is_empty() {
            handle_id.to_string()
        } else {
            "Unknown".to_string()
        };

        let service = if chat_identifier.starts_with('+') || chat_identifier.contains('@') {
            "iMessage"
        } else {
            "SMS"
        };

        records.push(Message {
            id: format!("msg_{rowid}"),
            text: truncate_for_title(&body_text),
            sender,
            is_from_me,
            service: service.to_string(),
            conversation,
            timestamp,
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "42\tHey, how are you?\t1707350400\t0\t+15551234567\tAlice\t+15551234567\t0\n\
                       43\tI'm good thanks!\t1707350500\t1\t+15551234567\tAlice\t+15551234567\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "msg_42");
        assert_eq!(records[0].text, "Hey, how are you?");
        assert_eq!(records[0].conversation, "Alice");
        assert!(!records[0].is_from_me);
        assert!(records[0].timestamp.is_some());
        assert_eq!(records[1].sender, "Me");
        assert!(records[1].is_from_me);
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }

    #[test]
    fn test_parse_output_skips_empty_text() {
        let output = "101\t\t1707350400\t0\t+15551234567\tAlice\t+15551234567\t0\n";
        assert!(parse_output(output).is_empty());
    }

    #[test]
    fn test_parse_output_attributed_body() {
        let output = "102\t\t1707350400\t0\t+15551234567\tAlice\t+15551234567\t1\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].text, "[Attachment or rich text]");
    }

    #[test]
    fn test_parse_output_email_service() {
        let output = "99\tCheck this out\t1707350400\t0\tuser@example.com\t\tuser@example.com\t0\n";
        let records = parse_output(output);
        assert_eq!(records[0].service, "iMessage");
    }
}
