use super::util::{run_command_with_timeout, APPLE_EPOCH};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct VoiceMemo {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<chrono::Utc>>,
    pub duration_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<VoiceMemo>> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Try the CloudRecordings database first (newer macOS)
    let cloud_db = format!(
        "{home}/Library/Group Containers/group.com.apple.VoiceMemos.shared/Recordings/CloudRecordings.db"
    );
    if tokio::fs::metadata(&cloud_db).await.is_ok() {
        return fetch_from_db(&cloud_db).await;
    }

    // Fallback: older path
    let old_db = format!(
        "{home}/Library/Group Containers/group.com.apple.VoiceMemos/Recordings/CloudRecordings.db"
    );
    if tokio::fs::metadata(&old_db).await.is_ok() {
        return fetch_from_db(&old_db).await;
    }

    anyhow::bail!("Voice Memos database not found")
}

async fn fetch_from_db(db_path: &str) -> anyhow::Result<Vec<VoiceMemo>> {
    let query = r#"
SELECT
    ZUUID,
    ZENCRYPTEDTITLE,
    ZDATE,
    ZDURATION,
    ZPATH
FROM ZCLOUDRECORDING
WHERE ZEVICTIONDATE IS NULL
ORDER BY ZDATE DESC
LIMIT 200;
"#;

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(parse_output(&stdout))
}

fn parse_output(output: &str) -> Vec<VoiceMemo> {
    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let uuid = parts[0].trim();
        let title = parts[1].trim();
        let date_ts: f64 = parts[2].trim().parse().unwrap_or(0.0);
        let duration: f64 = parts[3].trim().parse().unwrap_or(0.0);
        let path = parts.get(4).copied().unwrap_or("").trim();

        let date = if date_ts != 0.0 {
            DateTime::from_timestamp(date_ts as i64 + APPLE_EPOCH, 0)
        } else {
            None
        };

        let display_title = if title.is_empty() {
            format!("Voice Memo {}", &uuid[..8.min(uuid.len())])
        } else {
            title.to_string()
        };

        records.push(VoiceMemo {
            id: uuid.to_string(),
            title: display_title,
            date,
            duration_seconds: duration,
            path: if path.is_empty() {
                None
            } else {
                Some(path.to_string())
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
        let output = "ABC-123\tMeeting notes\t726000000\t125.5\t/path/to/file.m4a\n\
                       DEF-456\t\t726000000\t30.0\t\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Meeting notes");
        assert_eq!(records[0].duration_seconds, 125.5);
        assert!(records[0].path.is_some());
        assert!(records[1].title.starts_with("Voice Memo"));
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
