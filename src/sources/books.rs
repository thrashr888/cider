use super::util::{run_command_with_timeout, slug, APPLE_EPOCH};
use chrono::DateTime;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Book {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub content_type: String,
    pub status: String,
    pub reading_progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<DateTime<chrono::Utc>>,
}

pub async fn fetch() -> anyhow::Result<Vec<Book>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let container = format!("{home}/Library/Containers/com.apple.iBooksX/Data/Documents/BKLibrary");

    let listing =
        run_command_with_timeout("ls", &[&container], std::time::Duration::from_secs(5)).await?;
    let db_file = listing
        .lines()
        .find(|l| l.starts_with("BKLibrary") && l.ends_with(".sqlite"))
        .ok_or_else(|| anyhow::anyhow!("BKLibrary sqlite file not found"))?;

    let db_path = format!("{container}/{db_file}");

    let query = r#"
SELECT
    ZASSETID AS asset_id,
    ZTITLE AS title,
    ZAUTHOR AS author,
    ZCONTENTTYPE AS content_type,
    ZGENRE AS genre,
    ZREADINGPROGRESS AS reading_progress,
    ZISFINISHED AS is_finished,
    ZLASTOPENDATE AS last_open_ts,
    ZCREATIONDATE AS creation_ts,
    ZFILESIZE AS file_size,
    ZLANGUAGE AS language,
    ZYEAR AS year,
    ZBOOKDESCRIPTION AS description,
    ZPATH AS path
FROM ZBKLIBRARYASSET
WHERE ZTITLE IS NOT NULL AND ZTITLE != ''
ORDER BY ZLASTOPENDATE DESC
LIMIT 200;
"#;

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-json", &db_path, query.trim()],
        std::time::Duration::from_secs(15),
    )
    .await?;

    Ok(parse_json_rows(&stdout))
}

/// A JSON field that sqlite3 may emit as a number or a string, read as f64.
fn json_f64(value: &serde_json::Value) -> f64 {
    match value {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// An optional trimmed string field; empty and null both come back as None.
fn json_opt_str(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Parse `sqlite3 -json` output into books. JSON is the point: a
/// description is prose that routinely contains newlines, and the old
/// 15-column tab-separated read sheared any multiline blurb into broken
/// rows — plus the data layer capped descriptions at 120 chars (display
/// truncation belongs to `--pretty`, not here).
fn parse_json_rows(output: &str) -> Vec<Book> {
    let output = output.trim();
    if output.is_empty() {
        return Vec::new();
    }

    let rows: Vec<serde_json::Value> = match serde_json::from_str(output) {
        Ok(rows) => rows,
        Err(e) => {
            log::warn!("Skipping unparseable books output: {e}");
            return Vec::new();
        }
    };

    let mut records = Vec::new();

    for row in &rows {
        let asset_id = row["asset_id"].as_str().unwrap_or("").trim();
        let title = row["title"].as_str().unwrap_or("").trim();
        if title.is_empty() {
            continue;
        }

        let content_type_num = json_f64(&row["content_type"]) as i32;
        let reading_progress = json_f64(&row["reading_progress"]);
        let is_finished = json_f64(&row["is_finished"]) as i64 == 1;
        let last_open_ts = json_f64(&row["last_open_ts"]);
        let creation_ts = json_f64(&row["creation_ts"]);
        let file_size = json_f64(&row["file_size"]) as i64;

        let last_opened = if last_open_ts > 0.0 {
            DateTime::from_timestamp(last_open_ts as i64 + APPLE_EPOCH, 0)
        } else if creation_ts > 0.0 {
            DateTime::from_timestamp(creation_ts as i64 + APPLE_EPOCH, 0)
        } else {
            None
        };

        let content_type = match content_type_num {
            1 => "epub",
            3 => "pdf",
            6 => "audiobook",
            _ => "unknown",
        };

        let status = if is_finished {
            "read"
        } else if reading_progress > 0.0 {
            "reading"
        } else {
            "unread"
        };

        let id = if asset_id.is_empty() {
            slug(title)
        } else {
            slug(asset_id)
        };

        records.push(Book {
            id,
            title: title.to_string(),
            author: json_opt_str(&row["author"]),
            content_type: content_type.to_string(),
            status: status.to_string(),
            reading_progress,
            genre: json_opt_str(&row["genre"]),
            description: json_opt_str(&row["description"]),
            year: json_opt_str(&row["year"]),
            language: json_opt_str(&row["language"]),
            file_size,
            path: json_opt_str(&row["path"]),
            last_opened,
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
            {"asset_id":"ASSET123","title":"The Rust Programming Language","author":"Steve Klabnik","content_type":1,"genre":"Computers & Technology","reading_progress":0.45,"is_finished":0,"last_open_ts":726000000,"creation_ts":725000000,"file_size":5242880,"language":"en","year":"2023","description":"A great book about Rust","path":null},
            {"asset_id":"ASSET456","title":"My PDF Book","author":"Jane Author","content_type":3,"genre":null,"reading_progress":0.0,"is_finished":0,"last_open_ts":null,"creation_ts":725000000,"file_size":1048576,"language":null,"year":"2024","description":null,"path":"/Users/test/Library/Books/mypdf.pdf"}
        ]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "The Rust Programming Language");
        assert_eq!(records[0].content_type, "epub");
        assert_eq!(records[0].status, "reading");
        assert_eq!(records[0].reading_progress, 0.45);
        assert!(records[0].last_opened.is_some());
        assert_eq!(
            records[0].description.as_deref(),
            Some("A great book about Rust")
        );
        assert_eq!(records[1].content_type, "pdf");
        assert_eq!(records[1].status, "unread");
        assert!(records[1].path.is_some());
        assert!(records[1].description.is_none());
    }

    #[test]
    fn test_parse_json_rows_empty() {
        assert!(parse_json_rows("").is_empty());
        assert!(parse_json_rows("[]").is_empty());
    }

    #[test]
    fn test_parse_json_rows_finished() {
        let output = r#"[{"asset_id":"DONE1","title":"Finished Book","author":"Author","content_type":1,"genre":null,"reading_progress":1.0,"is_finished":1,"last_open_ts":726000000,"creation_ts":725000000,"file_size":0,"language":null,"year":null,"description":null,"path":null}]"#;
        let records = parse_json_rows(output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, "read");
    }

    /// The reason for `-json`: a multiline blurb used to shear one book into
    /// several broken TSV rows, and descriptions were capped at 120 chars in
    /// the data layer.
    #[test]
    fn test_parse_json_rows_multiline_description_survives() {
        let description = format!(
            "An epic tale.\n\nSecond paragraph\twith a tab.\n{}",
            "d".repeat(2000)
        );
        let output = serde_json::json!([{
            "asset_id": "LONG1",
            "title": "Long Blurb Book",
            "author": "Author",
            "content_type": 1,
            "genre": null,
            "reading_progress": 0.0,
            "is_finished": 0,
            "last_open_ts": null,
            "creation_ts": 725000000,
            "file_size": 0,
            "language": null,
            "year": null,
            "description": description,
            "path": null,
        }])
        .to_string();

        let records = parse_json_rows(&output);
        assert_eq!(records.len(), 1, "one book must parse as one record");
        let got = records[0].description.as_deref().unwrap();
        assert_eq!(
            got, description,
            "full description must survive untruncated"
        );
        assert!(got.contains('\n'), "newlines must survive");
        assert!(got.len() > 2000, "no 120-char cap");
    }
}
