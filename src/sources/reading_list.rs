use super::util::{parse_plist_date, run_command_with_timeout, slug};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReadingListItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_added: Option<DateTime<chrono::Utc>>,
}

pub async fn fetch() -> anyhow::Result<Vec<ReadingListItem>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let plist_path = format!("{home}/Library/Safari/Bookmarks.plist");

    if tokio::fs::metadata(&plist_path).await.is_err() {
        anyhow::bail!("Safari Bookmarks.plist not accessible at {plist_path}");
    }

    let json_result = run_command_with_timeout(
        "plutil",
        &["-convert", "json", "-o", "-", &plist_path],
        std::time::Duration::from_secs(15),
    )
    .await;

    if let Ok(json_str) = json_result {
        if let Ok(plist) = serde_json::from_str::<serde_json::Value>(&json_str) {
            return Ok(parse_plist(&plist));
        }
    }

    eprintln!("plutil JSON conversion failed, trying Python plistlib fallback");

    let python_script = format!(
        r#"
import plistlib, json, sys
with open("{plist_path}", "rb") as f:
    data = plistlib.load(f)

def convert(obj):
    if isinstance(obj, bytes):
        return None
    if isinstance(obj, dict):
        return {{k: convert(v) for k, v in obj.items() if convert(v) is not None}}
    if isinstance(obj, list):
        return [convert(i) for i in obj if convert(i) is not None]
    if hasattr(obj, 'isoformat'):
        return obj.isoformat()
    return obj

print(json.dumps(convert(data)))
"#
    );

    let stdout = run_command_with_timeout(
        "python3",
        &["-c", &python_script],
        std::time::Duration::from_secs(15),
    )
    .await?;

    let plist: serde_json::Value = serde_json::from_str(&stdout)?;
    Ok(parse_plist(&plist))
}

fn parse_plist(plist: &serde_json::Value) -> Vec<ReadingListItem> {
    let mut records = Vec::new();

    let children = match plist.get("Children").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return records,
    };

    let reading_list_node = children.iter().find(|child| {
        child
            .get("Title")
            .and_then(|t| t.as_str())
            .map(|t| t == "com.apple.ReadingList")
            .unwrap_or(false)
    });

    let items = match reading_list_node
        .and_then(|n| n.get("Children"))
        .and_then(|c| c.as_array())
    {
        Some(items) => items,
        None => return records,
    };

    for item in items {
        let url = item
            .get("URLString")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .trim();
        if url.is_empty() {
            continue;
        }

        let title = item
            .get("URIDictionary")
            .and_then(|d| d.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or(url)
            .trim();

        let reading_list = item.get("ReadingList");

        let preview = reading_list
            .and_then(|rl| rl.get("PreviewText"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim();

        let date_added = reading_list
            .and_then(|rl| rl.get("DateAdded"))
            .and_then(|d| d.as_str())
            .and_then(parse_plist_date);

        let uuid = item
            .get("WebBookmarkUUID")
            .and_then(|u| u.as_str())
            .unwrap_or("");

        let id = if uuid.is_empty() {
            slug(url)
        } else {
            slug(uuid)
        };

        let domain = url
            .split("://")
            .nth(1)
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or("")
            .trim_start_matches("www.");

        records.push(ReadingListItem {
            id,
            title: title.to_string(),
            url: url.to_string(),
            domain: domain.to_string(),
            preview: if preview.is_empty() {
                None
            } else {
                Some(preview.to_string())
            },
            date_added,
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plist() {
        let plist: serde_json::Value = serde_json::json!({
            "Children": [
                {
                    "Title": "BookmarksBar",
                    "Children": []
                },
                {
                    "Title": "com.apple.ReadingList",
                    "Children": [
                        {
                            "URLString": "https://example.com/article",
                            "URIDictionary": { "title": "Great Article" },
                            "ReadingList": {
                                "DateAdded": "2024-06-15T10:30:00Z",
                                "PreviewText": "This is a preview."
                            },
                            "WebBookmarkUUID": "ABC-123-DEF"
                        },
                        {
                            "URLString": "https://blog.example.com/post",
                            "URIDictionary": { "title": "Blog Post" },
                            "ReadingList": { "DateAdded": "2024-07-01T14:00:00Z" },
                            "WebBookmarkUUID": "GHI-456-JKL"
                        }
                    ]
                }
            ]
        });

        let records = parse_plist(&plist);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "Great Article");
        assert_eq!(records[0].url, "https://example.com/article");
        assert_eq!(records[0].preview.as_deref(), Some("This is a preview."));
        assert!(records[0].date_added.is_some());
        assert_eq!(records[0].domain, "example.com");
        assert_eq!(records[1].title, "Blog Post");
        assert!(records[1].preview.is_none());
        assert_eq!(records[1].domain, "blog.example.com");
    }

    #[test]
    fn test_parse_plist_empty() {
        let plist: serde_json::Value = serde_json::json!({
            "Children": [{
                "Title": "com.apple.ReadingList",
                "Children": []
            }]
        });
        assert!(parse_plist(&plist).is_empty());
    }

    #[test]
    fn test_parse_plist_no_reading_list() {
        let plist: serde_json::Value = serde_json::json!({
            "Children": [{ "Title": "BookmarksBar", "Children": [] }]
        });
        assert!(parse_plist(&plist).is_empty());
    }

    #[test]
    fn test_parse_plist_skips_empty_url() {
        let plist: serde_json::Value = serde_json::json!({
            "Children": [{
                "Title": "com.apple.ReadingList",
                "Children": [{
                    "URLString": "",
                    "URIDictionary": { "title": "No URL" },
                    "ReadingList": {}
                }]
            }]
        });
        assert!(parse_plist(&plist).is_empty());
    }
}
