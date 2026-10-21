use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub process: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// What to read from the unified log.
#[derive(Debug, Default)]
pub struct LogQuery<'a> {
    pub minutes: u32,
    /// Every level, not only errors and faults. Needs a process or search.
    pub all_levels: bool,
    pub process: Option<&'a str>,
    pub search: Option<&'a str>,
    pub limit: usize,
}

const ERRORS: &str = "(messageType == error OR messageType == fault)";

/// Errors and faults from the last `minutes`, oldest first (the first 200).
pub async fn fetch(minutes: u32) -> anyhow::Result<Vec<LogEntry>> {
    let last = format!("{}m", minutes);
    let predicate = format!("eventType == logEvent AND {ERRORS}");
    let output = run_command_with_timeout(
        "log",
        &[
            "show",
            "--last",
            &last,
            "--style",
            "ndjson",
            "--predicate",
            &predicate,
        ],
        std::time::Duration::from_secs(30),
    )
    .await?;
    let mut entries = parse(&output);
    entries.truncate(200);
    Ok(entries)
}

/// The newest `limit` entries that match, oldest first.
pub async fn query(q: &LogQuery<'_>) -> anyhow::Result<Vec<LogEntry>> {
    let (minutes, predicate) = predicate(q)?;
    let last = format!("{}m", minutes);
    let output = run_command_with_timeout(
        "log",
        &[
            "show",
            "--last",
            &last,
            "--style",
            "ndjson",
            "--predicate",
            &predicate,
        ],
        std::time::Duration::from_secs(45),
    )
    .await?;
    let mut entries = parse(&output);
    let skip = entries.len().saturating_sub(q.limit.max(1));
    entries.drain(..skip);
    Ok(entries)
}

/// The window and `log show` predicate for a query. Process names and search text are
/// checked and quoted here, so neither can change the predicate's shape.
pub fn predicate(q: &LogQuery<'_>) -> anyhow::Result<(u32, String)> {
    let process = q.process.map(str::trim).filter(|p| !p.is_empty());
    let search = q.search.map(str::trim).filter(|s| !s.is_empty());
    if let Some(p) = process {
        anyhow::ensure!(
            p.len() <= 64
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c)),
            "process must be a plain process name, like Safari or WindowServer"
        );
    }
    if let Some(s) = search {
        anyhow::ensure!(s.chars().count() <= 200, "search is at most 200 characters");
    }
    anyhow::ensure!(
        !q.all_levels || process.is_some() || search.is_some(),
        "level all needs a process or search; every log line on a Mac is too much"
    );
    // Every level is much more data, so its window is shorter.
    let minutes = q.minutes.clamp(1, if q.all_levels { 15 } else { 60 });

    let mut parts = vec!["eventType == logEvent".to_string()];
    if !q.all_levels {
        parts.push(ERRORS.to_string());
    }
    if let Some(p) = process {
        parts.push(format!("process == \"{p}\""));
    }
    if let Some(s) = search {
        let quoted = s
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        parts.push(format!("eventMessage CONTAINS[c] \"{quoted}\""));
    }
    Ok((minutes, parts.join(" AND ")))
}

fn parse(output: &str) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            let timestamp = obj["timestamp"].as_str().unwrap_or("").to_string();
            let process = obj["processImagePath"]
                .as_str()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let message = obj["eventMessage"].as_str().unwrap_or("").to_string();
            let level = obj["messageType"]
                .as_str()
                .map(String::from)
                .or_else(|| obj["eventType"].as_str().map(String::from));

            if !message.is_empty() {
                entries.push(LogEntry {
                    timestamp,
                    process,
                    message,
                    level,
                });
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_grouped_before_and() {
        let (minutes, p) = predicate(&LogQuery {
            minutes: 500,
            limit: 50,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(minutes, 60);
        assert_eq!(
            p,
            "eventType == logEvent AND (messageType == error OR messageType == fault)"
        );
    }

    #[test]
    fn quotes_search_and_checks_process() {
        let (minutes, p) = predicate(&LogQuery {
            minutes: 30,
            all_levels: true,
            process: Some("WindowServer"),
            search: Some(r#"say "hi" \ OR 1"#),
            limit: 50,
        })
        .unwrap();
        assert_eq!(minutes, 15);
        assert_eq!(
            p,
            r#"eventType == logEvent AND process == "WindowServer" AND eventMessage CONTAINS[c] "say \"hi\" \\ OR 1""#
        );
        assert!(predicate(&LogQuery {
            process: Some(r#"x" OR 1 == 1"#),
            ..Default::default()
        })
        .is_err());
        assert!(predicate(&LogQuery {
            all_levels: true,
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn parses_ndjson() {
        let out = r#"{"timestamp":"2026-10-21 09:00:00.000000-0700","processImagePath":"/usr/libexec/airportd","eventMessage":"link down","messageType":"Error"}
not json
{"timestamp":"t","processImagePath":"/x","eventMessage":""}"#;
        let e = parse(out);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].process, "airportd");
        assert_eq!(e[0].level.as_deref(), Some("Error"));
    }
}
