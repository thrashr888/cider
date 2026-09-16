//! Read-only access to macOS's local Core Duet activity store.
//!
//! The private schema varies by macOS version. Keep the stored scalar values
//! intact rather than guessing the meaning of value-type hashes or metadata.

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::util::{run_command_with_timeout, APPLE_EPOCH};

pub const DATABASE_RELATIVE_PATH: &str = "Library/Application Support/Knowledge/knowledgeC.db";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEvent {
    /// Core Data UUID, or `local:<rowid>` when the UUID is absent.
    pub id: String,
    pub stream: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_integer: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_double: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type_code: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStream {
    pub stream: String,
    pub event_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_start_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_start_date: Option<DateTime<Utc>>,
}

/// Filters apply to event start times: `since` is inclusive, `until` exclusive.
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub stream: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            stream: None,
            since: None,
            until: None,
            limit: 100,
            offset: 0,
        }
    }
}

/// List newest events first, breaking timestamp ties by database row ID.
pub async fn list(options: &ListOptions) -> anyhow::Result<Vec<KnowledgeEvent>> {
    list_at(&database_path()?, options).await
}

/// List available streams alphabetically, with counts and their time ranges.
pub async fn streams() -> anyhow::Result<Vec<KnowledgeStream>> {
    streams_at(&database_path()?).await
}

fn database_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty());
    Ok(
        PathBuf::from(home.context("HOME is not set; cannot locate knowledgeC.db")?)
            .join(DATABASE_RELATIVE_PATH),
    )
}

async fn query<T: serde::de::DeserializeOwned>(path: &Path, sql: &str) -> anyhow::Result<Vec<T>> {
    // Opening, rather than stat, detects macOS privacy denials. Never create a
    // missing store, and let sqlite read the live WAL instead of copying the DB.
    std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("Knowledge database not found at {}", path.display())
        } else if error.kind() == std::io::ErrorKind::PermissionDenied {
            anyhow::anyhow!(
                "Cannot read Knowledge database {}: {error}. Full Disk Access may be required for the app launching cider",
                path.display()
            )
        } else {
            anyhow::anyhow!("Cannot read Knowledge database {}: {error}", path.display())
        }
    })?;
    let path = path
        .to_str()
        .context("Knowledge database path is not UTF-8")?;
    let output = run_command_with_timeout(
        "/usr/bin/sqlite3",
        &["-readonly", "-json", "-cmd", ".timeout 1000", path, sql],
        Duration::from_secs(10),
    )
    .await
    .map_err(|error| anyhow::anyhow!("Cannot query knowledgeC.db: {error}"))?;
    // sqlite3 emits no text for a successful SELECT with no rows.
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&output).context("Invalid knowledgeC.db query result")
}

fn apple_time(value: Option<f64>) -> anyhow::Result<Option<DateTime<Utc>>> {
    value
        .map(|seconds| {
            let unix_seconds = seconds + APPLE_EPOCH as f64;
            anyhow::ensure!(unix_seconds.is_finite(), "Invalid Knowledge timestamp");
            DateTime::from_timestamp(
                unix_seconds.floor() as i64,
                (unix_seconds.rem_euclid(1.0) * 1_000_000_000.0) as u32,
            )
            .context("Invalid Knowledge timestamp")
        })
        .transpose()
}

fn apple_seconds(date: DateTime<Utc>) -> f64 {
    (date.timestamp() - APPLE_EPOCH) as f64 + date.timestamp_subsec_nanos() as f64 / 1e9
}

fn list_query(options: &ListOptions) -> anyhow::Result<String> {
    if let (Some(since), Some(until)) = (options.since, options.until) {
        anyhow::ensure!(
            since < until,
            "invalid time range: --since must precede --until"
        );
    }
    let mut filters = vec!["ZSTREAMNAME IS NOT NULL".to_string()];
    if let Some(stream) = &options.stream {
        anyhow::ensure!(
            !stream.contains('\0'),
            "invalid stream: contains a NUL character"
        );
        filters.push(format!("ZSTREAMNAME = '{}'", stream.replace('\'', "''")));
    }
    if let Some(since) = options.since {
        filters.push(format!("ZSTARTDATE >= {}", apple_seconds(since)));
    }
    if let Some(until) = options.until {
        filters.push(format!("ZSTARTDATE < {}", apple_seconds(until)));
    }
    Ok(format!(
        "SELECT COALESCE(NULLIF(ZUUID, ''), 'local:' || Z_PK) AS id,
         ZSTREAMNAME AS stream, ZSTARTDATE AS start_date, ZENDDATE AS end_date,
         ZCREATIONDATE AS creation_date, ZVALUESTRING AS value_string,
         ZVALUEINTEGER AS value_integer, ZVALUEDOUBLE AS value_double,
         ZVALUETYPECODE AS value_type_code FROM ZOBJECT
         WHERE {} ORDER BY ZSTARTDATE DESC, Z_PK DESC LIMIT {} OFFSET {}",
        filters.join(" AND "),
        options.limit,
        options.offset
    ))
}

