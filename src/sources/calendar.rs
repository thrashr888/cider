use super::util::{escape_jxa, run_command_with_timeout, run_jxa_with_timeout, ActionResult};
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

/// List calendar events, with optional day range and calendar filter.
pub async fn list(
    days_back: Option<u32>,
    days_ahead: Option<u32>,
    calendar_filter: Option<&str>,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let home = std::env::var("HOME").unwrap_or_default();

    let back = days_back.unwrap_or(7);
    let ahead = days_ahead.unwrap_or(30);

    // Try Group Container database first (modern macOS)
    let group_db =
        format!("{home}/Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb");
    if tokio::fs::metadata(&group_db).await.is_ok() {
        let events = fetch_from_group_db(&group_db, back, ahead).await?;
        return Ok(filter_by_calendar(events, calendar_filter));
    }

    // Try legacy Calendar Cache
    let cache_db = format!("{home}/Library/Calendars/Calendar Cache");
    if tokio::fs::metadata(&cache_db).await.is_ok() {
        let events = fetch_from_cache_db(&cache_db, back, ahead).await?;
        return Ok(filter_by_calendar(events, calendar_filter));
    }

    // Fall back to JXA — slower but works when no local database exists
    let events = fetch_from_jxa(back, ahead).await?;
    Ok(filter_by_calendar(events, calendar_filter))
}

/// Create a calendar event via JXA.
pub async fn create(
    title: &str,
    start: &str,
    end: &str,
    calendar: Option<&str>,
    location: Option<&str>,
    notes: Option<&str>,
    all_day: bool,
) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_jxa(title);
    let escaped_start = escape_jxa(start);
    let escaped_end = escape_jxa(end);
    let cal_name = calendar.unwrap_or("Calendar");
    let escaped_cal = escape_jxa(cal_name);

    let mut props = format!(
        "summary: \"{}\", startDate: new Date(\"{}\"), endDate: new Date(\"{}\")",
        escaped_title, escaped_start, escaped_end
    );

    if all_day {
        props.push_str(", alldayEvent: true");
    }

    if let Some(loc) = location {
        let escaped_loc = escape_jxa(loc);
        props.push_str(&format!(", location: \"{}\"", escaped_loc));
    }

    if let Some(n) = notes {
        let escaped_notes = escape_jxa(n);
        props.push_str(&format!(", description: \"{}\"", escaped_notes));
    }

    let script = format!(
        r#"
const app = Application("Calendar");
const cal = app.calendars.byName("{}");
const ev = app.Event({{ {} }});
cal.events.push(ev);
ev.summary();
"#,
        escaped_cal, props
    );

    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "created",
        &format!("Created event '{}'", output.trim()),
    ))
}

/// Delete a calendar event by title and date via JXA.
pub async fn delete(
    title: &str,
    date: &str,
    calendar: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_jxa(title);
    let escaped_date = escape_jxa(date);
    let calendar_setup = if let Some(calendar_name) = calendar {
        let escaped_calendar = escape_jxa(calendar_name);
        format!(
            "const cals = [app.calendars.byName(\"{}\")];",
            escaped_calendar
        )
    } else {
        "const cals = app.calendars();".to_string()
    };

    let script = format!(
        r#"
const app = Application("Calendar");
const targetDate = new Date("{}T00:00:00");
const targetKey = [targetDate.getFullYear(), targetDate.getMonth(), targetDate.getDate()].join("-");
{}
let found = false;
for (let i = 0; i < cals.length; i++) {{
    let events;
    try {{ events = cals[i].events(); }} catch(e) {{ continue; }}
    for (let j = 0; j < events.length; j++) {{
        try {{
            const ev = events[j];
            const sd = ev.startDate();
            const sdKey = [sd.getFullYear(), sd.getMonth(), sd.getDate()].join("-");
            if (ev.summary() === "{}" && sdKey === targetKey) {{
                app.delete(ev);
                found = true;
                break;
            }}
        }} catch(e) {{ continue; }}
    }}
    if (found) break;
}}
if (!found) throw new Error("Event not found: {} on {}");
"deleted"
"#,
        escaped_date, calendar_setup, escaped_title, escaped_title, escaped_date
    );

    run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;

    Ok(ActionResult::success_with_message(
        "deleted",
        &format!("Deleted event '{}' on {}", title, date),
    ))
}

