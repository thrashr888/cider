use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StickyNote {
    pub text: String,
}

pub async fn fetch() -> anyhow::Result<Vec<StickyNote>> {
    // Stickies stores data in a binary NSKeyedArchiver format.
    // We use Python to decode it since there's no simple CLI tool.
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path =
        format!("{home}/Library/Containers/com.apple.Stickies/Data/Library/StickiesDatabase");

    if tokio::fs::metadata(&db_path).await.is_err() {
        // Try legacy path
        let legacy = format!("{home}/Library/StickiesDatabase");
        if tokio::fs::metadata(&legacy).await.is_err() {
            anyhow::bail!("Stickies database not found");
        }
        return fetch_with_python(&legacy).await;
    }

    fetch_with_python(&db_path).await
}

async fn fetch_with_python(db_path: &str) -> anyhow::Result<Vec<StickyNote>> {
    let script = format!(
        r#"
import json, sys
try:
    # Try reading as NSKeyedArchiver plist
    import plistlib
    with open("{db_path}", "rb") as f:
        data = plistlib.load(f)

    notes = []
    # The plist contains an array of note objects
    if isinstance(data, dict):
        for key, value in data.items():
            if isinstance(value, (str, bytes)):
                text = value if isinstance(value, str) else value.decode('utf-8', errors='replace')
                if text.strip():
                    notes.append({{"text": text.strip()[:500]}})
            elif isinstance(value, list):
                for item in value:
                    if isinstance(item, dict):
                        for k, v in item.items():
                            if isinstance(v, str) and v.strip():
                                notes.append({{"text": v.strip()[:500]}})
                                break
    elif isinstance(data, list):
        for item in data:
            if isinstance(item, str) and item.strip():
                notes.append({{"text": item.strip()[:500]}})
    print(json.dumps(notes))
except Exception as e:
    # Fallback: try to extract text strings from binary data
    with open("{db_path}", "rb") as f:
        raw = f.read()
    # Look for readable text runs
    import re
    texts = re.findall(rb'[A-Za-z0-9 ,.\-!?\'\"]+', raw)
    notes = []
    for t in texts:
        decoded = t.decode('utf-8', errors='replace').strip()
        if len(decoded) > 10:
            notes.append({{"text": decoded[:500]}})
    print(json.dumps(notes[:50]))
"#
    );

    let output = run_command_with_timeout(
        "python3",
        &["-c", &script],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let items: Vec<serde_json::Value> = serde_json::from_str(&output)?;
    Ok(items
        .iter()
        .filter_map(|item| {
            item["text"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| StickyNote {
                    text: t.to_string(),
                })
        })
        .collect())
}
