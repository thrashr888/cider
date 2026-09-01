use super::util::{
    escape_jxa, run_command_with_timeout, run_jxa_with_timeout, ActionResult, BatchActionResult,
    BatchItemResult,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct CalendarEvent {
    pub id: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub has_attendees: bool,
    pub has_recurrences: bool,
    pub attendee_count: usize,
    pub alarm_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCalendarEvent {
    pub title: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub calendar: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub all_day: bool,
}

#[derive(Debug, Default)]
pub struct UpdateFields<'a> {
    pub title: Option<&'a str>,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub location: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub all_day: Option<bool>,
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
        match fetch_from_group_db(&group_db, back, ahead).await {
            Ok(events) => return Ok(filter_by_calendar(events, calendar_filter)),
            Err(error) => eprintln!("Calendar database read failed; trying fallback: {error}"),
        }
    }

    // Try legacy Calendar Cache
    let cache_db = format!("{home}/Library/Calendars/Calendar Cache");
    if tokio::fs::metadata(&cache_db).await.is_ok() {
        match fetch_from_cache_db(&cache_db, back, ahead).await {
            Ok(events) => return Ok(filter_by_calendar(events, calendar_filter)),
            Err(error) => eprintln!("Legacy Calendar database read failed; trying JXA: {error}"),
        }
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
    validate_event_range(start, end)?;
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
ev.uid();
"#,
        escaped_cal, props
    );

    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_id("created", output.trim()))
}

/// Create several calendar events in one Calendar automation session.
pub async fn batch_create(events: &[NewCalendarEvent]) -> anyhow::Result<BatchActionResult> {
    validate_batch(events)?;
    let mut statements = String::new();
    for (index, event) in events.iter().enumerate() {
        let mut props = format!(
            "summary: \"{}\", startDate: new Date(\"{}\"), endDate: new Date(\"{}\")",
            escape_jxa(&event.title),
            escape_jxa(&event.start),
            escape_jxa(&event.end)
        );
        if event.all_day {
            props.push_str(", alldayEvent: true");
        }
        if let Some(location) = &event.location {
            props.push_str(&format!(", location: \"{}\"", escape_jxa(location)));
        }
        if let Some(notes) = &event.notes {
            props.push_str(&format!(", description: \"{}\"", escape_jxa(notes)));
        }
        let calendar = escape_jxa(event.calendar.as_deref().unwrap_or("Calendar"));
        statements.push_str(&format!(
            r#"
try {{
    const cal{index} = app.calendars.byName("{calendar}");
    const ev{index} = app.Event({{{props}}});
    cal{index}.events.push(ev{index});
    results.push({{id: ev{index}.uid(), ok: true}});
}} catch (error) {{
    results.push({{id: "item:{index}", ok: false, error: String(error)}});
}}
"#
        ));
    }
    let script = format!(
        r#"
const app = Application("Calendar");
const results = [];
{statements}
JSON.stringify(results)
"#
    );
    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;
    let results: Vec<BatchItemResult> = serde_json::from_str(&output)?;
    Ok(BatchActionResult::new("batch-create", results))
}

pub fn validate_batch(events: &[NewCalendarEvent]) -> anyhow::Result<()> {
    if events.is_empty() {
        anyhow::bail!("At least one event is required");
    }
    if events.len() > 100 {
        anyhow::bail!("Calendar batches are limited to 100 events");
    }
    for (index, event) in events.iter().enumerate() {
        if event.title.trim().is_empty()
            || event.start.trim().is_empty()
            || event.end.trim().is_empty()
        {
            anyhow::bail!("Calendar batch item {index} requires title, start, and end");
        }
        validate_event_range(&event.start, &event.end)
            .map_err(|error| anyhow::anyhow!("Calendar batch item {index}: {error}"))?;
    }
    Ok(())
}

pub fn validate_event_range(start: &str, end: &str) -> anyhow::Result<()> {
    fn parse(value: &str) -> anyhow::Result<chrono::DateTime<chrono::Utc>> {
        if let Ok(date_time) = chrono::DateTime::parse_from_rfc3339(value) {
            return Ok(date_time.with_timezone(&chrono::Utc));
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            return Ok(date
                .and_hms_opt(0, 0, 0)
                .expect("midnight is valid")
                .and_utc());
        }
        anyhow::bail!("Invalid ISO 8601 date/time: {value}")
    }

    let start = parse(start)?;
    let end = parse(end)?;
    if end < start {
        anyhow::bail!("Event end must not be before start");
    }
    Ok(())
}

