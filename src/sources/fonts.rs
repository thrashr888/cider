use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Font {
    pub name: String,
    pub path: String,
    pub location: String,
}

pub async fn fetch() -> anyhow::Result<Vec<Font>> {
    let mut all = Vec::new();

    let dirs = [
        (
            "user",
            format!(
                "{}/Library/Fonts",
                std::env::var("HOME").unwrap_or_default()
            ),
        ),
        ("system", "/Library/Fonts".to_string()),
        ("core", "/System/Library/Fonts".to_string()),
    ];

    for (location, dir) in &dirs {
        match run_command_with_timeout("ls", &[dir], std::time::Duration::from_secs(5)).await {
            Ok(listing) => {
                for file in listing.lines() {
                    let file = file.trim();
                    if file.is_empty() {
                        continue;
                    }
                    let lower = file.to_lowercase();
                    if lower.ends_with(".ttf")
                        || lower.ends_with(".otf")
                        || lower.ends_with(".ttc")
                        || lower.ends_with(".dfont")
                        || lower.ends_with(".woff")
                        || lower.ends_with(".woff2")
                    {
                        let name = file
                            .rsplit('.')
                            .next_back()
                            .unwrap_or(file)
                            .replace(['-', '_'], " ");
                        all.push(Font {
                            name,
                            path: format!("{dir}/{file}"),
                            location: location.to_string(),
                        });
                    }
                }
            }
            Err(_) => continue,
        }
    }

    Ok(all)
}
