use super::local_store;
use super::util::run_command_with_timeout;
use super::util::{run_jxa_with_timeout, ActionResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryItem {
    pub title: String,
    pub url: String,
    pub visit_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_visited: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenTab {
    pub title: String,
    pub url: String,
    pub window: i64,
    /// One-based position; refresh after tabs are moved or closed.
    pub tab: i64,
    pub window_id: i64,
}

/// List Safari bookmarks (excludes Reading List, which has its own command).
pub async fn bookmarks() -> anyhow::Result<Vec<Bookmark>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let plist_path = format!("{home}/Library/Safari/Bookmarks.plist");

    // Use Python to parse the binary plist
    let script = format!(
        r#"
import plistlib, json
with open("{plist_path}", "rb") as f:
    data = plistlib.load(f)

bookmarks = []
def extract(obj, folder=""):
    if not isinstance(obj, dict):
        return
    title = obj.get("Title", "")
    # Skip Reading List and special folders
    if title == "com.apple.ReadingList":
        return
    url = obj.get("URLString", "")
    if url:
        bm_title = obj.get("URIDictionary", {{}}).get("title", title or url)
        entry = {{"title": bm_title, "url": url}}
        if folder:
            entry["folder"] = folder
        bookmarks.append(entry)
    children = obj.get("Children", [])
    child_folder = title if title and title != "BookmarksBar" and title != "BookmarksMenu" else folder
    if title == "BookmarksBar":
        child_folder = "Favorites"
    elif title == "BookmarksMenu":
        child_folder = "Bookmarks Menu"
    for child in children:
        extract(child, child_folder)

extract(data)
print(json.dumps(bookmarks))
"#
    );

    let output = run_command_with_timeout(
        "python3",
        &["-c", &script],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let items: Vec<serde_json::Value> = serde_json::from_str(&output)?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let title = item["title"].as_str()?.to_string();
            let url = item["url"].as_str()?.to_string();
            if url.is_empty() {
                return None;
            }
            Some(Bookmark {
                title,
                url,
                folder: item["folder"].as_str().map(String::from),
            })
        })
        .collect())
}

/// List Safari browsing history, preserving the original library entry point.
pub async fn history(limit: Option<u32>) -> anyhow::Result<Vec<HistoryItem>> {
    search_history(None, limit.unwrap_or(100), 0).await
}

/// Search retained history by literal URL/title substring (ASCII case-insensitive).
/// Results are visits, so repeated visits to one URL remain separate records.
pub async fn search_history(
    search: Option<&str>,
    limit: u32,
    offset: u32,
) -> anyhow::Result<Vec<HistoryItem>> {
    let sql = history_sql(search, limit, offset)?;
    local_store::query(&local_store::home_path("Library/Safari/History.db")?, &sql)
        .await
        .map_err(safari_error)
}

