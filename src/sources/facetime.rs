use super::util::APPLE_EPOCH;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct CallRecord {
    pub id: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub call_type: String,
    pub originated: bool,
    pub answered: bool,
    pub duration_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<Utc>>,
}

fn call_type_label(code: i64) -> String {
    match code {
        1 => "phone".to_string(),
        8 => "facetime-video".to_string(),
        16 => "facetime-audio".to_string(),
        _ => format!("unknown-{code}"),
    }
}

fn db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join("Library/Application Support/CallHistoryDB/CallHistory.storedata")
}

/// List recent call history (FaceTime + Phone).
pub async fn list(limit: u32) -> anyhow::Result<Vec<CallRecord>> {
    let path = db_path();
    if !path.exists() {
        anyhow::bail!("Call history database not found. Full Disk Access may be required.");
    }

    let path_str = path.to_string_lossy().to_string();
    let limit_str = limit.to_string();

    let output = super::util::run_command_with_timeout(
        "sqlite3",
        &[
            "-json",
            &path_str,
            &format!(
                "SELECT ZUNIQUE_ID, ZADDRESS, ZNAME, ZCALLTYPE, ZORIGINATED, ZANSWERED, \
                 ZDURATION, ZDATE FROM ZCALLRECORD ORDER BY ZDATE DESC LIMIT {limit_str}"
            ),
        ],
        std::time::Duration::from_secs(10),
    )
    .await?;

    if output.trim().is_empty() {
        return Ok(vec![]);
    }

    let rows: Vec<serde_json::Value> = serde_json::from_str(&output)?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.get("ZUNIQUE_ID")?.as_str()?.to_string();
            let address = row.get("ZADDRESS")?.as_str().unwrap_or("").to_string();
            let name = row
                .get("ZNAME")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            let call_type = call_type_label(row.get("ZCALLTYPE")?.as_i64().unwrap_or(0));
            let originated = row.get("ZORIGINATED")?.as_i64().unwrap_or(0) == 1;
            let answered = row.get("ZANSWERED")?.as_i64().unwrap_or(0) == 1;
            let duration_seconds = row.get("ZDURATION")?.as_f64().unwrap_or(0.0);
            let date = row
                .get("ZDATE")
                .and_then(|v| v.as_f64())
                .map(|ts| DateTime::from_timestamp(ts as i64 + APPLE_EPOCH, 0).unwrap_or_default());

            Some(CallRecord {
                id,
                address,
                name,
                call_type,
                originated,
                answered,
                duration_seconds,
                date,
            })
        })
        .collect())
}
