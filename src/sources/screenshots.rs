use super::util::run_command_with_timeout;
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Screenshot {
    pub filename: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<chrono::Utc>>,
    pub size_bytes: i64,
}

pub async fn fetch() -> anyhow::Result<Vec<Screenshot>> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Check screenshot location preference
    let screenshot_dir = run_command_with_timeout(
        "defaults",
        &["read", "com.apple.screencapture", "location"],
        std::time::Duration::from_secs(3),
    )
    .await
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| format!("{home}/Desktop"));

    // Find screenshot files (macOS names them "Screenshot ...")
    let output = run_command_with_timeout(
        "find",
        &[
            &screenshot_dir,
            "-maxdepth",
            "1",
            "-name",
            "Screenshot*",
            "-type",
            "f",
        ],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let mut screenshots = Vec::new();
    for line in output.lines() {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }

        let filename = path.rsplit('/').next().unwrap_or(path).to_string();

        // Get file info
        let stat = run_command_with_timeout(
            "stat",
            &["-f", "%m %z", path],
            std::time::Duration::from_secs(2),
        )
        .await
        .ok();

        let (date, size) = if let Some(stat_str) = stat {
            let parts: Vec<&str> = stat_str.split_whitespace().collect();
            let ts: i64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            let sz: i64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            (DateTime::from_timestamp(ts, 0), sz)
        } else {
            (None, 0)
        };

        screenshots.push(Screenshot {
            filename,
            path: path.to_string(),
            date,
            size_bytes: size,
        });
    }

    screenshots.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(screenshots)
}
