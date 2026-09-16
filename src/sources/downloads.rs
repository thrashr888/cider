//! Retained quarantine/download-origin events; does not download or open files.
pub use super::local_store::HistoryOptions as ListOptions;
use super::local_store::{apple_time, home_path, query};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
pub const DATABASE_RELATIVE_PATH: &str =
    "Library/Preferences/com.apple.LaunchServices.QuarantineEventsV2";

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadEvent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_code: Option<i64>,
}
#[derive(Deserialize)]
struct RawEvent {
    id: String,
    timestamp: Option<f64>,
    app: Option<String>,
    agent_name: Option<String>,
    url: Option<String>,
    origin_url: Option<String>,
    origin_title: Option<String>,
    sender_name: Option<String>,
    sender_address: Option<String>,
    type_code: Option<i64>,
}
pub async fn list(options: &ListOptions) -> anyhow::Result<Vec<DownloadEvent>> {
    list_at(&home_path(DATABASE_RELATIVE_PATH)?, options).await
}
async fn list_at(path: &Path, options: &ListOptions) -> anyhow::Result<Vec<DownloadEvent>> {
    let filter =
        options.sql_filter("LSQuarantineTimeStamp", "LSQuarantineAgentBundleIdentifier")?;
    let rows: Vec<RawEvent> = query(
        path,
        &format!(
            "SELECT
        COALESCE(NULLIF(LSQuarantineEventIdentifier,''), 'local:' || rowid) AS id,
        LSQuarantineTimeStamp AS timestamp, LSQuarantineAgentBundleIdentifier AS app,
        LSQuarantineAgentName AS agent_name, LSQuarantineDataURLString AS url,
        LSQuarantineOriginURLString AS origin_url, LSQuarantineOriginTitle AS origin_title,
        LSQuarantineSenderName AS sender_name, LSQuarantineSenderAddress AS sender_address,
        LSQuarantineTypeNumber AS type_code FROM LSQuarantineEvent WHERE {filter}
        ORDER BY LSQuarantineTimeStamp DESC, rowid DESC LIMIT {} OFFSET {}",
            options.limit, options.offset
        ),
    )
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(DownloadEvent {
                id: r.id,
                timestamp: apple_time(r.timestamp)?,
                app: r.app,
                agent_name: r.agent_name,
                url: r.url,
                origin_url: r.origin_url,
                origin_title: r.origin_title,
                sender_name: r.sender_name,
                sender_address: r.sender_address,
                type_code: r.type_code,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::local_store::tests::Database;
    #[tokio::test]
    async fn filters_origins_before_pagination_and_preserves_nulls() {
        let db=Database::new("CREATE TABLE LSQuarantineEvent (LSQuarantineEventIdentifier TEXT, LSQuarantineTimeStamp REAL,
            LSQuarantineAgentBundleIdentifier TEXT, LSQuarantineAgentName TEXT, LSQuarantineDataURLString TEXT,
            LSQuarantineOriginURLString TEXT, LSQuarantineOriginTitle TEXT, LSQuarantineSenderName TEXT,
            LSQuarantineSenderAddress TEXT, LSQuarantineTypeNumber INTEGER);
            INSERT INTO LSQuarantineEvent VALUES ('a',0.25,'com.test','Test','https://example.com/a','https://example.com','雪',NULL,NULL,8),
            ('b',2,'com.test','Test',NULL,NULL,NULL,NULL,NULL,NULL), ('c',3,'other',NULL,NULL,NULL,NULL,NULL,NULL,NULL);").await;
        let mut o = ListOptions {
            app: Some("com.test".into()),
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        let rows = list_at(&db.0, &o).await.unwrap();
        assert_eq!(rows[0].id, "a");
        assert_eq!(rows[0].origin_title.as_deref(), Some("雪"));
        assert_eq!(rows[0].timestamp, apple_time(Some(0.25)).unwrap());
        o.offset = 0;
        o.since = apple_time(Some(2.0)).unwrap();
        o.until = apple_time(Some(3.0)).unwrap();
        let value = serde_json::to_value(&list_at(&db.0, &o).await.unwrap()[0]).unwrap();
        assert_eq!(value["id"], "b");
        assert!(value.get("url").is_none());
        o.app = Some("' OR 1=1 --".into());
        assert!(list_at(&db.0, &o).await.unwrap().is_empty());
    }
}