/// List all calendar names via JXA.
pub async fn calendars() -> anyhow::Result<Vec<String>> {
    let script = r#"
const app = Application("Calendar");
const names = app.calendars.name();
names.join("\n");
"#;

    let output = run_jxa_with_timeout(script, std::time::Duration::from_secs(30)).await?;

    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn filter_by_calendar(
    events: Vec<CalendarEvent>,
    calendar_filter: Option<&str>,
) -> Vec<CalendarEvent> {
    match calendar_filter {
        Some(filter) => {
            let filter_lower = filter.to_lowercase();
            events
                .into_iter()
                .filter(|e| e.calendar.to_lowercase() == filter_lower)
                .collect()
        }
        None => events,
    }
}

/// Modern macOS: ~/Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb
async fn fetch_from_group_db(
    db_path: &str,
    days_back: u32,
    days_ahead: u32,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(i64::from(days_back));
    let end = now + chrono::Duration::days(i64::from(days_ahead));
    let start_cd = start.timestamp() - 978_307_200;
    let end_cd = end.timestamp() - 978_307_200;

    let query = format!(
        r#"
SELECT
    COALESCE(ci.summary, '') AS title,
    COALESCE(c.title, '') AS calendar,
    COALESCE(l.title, '') AS location,
    datetime(ci.start_date + 978307200, 'unixepoch') AS start_date,
    datetime(ci.end_date + 978307200, 'unixepoch') AS end_date,
    COALESCE(ci.all_day, 0) AS all_day,
    COALESCE(ci.description, '') AS notes
FROM CalendarItem ci
LEFT JOIN Calendar c ON ci.calendar_id = c.ROWID
LEFT JOIN Location l ON ci.location_id = l.ROWID
WHERE ci.start_date >= {start_cd}
  AND ci.start_date <= {end_cd}
ORDER BY ci.start_date ASC
LIMIT 500;
"#
    );

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-json", db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(parse_json_rows(&stdout))
}

/// Legacy macOS: ~/Library/Calendars/Calendar Cache (Core Data format)
async fn fetch_from_cache_db(
    db_path: &str,
    days_back: u32,
    days_ahead: u32,
) -> anyhow::Result<Vec<CalendarEvent>> {
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(i64::from(days_back));
    let end = now + chrono::Duration::days(i64::from(days_ahead));
    let start_cd = start.timestamp() - 978_307_200;
    let end_cd = end.timestamp() - 978_307_200;

    let query = format!(
        r#"
SELECT
    COALESCE(ci.ZSUMMARY, '') AS title,
    COALESCE(cal.ZTITLE, '') AS calendar,
    COALESCE(ci.ZLOCATION, '') AS location,
    datetime(ci.ZSTARTDATE + 978307200, 'unixepoch') AS start_date,
    datetime(ci.ZENDDATE + 978307200, 'unixepoch') AS end_date,
    COALESCE(ci.ZISALLDAY, 0) AS all_day,
    COALESCE(ci.ZNOTES, '') AS notes
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
        &["-json", db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    Ok(parse_json_rows(&stdout))
}

async fn fetch_from_jxa(days_back: u32, days_ahead: u32) -> anyhow::Result<Vec<CalendarEvent>> {
    let script = format!(
        r#"
const app = Application("Calendar");
const now = new Date();
const start = new Date(now.getTime() - {} * 24 * 3600 * 1000);
const end = new Date(now.getTime() + {} * 24 * 3600 * 1000);
const results = [];
const cals = app.calendars();
for (let ci = 0; ci < cals.length; ci++) {{
    const cal = cals[ci];
    const calName = cal.name();
    let events;
    try {{ events = cal.events(); }} catch(e) {{ continue; }}
    if (!events || events.length === 0) continue;
    let count = 0;
    for (let i = events.length - 1; i >= 0 && count < 100; i--) {{
        const ev = events[i];
        let sd;
        try {{ sd = ev.startDate(); }} catch(e) {{ continue; }}
        if (!sd || sd < start || sd > end) continue;
        count++;
        let title = "", loc = "", ed = "", allday = false, notes = "";
        try {{ title = ev.summary() || ""; }} catch(e) {{}}
        try {{ loc = ev.location() || ""; }} catch(e) {{}}
        try {{ ed = ev.endDate().toISOString(); }} catch(e) {{}}
        try {{ allday = ev.alldayEvent(); }} catch(e) {{}}
        try {{ notes = ev.description() || ""; }} catch(e) {{}}
        if (title) results.push({{title: title, calendar: calName, location: loc, start_date: sd.toISOString(), end_date: ed, all_day: allday ? 1 : 0, notes: notes}});
    }}
}}
JSON.stringify(results)
"#,
        days_back, days_ahead
    );

    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;
    Ok(parse_json_rows(&output))
}

/// Rows arrive as JSON (`sqlite3 -json` or `JSON.stringify` from JXA), so
/// multiline event notes — the normal case for meeting invites — survive
/// intact. The old tab-separated read emitted a literal newline into the row,
/// truncating notes at the first line and silently dropping the rest.
fn parse_json_rows(output: &str) -> Vec<CalendarEvent> {
    let output = output.trim();
    if output.is_empty() {
        return Vec::new();
    }

    let rows: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Skipping unparseable calendar output: {e}");
            return Vec::new();
        }
    };

    let mut records = Vec::new();
    for row in &rows {
        let title = row["title"].as_str().unwrap_or("").trim();
        if title.is_empty() {
            continue;
        }
        let calendar = row["calendar"].as_str().unwrap_or("").trim();
        let location = row["location"].as_str().unwrap_or("").trim();
        let start_date = row["start_date"].as_str().unwrap_or("").trim();
        let end_date = row["end_date"].as_str().unwrap_or("").trim();
        let is_all_day = row["all_day"].as_i64().unwrap_or(0) != 0;
        let notes = row["notes"].as_str().map(str::trim).unwrap_or("");

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
    fn test_parse_json_rows() {
        let output = r#"[
            {"title":"Team standup","calendar":"Work","location":"Zoom","start_date":"2026-03-14 10:00:00","end_date":"2026-03-14 10:30:00","all_day":0,"notes":""},
            {"title":"Birthday","calendar":"Personal","location":"","start_date":"2026-03-15 00:00:00","end_date":"2026-03-16 00:00:00","all_day":1,"notes":"Bring cake"}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Team standup");
        assert_eq!(records[0].calendar, "Work");
        assert_eq!(records[0].location.as_deref(), Some("Zoom"));
        assert!(!records[0].is_all_day);
        assert!(records[1].is_all_day);
        assert_eq!(records[1].notes.as_deref(), Some("Bring cake"));
    }

    #[test]
    fn test_parse_json_rows_empty() {
        assert!(parse_json_rows("").is_empty());
        assert!(parse_json_rows("[]").is_empty());
    }

    /// Meeting-invite notes are multiline by definition (Zoom blocks,
    /// agendas, dial-ins); they must come through whole, not cut at the
    /// first newline or capped at 500 chars.
    #[test]
    fn test_parse_json_rows_multiline_notes() {
        let notes = format!(
            "Agenda:\n- item one\n- item two\n\nJoin: https://zoom.example\n{}",
            "x".repeat(2000)
        );
        let output = serde_json::json!([{
            "title": "Planning",
            "calendar": "Work",
            "location": "",
            "start_date": "2026-03-14 10:00:00",
            "end_date": "2026-03-14 11:00:00",
            "all_day": 0,
            "notes": notes,
        }])
        .to_string();
        let records = parse_json_rows(&output);
        assert_eq!(records.len(), 1);
        let n = records[0].notes.as_deref().unwrap();
        assert!(n.contains('\n'), "newlines survive");
        assert!(n.len() > 2000, "no 500-char cap");
    }

    #[test]
    fn test_filter_by_calendar() {
        let events = vec![
            CalendarEvent {
                title: "Meeting".to_string(),
                calendar: "Work".to_string(),
                location: None,
                start_date: None,
                end_date: None,
                is_all_day: false,
                notes: None,
            },
            CalendarEvent {
                title: "Birthday".to_string(),
                calendar: "Personal".to_string(),
                location: None,
                start_date: None,
                end_date: None,
                is_all_day: false,
                notes: None,
            },
        ];

        let filtered = filter_by_calendar(events, Some("work"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Meeting");
    }

    #[test]
    fn test_filter_by_calendar_none() {
        let events = vec![CalendarEvent {
            title: "Meeting".to_string(),
            calendar: "Work".to_string(),
            location: None,
            start_date: None,
            end_date: None,
            is_all_day: false,
            notes: None,
        }];

        let filtered = filter_by_calendar(events, None);
        assert_eq!(filtered.len(), 1);
    }
}
