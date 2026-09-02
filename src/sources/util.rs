use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::Duration;
use tokio::process::Command;

/// Default timeout for subprocess calls (120 seconds).
pub const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn run_osascript_with_timeout(script: &str, timeout: Duration) -> anyhow::Result<String> {
    let mut command = Command::new("/usr/bin/osascript");
    command
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("osascript timed out after {timeout:?}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("osascript failed: {stderr}");
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub async fn run_jxa(script: &str) -> anyhow::Result<String> {
    run_jxa_with_timeout(script, SUBPROCESS_TIMEOUT).await
}

pub async fn run_jxa_with_timeout(script: &str, timeout: Duration) -> anyhow::Result<String> {
    let mut command = Command::new("/usr/bin/osascript");
    command
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("JXA timed out after {timeout:?}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("JXA failed: {stderr}");
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub async fn run_command_with_timeout(
    cmd: &str,
    args: &[&str],
    timeout: Duration,
) -> anyhow::Result<String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("{cmd} timed out after {timeout:?}"))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cmd} failed: {stderr}");
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Parse dates from AppleScript output.
pub fn parse_applescript_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    let s = s
        .strip_prefix("date \"")
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s);

    let normalized: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = &normalized;

    // Strip leading day-of-week if present
    let without_dow = if let Some(comma_pos) = s.find(", ") {
        let before = &s[..comma_pos];
        if before.chars().all(|c| c.is_ascii_alphabetic()) && before.len() >= 3 {
            s[comma_pos + 2..].to_string()
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    };

    if let Some(ts) = parse_us_date_at_time(&without_dow) {
        return Some(ts);
    }

    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(ndt.and_utc());
    }

    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some(ndt.and_utc());
    }

    None
}

/// Parse "February 8, 2026 at 2:30:00 PM" style dates manually.
fn parse_us_date_at_time(s: &str) -> Option<DateTime<Utc>> {
    let (date_part, time_part) = s.split_once(" at ")?;

    let date_part = date_part.trim();
    let parts: Vec<&str> = date_part.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return None;
    }

    let month_str = parts[0];
    let day_str = parts[1].trim_end_matches(',');
    let year_str = parts[2];

    let month = match month_str.to_lowercase().as_str() {
        "january" => 1u32,
        "february" => 2,
        "march" => 3,
        "april" => 4,
        "may" => 5,
        "june" => 6,
        "july" => 7,
        "august" => 8,
        "september" => 9,
        "october" => 10,
        "november" => 11,
        "december" => 12,
        _ => return None,
    };
    let day: u32 = day_str.parse().ok()?;
    let year: i32 = year_str.parse().ok()?;

    let time_part = time_part.trim();
    let is_pm = time_part.to_uppercase().ends_with("PM");
    let time_digits = time_part
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == ' ')
        .trim();

    let time_parts: Vec<&str> = time_digits.split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }

    let mut hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = time_parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    if is_pm && hour != 12 {
        hour += 12;
    } else if !is_pm && hour == 12 {
        hour = 0;
    }

    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_opt(hour, minute, second)?;
    let ndt = NaiveDateTime::new(date, time);
    Some(ndt.and_utc())
}

/// Parse plist date strings (ISO 8601).
pub fn parse_plist_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}

/// Create a URL-safe slug from a string.
pub fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(60)
        .collect()
}

/// Apple epoch offset: seconds between Unix epoch and 2001-01-01.
pub const APPLE_EPOCH: i64 = 978_307_200;

/// Seconds since the Apple epoch for a UTC timestamp — the unit Core Data
/// and EventKit stores keep their dates in.
pub fn apple_seconds(dt: DateTime<Utc>) -> i64 {
    dt.timestamp() - APPLE_EPOCH
}

/// Nanoseconds since the Apple epoch — the unit Messages' `chat.db` keeps
/// `message.date` in.
pub fn apple_nanos(dt: DateTime<Utc>) -> i64 {
    apple_seconds(dt) * 1_000_000_000
}

