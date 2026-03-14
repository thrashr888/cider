use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HistoryItem {
    pub title: String,
    pub url: String,
    pub visit_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_visited: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenTab {
    pub title: String,
    pub url: String,
    pub window: i64,
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

/// List Safari browsing history.
pub async fn history(limit: Option<u32>) -> anyhow::Result<Vec<HistoryItem>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = format!("{home}/Library/Safari/History.db");

    if tokio::fs::metadata(&db_path).await.is_err() {
        anyhow::bail!("Safari History.db not found. Full Disk Access may be required.");
    }

    let lim = limit.unwrap_or(100);
    let query = format!(
        r#"
SELECT
    COALESCE(hv.title, ''),
    hi.url,
    hi.visit_count,
    datetime(hv.visit_time + 978307200, 'unixepoch')
FROM history_items hi
JOIN history_visits hv ON hi.id = hv.history_item
ORDER BY hv.visit_time DESC
LIMIT {lim};
"#
    );

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", &db_path, query.trim()],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let mut items = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let title = parts[0].trim();
        let url = parts[1].trim();
        if url.is_empty() {
            continue;
        }
        let visit_count: i64 = parts
            .get(2)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let last_visited = parts.get(3).map(|s| s.trim().to_string());

        items.push(HistoryItem {
            title: if title.is_empty() {
                url.to_string()
            } else {
                title.to_string()
            },
            url: url.to_string(),
            visit_count,
            last_visited,
        });
    }

    Ok(items)
}

/// List currently open Safari tabs.
pub async fn tabs() -> anyhow::Result<Vec<OpenTab>> {
    let script = r#"
const app = Application("Safari");
const results = [];
const wins = app.windows();
for (let w = 0; w < wins.length; w++) {
    const tabs = wins[w].tabs();
    for (let t = 0; t < tabs.length; t++) {
        let name = "", url = "";
        try { name = (tabs[t].name() || "").replace(/[\t\n\r]/g, " "); } catch(e) {}
        try { url = tabs[t].url() || ""; } catch(e) {}
        if (url) results.push([name, url, w + 1].join("\t"));
    }
}
results.join("\n")
"#;

    let output =
        super::util::run_jxa_with_timeout(script, std::time::Duration::from_secs(15)).await?;

    let mut tabs = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let title = parts[0].trim();
        let url = parts[1].trim();
        let window: i64 = parts
            .get(2)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

        if !url.is_empty() {
            tabs.push(OpenTab {
                title: title.to_string(),
                url: url.to_string(),
                window,
            });
        }
    }

    Ok(tabs)
}
