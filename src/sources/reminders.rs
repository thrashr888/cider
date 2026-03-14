use super::util::{
    escape_jxa, run_command_with_timeout, run_jxa_with_timeout, slug, truncate_for_title,
    ActionResult, APPLE_EPOCH,
};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub list: String,
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// List incomplete reminders, optionally filtered by list name.
pub async fn list(list_filter: Option<&str>) -> anyhow::Result<Vec<Reminder>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let stores_dir =
        format!("{home}/Library/Group Containers/group.com.apple.reminders/Container_v1/Stores");

    let listing =
        run_command_with_timeout("ls", &[&stores_dir], std::time::Duration::from_secs(5)).await?;

    let db_files: Vec<String> = listing
        .lines()
        .filter(|l| l.starts_with("Data-") && l.ends_with(".sqlite"))
        .map(|l| format!("{stores_dir}/{l}"))
        .collect();

    if db_files.is_empty() {
        anyhow::bail!("No Reminders database found");
    }

    let mut all = Vec::new();

    for db_path in &db_files {
        let has_base_list = run_command_with_timeout(
            "sqlite3",
            &[
                db_path,
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='ZREMCDBASELIST';",
            ],
            std::time::Duration::from_secs(5),
        )
        .await
        .map(|s| s.trim() == "1")
        .unwrap_or(false);

        let query = if has_base_list {
            r#"
SELECT
    COALESCE(r.ZEXTERNALIDENTIFIER, r.ZCKIDENTIFIER, CAST(r.Z_PK AS TEXT)),
    COALESCE(l.ZNAME, ''),
    COALESCE(r.ZTITLE, ''),
    COALESCE(r.ZPRIORITY, 0),
    r.ZDUEDATE,
    COALESCE(r.ZFLAGGED, 0),
    COALESCE(SUBSTR(r.ZNOTES, 1, 500), '')
FROM ZREMCDREMINDER r
LEFT JOIN ZREMCDBASELIST l ON r.ZLIST = l.Z_PK
WHERE r.ZCOMPLETED = 0
ORDER BY r.ZDUEDATE ASC
LIMIT 500;
"#
        } else {
            r#"
SELECT
    COALESCE(r.ZEXTERNALIDENTIFIER, r.ZCKIDENTIFIER, CAST(r.Z_PK AS TEXT)),
    '',
    COALESCE(r.ZTITLE, ''),
    COALESCE(r.ZPRIORITY, 0),
    r.ZDUEDATE,
    COALESCE(r.ZFLAGGED, 0),
    COALESCE(SUBSTR(r.ZNOTES, 1, 500), '')
FROM ZREMCDREMINDER r
WHERE r.ZCOMPLETED = 0
ORDER BY r.ZDUEDATE ASC
LIMIT 500;
"#
        };

        match run_command_with_timeout(
            "sqlite3",
            &["-separator", "\t", db_path, query.trim()],
            std::time::Duration::from_secs(10),
        )
        .await
        {
            Ok(stdout) => all.extend(parse_output(&stdout)),
            Err(e) => eprintln!("Skipping reminders DB {db_path}: {e}"),
        }
    }

    // Apply list filter if provided
    if let Some(filter) = list_filter {
        let filter_lower = filter.to_lowercase();
        all.retain(|r| r.list.to_lowercase() == filter_lower);
    }

    Ok(all)
}

/// Create a new reminder via JXA.
pub async fn create(
    title: &str,
    list: Option<&str>,
    due: Option<&str>,
    priority: Option<i32>,
    notes: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_jxa(title);
    let list_name = list.unwrap_or("Reminders");
    let escaped_list = escape_jxa(list_name);

    let mut props = format!("name: \"{}\"", escaped_title);

    if let Some(due_str) = due {
        let escaped_due = escape_jxa(due_str);
        props.push_str(&format!(", dueDate: new Date(\"{}\")", escaped_due));
    }

    if let Some(p) = priority {
        props.push_str(&format!(", priority: {}", p));
    }

    if let Some(n) = notes {
        let escaped_notes = escape_jxa(n);
        props.push_str(&format!(", body: \"{}\"", escaped_notes));
    }

    let script = format!(
        r#"
const app = Application("Reminders");
const list = app.lists.byName("{}");
const r = app.Reminder({{ {} }});
list.reminders.push(r);
r.name();
"#,
        escaped_list, props
    );

    let output = run_jxa_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    let id = slug(&output);
    Ok(ActionResult::success_with_id("created", &id))
}

