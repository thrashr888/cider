use super::util::{
    escape_applescript, modified_since, parse_applescript_date, run_osascript_with_timeout, slug,
    ActionResult, SUBPROCESS_TIMEOUT,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub folder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct NoteFolder {
    pub name: String,
}

fn folder_clause(folder_filter: Option<&str>) -> String {
    if let Some(folder) = folder_filter {
        let escaped = escape_applescript(folder);
        format!(
            r#"
                set targetFolders to {{folder "{escaped}"}}
            "#
        )
    } else {
        r#"
                set targetFolders to every folder
            "#
        .to_string()
    }
}

/// List notes, optionally filtered by folder name and modification date.
/// Includes the full body text for each note. Reading each body is one Apple
/// event per note (~35ms), so `cap` bounds the walk; pass `None` to walk
/// everything. `since` is applied to the notes the walk returned, so a capped
/// walk can surface fewer than `cap` matches — [`list_brief`] is the call for
/// finding everything that changed.
pub async fn list(
    folder_filter: Option<&str>,
    cap: Option<usize>,
    since: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<Note>> {
    let folder_clause = folder_clause(folder_filter);
    let cap_clause = cap
        .map(|c| format!("if noteCount >= {c} then exit repeat"))
        .unwrap_or_default();

    let script = format!(
        r#"
        set output to "["
        set noteCount to 0
        with timeout of 600 seconds
        tell application "Notes"
            {folder_clause}
            repeat with f in targetFolders
                set folderName to my escapeJSON(name of f)
                repeat with n in every note of f
                    set noteCount to noteCount + 1
                    if noteCount > 1 then
                        set output to output & ","
                    end if

                    set nId to id of n
                    set nName to my escapeJSON(name of n)
                    set nMod to modification date of n
                    set nBody to ""
                    try
                        set nBody to plaintext of n
                    end try
                    set nBody to my escapeJSON(nBody)

                    set noteJSON to "{{\"id\": \"" & nId & "\", \"name\": \"" & nName & "\", \"modified\": \"" & (nMod as string) & "\", \"folder\": \"" & folderName & "\", \"body\": \"" & nBody & "\"}}"
                    set output to output & noteJSON
                    {cap_clause}
                end repeat
                {cap_clause}
            end repeat
        end tell
        end timeout
        set output to output & "]"
        return output

        on escapeJSON(txt)
            set txt to my replaceText(txt, "\\", "\\\\")
            set txt to my replaceText(txt, "\"", "\\\"")
            set txt to my replaceText(txt, return, "\\n")
            set txt to my replaceText(txt, linefeed, "\\n")
            set txt to my replaceText(txt, tab, "\\t")
            return txt
        end escapeJSON

        on replaceText(theText, searchString, replacementString)
            set AppleScript's text item delimiters to searchString
            set theTextItems to every text item of theText
            set AppleScript's text item delimiters to replacementString
            set theText to theTextItems as string
            set AppleScript's text item delimiters to ""
            return theText
        end replaceText
    "#
    );

    let raw = run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    Ok(filter_since(parse_json_output(&raw), since))
}

/// Notes carry their modification date as AppleScript text, so `since` is
/// applied here on the parsed value rather than inside the script.
fn filter_since(notes: Vec<Note>, since: Option<DateTime<Utc>>) -> Vec<Note> {
    match since {
        Some(_) => notes
            .into_iter()
            .filter(|n| modified_since(n.modified, since))
            .collect(),
        None => notes,
    }
}

/// List every note's id/title/folder/modified without bodies. Properties are
/// fetched in bulk (one Apple event per property per folder instead of one
/// per note), so this stays fast across a whole library — it's the catalog
/// call for pickers and sync sweeps, and with `since` the way to find what
/// changed.
pub async fn list_brief(
    folder_filter: Option<&str>,
    since: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<Note>> {
    let folder_clause = folder_clause(folder_filter);

    let script = format!(
        r#"
        set output to "["
        set noteCount to 0
        with timeout of 600 seconds
        tell application "Notes"
            {folder_clause}
            repeat with f in targetFolders
                set folderName to my escapeJSON(name of f)
                set nIds to id of every note of f
                set nNames to name of every note of f
                set nMods to modification date of every note of f
                repeat with i from 1 to count of nIds
                    set noteCount to noteCount + 1
                    if noteCount > 1 then
                        set output to output & ","
                    end if
                    set nName to my escapeJSON(item i of nNames)
                    set noteJSON to "{{\"id\": \"" & (item i of nIds) & "\", \"name\": \"" & nName & "\", \"modified\": \"" & ((item i of nMods) as string) & "\", \"folder\": \"" & folderName & "\", \"body\": \"\"}}"
                    set output to output & noteJSON
                end repeat
            end repeat
        end tell
        end timeout
        set output to output & "]"
        return output

        on escapeJSON(txt)
            set txt to my replaceText(txt, "\\", "\\\\")
            set txt to my replaceText(txt, "\"", "\\\"")
            set txt to my replaceText(txt, return, "\\n")
            set txt to my replaceText(txt, linefeed, "\\n")
            set txt to my replaceText(txt, tab, "\\t")
            return txt
        end escapeJSON

        on replaceText(theText, searchString, replacementString)
            set AppleScript's text item delimiters to searchString
            set theTextItems to every text item of theText
            set AppleScript's text item delimiters to replacementString
            set theText to theTextItems as string
            set AppleScript's text item delimiters to ""
            return theText
        end replaceText
    "#
    );

    let raw = run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    Ok(filter_since(parse_json_output(&raw), since))
}

/// Get a single note by ID with full body content.
///
/// The result comes back as JSON, not tab-separated text — a body containing
/// a tab used to be silently cut at that tab, which then round-tripped
/// truncated content through any read-before-write caller. The id is returned
/// raw (matching `list`), so it can be passed back to `get` again.
pub async fn get(id: &str) -> anyhow::Result<Note> {
    let escaped_id = escape_applescript(id);
    let script = format!(
        r#"
        with timeout of 600 seconds
        tell application "Notes"
            set n to note id "{escaped_id}"
            set nId to id of n
            set nName to my escapeJSON(name of n)
            set nMod to (modification date of n) as string
            set nFolder to ""
            try
                set nFolder to my escapeJSON(name of container of n)
            end try
            set nBody to ""
            try
                set nBody to my escapeJSON(plaintext of n)
            end try
            return "[{{\"id\": \"" & nId & "\", \"name\": \"" & nName & "\", \"modified\": \"" & nMod & "\", \"folder\": \"" & nFolder & "\", \"body\": \"" & nBody & "\"}}]"
        end tell
        end timeout

        on escapeJSON(txt)
            set txt to my replaceText(txt, "\\", "\\\\")
            set txt to my replaceText(txt, "\"", "\\\"")
            set txt to my replaceText(txt, return, "\\n")
            set txt to my replaceText(txt, linefeed, "\\n")
            set txt to my replaceText(txt, tab, "\\t")
            return txt
        end escapeJSON

        on replaceText(theText, searchString, replacementString)
            set AppleScript's text item delimiters to searchString
            set theTextItems to every text item of theText
            set AppleScript's text item delimiters to replacementString
            set theText to theTextItems as string
            set AppleScript's text item delimiters to ""
            return theText
        end replaceText
    "#
    );

    let raw = run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    parse_json_output(&raw)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Note not found: {id}"))
}

/// Notes bodies are HTML — bare newlines collapse when rendered. Escape
/// entities and give every line its own <div> (blank lines become
/// <div><br></div>), which is the Notes app's own line format.
fn body_to_html(text: &str) -> String {
    text.lines()
        .map(|line| {
            let esc = line
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            if esc.trim().is_empty() {
                "<div><br></div>".to_string()
            } else {
                format!("<div>{esc}</div>")
            }
        })
        .collect()
}

/// Create a new note in a specified folder (defaults to "Notes").
pub async fn create(
    title: &str,
    body: Option<&str>,
    folder: Option<&str>,
) -> anyhow::Result<ActionResult> {
    let title_esc = escape_applescript(title);
    let folder_name = folder.unwrap_or("Notes");
    let folder_esc = escape_applescript(folder_name);

    let body_clause = if let Some(b) = body {
        let b_esc = escape_applescript(&body_to_html(b));
        format!(", body:\"{b_esc}\"")
    } else {
        String::new()
    };

    let script = format!(
        r#"
        tell application "Notes"
            set theFolder to folder "{folder_esc}"
            set newNote to make new note at theFolder with properties {{name:"{title_esc}"{body_clause}}}
            return id of newNote
        end tell
    "#
    );

    let raw = run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    let new_id = raw.trim().to_string();
    Ok(ActionResult::success_with_id("create", &new_id))
}

/// Update the body of an existing note by ID. The note's visible title is
/// derived from the body's first line, so callers should keep it as line one.
pub async fn update(id: &str, body: &str) -> anyhow::Result<ActionResult> {
    let escaped_id = escape_applescript(id);
    let body_esc = escape_applescript(&body_to_html(body));

    let script = format!(
        r#"
        tell application "Notes"
            set body of note id "{escaped_id}" to "{body_esc}"
        end tell
    "#
    );

    run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    Ok(ActionResult::success_with_id("update", id))
}

/// Delete a note by ID.
pub async fn delete(id: &str) -> anyhow::Result<ActionResult> {
    let escaped_id = escape_applescript(id);

    let script = format!(
        r#"
        tell application "Notes"
            delete note id "{escaped_id}"
        end tell
    "#
    );

    run_osascript_with_timeout(&script, SUBPROCESS_TIMEOUT).await?;
    Ok(ActionResult::success_with_id("delete", id))
}

/// List all note folders.
pub async fn folders() -> anyhow::Result<Vec<NoteFolder>> {
    let script = r#"
        tell application "Notes"
            set folderNames to name of every folder
            set output to ""
            repeat with f in folderNames
                if output is not "" then
                    set output to output & linefeed
                end if
                set output to output & f
            end repeat
            return output
        end tell
    "#;

    let raw = run_osascript_with_timeout(script, std::time::Duration::from_secs(15)).await?;
    let folders = raw
        .lines()
        .filter(|l| !l.is_empty())
        .map(|name| NoteFolder {
            name: name.trim().to_string(),
        })
        .collect();
    Ok(folders)
}

fn parse_json_output(output: &str) -> Vec<Note> {
    let items: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse notes JSON: {e}, falling back to line parser");
            return parse_tab_output(output);
        }
    };

    let mut records = Vec::new();

    for item in &items {
        let note_id = item["id"].as_str().unwrap_or("").trim();
        let name = item["name"].as_str().unwrap_or("").trim();
        let mod_str = item["modified"].as_str().unwrap_or("").trim();
        let folder = item["folder"].as_str().unwrap_or("").trim();
        let body_text = item["body"].as_str().unwrap_or("").trim();

        if name.is_empty() {
            continue;
        }

        let modified = if mod_str.is_empty() {
            None
        } else {
            parse_applescript_date(mod_str)
        };

        let id = if note_id.is_empty() {
            slug(name)
        } else {
            note_id.to_string()
        };

        records.push(Note {
            id,
            title: name.to_string(),
            folder: folder.to_string(),
            body: if body_text.is_empty() {
                None
            } else {
                Some(body_text.to_string())
            },
            modified,
        });
    }

    records
}

fn parse_tab_output(output: &str) -> Vec<Note> {
    let mut records = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts.first().copied().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }

        let mod_str = parts.get(1).copied().unwrap_or("").trim();
        let note_id = parts.get(2).copied().unwrap_or("").trim();
        let folder = parts.get(3).copied().unwrap_or("").trim();

        let modified = if mod_str.is_empty() {
            None
        } else {
            parse_applescript_date(mod_str)
        };

        let id = if note_id.is_empty() {
            slug(name)
        } else {
            note_id.to_string()
        };

        records.push(Note {
            id,
            title: name.to_string(),
            folder: folder.to_string(),
            body: None,
            modified,
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_output() {
        let json = r#"[{"id":"x-coredata://abc123","name":"Meeting Notes","modified":"Saturday, February  8, 2026 at 10:00:00 AM","folder":"Work","body":""},{"id":"x-coredata://def456","name":"Shopping List","modified":"","folder":"Personal","body":""}]"#;
        let records = parse_json_output(json);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Meeting Notes");
        assert_eq!(records[0].folder, "Work");
        assert!(records[0].modified.is_some());
        assert_eq!(records[1].title, "Shopping List");
    }

    #[test]
    fn test_parse_json_output_empty() {
        assert!(parse_json_output("[]").is_empty());
    }

    #[test]
    fn test_filter_since() {
        let since = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let note = |id: &str, modified: Option<DateTime<Utc>>| Note {
            id: id.to_string(),
            title: id.to_string(),
            folder: "Notes".to_string(),
            body: None,
            modified,
        };
        let notes = vec![
            note("before", Some(since - chrono::Duration::seconds(1))),
            note("exact", Some(since)),
            note("after", Some(since + chrono::Duration::seconds(1))),
            note("unknown", None),
        ];

        let kept: Vec<String> = filter_since(notes, Some(since))
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(kept, vec!["exact", "after"]);
        assert_eq!(filter_since(vec![note("unknown", None)], None).len(), 1);
    }

    /// Bodies with tabs/newlines and long titles must come through intact —
    /// the data layer never truncates; only `--pretty` display may.
    #[test]
    fn test_parse_json_output_full_fidelity() {
        let long_title = "t".repeat(300);
        let body = format!("col1\tcol2\nline two\n{}", "b".repeat(5000));
        let json = serde_json::json!([{
            "id": "x-coredata://ABC-123/ICNote/p1",
            "name": long_title,
            "modified": "",
            "folder": "Work",
            "body": body,
        }])
        .to_string();
        let records = parse_json_output(&json);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].title.len(), 300, "title must not be truncated");
        // The raw id round-trips to get() — no slug mangling.
        assert_eq!(records[0].id, "x-coredata://ABC-123/ICNote/p1");
        let b = records[0].body.as_deref().unwrap();
        assert!(
            b.contains('\t') && b.contains('\n'),
            "tabs/newlines survive"
        );
        assert!(b.len() > 5000, "no length cap");
    }

    #[test]
    fn test_parse_tab_output() {
        let output = "Meeting Notes\tSaturday, February  8, 2026 at 10:00:00 AM\tx-coredata://abc123\tWork\n\
                       Shopping List\t\t\tPersonal\n";
        let records = parse_tab_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Meeting Notes");
        assert!(records[0].modified.is_some());
        assert_eq!(records[1].title, "Shopping List");
    }

    #[test]
    fn test_parse_tab_output_empty() {
        assert!(parse_tab_output("").is_empty());
    }

    #[test]
    fn test_body_to_html_lines_and_escapes() {
        assert_eq!(
            body_to_html("Title\n\na < b & c"),
            "<div>Title</div><div><br></div><div>a &lt; b &amp; c</div>"
        );
        assert_eq!(body_to_html("one"), "<div>one</div>");
    }
}
