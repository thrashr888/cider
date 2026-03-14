use super::util::{
    escape_applescript, parse_applescript_date, run_osascript_with_timeout, slug,
    truncate_for_title, ActionResult,
};
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

/// List notes, optionally filtered by folder name.
/// Includes body text (truncated to 2000 chars) for each note.
pub async fn list(folder_filter: Option<&str>) -> anyhow::Result<Vec<Note>> {
    let folder_clause = if let Some(folder) = folder_filter {
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
    };

    let script = format!(
        r#"
        set output to "["
        set noteCount to 0
        tell application "Notes"
            {folder_clause}
            repeat with f in targetFolders
                set folderName to name of f
                repeat with n in every note of f
                    set noteCount to noteCount + 1
                    if noteCount > 1 then
                        set output to output & ","
                    end if

                    set nId to id of n
                    set nName to name of n
                    set nMod to modification date of n
                    set nBody to ""
                    try
                        set nBody to plaintext of n
                        if length of nBody > 2000 then
                            set nBody to text 1 thru 2000 of nBody
                        end if
                    end try

                    set nName to my escapeJSON(nName)
                    set folderName to my escapeJSON(folderName)
                    set nBody to my escapeJSON(nBody)

                    set noteJSON to "{{"id\": \"" & nId & "\", \"name\": \"" & nName & "\", \"modified\": \"" & (nMod as string) & "\", \"folder\": \"" & folderName & "\", \"body\": \"" & nBody & "\"}}"
                    set output to output & noteJSON
                    if noteCount >= 50 then exit repeat
                end repeat
                if noteCount >= 50 then exit repeat
            end repeat
        end tell
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

    let raw = run_osascript_with_timeout(&script, std::time::Duration::from_secs(60)).await?;
    Ok(parse_json_output(&raw))
}

/// Get a single note by ID with full body content.
pub async fn get(id: &str) -> anyhow::Result<Note> {
    let escaped_id = escape_applescript(id);
    let script = format!(
        r#"
        tell application "Notes"
            set nId to id of note id "{escaped_id}"
            set nName to name of note id "{escaped_id}"
            set nMod to modification date of note id "{escaped_id}"
            set nFolder to name of container of note id "{escaped_id}"
            set nBody to ""
            try
                set nBody to plaintext of note id "{escaped_id}"
            end try
            return nName & tab & (nMod as string) & tab & nId & tab & nFolder & tab & nBody
        end tell
    "#
    );

    let raw = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
    let parts: Vec<&str> = raw.split('\t').collect();
    if parts.is_empty() {
        anyhow::bail!("Note not found: {id}");
    }

    let name = parts.first().copied().unwrap_or("").trim();
    if name.is_empty() {
        anyhow::bail!("Note not found: {id}");
    }
    let mod_str = parts.get(1).copied().unwrap_or("").trim();
    let note_id = parts.get(2).copied().unwrap_or("").trim();
    let folder = parts.get(3).copied().unwrap_or("").trim();
    let body_text = parts.get(4).copied().unwrap_or("").trim();

    let modified = if mod_str.is_empty() {
        None
    } else {
        parse_applescript_date(mod_str)
    };

    let parsed_id = if note_id.is_empty() {
        slug(name)
    } else {
        slug(note_id)
    };

    Ok(Note {
        id: parsed_id,
        title: truncate_for_title(name),
        folder: folder.to_string(),
        body: if body_text.is_empty() {
            None
        } else {
            Some(body_text.to_string())
        },
        modified,
    })
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
        let b_esc = escape_applescript(b);
        format!(", body:\"<div>{b_esc}</div>\"")
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

    let raw = run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
    let new_id = raw.trim().to_string();
    Ok(ActionResult::success_with_id("create", &new_id))
}

/// Update the body of an existing note by ID.
pub async fn update(id: &str, body: &str) -> anyhow::Result<ActionResult> {
    let escaped_id = escape_applescript(id);
    let body_esc = escape_applescript(body);

    let script = format!(
        r#"
        tell application "Notes"
            set body of note id "{escaped_id}" to "<div>{body_esc}</div>"
        end tell
    "#
    );

    run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
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

    run_osascript_with_timeout(&script, std::time::Duration::from_secs(30)).await?;
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
            title: truncate_for_title(name),
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
            title: truncate_for_title(name),
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
}
