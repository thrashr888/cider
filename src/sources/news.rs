use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SavedArticle {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<SavedArticle>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let container = format!("{home}/Library/Containers/com.apple.news");

    if tokio::fs::metadata(&container).await.is_err() {
        anyhow::bail!("Apple News data not found");
    }

    // Find any SQLite databases
    let listing = run_command_with_timeout(
        "find",
        &[&container, "-name", "*.sqlite", "-o", "-name", "*.db"],
        std::time::Duration::from_secs(5),
    )
    .await?;

    let mut articles = Vec::new();

    for db_path in listing.lines() {
        let db_path = db_path.trim();
        if db_path.is_empty() {
            continue;
        }

        // Try to find saved/bookmarked articles
        // The schema varies by macOS version
        let queries = [
            "SELECT ZTITLE, ZPUBLISHERNAME, ZURL FROM ZFEEDARTICLE WHERE ZSAVED = 1 ORDER BY ZDATE DESC LIMIT 100;",
            "SELECT ZTITLE, ZSOURCENAME, ZURL FROM ZARTICLE WHERE ZSAVED = 1 ORDER BY ZCREATIONDATE DESC LIMIT 100;",
            "SELECT ZTITLE, ZPUBLISHER, ZURL FROM ZNEWSARTICLE LIMIT 100;",
        ];

        for query in &queries {
            if let Ok(output) = run_command_with_timeout(
                "sqlite3",
                &["-separator", "\t", db_path, query],
                std::time::Duration::from_secs(5),
            )
            .await
            {
                for line in output.lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    let title = parts.first().copied().unwrap_or("").trim();
                    if title.is_empty() {
                        continue;
                    }
                    let source = parts.get(1).copied().unwrap_or("").trim();
                    let url = parts.get(2).copied().unwrap_or("").trim();

                    articles.push(SavedArticle {
                        title: title.to_string(),
                        source: if source.is_empty() {
                            None
                        } else {
                            Some(source.to_string())
                        },
                        url: if url.is_empty() {
                            None
                        } else {
                            Some(url.to_string())
                        },
                    });
                }
                if !articles.is_empty() {
                    return Ok(articles);
                }
            }
        }
    }

    if articles.is_empty() {
        anyhow::bail!("No saved articles found in Apple News");
    }

    Ok(articles)
}
