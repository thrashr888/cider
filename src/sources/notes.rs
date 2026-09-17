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
    /// Apple's `plaintext` rendering: every block on its own line, with no
    /// list markers and no blank line between paragraphs. Kept as-is for
    /// callers that have always read it; [`Note::markdown`] is the faithful one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The note's rich text as Markdown, converted from the HTML the Notes
    /// app actually stores. Bullets, numbering, checkboxes, headings, links
    /// and paragraph breaks survive here; in `body` they do not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
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
/// Includes each note's body, both as Apple's flattened `plaintext` and as
/// Markdown converted from the stored HTML. Reading a note's text is an Apple
/// event per property, so `cap` bounds the walk; pass `None` to walk
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
                    set nHtml to ""
                    try
                        set nHtml to body of n
                    end try
                    set nHtml to my escapeJSON(nHtml)

                    set noteJSON to "{{\"id\": \"" & nId & "\", \"name\": \"" & nName & "\", \"modified\": \"" & (nMod as string) & "\", \"folder\": \"" & folderName & "\", \"body\": \"" & nBody & "\", \"html\": \"" & nHtml & "\"}}"
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
                set nContainer to container of n
                set nFolder to my escapeJSON(name of nContainer)
            end try
            set nBody to ""
            try
                set nBody to my escapeJSON(plaintext of n)
            end try
            set nHtml to ""
            try
                set nHtml to my escapeJSON(body of n)
            end try
            return "[{{\"id\": \"" & nId & "\", \"name\": \"" & nName & "\", \"modified\": \"" & nMod & "\", \"folder\": \"" & nFolder & "\", \"body\": \"" & nBody & "\", \"html\": \"" & nHtml & "\"}}]"
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

// --- HTML -> Markdown -------------------------------------------------------
//
// A note's `plaintext` is a flattened rendering: every block lands on its own
// line with no list markers and no blank line between paragraphs, so a bullet
// arrives indistinguishable from a paragraph — and a consumer that renders the
// result as Markdown joins those single newlines back into one flat paragraph.
// The structure only exists in `body`, which is HTML, so convert that.

#[derive(Debug)]
enum Tok {
    Text(String),
    Open { name: String, attrs: String },
    Close(String),
}