#[derive(Deserialize)]
struct RawEvent {
    id: String,
    stream: String,
    start_date: Option<f64>,
    end_date: Option<f64>,
    creation_date: Option<f64>,
    value_string: Option<String>,
    value_integer: Option<i64>,
    value_double: Option<f64>,
    value_type_code: Option<i64>,
}

async fn list_at(path: &Path, options: &ListOptions) -> anyhow::Result<Vec<KnowledgeEvent>> {
    let rows: Vec<RawEvent> = query(path, &list_query(options)?).await?;
    rows.into_iter()
        .map(|row| {
            Ok(KnowledgeEvent {
                id: row.id,
                stream: row.stream,
                start_date: apple_time(row.start_date)?,
                end_date: apple_time(row.end_date)?,
                creation_date: apple_time(row.creation_date)?,
                // An incomplete or reversed interval has no known duration.
                duration_seconds: row
                    .start_date
                    .zip(row.end_date)
                    .and_then(|(start, end)| (end >= start).then_some(end - start)),
                value_string: row.value_string,
                value_integer: row.value_integer,
                value_double: row.value_double,
                value_type_code: row.value_type_code,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct RawStream {
    stream: String,
    event_count: u64,
    first_start_date: Option<f64>,
    last_start_date: Option<f64>,
}

async fn streams_at(path: &Path) -> anyhow::Result<Vec<KnowledgeStream>> {
    let rows: Vec<RawStream> = query(
        path,
        "SELECT ZSTREAMNAME AS stream, COUNT(*) AS event_count,
         MIN(ZSTARTDATE) AS first_start_date, MAX(ZSTARTDATE) AS last_start_date
         FROM ZOBJECT WHERE ZSTREAMNAME IS NOT NULL GROUP BY ZSTREAMNAME ORDER BY ZSTREAMNAME",
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(KnowledgeStream {
                stream: row.stream,
                event_count: row.event_count,
                first_start_date: apple_time(row.first_start_date)?,
                last_start_date: apple_time(row.last_start_date)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl Fixture {
        async fn new() -> Self {
            let fixture = Self(
                std::env::temp_dir().join(format!("cider-knowledge-{}", uuid::Uuid::new_v4())),
            );
            std::fs::create_dir(&fixture.0).unwrap();
            run_command_with_timeout("/usr/bin/sqlite3", &[
                fixture.path().to_str().unwrap(),
                "CREATE TABLE ZOBJECT (
                    Z_PK INTEGER PRIMARY KEY, ZUUID TEXT, ZSTREAMNAME TEXT,
                    ZSTARTDATE REAL, ZENDDATE REAL, ZCREATIONDATE REAL,
                    ZVALUESTRING TEXT, ZVALUEINTEGER INTEGER, ZVALUEDOUBLE REAL,
                    ZVALUETYPECODE INTEGER);
                 INSERT INTO ZOBJECT VALUES
                    (1, 'first', '/app/usage', 0.25, 2.75, 0, 'com.example.app', NULL, NULL, 6584185901589580638),
                    (2, 'second', '/display/isBacklit', 1, 3, NULL, NULL, 0, 0, -2475731913145812025),
                    (3, 'third', '/app/usage', 1, NULL, 1, 'line one' || char(10) || 'line two' || char(9) || '雪', NULL, NULL, NULL),
                    (4, NULL, '/quoted''stream', 4, 3, NULL, 'quoted', NULL, NULL, NULL),
                    (5, 'undated', '/empty', NULL, NULL, NULL, NULL, NULL, NULL, NULL),
                    (6, 'metadata', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);"
            ], Duration::from_secs(5)).await.unwrap();
            fixture
        }

        fn path(&self) -> PathBuf {
            self.0.join("knowledgeC.db")
        }
    }

    #[tokio::test]
    async fn reads_dates_values_ids_and_nulls_without_changing_database() {
        let fixture = Fixture::new().await;
        let before = std::fs::read(fixture.path()).unwrap();
        let events = list_at(&fixture.path(), &ListOptions::default())
            .await
            .unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["local:4", "third", "second", "first", "undated"]
        );
        assert!(
            events[0].duration_seconds.is_none(),
            "negative intervals are not durations"
        );
        assert_eq!(
            events[1].value_string.as_deref(),
            Some("line one\nline two\t雪")
        );
        assert_eq!(events[2].value_integer, Some(0));
        assert_eq!(events[2].value_type_code, Some(-2475731913145812025));
        let event = serde_json::to_value(&events[3]).unwrap();
        assert_eq!(event["start_date"], "2001-01-01T00:00:00.250Z");
        assert_eq!(event["duration_seconds"], 2.5);
        assert!(event.get("value_integer").is_none());
        assert!(events[4].start_date.is_none());
        assert_eq!(before, std::fs::read(fixture.path()).unwrap());

        let write =
            query::<serde_json::Value>(&fixture.path(), "DELETE FROM ZOBJECT RETURNING Z_PK").await;
        assert!(write.unwrap_err().to_string().contains("readonly"));
        assert_eq!(before, std::fs::read(fixture.path()).unwrap());
    }

    #[tokio::test]
    async fn filters_before_pagination_with_exact_stream_and_half_open_time_range() {
        let fixture = Fixture::new().await;
        let mut options = ListOptions {
            stream: Some("/app/usage".into()),
            since: apple_time(Some(0.25)).unwrap(),
            until: apple_time(Some(1.0)).unwrap(),
            ..Default::default()
        };
        let events = list_at(&fixture.path(), &options).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "first");
        options.until = None;
        options.offset = 1;
        options.limit = 1;
        assert_eq!(
            list_at(&fixture.path(), &options).await.unwrap()[0].id,
            "first"
        );
        options.offset = 2;
        assert!(list_at(&fixture.path(), &options).await.unwrap().is_empty());
        options.offset = 0;
        options.limit = 0;
        assert!(list_at(&fixture.path(), &options).await.unwrap().is_empty());
        options.limit = 100;
        options.stream = Some("/quoted'stream".into());
        assert_eq!(
            list_at(&fixture.path(), &options).await.unwrap()[0].id,
            "local:4"
        );
        options.stream = Some("' OR 1=1 --".into());
        assert!(list_at(&fixture.path(), &options).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn streams_count_only_events_and_preserve_unknown_dates() {
        let fixture = Fixture::new().await;
        let streams = streams_at(&fixture.path()).await.unwrap();
        assert_eq!(streams.len(), 4);
        assert_eq!(streams[0].stream, "/app/usage");
        assert_eq!(streams[0].event_count, 2);
        assert_eq!(streams[0].first_start_date, apple_time(Some(0.25)).unwrap());
        assert_eq!(streams[0].last_start_date, apple_time(Some(1.0)).unwrap());
        assert_eq!(streams[2].stream, "/empty");
        assert!(streams[2].first_start_date.is_none());
    }

    #[tokio::test]
    async fn missing_and_incompatible_stores_fail_but_empty_stores_succeed() {
        let fixture = Fixture::new().await;
        let missing = fixture.0.join("missing.db");
        assert!(list_at(&missing, &ListOptions::default())
            .await
            .unwrap_err()
            .to_string()
            .contains("not found"));
        assert!(!missing.exists());
        run_command_with_timeout(
            "/usr/bin/sqlite3",
            &[fixture.path().to_str().unwrap(), "DELETE FROM ZOBJECT"],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(list_at(&fixture.path(), &ListOptions::default())
            .await
            .unwrap()
            .is_empty());
        assert!(streams_at(&fixture.path()).await.unwrap().is_empty());
        run_command_with_timeout(
            "/usr/bin/sqlite3",
            &[fixture.path().to_str().unwrap(), "DROP TABLE ZOBJECT"],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(list_at(&fixture.path(), &ListOptions::default())
            .await
            .unwrap_err()
            .to_string()
            .contains("no such table"));
        assert!(streams_at(&fixture.path()).await.is_err());
    }

    #[test]
    fn rejects_invalid_filters_and_converts_pre_epoch_dates() {
        let mut options = ListOptions {
            since: apple_time(Some(1.0)).unwrap(),
            until: apple_time(Some(1.0)).unwrap(),
            ..Default::default()
        };
        assert!(list_query(&options).is_err());
        options.until = apple_time(Some(0.0)).unwrap();
        assert!(list_query(&options).is_err());
        options.until = None;
        options.stream = Some("bad\0stream".into());
        assert!(list_query(&options).is_err());
        assert_eq!(
            apple_time(Some(-0.25)).unwrap().unwrap().to_rfc3339(),
            "2000-12-31T23:59:59.750+00:00"
        );
        assert!(apple_time(Some(f64::NAN)).is_err());
        assert!(apple_time(Some(f64::MAX)).is_err());
    }
}
