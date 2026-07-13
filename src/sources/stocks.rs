//! Apple Stocks, read-only. Two local stores feed this source:
//!
//! - Watchlists live in the app's CloudKit mirror
//!   (`group.com.apple.stocks/…/com.apple.stocks.private-production-dbstore.json`),
//!   a JSON file of base64 NSKeyedArchiver CKRecords. Each Watchlist record
//!   carries `name` and `symbols` as length-prefixed protobuf strings.
//! - Quotes live in the shared cache sqlite
//!   (`group.com.apple.stocks/Library/Caches/shared-database`), refreshed by
//!   the Stocks app/widget on its own schedule; `as_of` reports how fresh.
//!
//! Both are inside the group container, so Full Disk Access applies (same as
//! Reminders/Calendar).

use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Watchlist {
    pub name: String,
    pub symbols: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StockQuote {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<chrono::DateTime<chrono::Utc>>,
}

fn group_container() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/Library/Group Containers/group.com.apple.stocks")
}

/// Parse the CloudKit mirror for Watchlist records. The symbols array and
/// list name are length-prefixed strings inside CKEncrypted* blobs (tag 0x6a
/// for a symbols item, 0x32 for the name) — "encrypted" in name only.
pub async fn watchlists() -> anyhow::Result<Vec<Watchlist>> {
    let path = format!(
        "{}/Library/Documents/PrivateData/com.apple.stocks.private-production-dbstore.json",
        group_container()
    );
    if tokio::fs::metadata(&path).await.is_err() {
        anyhow::bail!(
            "Stocks data not found at {path} — open the Stocks app once, and make sure Full Disk Access is granted"
        );
    }

    let script = format!(
        r#"
import json, base64, plistlib, re

def parse_blob(blob):
    """Length-prefixed strings: 0x6a tags a symbols item, 0x32 the name."""
    name, syms = None, []
    i = 0
    while i + 1 < len(blob):
        tag, ln = blob[i], blob[i + 1]
        if tag not in (0x6A, 0x32) or i + 2 + ln > len(blob):
            break
        try:
            s = blob[i + 2 : i + 2 + ln].decode("utf-8")
        except UnicodeDecodeError:
            break
        if tag == 0x6A and re.fullmatch(r"[A-Z0-9.^=:-]{{1,15}}", s):
            syms.append(s)
        elif tag == 0x32:
            name = name or s
        i += 2 + ln
    return name, syms

out = []
d = json.load(open("{path}"))
for z in d.get("database", {{}}).get("zones", []):
    if z.get("name") != "Watchlist":
        continue
    for rec in z.get("serverRecords", []):
        try:
            pl = plistlib.loads(base64.b64decode(rec))
        except Exception:
            continue
        name, syms = None, []
        for o in pl.get("$objects", []):
            if isinstance(o, bytes):
                n, s = parse_blob(o)
                name = name or n
                syms.extend(s)
        if syms:
            out.append({{"name": name or "Watchlist", "symbols": syms}})
print(json.dumps(out))
"#
    );

    let output = run_command_with_timeout(
        "python3",
        &["-c", &script],
        std::time::Duration::from_secs(15),
    )
    .await?;

    let items: Vec<serde_json::Value> = serde_json::from_str(output.trim())?;
    Ok(items
        .iter()
        .filter_map(|w| {
            Some(Watchlist {
                name: w["name"].as_str()?.to_string(),
                symbols: w["symbols"]
                    .as_array()?
                    .iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect(),
            })
        })
        .collect())
}