/// Mark a reminder as complete by title via JXA.
pub async fn complete(title: &str) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_jxa(title);

    let script = format!(
        r#"
const app = Application("Reminders");
const lists = app.lists();
let found = false;
for (let i = 0; i < lists.length; i++) {{
    let rems;
    try {{ rems = lists[i].reminders(); }} catch(e) {{ continue; }}
    for (let j = 0; j < rems.length; j++) {{
        try {{
            if (rems[j].name() === "{}") {{
                rems[j].completed = true;
                found = true;
                break;
            }}
        }} catch(e) {{ continue; }}
    }}
    if (found) break;
}}
if (!found) throw new Error("Reminder not found: {}");
"completed"
"#,
        escaped_title, escaped_title
    );

    run_jxa_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "completed",
        &format!("Marked '{}' as complete", title),
    ))
}

/// Delete a reminder by title via JXA.
pub async fn delete(title: &str) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_jxa(title);

    let script = format!(
        r#"
const app = Application("Reminders");
const lists = app.lists();
let found = false;
for (let i = 0; i < lists.length; i++) {{
    let rems;
    try {{ rems = lists[i].reminders(); }} catch(e) {{ continue; }}
    for (let j = 0; j < rems.length; j++) {{
        try {{
            if (rems[j].name() === "{}") {{
                app.delete(rems[j]);
                found = true;
                break;
            }}
        }} catch(e) {{ continue; }}
    }}
    if (found) break;
}}
if (!found) throw new Error("Reminder not found: {}");
"deleted"
"#,
        escaped_title, escaped_title
    );

    run_jxa_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "deleted",
        &format!("Deleted reminder '{}'", title),
    ))
}

/// List all reminder list names via JXA.
pub async fn lists() -> anyhow::Result<Vec<String>> {
    let script = r#"
const app = Application("Reminders");
const names = app.lists.name();
names.join("\n");
"#;

    let output = run_jxa_with_timeout(script, std::time::Duration::from_secs(30)).await?;

    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn parse_output(output: &str) -> Vec<Reminder> {
    let mut records = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let reminder_id = parts[0].trim();
        let list_name = parts.get(1).copied().unwrap_or("").trim();
        let name = parts.get(2).copied().unwrap_or("").trim();
        let priority_str = parts.get(3).copied().unwrap_or("0").trim();
        let due_str = parts.get(4).copied().unwrap_or("").trim();
        // parts[5] is flagged — skipped
        let notes_str = parts.get(6).copied().unwrap_or("").trim();

        if name.is_empty() {
            continue;
        }

        let priority: i32 = priority_str.parse().unwrap_or(0);

        let due_date = if due_str.is_empty() {
            None
        } else if let Ok(core_data_ts) = due_str.parse::<f64>() {
            DateTime::from_timestamp(core_data_ts as i64 + APPLE_EPOCH, 0)
        } else {
            super::util::parse_plist_date(due_str)
        };

        let id = if reminder_id.is_empty() {
            slug(name)
        } else {
            slug(reminder_id)
        };

        records.push(Reminder {
            id,
            title: truncate_for_title(name),
            list: list_name.to_string(),
            priority,
            due_date,
            notes: if notes_str.is_empty() {
                None
            } else {
                Some(notes_str.to_string())
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
        let output = "ABC-123-DEF\tShopping\tBuy groceries\t5\t793900800.0\t0\n\
             GHI-456-JKL\tHealth\tCall dentist\t0\t\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Buy groceries");
        assert_eq!(records[0].list, "Shopping");
        assert_eq!(records[0].priority, 5);
        assert!(records[0].due_date.is_some());
        assert_eq!(records[1].title, "Call dentist");
        assert!(records[1].due_date.is_none());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }

    #[test]
    fn test_parse_output_with_notes() {
        let output = "ABC-123\tWork\tFinish report\t1\t793900800.0\t0\tDue by end of week\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].title, "Finish report");
        assert_eq!(records[0].notes.as_deref(), Some("Due by end of week"));
    }

    #[test]
    fn test_parse_output_no_notes_column() {
        let output = "ABC-123\tWork\tFinish report\t1\t793900800.0\t0\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert!(records[0].notes.is_none());
    }

    #[test]
    fn test_parse_output_empty_notes() {
        let output = "ABC-123\tWork\tFinish report\t1\t793900800.0\t0\t\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert!(records[0].notes.is_none());
    }
}
