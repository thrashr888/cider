use super::util::{run_command_with_timeout, slug, truncate_for_title, APPLE_EPOCH};
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
    ZASSETID,
    ZTITLE,
    ZAUTHOR,
    ZCONTENTTYPE,
    ZGENRE,
    ZREADINGPROGRESS,
    ZBOOKHIGHWATERMARKPROGRESS,
    ZISFINISHED,
    ZLASTOPENDATE,
    ZCREATIONDATE,
    ZFILESIZE,
    ZLANGUAGE,
    ZYEAR,
    ZBOOKDESCRIPTION,
    ZPATH
FROM ZBKLIBRARYASSET
WHERE ZTITLE IS NOT NULL AND ZTITLE != ''
ORDER BY ZLASTOPENDATE DESC
LIMIT 200;
"#;

    let stdout = run_command_with_timeout(
        "sqlite3",
        &["-separator", "\t", &db_path, query.trim()],
        std::time::Duration::from_secs(15),
    )
    .await?;

    Ok(parse_output(&stdout))
}

fn parse_output(output: &str) -> Vec<Book> {
    let mut records = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let asset_id = parts[0].trim();
        let title = parts[1].trim();
        if title.is_empty() {
            continue;
        }

        let author = parts.get(2).copied().unwrap_or("").trim();
        let content_type_num: i32 = parts
            .get(3)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let genre = parts.get(4).copied().unwrap_or("").trim();
        let reading_progress: f64 = parts
            .get(5)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let _high_water: f64 = parts
            .get(6)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let is_finished: bool = parts.get(7).map(|s| s.trim() == "1").unwrap_or(false);
        let last_open_ts: f64 = parts
            .get(8)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let creation_ts: f64 = parts
            .get(9)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let file_size: i64 = parts
            .get(10)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let language = parts.get(11).copied().unwrap_or("").trim();
        let year = parts.get(12).copied().unwrap_or("").trim();
        let description = parts.get(13).copied().unwrap_or("").trim();
        let path = parts.get(14).copied().unwrap_or("").trim();

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
            author: if author.is_empty() {
                None
            } else {
                Some(author.to_string())
            },
            content_type: content_type.to_string(),
            status: status.to_string(),
            reading_progress,
            genre: if genre.is_empty() {
                None
            } else {
                Some(genre.to_string())
            },
            description: if description.is_empty() {
                None
            } else {
                Some(truncate_for_title(description))
            },
            year: if year.is_empty() {
                None
            } else {
                Some(year.to_string())
            },
            language: if language.is_empty() {
                None
            } else {
                Some(language.to_string())
            },
            file_size,
            path: if path.is_empty() {
                None
            } else {
                Some(path.to_string())
            },
            last_opened,
        });
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "ASSET123\tThe Rust Programming Language\tSteve Klabnik\t1\tComputers & Technology\t0.45\t0.45\t0\t726000000\t725000000\t5242880\ten\t2023\tA great book about Rust\t\n\
                       ASSET456\tMy PDF Book\tJane Author\t3\t\t0.0\t0.0\t0\t0\t725000000\t1048576\t\t2024\t\t/Users/test/Library/Books/mypdf.pdf\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].title, "The Rust Programming Language");
        assert_eq!(records[0].content_type, "epub");
        assert_eq!(records[0].status, "reading");
        assert_eq!(records[0].reading_progress, 0.45);
        assert!(records[0].last_opened.is_some());
        assert_eq!(records[1].content_type, "pdf");
        assert_eq!(records[1].status, "unread");
        assert!(records[1].path.is_some());
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }

    #[test]
    fn test_parse_output_finished() {
        let output =
            "DONE1\tFinished Book\tAuthor\t1\t\t1.0\t1.0\t1\t726000000\t725000000\t0\t\t\t\t\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, "read");
    }
}