fn history_sql(search: Option<&str>, limit: u32, offset: u32) -> anyhow::Result<String> {
    anyhow::ensure!(limit <= 10_000, "invalid limit: maximum is 10000");
    let filter = match search {
        Some(search) => {
            let value = local_store::sql_string(search)?;
            format!("(instr(lower(hi.url), lower({value})) > 0 OR instr(lower(COALESCE(hv.title, '')), lower({value})) > 0)")
        }
        None => "1=1".into(),
    };
    Ok(format!("SELECT COALESCE(NULLIF(hv.title, ''), hi.url) AS title, hi.url AS url,
        hi.visit_count AS visit_count, datetime(hv.visit_time + 978307200, 'unixepoch') AS last_visited
        FROM history_items hi JOIN history_visits hv ON hi.id = hv.history_item
        WHERE {filter} ORDER BY hv.visit_time DESC, hv.id DESC LIMIT {limit} OFFSET {offset}"))
}

/// List currently open Safari tabs, without starting Safari if it is closed.
pub async fn tabs() -> anyhow::Result<Vec<OpenTab>> {
    let output = run_jxa_with_timeout(
        r#"
const app = Application("com.apple.Safari");
const results = [];
if (app.running()) {
    const wins = app.windows();
    for (let w = 0; w < wins.length; w++) {
        const tabs = wins[w].tabs();
        for (let t = 0; t < tabs.length; t++) {
            results.push({title: tabs[t].name() || "", url: tabs[t].url() || "",
                window: w + 1, tab: t + 1, window_id: wins[w].id()});
        }
    }
}
JSON.stringify(results)
"#,
        Duration::from_secs(15),
    )
    .await
    .map_err(safari_error)?;
    Ok(serde_json::from_str(&output)?)
}

/// Safari exposes positional tab references, not durable tab IDs.
#[derive(Debug, Clone, Copy)]
pub struct TabTarget {
    pub window: u32,
    pub tab: u32,
}
impl Default for TabTarget {
    fn default() -> Self {
        Self { window: 1, tab: 1 }
    }
}
impl TabTarget {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.window > 0 && self.tab > 0,
            "invalid target: window and tab are one-based"
        );
        Ok(())
    }
    fn script(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(format!(
            r#"
const app = Application("com.apple.Safari");
if (!app.running()) throw new Error("Safari is not running. Open Safari and a window in the desired profile, then run cider safari tabs --pretty");
const wins = app.windows();
if (wins.length < {window}) throw new Error("Safari window not found. Run cider safari tabs --pretty and choose a current one-based --window value");
const win = wins[{window} - 1];
const tabs = win.tabs();
if (tabs.length < {tab}) throw new Error("Safari tab not found. Run cider safari tabs --pretty and choose current one-based --window and --tab values");
const target = tabs[{tab} - 1];
"#,
            window = self.window,
            tab = self.tab
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormat {
    Text,
    Html,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PageContent {
    pub title: String,
    pub url: String,
    pub window: u32,
    pub tab: u32,
    pub window_id: i64,
    pub format: ContentFormat,
    pub content: String,
    pub truncated: bool,
}

pub fn validate_content_limit(max_chars: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=1_000_000).contains(&max_chars),
        "invalid max_chars: expected 1-1000000"
    );
    Ok(())
}

/// Read Safari's native text/source properties; no page JavaScript required.
/// `max_chars` counts Unicode scalar values, not bytes.
pub async fn content(
    target: TabTarget,
    format: ContentFormat,
    max_chars: u32,
) -> anyhow::Result<PageContent> {
    validate_content_limit(max_chars)?;
    let property = match format {
        ContentFormat::Text => "text",
        ContentFormat::Html => "source",
    };
    let script = format!(
        r#"{}
const before = target.url();
const body = target.{}();
if (typeof body !== "string") throw new Error("Safari page content unavailable. Load a normal web page in this tab, wait for it to finish, then retry cider safari content");
if (target.url() !== before) throw new Error("Safari tab navigated while reading; retry");
JSON.stringify({{title: target.name() || "", url: before || "", window: {}, tab: {},
    window_id: win.id(), format: {}, content: Array.from(body).slice(0, {}).join(""),
    truncated: Array.from(body).length > {}}})
"#,
        target.script()?,
        property,
        target.window,
        target.tab,
        serde_json::to_string(&format)?,
        max_chars,
        max_chars
    );
    Ok(serde_json::from_str(
        &run_jxa_with_timeout(&script, Duration::from_secs(20))
            .await
            .map_err(safari_error)?,
    )?)
}

pub fn validate_url(value: &str) -> anyhow::Result<()> {
    let url = url::Url::parse(value).map_err(|e| anyhow::anyhow!("invalid URL: {e}"))?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "invalid URL: expected an absolute HTTP(S) URL without embedded credentials"
    );
    Ok(())
}

pub fn validate_timeout(timeout: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=120).contains(&timeout),
        "invalid timeout: expected 1-120 seconds"
    );
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct FetchResult {
    #[serde(flatten)]
    pub action: ActionResult,
    pub requested_url: String,
    pub page: PageContent,
}

/// Navigate a NEW tab in an existing window, wait for document readiness,
/// and extract content. The new tab stays open, including on failure.
pub async fn fetch(
    url: &str,
    window: u32,
    format: ContentFormat,
    max_chars: u32,
    timeout: u32,
) -> anyhow::Result<FetchResult> {
    validate_url(url)?;
    validate_timeout(timeout)?;
    validate_content_limit(max_chars)?;
    let selection = TabTarget { window, tab: 1 }.script()?;
    let property = match format {
        ContentFormat::Text => "text",
        ContentFormat::Html => "source",
    };
    let script = format!(
        r#"{selection}
// Check the JavaScript permission BEFORE creating a tab.
app.doJavaScript("1", {{in: target}});
const created = app.Tab({{url: {url}}});
win.tabs.push(created);
const pageTab = win.tabs[win.tabs.length - 1];
const deadline = Date.now() + {timeout} * 1000;
let ready = false;
while (Date.now() < deadline) {{
    if (pageTab.url() && pageTab.url() !== "about:blank") {{
        const state = app.doJavaScript("location.href !== 'about:blank' && document.readyState === 'complete'", {{in: pageTab}});
        if (state === true) {{ ready = true; break; }}
    }}
    delay(0.2);
}}
if (!ready) throw new Error("Safari fetch timed out; new tab left open");
const before = pageTab.url();
const body = pageTab.{property}();
if (typeof body !== "string") throw new Error("Safari page content unavailable; new tab left open");
if (pageTab.url() !== before) throw new Error("Safari tab navigated while reading; retry content");
const chars = Array.from(body);
JSON.stringify({{title: pageTab.name() || "", url: before, window: {window}, tab: pageTab.index(),
    window_id: win.id(), format: {format}, content: chars.slice(0, {max_chars}).join(""), truncated: chars.length > {max_chars}}})
"#,
        url = serde_json::to_string(url)?,
        format = serde_json::to_string(&format)?
    );
    let page = serde_json::from_str(&run_browser_script(&script, timeout + 10).await?)?;
    Ok(FetchResult {
        action: ActionResult::success("fetch"),
        requested_url: url.into(),
        page,
    })
}

fn safari_error(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    let lower = message.to_lowercase();
    let fix = if lower.contains("javascript from apple events") {
        Some("Safari permission required. In Safari > Settings > Advanced, enable Show features for web developers if the Developer tab is hidden. Then in Settings > Developer, enable Allow JavaScript from Apple Events (older Safari: Develop menu > Allow JavaScript from Apple Events). Retry the command. Remote automation and JavaScript from Smart Search field are not required. You can use cider safari content without this setting.")
    } else if lower.contains("-1743") || lower.contains("not authorized to send apple events") {
        Some("Safari Automation permission required. Open System Settings > Privacy & Security > Automation, expand the app launching cider (Terminal, Codex, or your host app), and enable Safari. If it is not listed, run cider safari tabs from that app and allow the macOS prompt, then retry. A packaged host app must declare NSAppleEventsUsageDescription and the Apple Events entitlement when hardened.")
    } else if lower.contains("full disk access")
        || lower.contains("operation not permitted")
        || lower.contains("permission denied")
    {
        Some("Safari store permission required. Open System Settings > Privacy & Security > Full Disk Access, add/enable the app launching cider (Terminal, Codex, or your host app), fully quit and reopen that app, then retry. Grant access to the launching app, not the cider binary. Run cider permissions --source safari for details.")
    } else if lower.contains("application can't be found") {
        Some("Safari could not be located. Open Safari once and retry cider safari tabs from your normal macOS terminal. If Safari is installed but this only fails in an agent sandbox, allow the command to run with macOS app access.")
    } else if lower.contains("timed out") {
        Some("Safari timed out. Check the selected tab for login, consent, or loading dialogs; finish those in Safari, then retry. Run cider safari tabs --pretty to refresh tab positions. For fetch/request, increase --timeout up to 120 seconds if needed. A fetch-created tab may remain open; read it with cider safari content.")
    } else {
        None
    };
    match fix {
        Some(fix) => anyhow::anyhow!("{fix} Original error: {message}"),
        None => error,
    }
}

async fn run_browser_script(script: &str, timeout: u32) -> anyhow::Result<String> {
    run_jxa_with_timeout(script, Duration::from_secs(u64::from(timeout)))
        .await
        .map_err(safari_error)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestResult {
    pub ok: bool,
    pub action: String,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub content_type: String,
    pub body: String,
    pub truncated: bool,
}

/// GET an absolute URL inside an existing tab's origin/session. No cookie export,
/// cross-origin access, arbitrary script execution, or custom credential headers.
pub async fn request(
    url: &str,
    target: TabTarget,
    max_bytes: u32,
    timeout: u32,
) -> anyhow::Result<RequestResult> {
    validate_url(url)?;
    validate_timeout(timeout)?;
    validate_content_limit(max_bytes)?;
    let config = serde_json::json!({"url":url,"max_bytes":max_bytes,"timeout":timeout,
        "key":format!("__cider_{}", uuid::Uuid::new_v4().simple())});
    // Safari does not reliably propagate uncaught page exceptions through
    // doJavaScript. Return startup failures explicitly instead of timing out.
    let page_script = format!(
        "(function(){{try{{return ({})({config});}}catch(error){{return JSON.stringify({{error:String(error.message || error)}});}}}})()",
        include_str!("safari_request.js")
    );
    let key = serde_json::to_string(config["key"].as_str().unwrap())?;
    let script = format!(
        r#"{}
const initialURL = target.url();
const started = app.doJavaScript({}, {{in: target}});
if (started !== "started") {{
    let message = "Safari request could not start in this tab. Load an HTTP(S) page, wait for it to finish, then run cider safari tabs --pretty and retry with its --window and --tab";
    try {{ message = JSON.parse(started).error || message; }} catch (e) {{}}
    throw new Error(message);
}}
const deadline = Date.now() + {timeout} * 1000 + 1000;
let result;
try {{
    while (Date.now() < deadline) {{
        if (target.url() !== initialURL) throw new Error("Safari tab navigated during request");
        const raw = app.doJavaScript('JSON.stringify(window[' + {key} + '] && window[' + {key} + '].result || null)', {{in: target}});
        if (raw && raw !== "null") {{ result = JSON.parse(raw); break; }}
        delay(0.1);
    }}
    if (!result) throw new Error("Safari request timed out");
    if (result.error) throw new Error(result.error);
}} finally {{
    try {{ app.doJavaScript('(function(){{const s=window[' + {key} + '];if(s){{s.controller.abort();clearTimeout(s.timer);delete window[' + {key} + '];}}}})()', {{in: target}}); }} catch (e) {{}}
}}
JSON.stringify(result)
"#,
        target.script()?,
        serde_json::to_string(&page_script)?,
        key = serde_json::to_string(&key)?
    );
    let mut result: RequestResult =
        serde_json::from_str(&run_browser_script(&script, timeout + 10).await?)?;
    result.hint = request_hint(result.status).map(String::from);
    Ok(result)
}

fn request_hint(status: u16) -> Option<&'static str> {
    match status {
        401 => Some("Sign in to the website in the selected Safari tab, then retry. Use cider safari tabs --pretty to check the account/profile context. Sites using bearer tokens need application-specific headers that this command does not infer; use cider safari content for the loaded page."),
        403 => Some("Check access to this URL in the selected Safari tab. The site may require application-specific authentication or CSRF headers that Cider does not infer. Use cider safari content to read the loaded page."),
        404 => Some("Check the URL and endpoint path in Safari; the server returned Not Found."),
        429 => Some("The website is rate-limiting requests. Wait before retrying and follow the website's rate-limit guidance."),
        500..=599 => Some("The website returned a server error. Check the page in Safari and retry later."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn history_search_is_literal_paginated_and_preserves_text() {
        let db = local_store::tests::Database::new("CREATE TABLE history_items (id INTEGER, url TEXT, visit_count INTEGER);
            CREATE TABLE history_visits (id INTEGER, history_item INTEGER, title TEXT, visit_time REAL);
            INSERT INTO history_items VALUES (1, 'https://example.com/a', 2), (2, 'https://example.com/b', 1);
            INSERT INTO history_visits VALUES (1, 1, 'O''Brien 100%_\nTabbed\ttitle', 10),
              (2, 1, 'new visit', 20), (3, 2, '', 20);").await;
        let rows: Vec<HistoryItem> =
            local_store::query(&db.0, &history_sql(Some("o'brien 100%_"), 10, 0).unwrap())
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "O'Brien 100%_\nTabbed\ttitle");
        assert_eq!(rows[0].last_visited.as_deref(), Some("2001-01-01 00:00:10"));
        let rows: Vec<HistoryItem> = local_store::query(&db.0, &history_sql(None, 1, 0).unwrap())
            .await
            .unwrap();
        assert_eq!(rows[0].title, "https://example.com/b");
        let rows: Vec<HistoryItem> =
            local_store::query(&db.0, &history_sql(Some("EXAMPLE.COM"), 1, 1).unwrap())
                .await
                .unwrap();
        assert_eq!(rows[0].title, "new visit");
        let rows: Vec<HistoryItem> =
            local_store::query(&db.0, &history_sql(Some("' OR 1=1 --"), 10, 0).unwrap())
                .await
                .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn rejects_unsafe_urls_and_unbounded_inputs_before_automation() {
        for url in [
            "file:///tmp/a",
            "javascript:alert(1)",
            "data:text/html,x",
            "https://user:pass@example.com",
            "/relative",
            "garbage",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        assert!(validate_url("https://example.com/a?q=hello%20world#x").is_ok());
        assert!(history_sql(Some("\0"), 1, 0).is_err());
        assert!(history_sql(None, 10001, 0).is_err());
        assert!(TabTarget { window: 0, tab: 1 }.validate().is_err());
        assert!(TabTarget { window: 1, tab: 0 }.validate().is_err());
        assert!(validate_content_limit(0).is_err());
        assert!(validate_content_limit(1000001).is_err());
        assert!(validate_timeout(0).is_err());
        assert!(validate_timeout(121).is_err());
    }

    #[test]
    fn failures_include_specific_recovery_instructions() {
        assert!(request_hint(401).unwrap().contains("Sign in"));
        assert!(request_hint(403).unwrap().contains("CSRF"));
        assert!(request_hint(429).unwrap().contains("Wait"));
        assert!(request_hint(200).is_none());
        for (error, expected) in [
            (
                "You must enable Allow JavaScript from Apple Events (8)",
                "Settings > Developer",
            ),
            (
                "Not authorized to send Apple events (-1743)",
                "Privacy & Security > Automation",
            ),
            (
                "Operation not permitted",
                "Privacy & Security > Full Disk Access",
            ),
            ("JXA timed out after 30s", "--timeout up to 120"),
            (
                "Application can't be found (-2700)",
                "normal macOS terminal",
            ),
        ] {
            let message = safari_error(anyhow::anyhow!("{error}")).to_string();
            assert!(message.contains(expected), "{message}");
            assert!(
                message.contains(error),
                "original diagnostic must remain available"
            );
        }
        assert_eq!(
            safari_error(anyhow::anyhow!("unexpected schema")).to_string(),
            "unexpected schema"
        );
    }

    #[test]
    fn browser_results_keep_content_and_http_failure_in_json() {
        let value = serde_json::json!({"ok":false,"action":"request","url":"https://example.com/api",
            "status":401,"status_text":"Unauthorized","content_type":"application/json","body":"{\"error\":\"login\"}","truncated":false});
        let result: RequestResult = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), value);
        let page = PageContent {
            title: "a\nb".into(),
            url: "https://example.com".into(),
            window: 1,
            tab: 2,
            window_id: 10,
            format: ContentFormat::Text,
            content: "你好\nbody".into(),
            truncated: true,
        };
        let result = FetchResult {
            action: ActionResult::success("fetch"),
            requested_url: page.url.clone(),
            page,
        };
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["action"], "fetch");
        assert_eq!(json["page"]["content"], "你好\nbody");
        assert_eq!(json["page"]["format"], "text");
    }
}
