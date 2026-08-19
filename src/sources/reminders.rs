use super::util::{
    escape_applescript, run_command_with_timeout, run_osascript_with_timeout, slug, ActionResult,
    APPLE_EPOCH,
};
use chrono::DateTime;
use serde::Serialize;

const ID_SCHEME: &str = "x-apple-reminder://";

#[derive(Debug, Serialize)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub list: String,
    pub priority: i32,
    pub completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Escape a value for embedding in a single-quoted SQLite string literal.
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// Read reminders straight from the Reminders SQLite stores.
///
/// Rows come back as `sqlite3 -json` so titles and notes survive intact —
/// the old tab-separated read had to flatten newlines to " | " and cap notes
/// to keep its line-per-record format parseable, which silently mangled any
/// reminder whose content was the point.
///
/// `extra_where` narrows the scan in SQL, which matters more than it looks:
/// the query is capped at 500 rows per store, and with completed reminders
/// included the years of finished items can crowd a specific target out of
/// that window — so a lookup must filter in the query, not on the result.
async fn fetch(
    include_completed: bool,
    extra_where: Option<&str>,
) -> anyhow::Result<Vec<Reminder>> {
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

    let mut where_clauses = Vec::new();
    if !include_completed {
        where_clauses.push("r.ZCOMPLETED = 0");
    }
    if let Some(extra) = extra_where {
        where_clauses.push(extra);
    }
    let where_clause = if where_clauses.is_empty() {
        "1 = 1".to_string()
    } else {
        where_clauses.join(" AND ")
    };

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

        let list_select = if has_base_list {
            "COALESCE(l.ZNAME, '')"
        } else {
            "''"
        };
        let list_join = if has_base_list {
            "LEFT JOIN ZREMCDBASELIST l ON r.ZLIST = l.Z_PK"
        } else {
            ""
        };

        let query = format!(
            r#"
SELECT
    COALESCE(r.ZEXTERNALIDENTIFIER, r.ZCKIDENTIFIER, CAST(r.Z_PK AS TEXT)) AS id,
    {list_select} AS list,
    COALESCE(r.ZTITLE, '') AS title,
    COALESCE(r.ZPRIORITY, 0) AS priority,
    COALESCE(r.ZCOMPLETED, 0) AS completed,
    r.ZDUEDATE AS due,
    r.ZNOTES AS notes
FROM ZREMCDREMINDER r
{list_join}
WHERE {where_clause}
ORDER BY r.ZDUEDATE ASC
LIMIT 500;
"#
        );

        match run_command_with_timeout(
            "sqlite3",
            &["-json", db_path, query.trim()],
            std::time::Duration::from_secs(10),
        )
        .await
        {
            Ok(stdout) => all.extend(parse_json_rows(&stdout)),
            Err(e) => eprintln!("Skipping reminders DB {db_path}: {e}"),
        }
    }

    Ok(all)
}

/// List incomplete reminders, optionally filtered by list name.
pub async fn list(list_filter: Option<&str>) -> anyhow::Result<Vec<Reminder>> {
    let mut all = fetch(false, None).await?;

    if let Some(filter) = list_filter {
        let filter_lower = filter.to_lowercase();
        all.retain(|r| r.list.to_lowercase() == filter_lower);
    }

    Ok(all)
}

/// Fetch a single reminder in full — complete title and notes, nothing
/// flattened or truncated — by id or title. Searches completed reminders
/// too, since an id is explicit about which item it means; a title match
/// prefers the incomplete one (the only one `reminders list` shows).
pub async fn get(target: Target<'_>, list_filter: Option<&str>) -> anyhow::Result<Reminder> {
    let where_sql = match target {
        Target::Id(id) => {
            let bare = id.trim().trim_start_matches(ID_SCHEME).to_lowercase();
            format!(
                "LOWER(COALESCE(r.ZEXTERNALIDENTIFIER, r.ZCKIDENTIFIER, CAST(r.Z_PK AS TEXT))) = '{}'",
                escape_sql(&bare)
            )
        }
        Target::Title(title) => format!("LOWER(r.ZTITLE) = LOWER('{}')", escape_sql(title)),
    };

    let mut all = fetch(true, Some(&where_sql)).await?;

    if let Some(filter) = list_filter {
        let filter_lower = filter.to_lowercase();
        all.retain(|r| r.list.to_lowercase() == filter_lower);
    }

    // A title can name several reminders; prefer the open one, since that is
    // the only one `reminders list` shows and therefore the one being named.
    let open = all.iter().position(|r| !r.completed);
    let found = open
        .or(if all.is_empty() { None } else { Some(0) })
        .map(|i| all.swap_remove(i));

    found.ok_or_else(|| anyhow::anyhow!("Reminder not found: {}", target.describe()))
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

    let output = run_osascript_with_timeout(&script, super::util::SUBPROCESS_TIMEOUT).await?;
    let id = slug(output.trim());
    Ok(ActionResult::success_with_id("created", &id))
}

