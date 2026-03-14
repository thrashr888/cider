use super::util::run_command_with_timeout;
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct JournalEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<chrono::Utc>>,
}

pub async fn fetch() -> anyhow::Result<Vec<JournalEntry>> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Apple Journal stores data in a Group Container
    let container = format!("{home}/Library/Group Containers/group.com.apple.journal");

    if tokio::fs::metadata(&container).await.is_err() {
        anyhow::bail!("Journal data not found. Make sure the Journal app has been used.");
    }

    // Find SQLite databases in the container
    let listing = run_command_with_timeout(
        "find",
        &[&container, "-name", "*.sqlite", "-o", "-name", "*.db"],
        std::time::Duration::from_secs(5),
    )
    .await?;

    for db_path in listing.lines() {
        let db_path = db_path.trim();
        if db_path.is_empty() {
            continue;
        }

        // Try to read tables
        let tables = run_command_with_timeout(
            "sqlite3",
            &[db_path, ".tables"],
            std::time::Duration::from_secs(5),
        )
        .await;

        if let Ok(table_list) = tables {
            // Look for a table that might contain journal entries
            let table_list = table_list.to_uppercase();
            if table_list.contains("ENTRY")
                || table_list.contains("JOURNAL")
                || table_list.contains("POST")
            {
                // Try common column patterns
                let query = "SELECT * FROM sqlite_master WHERE type='table' LIMIT 10;";
                if let Ok(schema) = run_command_with_timeout(
                    "sqlite3",
                    &[db_path, query],
                    std::time::Duration::from_secs(5),
                )
                .await
                {
                    eprintln!("Journal schema found at {db_path}: {schema}");
                }
            }
        }
    }

    // Journal is heavily encrypted on modern macOS - entries are typically
    // not accessible via direct SQLite access
    anyhow::bail!(
        "Journal entries are encrypted and not accessible via direct database access. \
         Apple Journal uses on-device encryption for privacy."
    )
}