/// A date as `sqlite3 -json` or JXA emits it: Apple-epoch seconds (as a
/// number or a numeric string) or an ISO 8601 string.
pub fn apple_json_date(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    match value {
        serde_json::Value::Number(n) => n
            .as_f64()
            .and_then(|ts| DateTime::from_timestamp(ts as i64 + APPLE_EPOCH, 0)),
        serde_json::Value::String(s) => {
            if let Ok(ts) = s.parse::<f64>() {
                DateTime::from_timestamp(ts as i64 + APPLE_EPOCH, 0)
            } else {
                parse_plist_date(s)
            }
        }
        _ => None,
    }
}

/// Whether a record last changed at `modified` passes a `since` filter. No
/// filter passes everything; a record with no known modification date fails
/// the filter, since it cannot be shown to have changed.
pub fn modified_since(modified: Option<DateTime<Utc>>, since: Option<DateTime<Utc>>) -> bool {
    match since {
        None => true,
        Some(since) => modified.is_some_and(|m| m >= since),
    }
}

/// Result of a write action (create, update, delete, etc.)
#[derive(Debug, serde::Serialize)]
pub struct ActionResult {
    pub ok: bool,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Per-item outcome from a batch mutation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BatchItemResult {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BatchItemResult {
    pub fn success(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ok: true,
            error: None,
        }
    }

    pub fn failure(id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ok: false,
            error: Some(error.into()),
        }
    }
}

/// Stable result shape for a mutation that targets several records.
///
/// `ok` is true only when every requested item succeeded. Callers can retry
/// only `results` entries whose `ok` is false without guessing how far a
/// partially successful AppleScript got.
#[derive(Debug, serde::Serialize)]
pub struct BatchActionResult {
    pub ok: bool,
    pub action: String,
    pub requested: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<BatchItemResult>,
}

impl BatchActionResult {
    pub fn new(action: &str, results: Vec<BatchItemResult>) -> Self {
        let succeeded = results.iter().filter(|result| result.ok).count();
        let requested = results.len();
        Self {
            ok: succeeded == requested,
            action: action.to_string(),
            requested,
            succeeded,
            failed: requested - succeeded,
            results,
        }
    }
}

impl ActionResult {
    pub fn success(action: &str) -> Self {
        Self {
            ok: true,
            action: action.to_string(),
            id: None,
            message: None,
        }
    }

    pub fn success_with_id(action: &str, id: &str) -> Self {
        Self {
            ok: true,
            action: action.to_string(),
            id: Some(id.to_string()),
            message: None,
        }
    }

    pub fn success_with_message(action: &str, msg: &str) -> Self {
        Self {
            ok: true,
            action: action.to_string(),
            id: None,
            message: Some(msg.to_string()),
        }
    }
}