/// Which fields `update` should change; `None` leaves the field alone.
#[derive(Debug, Default)]
pub struct UpdateFields<'a> {
    /// Rename the reminder.
    pub title: Option<&'a str>,
    /// Replace the notes wholesale.
    pub notes: Option<&'a str>,
    /// Append to the existing notes (after a newline); starts the notes if
    /// there are none yet.
    pub append_notes: Option<&'a str>,
    /// 0=none, 1=high, 5=medium, 9=low.
    pub priority: Option<i32>,
    /// Due date, in any format AppleScript's `date` accepts.
    pub due: Option<&'a str>,
}

/// The AppleScript statements that apply `fields` to `item 1 of matches`.
fn build_update_action(fields: &UpdateFields<'_>) -> anyhow::Result<String> {
    let mut sets = Vec::new();
    if let Some(t) = fields.title {
        sets.push(format!(
            "set name of item 1 of matches to \"{}\"",
            escape_applescript(t)
        ));
    }
    if let Some(n) = fields.notes {
        sets.push(format!(
            "set body of item 1 of matches to \"{}\"",
            escape_applescript(n)
        ));
    }
    if let Some(n) = fields.append_notes {
        let esc = escape_applescript(n);
        // `missing value & "\n"` errors, so an empty body starts fresh.
        sets.push(format!(
            "if body of item 1 of matches is missing value then\n                set body of item 1 of matches to \"{esc}\"\n            else\n                set body of item 1 of matches to (body of item 1 of matches) & \"\\n\" & \"{esc}\"\n            end if"
        ));
    }
    if let Some(p) = fields.priority {
        sets.push(format!("set priority of item 1 of matches to {p}"));
    }
    if let Some(d) = fields.due {
        sets.push(format!(
            "set due date of item 1 of matches to date \"{}\"",
            escape_applescript(d)
        ));
    }
    if sets.is_empty() {
        anyhow::bail!(
            "Nothing to update — pass at least one of title, notes, append_notes, priority, due"
        );
    }
    Ok(sets.join("\n            "))
}

