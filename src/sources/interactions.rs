//! Core Duet communication metadata, not message bodies.
pub use super::local_store::HistoryOptions as ListOptions;
use super::local_store::{apple_time, query};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
pub const DATABASE_PATH: &str = "/private/var/db/CoreDuet/People/interactionC.db";
#[derive(Debug, Serialize, Deserialize)]
pub struct Participant {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Interaction {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mechanism_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_response: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<Participant>,
    pub recipients: Vec<Participant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_url: Option<String>,
}
#[derive(Deserialize)]
struct RawInteraction {
    id: String,
    app: Option<String>,
    start_date: Option<f64>,
    end_date: Option<f64>,
    direction_code: Option<i64>,
    mechanism_code: Option<i64>,
    is_response: Option<i64>,
    sender_id: Option<i64>,
    sender_identifier: Option<String>,
    sender_name: Option<String>,
    recipients_json: String,
    recipient_count: Option<i64>,
    account: Option<String>,
    content_url: Option<String>,
}
pub async fn list(options: &ListOptions) -> anyhow::Result<Vec<Interaction>> {
    list_at(Path::new(DATABASE_PATH), options).await
}
async fn list_at(path: &Path, options: &ListOptions) -> anyhow::Result<Vec<Interaction>> {
    let filter = options.sql_filter("i.ZSTARTDATE", "i.ZBUNDLEID")?;
    let rows:Vec<RawInteraction>=query(path,&format!("SELECT COALESCE(NULLIF(i.ZUUID,''),'local:' || i.Z_PK) AS id,
        i.ZBUNDLEID AS app,i.ZSTARTDATE AS start_date,i.ZENDDATE AS end_date,i.ZDIRECTION AS direction_code,
        i.ZMECHANISM AS mechanism_code,i.ZISRESPONSE AS is_response,i.ZRECIPIENTCOUNT AS recipient_count,
        i.ZACCOUNT AS account,i.ZCONTENTURL AS content_url,i.ZSENDER AS sender_id,s.ZIDENTIFIER AS sender_identifier,s.ZDISPLAYNAME AS sender_name,
        (SELECT json_group_array(json_object('id','local:' || recipient_id,'identifier',identifier,'display_name',display_name)) FROM
          (SELECT r.Z_2RECIPIENTS AS recipient_id,c.ZIDENTIFIER AS identifier,c.ZDISPLAYNAME AS display_name
           FROM Z_2INTERACTIONRECIPIENT r LEFT JOIN ZCONTACTS c ON c.Z_PK=r.Z_2RECIPIENTS
           WHERE r.Z_3INTERACTIONRECIPIENT=i.Z_PK ORDER BY r.Z_2RECIPIENTS)) AS recipients_json
        FROM ZINTERACTIONS i LEFT JOIN ZCONTACTS s ON s.Z_PK=i.ZSENDER
        WHERE {filter} ORDER BY i.ZSTARTDATE DESC,i.Z_PK DESC LIMIT {} OFFSET {}",options.limit,options.offset)).await?;
    rows.into_iter()
        .map(|r| {
            Ok(Interaction {
                id: r.id,
                app: r.app,
                start_date: apple_time(r.start_date)?,
                end_date: apple_time(r.end_date)?,
                direction_code: r.direction_code,
                mechanism_code: r.mechanism_code,
                is_response: r.is_response.map(|v| v != 0),
                sender: r.sender_id.map(|id| Participant {
                    id: format!("local:{id}"),
                    identifier: r.sender_identifier,
                    display_name: r.sender_name,
                }),
                recipients: serde_json::from_str(&r.recipients_json)?,
                recipient_count: r.recipient_count,
                account: r.account,
                content_url: r.content_url,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::local_store::tests::Database;
    #[tokio::test]
    async fn preserves_relationships_without_multiplying_paginated_events() {
        let db=Database::new("CREATE TABLE ZCONTACTS(Z_PK INTEGER,ZIDENTIFIER TEXT,ZDISPLAYNAME TEXT);
            CREATE TABLE Z_2INTERACTIONRECIPIENT(Z_2RECIPIENTS INTEGER,Z_3INTERACTIONRECIPIENT INTEGER);
            CREATE TABLE ZINTERACTIONS(Z_PK INTEGER,ZUUID TEXT,ZBUNDLEID TEXT,ZSTARTDATE REAL,ZENDDATE REAL,ZDIRECTION INTEGER,ZMECHANISM INTEGER,ZISRESPONSE INTEGER,ZRECIPIENTCOUNT INTEGER,ZACCOUNT TEXT,ZCONTENTURL TEXT,ZSENDER INTEGER);
            INSERT INTO ZCONTACTS VALUES(1,'sender@example.com','Sender'),(2,'a@example.com','A'),(3,'b@example.com','B');
            INSERT INTO ZINTERACTIONS VALUES(10,'one','com.test',1,2,1,2,0,2,NULL,NULL,1),(11,NULL,'com.test',2,3,NULL,NULL,NULL,1,NULL,NULL,9);
            INSERT INTO Z_2INTERACTIONRECIPIENT VALUES(2,10),(3,10),(9,11);").await;
        let mut o = ListOptions {
            app: Some("com.test".into()),
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        let r = list_at(&db.0, &o).await.unwrap().remove(0);
        assert_eq!(r.id, "one");
        assert_eq!(r.recipients.len(), 2);
        assert_eq!(
            r.sender.unwrap().identifier.as_deref(),
            Some("sender@example.com")
        );
        o.offset = 0;
        o.since = apple_time(Some(2.0)).unwrap();
        o.until = apple_time(Some(3.0)).unwrap();
        let r = list_at(&db.0, &o).await.unwrap().remove(0);
        assert_eq!(r.id, "local:11");
        assert_eq!(r.recipients[0].id, "local:9");
        assert!(r.recipients[0].identifier.is_none());
        assert!(r.sender.unwrap().identifier.is_none());
        o.app = Some("' OR 1=1 --".into());
        assert!(list_at(&db.0, &o).await.unwrap().is_empty());
    }
}