/// Read cached quotes (+ display names) for the given symbols; empty slice
/// means every cached quote.
pub async fn quotes(symbols: &[String]) -> anyhow::Result<Vec<StockQuote>> {
    let db = format!("{}/Library/Caches/shared-database", group_container());
    if tokio::fs::metadata(&db).await.is_err() {
        anyhow::bail!(
            "Stocks quote cache not found at {db} — open the Stocks app once, and make sure Full Disk Access is granted"
        );
    }

    let filter = if symbols.is_empty() {
        String::new()
    } else {
        let quoted: Vec<String> = symbols
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        format!("WHERE q.id IN ({})", quoted.join(","))
    };
    // stock_metadata ids look like "VTI;en;US"; quotes ids are bare symbols.
    let sql = format!(
        r#"SELECT json_object(
            'symbol', q.id,
            'quote', json(q.valueJson),
            'meta', (SELECT json(m.valueJson) FROM stock_metadata m
                     WHERE m.id LIKE q.id || ';%' LIMIT 1)
        ) FROM quotes q {filter} ORDER BY q.id;"#
    );

    let output = run_command_with_timeout(
        "sqlite3",
        &["-readonly", &db, &sql],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let mut result = Vec::new();
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        let row: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        result.push(parse_quote_row(&row));
    }

    // Keep the caller's symbol order (watchlist order beats alphabetical).
    if !symbols.is_empty() {
        result.sort_by_key(|q| symbols.iter().position(|s| s == &q.symbol));
    }
    Ok(result)
}

fn parse_quote_row(row: &serde_json::Value) -> StockQuote {
    let v = &row["quote"]["v"];
    let price = v["price"].as_f64();
    let change = v["priceChange"].as_f64();
    let change_percent = match (price, change) {
        (Some(p), Some(c)) if (p - c).abs() > f64::EPSILON => {
            Some((c / (p - c) * 10000.0).round() / 100.0)
        }
        _ => None,
    };
    // dateLastRefreshed is seconds since the Apple epoch (2001-01-01).
    let as_of = v["dateLastRefreshed"]
        .as_f64()
        .and_then(|t| chrono::DateTime::from_timestamp(978_307_200 + t as i64, 0));
    StockQuote {
        symbol: row["symbol"].as_str().unwrap_or_default().to_string(),
        name: row["meta"]["v"]["stock"]["name"].as_str().map(String::from),
        price,
        change,
        change_percent,
        currency: v["currencyCode"].as_str().map(String::from),
        exchange_status: v["exchangeStatus"].as_str().map(String::from),
        as_of,
    }
}

/// Default listing: every watchlist symbol with its cached quote, in
/// watchlist order.
pub async fn fetch() -> anyhow::Result<Vec<StockQuote>> {
    let lists = watchlists().await?;
    let mut symbols: Vec<String> = Vec::new();
    for l in &lists {
        for s in &l.symbols {
            if !symbols.contains(s) {
                symbols.push(s.clone());
            }
        }
    }
    if symbols.is_empty() {
        anyhow::bail!("No watchlist symbols found — add symbols in the Stocks app first");
    }
    let mut result = quotes(&symbols).await?;
    // A symbol the cache hasn't seen yet still belongs in the listing.
    for s in &symbols {
        if !result.iter().any(|q| &q.symbol == s) {
            result.push(StockQuote {
                symbol: s.clone(),
                name: None,
                price: None,
                change: None,
                change_percent: None,
                currency: None,
                exchange_status: None,
                as_of: None,
            });
        }
    }
    result.sort_by_key(|q| symbols.iter().position(|s| s == &q.symbol));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quote_row() {
        let row: serde_json::Value = serde_json::from_str(
            r#"{"symbol":"SPY","quote":{"v":{"price":420.65,"priceChange":-6.78,
                "currencyCode":"USD","exchangeStatus":"open",
                "dateLastRefreshed":799956220.1}},
                "meta":{"v":{"stock":{"name":"SPDR S&P 500 ETF"}}}}"#,
        )
        .unwrap();
        let q = parse_quote_row(&row);
        assert_eq!(q.symbol, "SPY");
        assert_eq!(q.name.as_deref(), Some("SPDR S&P 500 ETF"));
        assert_eq!(q.price, Some(420.65));
        assert_eq!(q.change, Some(-6.78));
        // -6.78 / (420.65 + 6.78) ≈ -1.59%
        assert_eq!(q.change_percent, Some(-1.59));
        assert!(q.as_of.is_some());
        assert_eq!(q.exchange_status.as_deref(), Some("open"));
    }

    #[test]
    fn test_parse_quote_row_sparse() {
        let row: serde_json::Value =
            serde_json::from_str(r#"{"symbol":"X","quote":{"v":{}},"meta":null}"#).unwrap();
        let q = parse_quote_row(&row);
        assert_eq!(q.symbol, "X");
        assert!(q.price.is_none() && q.change_percent.is_none() && q.name.is_none());
    }
}
