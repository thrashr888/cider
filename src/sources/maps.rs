use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MapBookmark {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

pub async fn fetch() -> anyhow::Result<Vec<MapBookmark>> {
    // Maps bookmarks are stored in a binary format that's hard to parse directly.
    // Use Python to attempt reading the GeoBookmarks plist.
    let home = std::env::var("HOME").unwrap_or_default();
    let bookmarks_path =
        format!("{home}/Library/Containers/com.apple.Maps/Data/Library/Maps/Bookmarks.plist");

    if tokio::fs::metadata(&bookmarks_path).await.is_ok() {
        return fetch_from_plist(&bookmarks_path).await;
    }

    // Try alternate path
    let alt_path = format!("{home}/Library/Maps/Bookmarks.plist");
    if tokio::fs::metadata(&alt_path).await.is_ok() {
        return fetch_from_plist(&alt_path).await;
    }

    anyhow::bail!("Maps bookmarks not found")
}

async fn fetch_from_plist(plist_path: &str) -> anyhow::Result<Vec<MapBookmark>> {
    let script = format!(
        r#"
import plistlib, json, sys
try:
    with open("{plist_path}", "rb") as f:
        data = plistlib.load(f)

    bookmarks = []
    def extract(obj, depth=0):
        if depth > 5:
            return
        if isinstance(obj, dict):
            name = obj.get("Name", obj.get("name", obj.get("title", "")))
            if isinstance(name, str) and name.strip():
                lat = obj.get("Latitude", obj.get("latitude"))
                lon = obj.get("Longitude", obj.get("longitude"))
                addr = obj.get("Address", obj.get("address", ""))
                entry = {{"name": name.strip()}}
                if isinstance(addr, str) and addr.strip():
                    entry["address"] = addr.strip()
                if isinstance(lat, (int, float)):
                    entry["latitude"] = lat
                if isinstance(lon, (int, float)):
                    entry["longitude"] = lon
                bookmarks.append(entry)
            for v in obj.values():
                extract(v, depth + 1)
        elif isinstance(obj, list):
            for item in obj:
                extract(item, depth + 1)

    extract(data)
    print(json.dumps(bookmarks))
except Exception as e:
    print("[]")
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
            let name = item["name"].as_str()?;
            if name.is_empty() {
                return None;
            }
            Some(MapBookmark {
                name: name.to_string(),
                address: item["address"].as_str().map(String::from),
                latitude: item["latitude"].as_f64(),
                longitude: item["longitude"].as_f64(),
            })
        })
        .collect())
}
