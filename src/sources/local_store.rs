//! Shared read-only SQLite and history-filter plumbing.
use super::util::{run_command_with_timeout, APPLE_EPOCH};
use anyhow::Context;
use chrono::{DateTime, Utc};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Common filters for retained local history. Date bounds are [since, until).
#[derive(Debug, Clone)]
pub struct HistoryOptions {
    pub app: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: u32,
    pub offset: u32,
}
impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            app: None,
            since: None,
            until: None,
            limit: 100,
            offset: 0,
        }
    }
}
impl HistoryOptions {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if let (Some(since), Some(until)) = (self.since, self.until) {
            anyhow::ensure!(
                since < until,
                "invalid time range: --since must precede --until"
            );
        }
        anyhow::ensure!(self.limit <= 10_000, "invalid limit: maximum is 10000");
        if let Some(app) = &self.app {
            sql_string(app)?;
        }
        Ok(())
    }
    pub(crate) fn sql_filter(&self, date: &str, app: &str) -> anyhow::Result<String> {
        self.validate()?;
        let mut terms = vec!["1=1".to_string()];
        if let Some(value) = &self.app {
            terms.push(format!("{app} = {}", sql_string(value)?));
        }
        if let Some(value) = self.since {
            terms.push(format!("{date} >= {}", apple_seconds(value)));
        }
        if let Some(value) = self.until {
            terms.push(format!("{date} < {}", apple_seconds(value)));
        }
        Ok(terms.join(" AND "))
    }
    pub(crate) fn contains(&self, date: DateTime<Utc>) -> bool {
        self.since.map_or(true, |s| date >= s) && self.until.map_or(true, |u| date < u)
    }
}

pub(crate) fn home_path(relative: &str) -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .context("HOME is not set")?;
    Ok(PathBuf::from(home).join(relative))
}
pub(crate) fn sql_string(value: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !value.contains('\0'),
        "invalid filter: contains a NUL character"
    );
    Ok(format!("'{}'", value.replace('\'', "''")))
}
pub(crate) fn read_error(path: &Path, error: std::io::Error) -> anyhow::Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => anyhow::anyhow!("Store not found at {}", path.display()),
        std::io::ErrorKind::PermissionDenied => anyhow::anyhow!(
            "Cannot read {}: {error}. Full Disk Access may be required for the app launching cider",
            path.display()
        ),
        _ => anyhow::anyhow!("Cannot read {}: {error}", path.display()),
    }
}
pub(crate) async fn query<T: serde::de::DeserializeOwned>(
    path: &Path,
    sql: &str,
) -> anyhow::Result<Vec<T>> {
    tokio::fs::File::open(path)
        .await
        .map_err(|e| read_error(path, e))?;
    let name = path.to_str().context("Store path is not UTF-8")?;
    let output = run_command_with_timeout(
        "/usr/bin/sqlite3",
        &["-readonly", "-json", "-cmd", ".timeout 1000", name, sql],
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Cannot query {}: {e}", path.display()))?;
    if output.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&output).context("Cannot decode local database query result")
}
pub(crate) fn apple_time(value: Option<f64>) -> anyhow::Result<Option<DateTime<Utc>>> {
    value
        .map(|seconds| {
            anyhow::ensure!(seconds.is_finite(), "Invalid stored timestamp");
            // Add the epoch as an integer so an exported fractional timestamp
            // can be reused as an inclusive filter without losing float bits.
            let unix = (seconds.floor() as i64)
                .checked_add(APPLE_EPOCH)
                .context("Invalid stored timestamp")?;
            DateTime::from_timestamp(unix, (seconds.rem_euclid(1.0) * 1e9) as u32)
                .context("Invalid stored timestamp")
        })
        .transpose()
}
pub(crate) fn apple_seconds(date: DateTime<Utc>) -> f64 {
    (date.timestamp() - APPLE_EPOCH) as f64 + date.timestamp_subsec_nanos() as f64 / 1e9
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub struct Database(pub PathBuf);
    impl Database {
        pub async fn new(sql: &str) -> Self {
            let p = std::env::temp_dir().join(format!("cider-history-{}.db", uuid::Uuid::new_v4()));
            run_command_with_timeout(
                "/usr/bin/sqlite3",
                &[p.to_str().unwrap(), sql],
                Duration::from_secs(5),
            )
            .await
            .unwrap();
            Self(p)
        }
    }
    impl Drop for Database {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    #[tokio::test]
    async fn readonly_missing_empty_and_malformed_stores_are_distinct() {
        let db = Database::new("CREATE TABLE items (id INTEGER);").await;
        assert!(query::<serde_json::Value>(&db.0, "SELECT * FROM items")
            .await
            .unwrap()
            .is_empty());
        assert!(query::<serde_json::Value>(&db.0, "DROP TABLE items")
            .await
            .unwrap_err()
            .to_string()
            .contains("readonly"));
        assert!(query::<serde_json::Value>(&db.0, "SELECT * FROM missing")
            .await
            .is_err());
        let missing = db.0.with_extension("missing");
        assert!(query::<serde_json::Value>(&missing, "SELECT 1")
            .await
            .unwrap_err()
            .to_string()
            .contains("not found"));
        assert!(!missing.exists());
    }
    #[test]
    fn validates_dates_limits_and_filter_escaping() {
        assert_eq!(sql_string("a'b").unwrap(), "'a''b'");
        assert!(sql_string("a\0b").is_err());
        let mut o = HistoryOptions {
            since: apple_time(Some(1.0)).unwrap(),
            until: apple_time(Some(1.0)).unwrap(),
            ..Default::default()
        };
        assert!(o.validate().is_err());
        o.until = None;
        o.limit = 10_001;
        assert!(o.validate().is_err());
        assert_eq!(
            apple_time(Some(0.25)).unwrap().unwrap().to_rfc3339(),
            "2001-01-01T00:00:00.250+00:00"
        );
    }

    #[test]
    fn fractional_timestamps_round_trip_into_inclusive_filters() {
        for seconds in [807721184.200309, 807721184.1234567, -0.25, 0.25] {
            assert_eq!(
                apple_seconds(apple_time(Some(seconds)).unwrap().unwrap()),
                seconds
            );
        }
        assert!(apple_time(Some(f64::MAX)).is_err());
        assert!(apple_time(Some(f64::NAN)).is_err());
    }
}