/// Fetch one event by the stable UID printed by `calendar list`.
pub async fn get(id: &str) -> anyhow::Result<CalendarEvent> {
    let home = std::env::var("HOME").unwrap_or_default();
    let group_db =
        format!("{home}/Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb");
    if tokio::fs::metadata(&group_db).await.is_ok() {
        match fetch_one_from_group_db(&group_db, id).await {
            Ok(Some(event)) => return Ok(event),
            Ok(None) => {}
            Err(error) => eprintln!("Calendar database lookup failed; trying JXA: {error}"),
        }
    }
    let script = build_find_by_id_script(id, "return JSON.stringify(eventRecord(ev, cal.name()));");
    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;
    parse_json_rows(&format!("[{output}]"))
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Calendar event not found: {id}"))
}

fn escape_sql(value: &str) -> String {
    value.replace('\'', "''")
}

async fn fetch_one_from_group_db(db_path: &str, id: &str) -> anyhow::Result<Option<CalendarEvent>> {
    let id = escape_sql(id);
    let query = format!(
        r#"
SELECT
    COALESCE(NULLIF(ci.unique_identifier, ''), NULLIF(ci.external_id, ''), NULLIF(ci.UUID, ''), CAST(ci.ROWID AS TEXT)) AS id,
    COALESCE(ci.summary, '') AS title,
    COALESCE(c.title, '') AS calendar,
    COALESCE(l.title, '') AS location,
    datetime(ci.start_date + 978307200, 'unixepoch') AS start_date,
    datetime(ci.end_date + 978307200, 'unixepoch') AS end_date,
    COALESCE(ci.all_day, 0) AS all_day,
    COALESCE(ci.description, '') AS notes,
    COALESCE(ci.url, '') AS url,
    COALESCE(ci.has_attendees, 0) AS has_attendees,
    COALESCE(ci.has_recurrences, 0) AS has_recurrences,
    (SELECT COUNT(*) FROM Participant p WHERE p.owner_id = ci.ROWID) AS attendee_count,
    (SELECT COUNT(*) FROM Alarm a WHERE a.calendaritem_owner_id = ci.ROWID) AS alarm_count
FROM CalendarItem ci
LEFT JOIN Calendar c ON ci.calendar_id = c.ROWID
LEFT JOIN Location l ON ci.location_id = l.ROWID
WHERE ci.unique_identifier = '{id}'
   OR ci.external_id = '{id}'
   OR ci.UUID = '{id}'
LIMIT 1;
"#
    );
    let output = run_command_with_timeout(
        "sqlite3",
        &["-json", db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;
    Ok(parse_json_rows(&output).into_iter().next())
}

/// Update one event in place by stable UID.
pub async fn update(id: &str, fields: &UpdateFields<'_>) -> anyhow::Result<ActionResult> {
    if let Some(start) = fields.start {
        validate_event_range(start, fields.end.unwrap_or(start))?;
    } else if let Some(end) = fields.end {
        validate_event_range(end, end)?;
    }
    let mut updates = Vec::new();
    if let Some(title) = fields.title {
        updates.push(format!("ev.summary = \"{}\";", escape_jxa(title)));
    }
    if let Some(start) = fields.start {
        updates.push(format!(
            "ev.startDate = new Date(\"{}\");",
            escape_jxa(start)
        ));
    }
    if let Some(end) = fields.end {
        updates.push(format!("ev.endDate = new Date(\"{}\");", escape_jxa(end)));
    }
    if let Some(location) = fields.location {
        updates.push(format!("ev.location = \"{}\";", escape_jxa(location)));
    }
    if let Some(notes) = fields.notes {
        updates.push(format!("ev.description = \"{}\";", escape_jxa(notes)));
    }
    if let Some(all_day) = fields.all_day {
        updates.push(format!("ev.alldayEvent = {all_day};"));
    }
    if updates.is_empty() {
        anyhow::bail!("Nothing to update");
    }
    let action = format!("{} return ev.uid();", updates.join("\n"));
    let script = build_find_by_id_script(id, &action);
    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;
    Ok(ActionResult::success_with_id("updated", output.trim()))
}

/// Delete one event by the stable UID printed by `calendar list`.
pub async fn delete_by_id(id: &str) -> anyhow::Result<ActionResult> {
    let script = build_find_by_id_script(id, "app.delete(ev); return targetId;");
    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(120)).await?;
    Ok(ActionResult::success_with_id("deleted", output.trim()))
}

fn build_find_by_id_script(id: &str, action: &str) -> String {
    format!(
        r#"
(function() {{
const app = Application("Calendar");
const targetId = "{}";
function eventRecord(ev, calendar) {{
    let location = "", endDate = "", notes = "", url = "", allDay = false;
    try {{ location = ev.location() || ""; }} catch (error) {{}}
    try {{ endDate = ev.endDate().toISOString(); }} catch (error) {{}}
    try {{ notes = ev.description() || ""; }} catch (error) {{}}
    try {{ url = ev.url() || ""; }} catch (error) {{}}
    try {{ allDay = ev.alldayEvent(); }} catch (error) {{}}
    return {{
        id: ev.uid(), title: ev.summary() || "", calendar,
        location, start_date: ev.startDate().toISOString(), end_date: endDate,
        all_day: allDay ? 1 : 0, notes, url,
        has_attendees: 0, has_recurrences: 0,
        attendee_count: 0, alarm_count: 0
    }};
}}
const calendars = app.calendars();
for (let c = 0; c < calendars.length; c++) {{
    const cal = calendars[c];
    let events;
    try {{ events = cal.events(); }} catch (error) {{ continue; }}
    for (let i = 0; i < events.length; i++) {{
        const ev = events[i];
        let uid;
        try {{ uid = ev.uid(); }} catch (error) {{ continue; }}
        if (uid === targetId) {{ {action} }}
    }}
}}
throw new Error("Calendar event not found: " + targetId);
}})();
"#,
        escape_jxa(id)
    )
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
const matches = [];
for (let i = 0; i < cals.length; i++) {{
    let events;
    try {{ events = cals[i].events(); }} catch(e) {{ continue; }}
    for (let j = 0; j < events.length; j++) {{
        try {{
            const ev = events[j];
            const sd = ev.startDate();
            const sdKey = [sd.getFullYear(), sd.getMonth(), sd.getDate()].join("-");
            if (ev.summary() === "{}" && sdKey === targetKey) {{
                matches.push(ev);
            }}
        }} catch(e) {{ continue; }}
    }}
}}
if (matches.length === 0) throw new Error("Event not found: {} on {}");
if (matches.length > 1) throw new Error("Ambiguous event target: " + matches.length + " events match; pass --id");
app.delete(matches[0]);
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
    COALESCE(NULLIF(ci.unique_identifier, ''), NULLIF(ci.external_id, ''), NULLIF(ci.UUID, ''), CAST(ci.ROWID AS TEXT)) AS id,
    COALESCE(ci.summary, '') AS title,
    COALESCE(c.title, '') AS calendar,
    COALESCE(l.title, '') AS location,
    datetime(ci.start_date + 978307200, 'unixepoch') AS start_date,
    datetime(ci.end_date + 978307200, 'unixepoch') AS end_date,
    COALESCE(ci.all_day, 0) AS all_day,
    COALESCE(ci.description, '') AS notes,
    COALESCE(ci.url, '') AS url,
    COALESCE(ci.has_attendees, 0) AS has_attendees,
    COALESCE(ci.has_recurrences, 0) AS has_recurrences,
    (SELECT COUNT(*) FROM Participant p WHERE p.owner_id = ci.ROWID) AS attendee_count,
    (SELECT COUNT(*) FROM Alarm a WHERE a.calendaritem_owner_id = ci.ROWID) AS alarm_count
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
    COALESCE(NULLIF(ci.ZUNIQUEIDENTIFIER, ''), CAST(ci.Z_PK AS TEXT)) AS id,
    COALESCE(ci.ZSUMMARY, '') AS title,
    COALESCE(cal.ZTITLE, '') AS calendar,
    COALESCE(ci.ZLOCATION, '') AS location,
    datetime(ci.ZSTARTDATE + 978307200, 'unixepoch') AS start_date,
    datetime(ci.ZENDDATE + 978307200, 'unixepoch') AS end_date,
    COALESCE(ci.ZISALLDAY, 0) AS all_day,
    COALESCE(ci.ZNOTES, '') AS notes,
    '' AS url,
    0 AS has_attendees,
    0 AS has_recurrences,
    0 AS attendee_count,
    0 AS alarm_count
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
        let id = "", title = "", loc = "", ed = "", allday = false, notes = "", url = "";
        try {{ id = ev.uid() || ""; }} catch(e) {{}}
        try {{ title = ev.summary() || ""; }} catch(e) {{}}
        try {{ loc = ev.location() || ""; }} catch(e) {{}}
        try {{ ed = ev.endDate().toISOString(); }} catch(e) {{}}
        try {{ allday = ev.alldayEvent(); }} catch(e) {{}}
        try {{ notes = ev.description() || ""; }} catch(e) {{}}
        try {{ url = ev.url() || ""; }} catch(e) {{}}
        if (title) results.push({{id: id, title: title, calendar: calName, location: loc, start_date: sd.toISOString(), end_date: ed, all_day: allday ? 1 : 0, notes: notes, url: url, has_attendees: 0, has_recurrences: 0, attendee_count: 0, alarm_count: 0}});
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
        let id = row["id"].as_str().unwrap_or("").trim();
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
        let url = row["url"].as_str().map(str::trim).unwrap_or("");

        records.push(CalendarEvent {
            id: id.to_string(),
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
            url: if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            },
            has_attendees: row["has_attendees"].as_i64().unwrap_or(0) != 0,
            has_recurrences: row["has_recurrences"].as_i64().unwrap_or(0) != 0,
            attendee_count: row["attendee_count"].as_u64().unwrap_or(0) as usize,
            alarm_count: row["alarm_count"].as_u64().unwrap_or(0) as usize,
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
            {"id":"evt-1","title":"Team standup","calendar":"Work","location":"Zoom","start_date":"2026-03-14 10:00:00","end_date":"2026-03-14 10:30:00","all_day":0,"notes":"","url":"","has_attendees":1,"has_recurrences":1,"attendee_count":3,"alarm_count":1},
            {"id":"evt-2","title":"Birthday","calendar":"Personal","location":"","start_date":"2026-03-15 00:00:00","end_date":"2026-03-16 00:00:00","all_day":1,"notes":"Bring cake","url":"","has_attendees":0,"has_recurrences":0}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Team standup");
        assert_eq!(records[0].id, "evt-1");
        assert!(records[0].has_attendees);
        assert!(records[0].has_recurrences);
        assert_eq!(records[0].attendee_count, 3);
        assert_eq!(records[0].alarm_count, 1);
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
                id: "1".to_string(),
                title: "Meeting".to_string(),
                calendar: "Work".to_string(),
                location: None,
                start_date: None,
                end_date: None,
                is_all_day: false,
                notes: None,
                url: None,
                has_attendees: false,
                has_recurrences: false,
                attendee_count: 0,
                alarm_count: 0,
            },
            CalendarEvent {
                id: "2".to_string(),
                title: "Birthday".to_string(),
                calendar: "Personal".to_string(),
                location: None,
                start_date: None,
                end_date: None,
                is_all_day: false,
                notes: None,
                url: None,
                has_attendees: false,
                has_recurrences: false,
                attendee_count: 0,
                alarm_count: 0,
            },
        ];

        let filtered = filter_by_calendar(events, Some("work"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Meeting");
    }

    #[test]
    fn test_filter_by_calendar_none() {
        let events = vec![CalendarEvent {
            id: "1".to_string(),
            title: "Meeting".to_string(),
            calendar: "Work".to_string(),
            location: None,
            start_date: None,
            end_date: None,
            is_all_day: false,
            notes: None,
            url: None,
            has_attendees: false,
            has_recurrences: false,
            attendee_count: 0,
            alarm_count: 0,
        }];

        let filtered = filter_by_calendar(events, None);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn stable_id_script_targets_uid() {
        let script = build_find_by_id_script("ABC-123", "app.delete(ev); return targetId;");
        assert!(script.contains("uid = ev.uid()"));
        assert!(script.contains("uid === targetId"));
        assert!(script.contains("const targetId = \"ABC-123\""));
        assert!(!script.contains("ev.summary() === targetId"));
    }

    #[test]
    fn batch_validation_runs_before_automation() {
        assert!(validate_batch(&[]).is_err());
        assert!(validate_batch(&[NewCalendarEvent {
            title: String::new(),
            start: "2026-09-02T17:00:00Z".to_string(),
            end: "2026-09-02T17:30:00Z".to_string(),
            calendar: None,
            location: None,
            notes: None,
            all_day: false,
        }])
        .is_err());
        assert!(validate_event_range("not-a-date", "2026-09-02T17:30:00Z").is_err());
        assert!(validate_event_range("2026-09-02T18:00:00Z", "2026-09-02T17:30:00Z").is_err());
        assert!(validate_event_range("2026-09-02", "2026-09-03").is_ok());
    }
}
