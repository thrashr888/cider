use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub process: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

pub async fn fetch(minutes: u32) -> anyhow::Result<Vec<LogEntry>> {
    let last = format!("{}m", minutes);
    let output = run_command_with_timeout(
        "log",
        &[
            "show",
            "--last",
            &last,
            "--style",
            "ndjson",
            "--predicate",
            "eventType == logEvent AND messageType == error OR messageType == fault",
        ],
        std::time::Duration::from_secs(30),
    )
    .await?;

    let mut entries = Vec::new();
    for line in output.lines().take(200) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            let timestamp = obj["timestamp"].as_str().unwrap_or("").to_string();
            let process = obj["processImagePath"]
                .as_str()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let message = obj["eventMessage"].as_str().unwrap_or("").to_string();
            let level = obj["messageType"]
                .as_str()
                .map(String::from)
                .or_else(|| obj["eventType"].as_str().map(String::from));

            if !message.is_empty() {
                entries.push(LogEntry {
                    timestamp,
                    process,
                    message,
                    level,
                });
            }
        }
    }

    Ok(entries)
}
