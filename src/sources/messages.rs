use super::util::{
    escape_applescript, run_command_with_timeout, run_osascript_with_timeout, ActionResult,
};
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

pub async fn list(days: u32) -> anyhow::Result<Vec<Message>> {
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
    m.ROWID AS rowid,
    COALESCE(m.text, '') AS msg_text,
    m.date / 1000000000 + 978307200 AS unix_ts,
    m.is_from_me AS is_from_me,
    COALESCE(h.id, '') AS handle_id,
    COALESCE(c.display_name, '') AS chat_name,
    COALESCE(c.chat_identifier, '') AS chat_identifier,
    CASE WHEN m.text IS NULL AND m.attributedBody IS NOT NULL THEN 1 ELSE 0 END AS has_attributed
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
        &["-json", &db_path, query.trim()],
        std::time::Duration::from_secs(30),
    )
    .await?;

    Ok(parse_json_rows(&stdout))
}

pub async fn send(to: &str, text: &str) -> anyhow::Result<ActionResult> {
    let script = format!(
        r#"tell application "Messages"
    set targetBuddy to "{}"
    set targetService to 1st account whose service type = iMessage
    set theBuddy to buddy targetBuddy of targetService
    send "{}" to theBuddy
end tell"#,
        escape_applescript(to),
        escape_applescript(text)
    );

    run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
    Ok(ActionResult::success_with_message(
        "send",
        &format!("Sent to {to}"),
    ))
}

/// A JSON field that sqlite3 may emit as a number or a string, read as i64.
fn json_i64(value: &serde_json::Value) -> i64 {
    match value {
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Parse `sqlite3 -json` output into messages. JSON is the point: message
/// text keeps its newlines and full length — the old tab-separated read broke
/// any multiline message into garbage rows, and the data layer used to cap
/// text at 120 chars (display truncation belongs to `--pretty`, not here).
fn parse_json_rows(output: &str) -> Vec<Message> {
    let output = output.trim();
    if output.is_empty() {
        return Vec::new();
    }

    let rows: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Skipping unparseable messages output: {e}");
            return Vec::new();
        }
    };

    let mut records = Vec::new();

    for row in &rows {
        let rowid = json_i64(&row["rowid"]);
        let text = row["msg_text"].as_str().unwrap_or("").trim();
        let has_attributed = json_i64(&row["has_attributed"]) == 1;

        if text.is_empty() && !has_attributed {
            continue;
        }

        let body_text = if text.is_empty() {
            "[Attachment or rich text]".to_string()
        } else {
            text.to_string()
        };

        let unix_ts = json_i64(&row["unix_ts"]);
        let is_from_me = json_i64(&row["is_from_me"]) == 1;
        let handle_id = row["handle_id"].as_str().unwrap_or("").trim();
        let chat_name = row["chat_name"].as_str().unwrap_or("").trim();
        let chat_identifier = row["chat_identifier"].as_str().unwrap_or("").trim();

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
            text: body_text,
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
    fn test_parse_json_rows() {
        let output = r#"[
            {"rowid":42,"msg_text":"Hey, how are you?","unix_ts":1707350400,"is_from_me":0,"handle_id":"+15551234567","chat_name":"Alice","chat_identifier":"+15551234567","has_attributed":0},
            {"rowid":43,"msg_text":"I'm good thanks!","unix_ts":1707350500,"is_from_me":1,"handle_id":"+15551234567","chat_name":"Alice","chat_identifier":"+15551234567","has_attributed":0}
        ]"#;
        let records = parse_json_rows(output);
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
    fn test_parse_json_rows_empty() {
        assert!(parse_json_rows("").is_empty());
        assert!(parse_json_rows("[]").is_empty());
    }

    #[test]
    fn test_parse_json_rows_skips_empty_text() {
        let output = r#"[{"rowid":101,"msg_text":"","unix_ts":1707350400,"is_from_me":0,"handle_id":"+15551234567","chat_name":"Alice","chat_identifier":"+15551234567","has_attributed":0}]"#;
        assert!(parse_json_rows(output).is_empty());
    }

    #[test]
    fn test_parse_json_rows_attributed_body() {
        let output = r#"[{"rowid":102,"msg_text":"","unix_ts":1707350400,"is_from_me":0,"handle_id":"+15551234567","chat_name":"Alice","chat_identifier":"+15551234567","has_attributed":1}]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].text, "[Attachment or rich text]");
    }

    #[test]
    fn test_parse_json_rows_email_service() {
        let output = r#"[{"rowid":99,"msg_text":"Check this out","unix_ts":1707350400,"is_from_me":0,"handle_id":"user@example.com","chat_name":"","chat_identifier":"user@example.com","has_attributed":0}]"#;
        let records = parse_json_rows(output);
        assert_eq!(records[0].service, "iMessage");
    }

    /// The reason for `-json`: a multiline message used to shear into broken
    /// TSV rows, and long messages were capped at 120 chars in the data layer.
    #[test]
    fn test_parse_json_rows_multiline_message_survives() {
        let long_tail = "x".repeat(2000);
        let body = format!("line one\nline two\twith tab\n\nline four\n{long_tail}");
        let output = serde_json::json!([{
            "rowid": 7,
            "msg_text": body,
            "unix_ts": 1707350400,
            "is_from_me": 0,
            "handle_id": "+15551234567",
            "chat_name": "Alice",
            "chat_identifier": "+15551234567",
            "has_attributed": 0,
        }])
        .to_string();

        let records = parse_json_rows(&output);
        assert_eq!(records.len(), 1, "one message must parse as one record");
        assert_eq!(records[0].text, body, "full text must survive untruncated");
        assert!(records[0].text.contains('\n'), "newlines must survive");
        assert!(records[0].text.contains('\t'), "tabs must survive");
        assert!(records[0].text.len() > 2000, "no 120-char cap");
    }
}
