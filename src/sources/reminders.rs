use super::util::{
    escape_applescript, run_command_with_timeout, run_osascript_with_timeout, slug,
    truncate_for_title, ActionResult, APPLE_EPOCH,
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
    COALESCE(SUBSTR(REPLACE(REPLACE(REPLACE(r.ZNOTES, CHAR(9), ' '), CHAR(13), ' '), CHAR(10), ' | '), 1, 4000), '')
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
    COALESCE(SUBSTR(REPLACE(REPLACE(REPLACE(r.ZNOTES, CHAR(9), ' '), CHAR(13), ' '), CHAR(10), ' | '), 1, 4000), '')
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

/// Create a new reminder via AppleScript.
pub async fn create(
    title: &str,
    list: Option<&str>,
    due: Option<&str>,
    priority: Option<i32>,
    notes: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let escaped_title = escape_applescript(title);
    let list_clause = if let Some(list_name) = list {
        let escaped_list = escape_applescript(list_name);
        format!("set theList to list \"{}\"", escaped_list)
    } else {
        "set theList to first list".to_string()
    };

    let mut property_parts = vec![format!("name:\"{}\"", escaped_title)];
    if let Some(n) = notes {
        property_parts.push(format!("body:\"{}\"", escape_applescript(n)));
    }
    if let Some(p) = priority {
        property_parts.push(format!("priority:{}", p));
    }
    if let Some(due_str) = due {
        property_parts.push(format!("due date:date \"{}\"", escape_applescript(due_str)));
    }
    let properties = property_parts.join(", ");

    let script = format!(
        r#"
        tell application "Reminders"
            {}
            set newReminder to make new reminder at end of reminders of theList with properties {{{}}}
            return name of newReminder
        end tell
    "#,
        list_clause, properties
    );

    let output = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
    let id = slug(output.trim());
    Ok(ActionResult::success_with_id("created", &id))
}

/// Which reminder a mutating command means.
///
/// Titles are not unique — Reminders is happy to hold two items called
/// "[BUG] bob produced an error" — and by-title matching silently acts on the
/// first, so the caller has no way to say which. [`Target::Id`] takes the `id`
/// that `reminders list` already prints, which is unique.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    Id(&'a str),
    Title(&'a str),
}

impl Target<'_> {
    /// How the target reads back in messages and errors.
    pub fn describe(&self) -> String {
        match self {
            Target::Id(id) => format!("id {id}"),
            Target::Title(t) => format!("'{t}'"),
        }
    }
}

/// The AppleScript `whose` clause that selects the target, already escaped.
///
/// Reminders' AppleScript `id` is a URL (`x-apple-reminder://<UUID>`) while
/// `reminders list` prints the bare UUID, so accept either and normalize.
/// AppleScript's `is` compares strings case-insensitively, which is what
/// bridges the DB's lowercase form to AppleScript's uppercase one.
///
/// A title match is restricted to INCOMPLETE reminders, because that is the
/// only set `reminders list` shows and therefore the only set a caller can be
/// naming. Without the restriction a finished reminder of the same name joins
/// `matches`, can sort ahead of the live one, and absorbs the action — so
/// `complete --title` silently re-completes something already done and leaves
/// the open one untouched. An `--id` is explicit and matches either way.
fn match_clause(target: Target<'_>) -> String {
    const ID_SCHEME: &str = "x-apple-reminder://";
    match target {
        Target::Id(id) => {
            let bare = id.trim().trim_start_matches(ID_SCHEME);
            format!("id is \"{}{}\"", ID_SCHEME, escape_applescript(bare))
        }
        Target::Title(title) => format!(
            "name is \"{}\" and completed is false",
            escape_applescript(title)
        ),
    }
}

/// Build an AppleScript that finds a reminder and applies `action` to the
/// first match. Searches the named list if given, otherwise every list.
///
/// Returns the number of reminders that matched, so a by-title call can tell
/// the caller it acted on one of several rather than leaving them to guess.
fn build_find_and_act_script(target: Target<'_>, list: Option<&str>, action: &str) -> String {
    let clause = match_clause(target);
    let not_found = escape_applescript(&target.describe());
    if let Some(list_name) = list {
        let escaped_list = escape_applescript(list_name);
        format!(
            r#"
        tell application "Reminders"
            set theList to list "{escaped_list}"
            set matches to (every reminder of theList whose {clause})
            if (count of matches) is 0 then error "Reminder not found: {not_found}"
            {action}
            return (count of matches) as string
        end tell
    "#
        )
    } else {
        format!(
            r#"
        tell application "Reminders"
            repeat with theList in every list
                set matches to (every reminder of theList whose {clause})
                if (count of matches) > 0 then
                    {action}
                    return (count of matches) as string
                end if
            end repeat
            error "Reminder not found: {not_found}"
        end tell
    "#
        )
    }
}

/// "…as complete" plus, when a title matched more than one reminder, which of
/// them was acted on — silence there is how a caller ends up thinking a
/// duplicate title was handled when only one of the pair was.
fn acted_on(target: Target<'_>, matched: &str, past_tense: &str) -> String {
    let n: usize = matched.trim().parse().unwrap_or(1);
    let which = if n > 1 {
        format!(" (1 of {n} matching — pass --id to choose)")
    } else {
        String::new()
    };
    format!("{past_tense} {}{which}", target.describe())
}

