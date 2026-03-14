use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PhotoBoothItem {
    pub filename: String,
    pub path: String,
    pub kind: String,
}

pub async fn fetch() -> anyhow::Result<Vec<PhotoBoothItem>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let library = format!("{home}/Pictures/Photo Booth Library");

    if tokio::fs::metadata(&library).await.is_err() {
        anyhow::bail!("Photo Booth Library not found at {library}");
    }

    // Find all images and videos in the library
    let output = run_command_with_timeout(
        "find",
        &[
            &library, "-type", "f", "(", "-name", "*.jpg", "-o", "-name", "*.jpeg", "-o", "-name",
            "*.png", "-o", "-name", "*.mov", "-o", "-name", "*.mp4", ")",
        ],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let mut items = Vec::new();
    for line in output.lines() {
        let path = line.trim();
        if path.is_empty() {
            continue;
        }
        let filename = path.rsplit('/').next().unwrap_or(path).to_string();
        let lower = filename.to_lowercase();
        let kind = if lower.ends_with(".mov") || lower.ends_with(".mp4") {
            "video"
        } else {
            "photo"
        };
        items.push(PhotoBoothItem {
            filename,
            path: path.to_string(),
            kind: kind.to_string(),
        });
    }
    Ok(items)
}