fn tokenize(html: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut text = String::new();
    let mut rest = html;

    while let Some(lt) = rest.find('<') {
        text.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];

        if let Some(comment) = after.strip_prefix("!--") {
            rest = comment.find("-->").map_or("", |e| &comment[e + 3..]);
            continue;
        }
        let Some(gt) = after.find('>') else {
            // A bare `<` in the text, not a tag.
            text.push('<');
            rest = after;
            continue;
        };
        let inner = &after[..gt];
        rest = &after[gt + 1..];

        if inner.starts_with('!') || inner.starts_with('?') || inner.is_empty() {
            continue;
        }
        if !text.is_empty() {
            toks.push(Tok::Text(std::mem::take(&mut text)));
        }
        if let Some(close) = inner.strip_prefix('/') {
            toks.push(Tok::Close(close.trim().to_ascii_lowercase()));
        } else {
            let inner = inner.trim_end().trim_end_matches('/');
            let (name, attrs) = match inner.find(char::is_whitespace) {
                Some(i) => (&inner[..i], &inner[i..]),
                None => (inner, ""),
            };
            toks.push(Tok::Open {
                name: name.to_ascii_lowercase(),
                attrs: attrs.to_string(),
            });
        }
    }

    text.push_str(rest);
    if !text.is_empty() {
        toks.push(Tok::Text(text));
    }
    toks
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        // Entity names are short; a stray `&` must not swallow a whole line.
        let decoded = after.find(';').filter(|i| *i <= 8).and_then(|i| {
            let ent = &after[..i];
            let ch = match ent.to_ascii_lowercase().as_str() {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => ent.strip_prefix('#').and_then(|n| {
                    let cp = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    };
                    cp.and_then(char::from_u32)
                }),
            };
            ch.map(|c| (c, i))
        });

        match decoded {
            Some((ch, i)) => {
                out.push(ch);
                rest = &after[i + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }

    out.push_str(rest);
    out
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    // `to_ascii_lowercase` is byte-length preserving, so offsets agree.
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;

    while let Some(at) = lower[from..].find(name) {
        let start = from + at;
        let is_whole_attr = start == 0 || lower.as_bytes()[start - 1].is_ascii_whitespace();
        let after = attrs[start + name.len()..].trim_start();
        if is_whole_attr {
            if let Some(value) = after.strip_prefix('=') {
                let value = value.trim_start();
                let raw = match value.strip_prefix(['"', '\'']) {
                    Some(quoted) => {
                        let quote = value.as_bytes()[0] as char;
                        quoted.split(quote).next().unwrap_or("")
                    }
                    None => value.split_whitespace().next().unwrap_or(""),
                };
                return Some(decode_entities(raw));
            }
        }
        from = start + name.len();
    }
    None
}

fn font_size_px(attrs: &str) -> Option<u32> {
    let at = attrs.to_ascii_lowercase().find("font-size")? + "font-size".len();
    let digits: String = attrs[at..]
        .trim_start()
        .trim_start_matches(':')
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Apple Notes has no `<h1>`: its Title/Heading/Subheading styles round-trip
/// as a plain `<div>` whose text sits in a `<font face=".AppleSystemUIFontBold">`,
/// carrying `font-size: 24px` for Title, `18px` for Heading and no size for
/// Subheading. Ordinary bold text gets `<b>` with no such font face, so this
/// never mistakes it for a heading. `open` indexes the `<div>`'s own token.
fn apple_heading_level(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut styled = false;
    let mut size = None;

    for tok in &toks[open..] {
        match tok {
            Tok::Open { name, attrs } => {
                if name == "div" {
                    depth += 1;
                    // A block inside the block: this is a container, not a line.
                    if depth > 1 {
                        return None;
                    }
                    continue;
                }
                if name == "font"
                    && attrs
                        .to_ascii_lowercase()
                        .contains(".applesystemuifontbold")
                {
                    styled = true;
                }
                if let Some(px) = font_size_px(attrs) {
                    size = Some(px.max(size.unwrap_or(0)));
                }
            }
            Tok::Close(name) if name == "div" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }

    if !styled {
        return None;
    }
    Some(match size.unwrap_or(0) {
        24.. => 1,
        18..=23 => 2,
        _ => 3,
    })
}

#[derive(Clone, Copy)]
enum ListKind {
    Bullet,
    Ordered(u32),
    Check,
}

/// Accumulates one output line at a time. `prefix` decorates the line being
/// built (`- `, `## `, `> `); `cont` is what a line wrapped by a `<br>` gets
/// instead, so a wrapped bullet indents rather than sprouting a second bullet.
#[derive(Default)]
struct Md {
    lines: Vec<String>,
    line: String,
    prefix: String,
    cont: String,
    lists: Vec<ListKind>,
    emph: Vec<(&'static str, usize)>,
    link: Option<(String, usize)>,
    heading: bool,
    want_blank: bool,
}

impl Md {
    fn push_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_whitespace() {
                // HTML collapses whitespace runs, and leading space would
                // otherwise push the text off its marker.
                if !self.line.is_empty() && !self.line.ends_with(' ') {
                    self.line.push(' ');
                }
            } else {
                self.line.push(ch);
            }
        }
    }

    fn end_line(&mut self) {
        self.close_link();
        while let Some(&(marker, _)) = self.emph.last() {
            self.close_emph(marker);
        }
        let text = self.line.trim();
        if !text.is_empty() {
            if self.want_blank && !self.lines.is_empty() {
                self.lines.push(String::new());
            }
            self.lines.push(format!("{}{}", self.prefix, text));
            self.prefix = self.cont.clone();
            self.want_blank = false;
        }
        self.line.clear();
    }

    /// Ask for a blank line before the next line that carries text. Deferring
    /// it this way keeps blanks out of the start and end of the document.
    fn blank(&mut self) {
        if !self.lines.is_empty() {
            self.want_blank = true;
        }
    }

    fn start_block(&mut self, prefix: &str, cont: &str) {
        self.prefix = prefix.to_string();
        self.cont = cont.to_string();
    }

    fn start_item(&mut self, attrs: &str) {
        let indent = "  ".repeat(self.lists.len().saturating_sub(1));
        let marker = match self.lists.last_mut() {
            Some(ListKind::Ordered(n)) => {
                *n += 1;
                format!("{n}. ")
            }
            Some(ListKind::Check) => {
                let class = attr_value(attrs, "class").unwrap_or_default();
                // "unchecked" contains "checked", so test it first.
                if class.contains("unchecked") || !class.contains("checked") {
                    "- [ ] ".to_string()
                } else {
                    "- [x] ".to_string()
                }
            }
            // A stray <li> outside any list still reads as a bullet.
            Some(ListKind::Bullet) | None => "- ".to_string(),
        };
        self.cont = format!("{indent}{}", " ".repeat(marker.chars().count()));
        self.prefix = format!("{indent}{marker}");
    }

    fn open_emph(&mut self, marker: &'static str) {
        // Headings carry their own weight, and nesting the same marker twice
        // produces literal asterisks.
        if self.heading || self.emph.iter().any(|(m, _)| *m == marker) {
            return;
        }
        self.emph.push((marker, self.line.len()));
        self.line.push_str(marker);
    }

    fn close_emph(&mut self, marker: &'static str) {
        let Some(pos) = self.emph.iter().rposition(|(m, _)| *m == marker) else {
            return;
        };
        let (_, at) = self.emph.remove(pos);
        if self.line[at + marker.len()..].trim().is_empty() {
            // Nothing was emphasized — drop the opening marker again.
            self.line.truncate(at);
            return;
        }
        // "**bold **" renders its asterisks literally, so close before the space.
        let trailing_space = self.line.ends_with(' ');
        let end = self.line.trim_end().len();
        self.line.truncate(end);
        self.line.push_str(marker);
        if trailing_space {
            self.line.push(' ');
        }
    }

    fn open_link(&mut self, attrs: &str) {
        self.close_link();
        if let Some(href) = attr_value(attrs, "href") {
            self.link = Some((href, self.line.len()));
        }
    }

    fn close_link(&mut self) {
        let Some((href, at)) = self.link.take() else {
            return;
        };
        if at > self.line.len() {
            return;
        }
        let text = self.line[at..].trim().to_string();
        self.line.truncate(at);
        if text.is_empty() || text == href {
            self.line.push_str(&format!("<{href}>"));
        } else {
            self.line.push_str(&format!("[{text}]({href})"));
        }
    }

    fn finish(mut self) -> String {
        self.end_line();
        while matches!(self.lines.last(), Some(l) if l.trim().is_empty()) {
            self.lines.pop();
        }
        self.lines.join("\n")
    }
}

/// Convert a note's stored HTML to Markdown, keeping paragraphs, bullets,
/// numbering, checkboxes, headings and links.
fn html_to_markdown(html: &str) -> String {
    let toks = tokenize(html);
    let mut md = Md::default();
    // <style>/<script> bodies are markup, not prose.
    let mut skipping: Option<String> = None;

    for (i, tok) in toks.iter().enumerate() {
        if let Some(name) = &skipping {
            if matches!(tok, Tok::Close(close) if close == name) {
                skipping = None;
            }
            continue;
        }
        match tok {
            Tok::Text(text) => md.push_text(&decode_entities(text)),
            Tok::Open { name, attrs } => match name.as_str() {
                "style" | "script" | "head" => skipping = Some(name.clone()),
                "ul" | "ol" => {
                    md.end_line();
                    if md.lists.is_empty() {
                        md.blank();
                    }
                    md.lists.push(if name == "ol" {
                        ListKind::Ordered(0)
                    } else if attr_value(attrs, "class").is_some_and(|c| c.contains("checklist")) {
                        ListKind::Check
                    } else {
                        ListKind::Bullet
                    });
                }
                "li" => {
                    md.end_line();
                    md.start_item(attrs);
                }
                "br" => md.end_line(),
                "hr" => {
                    md.end_line();
                    md.blank();
                    md.start_block("", "");
                    md.push_text("---");
                    md.end_line();
                    md.blank();
                }
                "div" | "p" | "tr" => {
                    md.end_line();
                    // Inside a list these wrap the item's own text.
                    if md.lists.is_empty() {
                        md.blank();
                        match apple_heading_level(&toks, i) {
                            Some(level) => {
                                md.start_block(&format!("{} ", "#".repeat(level)), "");
                                md.heading = true;
                            }
                            None => md.start_block("", ""),
                        }
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    md.end_line();
                    md.blank();
                    let level = name[1..].parse().unwrap_or(1);
                    md.start_block(&format!("{} ", "#".repeat(level)), "");
                    md.heading = true;
                }
                "blockquote" => {
                    md.end_line();
                    md.blank();
                    md.start_block("> ", "> ");
                }
                "b" | "strong" => md.open_emph("**"),
                "i" | "em" => md.open_emph("*"),
                "a" => md.open_link(attrs),
                "td" | "th" if !md.line.trim().is_empty() => md.line.push_str(" | "),
                _ => {}
            },
            Tok::Close(name) => match name.as_str() {
                "ul" | "ol" => {
                    md.end_line();
                    md.lists.pop();
                    if md.lists.is_empty() {
                        md.blank();
                        md.start_block("", "");
                    }
                }
                "li" => md.end_line(),
                "div" | "p" | "tr" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    md.end_line();
                    md.heading = false;
                    if md.lists.is_empty() {
                        md.start_block("", "");
                        md.blank();
                    }
                }
                "b" | "strong" => md.close_emph("**"),
                "i" | "em" => md.close_emph("*"),
                "a" => md.close_link(),
                _ => {}
            },
        }
    }

    md.finish()
}

fn parse_json_output(output: &str) -> Vec<Note> {
    let items: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Failed to parse notes JSON: {e}, falling back to line parser");
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
        let html = item["html"].as_str().unwrap_or("").trim();

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

        // The HTML is the note's real structure; `plaintext` is the fallback
        // for a note with no rich text (and for `list --brief`, which asks
        // for neither).
        let markdown = if html.is_empty() {
            non_empty(body_text.to_string())
        } else {
            non_empty(html_to_markdown(html))
        };

        records.push(Note {
            id,
            title: name.to_string(),
            folder: folder.to_string(),
            body: non_empty(body_text.to_string()),
            markdown,
            modified,
        });
    }

    records
}

fn non_empty(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
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
            markdown: None,
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
            markdown: None,
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

    /// The bug the second user reported: a note whose body is a bulleted list
    /// arrived as one flat paragraph. This is that note's real HTML, copied
    /// from `body of note` on a Mac.
    #[test]
    fn test_html_to_markdown_real_note() {
        let html = r#"<div>Alchemy cider test</div>
<div><b><font face=".AppleSystemUIFontBold"><span style="font-size: 24px">Alchemy cider test</span></font></b></div>
<div>Intro paragraph before the list.</div>
<ul>
<li>First bullet item</li>
<li>Second bullet item</li>
<li>Third bullet item</li>
</ul>
<div>Closing paragraph after the list with a <u><a href="https://example.com/">link here</a></u>.</div>
<ol>
<li>Step one</li>
<li>Step two</li>
</ol>"#;
        assert_eq!(
            html_to_markdown(html),
            "Alchemy cider test\n\
             \n\
             # Alchemy cider test\n\
             \n\
             Intro paragraph before the list.\n\
             \n\
             - First bullet item\n\
             - Second bullet item\n\
             - Third bullet item\n\
             \n\
             Closing paragraph after the list with a [link here](https://example.com/).\n\
             \n\
             1. Step one\n\
             2. Step two"
        );
    }

    /// Notes writes a sublist as a sibling of the <li> it belongs to, not
    /// inside it. Both that and the standards-compliant nesting must indent.
    #[test]
    fn test_html_to_markdown_nested_list_both_shapes() {
        let apple = "<ul>\n<li>outer one</li>\n<ul>\n<li>inner a</li>\n<li>inner b</li>\n</ul>\n<li>outer two</li>\n</ul>";
        let standard =
            "<ul><li>outer one<ul><li>inner a</li><li>inner b</li></ul></li><li>outer two</li></ul>";
        let expected = "- outer one\n  - inner a\n  - inner b\n- outer two";
        assert_eq!(html_to_markdown(apple), expected);
        assert_eq!(html_to_markdown(standard), expected);
    }

    #[test]
    fn test_html_to_markdown_checklist() {
        let html = r#"<ul class="checklist"><li class="checked">done thing</li><li class="unchecked">todo thing</li><li>bare item</li></ul>"#;
        assert_eq!(
            html_to_markdown(html),
            "- [x] done thing\n- [ ] todo thing\n- [ ] bare item"
        );
    }

    #[test]
    fn test_html_to_markdown_ordered_list_numbers() {
        assert_eq!(
            html_to_markdown("<ol><li>one</li><li>two</li><li>three</li></ol>"),
            "1. one\n2. two\n3. three"
        );
    }

    /// Real <h1>..<h6> tags, and Apple's own encoding: a <div> whose text sits
    /// in `.AppleSystemUIFontBold` at 24px (Title), 18px (Heading) or no size
    /// (Subheading). Plain bold text carries no such font and stays a paragraph.
    #[test]
    fn test_html_to_markdown_headings() {
        assert_eq!(
            html_to_markdown("<h1>Big</h1><h3>Small</h3><div>body</div>"),
            "# Big\n\n### Small\n\nbody"
        );

        let apple = r#"<div><b><font face=".AppleSystemUIFontBold"><span style="font-size: 24px">Title</span></font></b></div>
<div><b><font face=".AppleSystemUIFontBold"><span style="font-size: 18px">Heading</span></font></b></div>
<div><b><font face=".AppleSystemUIFontBold">Subheading</font></b></div>
<div><b>just bold</b></div>"#;
        assert_eq!(
            html_to_markdown(apple),
            "# Title\n\n## Heading\n\n### Subheading\n\n**just bold**"
        );
    }

    #[test]
    fn test_html_to_markdown_links_and_inline() {
        assert_eq!(
            html_to_markdown(r#"<div>see <a href="https://apple.com/">Apple</a> now</div>"#),
            "see [Apple](https://apple.com/) now"
        );
        // A link whose text is its own URL becomes an autolink, not [url](url).
        assert_eq!(
            html_to_markdown(r#"<div><a href="https://apple.com/">https://apple.com/</a></div>"#),
            "<https://apple.com/>"
        );
        assert_eq!(
            html_to_markdown("<div><b>bold</b> and <i>italic</i></div>"),
            "**bold** and *italic*"
        );
        // Empty emphasis leaves no stray asterisks behind.
        assert_eq!(html_to_markdown("<div>x<b> </b></div>"), "x");
    }

    #[test]
    fn test_html_to_markdown_entities_and_blocks() {
        assert_eq!(
            html_to_markdown("<div>a &amp; b &lt; c &#39;q&#39; &nbsp;d</div>"),
            "a & b < c 'q' d"
        );
        assert_eq!(
            html_to_markdown("<blockquote>quoted</blockquote><div>after</div>"),
            "> quoted\n\nafter"
        );
        // A <br> wraps the line without sprouting a second bullet.
        assert_eq!(
            html_to_markdown("<ul><li>one<br>still one</li></ul>"),
            "- one\n  still one"
        );
        assert_eq!(html_to_markdown(""), "");
        assert_eq!(html_to_markdown("<div><br></div>"), "");
    }

    /// `markdown` comes from the HTML; when a note has none, it falls back to
    /// the flattened plaintext rather than going missing.
    #[test]
    fn test_parse_json_output_markdown_field() {
        let json = serde_json::json!([{
            "id": "x-coredata://A/ICNote/p1",
            "name": "Groceries",
            "modified": "",
            "folder": "Notes",
            "body": "Groceries\nmilk\neggs",
            "html": "<div>Groceries</div><ul><li>milk</li><li>eggs</li></ul>",
        }])
        .to_string();
        let records = parse_json_output(&json);
        assert_eq!(
            records[0].markdown.as_deref(),
            Some("Groceries\n\n- milk\n- eggs")
        );
        // `body` keeps its old meaning for callers that already read it.
        assert_eq!(records[0].body.as_deref(), Some("Groceries\nmilk\neggs"));

        let no_html = serde_json::json!([{
            "id": "x-coredata://A/ICNote/p2",
            "name": "Plain",
            "modified": "",
            "folder": "Notes",
            "body": "just text",
        }])
        .to_string();
        let records = parse_json_output(&no_html);
        assert_eq!(records[0].markdown.as_deref(), Some("just text"));
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