/// Mark a reminder as complete via AppleScript, by id or title.
/// Searches all lists unless a list name is given.
pub async fn complete(target: Target<'_>, list: Option<&str>) -> anyhow::Result<ActionResult> {
    let script =
        build_find_and_act_script(target, list, "set completed of item 1 of matches to true");

    let matched = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "completed",
        &acted_on(target, &matched, "Marked"),
    ))
}

/// Delete a reminder via AppleScript, by id or title.
/// Searches all lists unless a list name is given.
pub async fn delete(target: Target<'_>, list: Option<&str>) -> anyhow::Result<ActionResult> {
    let script = build_find_and_act_script(target, list, "delete item 1 of matches");

    let matched = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;

    Ok(ActionResult::success_with_message(
        "deleted",
        &acted_on(target, &matched, "Deleted"),
    ))
}

/// List all reminder list names via AppleScript.
pub async fn lists() -> anyhow::Result<Vec<String>> {
    let script = r#"
        tell application "Reminders"
            set listNames to name of every list
            set AppleScript's text item delimiters to linefeed
            return listNames as string
        end tell
    "#;

    let output = run_osascript_with_timeout(script, std::time::Duration::from_secs(30)).await?;

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
    fn test_find_script_with_list_targets_that_list() {
        let script = build_find_and_act_script(
            Target::Title("Buy milk"),
            Some("Scrapr"),
            "delete item 1 of matches",
        );
        assert!(script.contains("set theList to list \"Scrapr\""));
        assert!(script.contains(
            "every reminder of theList whose name is \"Buy milk\" and completed is false"
        ));
        assert!(!script.contains("repeat"));
    }

    #[test]
    fn test_find_script_without_list_searches_all_lists() {
        let script =
            build_find_and_act_script(Target::Title("Buy milk"), None, "delete item 1 of matches");
        assert!(script.contains("repeat with theList in every list"));
        assert!(script.contains(
            "every reminder of theList whose name is \"Buy milk\" and completed is false"
        ));
        assert!(script.contains("error \"Reminder not found: 'Buy milk'\""));
        assert!(!script.contains("first list"));
    }

    #[test]
    fn test_find_script_escapes_title() {
        let script = build_find_and_act_script(
            Target::Title("Say \"hi\""),
            None,
            "set completed of item 1 of matches to true",
        );
        assert!(script.contains("whose name is \"Say \\\"hi\\\"\" and completed is false"));
    }

    /// `reminders list` prints the bare UUID; AppleScript's `id` is a URL.
    /// Either spelling must select the same reminder.
    #[test]
    fn test_id_target_matches_on_the_applescript_url() {
        let bare = build_find_and_act_script(
            Target::Id("0e430734-961e-483f-9af7-220efb64b2b3"),
            None,
            "delete item 1 of matches",
        );
        assert!(bare
            .contains("whose id is \"x-apple-reminder://0e430734-961e-483f-9af7-220efb64b2b3\""));

        // A pasted full URL must not end up double-prefixed.
        let full = build_find_and_act_script(
            Target::Id("x-apple-reminder://0e430734-961e-483f-9af7-220efb64b2b3"),
            None,
            "delete item 1 of matches",
        );
        assert!(full
            .contains("whose id is \"x-apple-reminder://0e430734-961e-483f-9af7-220efb64b2b3\""));
        assert!(!full.contains("x-apple-reminder://x-apple-reminder://"));
    }

    /// A finished reminder of the same name must not absorb the action: the
    /// live "1 of 3 matching" that surfaced this counted a completed one.
    #[test]
    fn test_title_matching_ignores_completed_reminders() {
        let by_title =
            build_find_and_act_script(Target::Title("cider dup test"), None, "delete item 1");
        assert!(by_title.contains("and completed is false"), "{by_title}");

        // An id is explicit — it may name a finished reminder deliberately.
        let by_id = build_find_and_act_script(Target::Id("abc-123"), None, "delete item 1");
        assert!(!by_id.contains("completed is false"), "{by_id}");
    }

    /// The bug that started this: two reminders share a title, by-title acts
    /// on the first, and the caller is told it handled "the" reminder.
    #[test]
    fn test_duplicate_titles_are_reported_not_hidden() {
        let one = acted_on(Target::Title("[BUG] bob produced an error"), "1", "Marked");
        assert_eq!(one, "Marked '[BUG] bob produced an error'");

        let many = acted_on(Target::Title("[BUG] bob produced an error"), "2", "Marked");
        assert!(many.contains("1 of 2 matching"), "{many}");
        assert!(many.contains("--id"), "{many}");

        // An id matches at most one, so it never carries the caveat.
        assert_eq!(
            acted_on(Target::Id("abc-123"), "1", "Deleted"),
            "Deleted id abc-123"
        );
    }

    #[test]
    fn test_parse_output_empty_notes() {
        let output = "ABC-123\tWork\tFinish report\t1\t793900800.0\t0\t\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert!(records[0].notes.is_none());
    }
}
