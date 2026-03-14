use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StockQuote {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<StockQuote>> {
    // Apple Stocks stores watchlist in a plist in its container.
    let home = std::env::var("HOME").unwrap_or_default();
    let watchlist_path = format!(
        "{home}/Library/Containers/com.apple.stocks/Data/Library/Preferences/com.apple.stocks.plist"
    );

    if tokio::fs::metadata(&watchlist_path).await.is_err() {
        // Try Group Containers
        let alt = format!(
            "{home}/Library/Group Containers/group.com.apple.stocks/Library/Preferences/group.com.apple.stocks.plist"
        );
        if tokio::fs::metadata(&alt).await.is_ok() {
            return fetch_from_plist(&alt).await;
        }
        anyhow::bail!("Stocks watchlist not found. Make sure the Stocks app has been used.");
    }

    fetch_from_plist(&watchlist_path).await
}

async fn fetch_from_plist(path: &str) -> anyhow::Result<Vec<StockQuote>> {
    let script = format!(
        r#"
import plistlib, json
with open("{path}", "rb") as f:
    data = plistlib.load(f)

symbols = []
def extract(obj, depth=0):
    if depth > 5:
        return
    if isinstance(obj, dict):
        sym = obj.get("symbol", obj.get("Symbol", obj.get("ticker", "")))
        name = obj.get("shortName", obj.get("name", obj.get("companyName", "")))
        if isinstance(sym, str) and sym.strip():
            entry = {{"symbol": sym.strip()}}
            if isinstance(name, str) and name.strip():
                entry["name"] = name.strip()
            symbols.append(entry)
        for v in obj.values():
            extract(v, depth + 1)
    elif isinstance(obj, list):
        for item in obj:
            extract(item, depth + 1)

extract(data)
# Deduplicate by symbol
seen = set()
unique = []
for s in symbols:
    if s["symbol"] not in seen:
        seen.add(s["symbol"])
        unique.append(s)
print(json.dumps(unique))
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
            let symbol = item["symbol"].as_str()?;
            if symbol.is_empty() {
                return None;
            }
            Some(StockQuote {
                symbol: symbol.to_string(),
                name: item["name"].as_str().map(String::from),
            })
        })
        .collect())
}
