use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::Duration;
use tokio::process::Command;

/// Default timeout for subprocess calls (120 seconds).
pub const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn run_osascript_with_timeout(script: &str, timeout: Duration) -> anyhow::Result<String> {
    let child = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

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
    let child = Command::new("/usr/bin/osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-e")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

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
    let child = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

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

/// Truncate text to a reasonable title length.
pub fn truncate_for_title(text: &str) -> String {
    let text = text.trim();
    if text.len() <= 120 {
        text.to_string()
    } else {
        let mut end = 120;
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &text[..end])
    }
}

/// Apple epoch offset: seconds between Unix epoch and 2001-01-01.
pub const APPLE_EPOCH: i64 = 978_307_200;

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
    fn test_truncate_for_title_short() {
        assert_eq!(truncate_for_title("Hello"), "Hello");
    }

    #[test]
    fn test_truncate_for_title_long() {
        let long = "a".repeat(200);
        let title = truncate_for_title(&long);
        assert!(title.len() <= 123);
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_truncate_for_title_exact() {
        let exact = "a".repeat(120);
        let title = truncate_for_title(&exact);
        assert_eq!(title.len(), 120);
        assert!(!title.ends_with("..."));
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
    fn test_parse_plist_date() {
        assert!(parse_plist_date("2024-06-15T10:30:00Z").is_some());
        assert!(parse_plist_date("2024-06-15T10:30:00.000Z").is_some());
        assert!(parse_plist_date("2024-06-15T10:30:00+00:00").is_some());
        assert!(parse_plist_date("garbage").is_none());
        assert!(parse_plist_date("").is_none());
    }
}
