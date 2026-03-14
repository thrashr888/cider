use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CalendarEvent {
    pub title: String,
    pub calendar: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    pub is_all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<CalendarEvent>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{home}/Library/Calendars/Calendar Cache");

    if tokio::fs::metadata(&db_path).await.is_ok() {
        return fetch_from_sqlite(&db_path).await;
    }

    // Fall back to JXA — slower but works when Calendar Cache doesn't exist
    fetch_from_jxa().await
}

async fn fetch_from_sqlite(db_path: &str) -> anyhow::Result<Vec<CalendarEvent>> {
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(7);
    let end = now + chrono::Duration::days(30);
    let start_cd = start.timestamp() - 978_307_200;
    let end_cd = end.timestamp() - 978_307_200;

    let query = format!(
        r#"
SELECT
    COALESCE(ci.ZSUMMARY, ''),
    COALESCE(cal.ZTITLE, ''),
    COALESCE(ci.ZLOCATION, ''),
    datetime(ci.ZSTARTDATE + 978307200, 'unixepoch'),
    datetime(ci.ZENDDATE + 978307200, 'unixepoch'),
    COALESCE(ci.ZISALLDAY, 0),
    COALESCE(SUBSTR(ci.ZNOTES, 1, 500), '')
FROM ZCALENDARITEM ci
LEFT JOIN ZCALENDAR cal ON ci.ZCALENDAR = cal.Z_PK
WHERE ci.ZSTARTDATE >= {start_cd}
  AND ci.ZSTARTDATE <= {end_cd}
ORDER BY ci.ZSTARTDATE ASC
LIMIT 500;
"#
    );

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(parse_output(&stdout))
}

async fn fetch_from_jxa() -> anyhow::Result<Vec<CalendarEvent>> {
    // Per-event loop limited to 100 events per calendar. Slower but reliable.
    let script = r#"
const app = Application("Calendar");
const now = new Date();
const start = new Date(now.getTime() - 7 * 24 * 3600 * 1000);
const end = new Date(now.getTime() + 30 * 24 * 3600 * 1000);
const results = [];
const cals = app.calendars();
for (let ci = 0; ci < cals.length; ci++) {
    const cal = cals[ci];
    const calName = cal.name();
    let events;
    try { events = cal.events(); } catch(e) { continue; }
    if (!events || events.length === 0) continue;
    let count = 0;
    for (let i = events.length - 1; i >= 0 && count < 100; i--) {
        const ev = events[i];
        let sd;
        try { sd = ev.startDate(); } catch(e) { continue; }
        if (!sd || sd < start || sd > end) continue;
        count++;
        let title = "", loc = "", ed = "", allday = false, notes = "";
        try { title = (ev.summary() || "").replace(/[\t\n\r]/g, " "); } catch(e) {}
        try { loc = (ev.location() || "").replace(/[\t\n\r]/g, " "); } catch(e) {}
        try { ed = ev.endDate().toISOString(); } catch(e) {}
        try { allday = ev.alldayEvent(); } catch(e) {}
        try { notes = (ev.description() || "").slice(0, 500).replace(/[\t\n\r]/g, " "); } catch(e) {}
        if (title) results.push([title, calName, loc, sd.toISOString(), ed, allday ? "1" : "0", notes].join("\t"));
    }
}
results.join("\n")
"#;

    let output =
        super::util::run_jxa_with_timeout(script, std::time::Duration::from_secs(120)).await?;
    Ok(parse_output(&output))
}

fn parse_output(output: &str) -> Vec<CalendarEvent> {
    let mut records = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }
        let title = parts[0].trim();
        if title.is_empty() {
            continue;
        }
        let calendar = parts[1].trim();
        let location = parts[2].trim();
        let start_date = parts[3].trim();
        let end_date = parts[4].trim();
        let is_all_day = parts[5].trim() == "1";
        let notes = parts.get(6).copied().unwrap_or("").trim();

        records.push(CalendarEvent {
            title: title.to_string(),
            calendar: calendar.to_string(),
            location: if location.is_empty() {
                None
            } else {
                Some(location.to_string())
            },
            start_date: if start_date.is_empty() {
                None
            } else {
                Some(start_date.to_string())
            },
            end_date: if end_date.is_empty() {
                None
            } else {
                Some(end_date.to_string())
            },
            is_all_day,
            notes: if notes.is_empty() {
                None
            } else {
                Some(notes.to_string())
            },
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "Team standup\tWork\tZoom\t2026-03-14 10:00:00\t2026-03-14 10:30:00\t0\t\n\
                       Birthday\tPersonal\t\t2026-03-15 00:00:00\t2026-03-16 00:00:00\t1\tBring cake\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Team standup");
        assert_eq!(records[0].calendar, "Work");
        assert_eq!(records[0].location.as_deref(), Some("Zoom"));
        assert!(!records[0].is_all_day);
        assert!(records[1].is_all_day);
        assert_eq!(records[1].notes.as_deref(), Some("Bring cake"));
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}