/// Escape a string for safe embedding in JXA/JavaScript code.
pub fn escape_jxa(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Escape a string for safe embedding in AppleScript.
///
/// Control characters matter as much as quotes: a raw newline inside a
/// double-quoted AppleScript literal is a syntax error, so before these
/// escapes any multiline value (reminder notes, note bodies) silently broke
/// the whole script. AppleScript 2.0 string literals understand `\n`, `\r`,
/// and `\t`.
pub fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_parse_applescript_date_full() {
        let date = "Saturday, February  8, 2026 at 2:30:00 PM";
        let parsed = parse_applescript_date(date);
        assert!(parsed.is_some());
        let dt = parsed.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 8);
    }

    #[test]
    fn test_parse_applescript_date_short() {
        let parsed = parse_applescript_date("February  8, 2026 at 2:30:00 PM");
        assert!(parsed.is_some());
    }

    #[test]
    fn test_parse_applescript_date_wrapped() {
        let parsed = parse_applescript_date("date \"Saturday, February  8, 2026 at 2:30:00 PM\"");
        assert!(parsed.is_some());
    }

    #[test]
    fn test_parse_applescript_date_iso() {
        let parsed = parse_applescript_date("2026-02-08 14:30:00");
        assert!(parsed.is_some());
    }

    #[test]
    fn test_parse_applescript_date_invalid() {
        assert!(parse_applescript_date("garbage").is_none());
    }

    #[test]
    fn test_escape_applescript_control_chars() {
        assert_eq!(
            escape_applescript("line one\nline two\ttabbed\r"),
            "line one\\nline two\\ttabbed\\r"
        );
        assert_eq!(
            escape_applescript("say \"hi\" \\ bye"),
            "say \\\"hi\\\" \\\\ bye"
        );
    }

    #[test]
    fn batch_result_counts_partial_success() {
        let result = BatchActionResult::new(
            "batch-delete",
            vec![
                BatchItemResult::success("a"),
                BatchItemResult::failure("b", "not found"),
            ],
        );
        assert!(!result.ok);
        assert_eq!(result.requested, 2);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_slug() {
        assert_eq!(slug("Hello World!"), "hello_world_");
        assert_eq!(slug("foo-bar_baz"), "foo-bar_baz");
        assert_eq!(slug(""), "");
    }

    #[test]
    fn test_slug_long() {
        let long = "a".repeat(100);
        assert_eq!(slug(&long).len(), 60);
    }

    #[test]
    fn apple_epoch_arithmetic() {
        // 2001-01-01T00:00:00Z is the Apple epoch itself.
        let epoch = DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(apple_seconds(epoch), 0);
        assert_eq!(apple_nanos(epoch), 0);

        let next_day = DateTime::parse_from_rfc3339("2001-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(apple_seconds(next_day), 86_400);
        assert_eq!(apple_nanos(next_day), 86_400 * 1_000_000_000);
    }

    #[test]
    fn apple_json_date_reads_numbers_strings_and_iso() {
        let from_number = apple_json_date(&serde_json::json!(0)).unwrap();
        assert_eq!(from_number.to_rfc3339(), "2001-01-01T00:00:00+00:00");
        let from_float = apple_json_date(&serde_json::json!(86400.5)).unwrap();
        assert_eq!(from_float.to_rfc3339(), "2001-01-02T00:00:00+00:00");
        let from_string = apple_json_date(&serde_json::json!("86400")).unwrap();
        assert_eq!(from_string, from_float);
        let from_iso = apple_json_date(&serde_json::json!("2026-09-01T12:00:00.000Z")).unwrap();
        assert_eq!(from_iso.to_rfc3339(), "2026-09-01T12:00:00+00:00");
        assert!(apple_json_date(&serde_json::Value::Null).is_none());
        assert!(apple_json_date(&serde_json::json!("garbage")).is_none());
    }

    #[test]
    fn modified_since_filter() {
        let since = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let before = since - Duration::from_secs(1);
        let after = since + Duration::from_secs(1);

        assert!(modified_since(Some(before), None), "no filter passes all");
        assert!(modified_since(None, None));
        assert!(modified_since(Some(after), Some(since)));
        assert!(modified_since(Some(since), Some(since)), "inclusive");
        assert!(!modified_since(Some(before), Some(since)));
        assert!(
            !modified_since(None, Some(since)),
            "unknown modification date cannot be shown to have changed"
        );
    }

    #[test]
    fn test_parse_plist_date() {
        assert!(parse_plist_date("2024-06-15T10:30:00Z").is_some());
        assert!(parse_plist_date("2024-06-15T10:30:00.000Z").is_some());
        assert!(parse_plist_date("2024-06-15T10:30:00+00:00").is_some());
        assert!(parse_plist_date("garbage").is_none());
        assert!(parse_plist_date("").is_none());
    }

    #[tokio::test]
    async fn test_run_command_times_out() {
        let error =
            run_command_with_timeout("/bin/sh", &["-c", "sleep 10"], Duration::from_millis(10))
                .await
                .expect_err("sleep should exceed the timeout");

        assert!(error.to_string().contains("timed out"));
    }
}