/// Update a reminder in place via AppleScript, by id or title. Editing in
/// place (rather than delete + recreate) preserves the creation date and id.
/// Searches all lists unless a list name is given.
pub async fn update(
    target: Target<'_>,
    list: Option<&str>,
    fields: &UpdateFields<'_>,
) -> anyhow::Result<ActionResult> {
    let action = build_update_action(fields)?;
    let script = build_find_and_act_script(target, list, &action);

    let matched = run_osascript_with_timeout(&script, super::util::SUBPROCESS_TIMEOUT).await?;

    Ok(ActionResult::success_with_message(
        "updated",
        &acted_on(target, &matched, "Updated"),
    ))
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
/// The `with timeout` wrapper matters: a `whose` filter over a list with a
/// deep completed history takes several seconds per list, and the default
/// AppleEvent timeout gives up with -1712 partway through an all-lists scan.
fn build_find_and_act_script(target: Target<'_>, list: Option<&str>, action: &str) -> String {
    let clause = match_clause(target);
    let not_found = escape_applescript(&target.describe());
    if let Some(list_name) = list {
        let escaped_list = escape_applescript(list_name);
        format!(
            r#"
        with timeout of 600 seconds
        tell application "Reminders"
            set theList to list "{escaped_list}"
            set matches to (every reminder of theList whose {clause})
            if (count of matches) is 0 then error "Reminder not found: {not_found}"
            {action}
            return (count of matches) as string
        end tell
        end timeout
    "#
        )
    } else {
        format!(
            r#"
        with timeout of 600 seconds
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
        end timeout
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

    let matched = run_osascript_with_timeout(&script, super::util::SUBPROCESS_TIMEOUT).await?;

    Ok(ActionResult::success_with_message(
        "completed",
        &acted_on(target, &matched, "Marked"),
    ))
}

/// Delete a reminder via AppleScript, by id or title.
/// Searches all lists unless a list name is given.
pub async fn delete(target: Target<'_>, list: Option<&str>) -> anyhow::Result<ActionResult> {
    let script = build_find_and_act_script(target, list, "delete item 1 of matches");

    let matched = run_osascript_with_timeout(&script, super::util::SUBPROCESS_TIMEOUT).await?;

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

fn parse_json_rows(output: &str) -> Vec<Reminder> {
    let output = output.trim();
    if output.is_empty() {
        return Vec::new();
    }

    let rows: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Skipping unparseable reminders output: {e}");
            return Vec::new();
        }
    };

    let mut records = Vec::new();

    for row in &rows {
        let reminder_id = row["id"].as_str().unwrap_or("").trim();
        let list_name = row["list"].as_str().unwrap_or("").trim();
        let name = row["title"].as_str().unwrap_or("").trim();

        if name.is_empty() {
            continue;
        }

        let priority = row["priority"].as_i64().unwrap_or(0) as i32;
        let completed = row["completed"].as_i64().unwrap_or(0) != 0;

        let due_date = match &row["due"] {
            serde_json::Value::Number(n) => n
                .as_f64()
                .and_then(|ts| DateTime::from_timestamp(ts as i64 + APPLE_EPOCH, 0)),
            serde_json::Value::String(s) => {
                if let Ok(ts) = s.parse::<f64>() {
                    DateTime::from_timestamp(ts as i64 + APPLE_EPOCH, 0)
                } else {
                    super::util::parse_plist_date(s)
                }
            }
            _ => None,
        };

        let notes = row["notes"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        let id = if reminder_id.is_empty() {
            slug(name)
        } else {
            slug(reminder_id)
        };

        records.push(Reminder {
            id,
            title: name.to_string(),
            list: list_name.to_string(),
            priority,
            completed,
            due_date,
            notes,
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
            {"id":"ABC-123-DEF","list":"Shopping","title":"Buy groceries","priority":5,"completed":0,"due":793900800.0,"notes":null},
            {"id":"GHI-456-JKL","list":"Health","title":"Call dentist","priority":0,"completed":0,"due":null,"notes":null}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Buy groceries");
        assert_eq!(records[0].list, "Shopping");
        assert_eq!(records[0].priority, 5);
        assert!(records[0].due_date.is_some());
        assert!(!records[0].completed);
        assert_eq!(records[1].title, "Call dentist");
        assert!(records[1].due_date.is_none());
    }

    #[test]
    fn test_parse_json_rows_empty() {
        assert!(parse_json_rows("").is_empty());
        assert!(parse_json_rows("[]").is_empty());
    }

    /// The reason for `-json`: notes keep their newlines and full length,
    /// and titles are never truncated — this is the data layer, display
    /// truncation belongs to `--pretty`.
    #[test]
    fn test_parse_json_rows_full_fidelity() {
        let long_title = "t".repeat(300);
        let long_notes = format!(
            "P1/7 — rationale\n\nline two\twith tab\n{}",
            "n".repeat(3000)
        );
        let output = serde_json::json!([{
            "id": "ABC-123",
            "list": "Alchemy",
            "title": long_title,
            "priority": 1,
            "completed": 0,
            "due": null,
            "notes": long_notes,
        }])
        .to_string();

        let records = parse_json_rows(&output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].title.len(), 300, "title must not be truncated");
        let notes = records[0].notes.as_deref().unwrap();
        assert_eq!(notes, long_notes.trim());
        assert!(notes.contains('\n'), "newlines must survive the read");
        assert!(notes.contains('\t'), "tabs must survive the read");
    }

    #[test]
    fn test_parse_json_rows_completed_flag() {
        let output = r#"[{"id":"A","list":"L","title":"Done thing","priority":0,"completed":1,"due":null,"notes":null}]"#;
        let records = parse_json_rows(output);
        assert!(records[0].completed);
    }

    #[test]
    fn test_parse_json_rows_empty_notes_is_none() {
        let output = r#"[{"id":"A","list":"L","title":"T","priority":0,"completed":0,"due":null,"notes":"  "}]"#;
        let records = parse_json_rows(output);
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
    fn test_update_action_sets_each_field() {
        let action = build_update_action(&UpdateFields {
            title: Some("New name"),
            notes: Some("P1/7 — top of the list\n\ndetails"),
            priority: Some(1),
            due: Some("February 8, 2026 2:30:00 PM"),
            ..Default::default()
        })
        .unwrap();
        assert!(action.contains("set name of item 1 of matches to \"New name\""));
        // Multiline notes must arrive as AppleScript \n escapes, never raw
        // newlines inside the literal (a syntax error that reads as a cap).
        assert!(action
            .contains("set body of item 1 of matches to \"P1/7 — top of the list\\n\\ndetails\""));
        assert!(action.contains("set priority of item 1 of matches to 1"));
        assert!(action
            .contains("set due date of item 1 of matches to date \"February 8, 2026 2:30:00 PM\""));
    }

    #[test]
    fn test_update_action_append_handles_missing_body() {
        let action = build_update_action(&UpdateFields {
            append_notes: Some("addendum"),
            ..Default::default()
        })
        .unwrap();
        assert!(action.contains("if body of item 1 of matches is missing value"));
        assert!(action.contains("(body of item 1 of matches) & \"\\n\" & \"addendum\""));
    }

    #[test]
    fn test_update_action_requires_a_field() {
        assert!(build_update_action(&UpdateFields::default()).is_err());
    }

    #[test]
    fn test_update_script_composes_with_target() {
        let action = build_update_action(&UpdateFields {
            notes: Some("full replacement"),
            ..Default::default()
        })
        .unwrap();
        let script = build_find_and_act_script(Target::Id("abc-123"), Some("Alchemy"), &action);
        assert!(script.contains("set theList to list \"Alchemy\""));
        assert!(script.contains("set body of item 1 of matches to \"full replacement\""));
    }
}
