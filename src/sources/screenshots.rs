use super::util::{run_command_with_timeout, ActionResult};
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

pub async fn list() -> anyhow::Result<Vec<Screenshot>> {
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

/// Take a screenshot using macOS `screencapture`.
///
/// - `selection`: if true, use `-i` flag for interactive selection
/// - `window`: if true, use `-w` flag to capture a window
/// - `path`: optional output file path; if None, uses the default screenshot location
pub async fn capture(
    selection: bool,
    window: bool,
    path: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Determine output path
    let output_path = if let Some(p) = path {
        p.to_string()
    } else {
        // Use the configured screenshot location or Desktop
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

        let timestamp = chrono::Utc::now().format("%Y-%m-%d at %H.%M.%S");
        format!("{screenshot_dir}/Screenshot {timestamp}.png")
    };

    let mut args: Vec<&str> = Vec::new();
    if selection {
        args.push("-i");
    }
    if window {
        args.push("-w");
    }
    args.push(&output_path);

    run_command_with_timeout("screencapture", &args, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message("captured", &output_path))
}
