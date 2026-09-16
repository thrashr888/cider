//! Notification Center's retained records, including decoded plist content.
pub use super::local_store::HistoryOptions as ListOptions;
use super::{
    keyed_archive::{decode_bytes, hex_decode},
    local_store::{apple_time, home_path, query, read_error},
    util::run_command_with_timeout,
};
use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub const DATABASE_RELATIVE_PATH: &str =
    "Library/Group Containers/group.com.apple.usernoted/db2/db";

#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presented: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Preserves non-string localization arguments instead of guessing text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localized_content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_error: Option<String>,
}
#[derive(Deserialize)]
struct RawNotification {
    row_id: i64,
    uuid_hex: String,
    app: Option<String>,
    delivered_at: Option<f64>,
    requested_at: Option<f64>,
    presented: Option<i64>,
    data_hex: String,
}
pub(crate) async fn database_path() -> anyhow::Result<PathBuf> {
    let modern = home_path(DATABASE_RELATIVE_PATH)?;
    match tokio::fs::metadata(&modern).await {
        Ok(_) => return Ok(modern),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(read_error(&modern, e)),
    }
    let dir = run_command_with_timeout(
        "/usr/bin/getconf",
        &["DARWIN_USER_DIR"],
        Duration::from_secs(5),
    )
    .await?;
    anyhow::ensure!(
        Path::new(&dir).is_absolute(),
        "Cannot locate Notification Center user directory"
    );
    let legacy = Path::new(&dir).join("com.apple.notificationcenter/db2/db");
    tokio::fs::metadata(&legacy)
        .await
        .map_err(|e| read_error(&legacy, e))?;
    Ok(legacy)
}
pub async fn list(options: &ListOptions) -> anyhow::Result<Vec<Notification>> {
    options.validate()?;
    list_at(&database_path().await?, options).await
}
async fn list_at(path: &Path, options: &ListOptions) -> anyhow::Result<Vec<Notification>> {
    let filter = options.sql_filter("r.delivered_date", "a.identifier")?;
    let rows: Vec<RawNotification> = query(
        path,
        &format!(
            "SELECT r.rec_id AS row_id, hex(r.uuid) AS uuid_hex,
        a.identifier AS app, r.delivered_date AS delivered_at, r.request_date AS requested_at,
        r.presented, hex(r.data) AS data_hex FROM record r LEFT JOIN app a ON a.app_id=r.app_id
        WHERE {filter} ORDER BY r.delivered_date DESC, r.rec_id DESC LIMIT {} OFFSET {}",
            options.limit, options.offset
        ),
    )
    .await?;
    rows.into_iter().map(decode).collect()
}
fn decode(r: RawNotification) -> anyhow::Result<Notification> {
    let uuid = hex_decode(&r.uuid_hex)?;
    let id = uuid::Uuid::from_slice(&uuid)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("local:{}", r.row_id));
    let mut n = Notification {
        id,
        app: r.app,
        delivered_at: apple_time(r.delivered_at)?,
        requested_at: apple_time(r.requested_at)?,
        presented: r.presented.map(|v| v != 0),
        title: None,
        subtitle: None,
        body: None,
        identifier: None,
        localized_content: None,
        decode_error: None,
    };
    match hex_decode(&r.data_hex).and_then(|b| decode_bytes(&b)) {
        Ok(value) => {
            let req = value
                .get("req")
                .and_then(serde_json::Value::as_object)
                .context("Notification plist has no request dictionary");
            match req {
                Ok(req) => {
                    let text = |key: &str| {
                        req.get(key)
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    };
                    n.title = text("titl");
                    n.subtitle = text("subt");
                    n.body = text("body");
                    n.identifier = text("iden");
                    let localized: serde_json::Map<String, serde_json::Value> =
                        ["titl", "subt", "body"]
                            .iter()
                            .filter_map(|key| {
                                req.get(*key)
                                    .filter(|v| !v.is_string() && !v.is_null())
                                    .map(|v| ((*key).to_string(), v.clone()))
                            })
                            .collect();
                    if !localized.is_empty() {
                        n.localized_content = Some(localized.into());
                    }
                }
                Err(e) => n.decode_error = Some(e.to_string()),
            }
        }
        Err(e) => n.decode_error = Some(e.to_string()),
    }
    Ok(n)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{keyed_archive::hex_encode, local_store::tests::Database};
    fn payload() -> String {
        let value=plist::Value::from_reader_xml(std::io::Cursor::new(br#"<plist version="1.0"><dict><key>req</key><dict><key>titl</key><string>A &amp; B</string><key>body</key><string>line one&#10;line two</string><key>subt</key><array><string>localized</string><integer>3</integer></array></dict></dict></plist>"#)).unwrap();
        let mut bytes = Vec::new();
        value.to_writer_binary(&mut bytes).unwrap();
        hex_encode(&bytes)
    }
    #[tokio::test]
    async fn decodes_plists_uuid_and_localization_and_filters_in_sql() {
        let db=Database::new(&format!("CREATE TABLE app(app_id INTEGER,identifier TEXT); CREATE TABLE record(rec_id INTEGER,app_id INTEGER,uuid BLOB,data BLOB,delivered_date REAL,request_date REAL,presented INTEGER);
            INSERT INTO app VALUES(1,'com.test'); INSERT INTO record VALUES(1,1,X'00112233445566778899aabbccddeeff',X'{}',0.5,0,1),(2,1,NULL,X'00',2,1,0),(3,2,NULL,X'00',3,1,0);",payload())).await;
        let mut o = ListOptions {
            app: Some("com.test".into()),
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        let n = list_at(&db.0, &o).await.unwrap().remove(0);
        assert_eq!(n.id, "00112233-4455-6677-8899-aabbccddeeff");
        assert_eq!(n.title.as_deref(), Some("A & B"));
        assert_eq!(n.body.as_deref(), Some("line one\nline two"));
        assert!(n.subtitle.is_none());
        assert!(n.localized_content.is_some());
        assert!(n.decode_error.is_none());
        assert_eq!(n.delivered_at, apple_time(Some(0.5)).unwrap());
        o.offset = 0;
        o.since = apple_time(Some(2.0)).unwrap();
        o.until = apple_time(Some(3.0)).unwrap();
        let n = list_at(&db.0, &o).await.unwrap().remove(0);
        assert_eq!(n.id, "local:2");
        assert!(n.decode_error.is_some());
        assert_eq!(n.presented, Some(false));
        o.app = Some("' OR 1=1 --".into());
        assert!(list_at(&db.0, &o).await.unwrap().is_empty());
    }
}
