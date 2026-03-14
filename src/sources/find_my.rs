use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FindMyDevice {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_level: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

pub async fn fetch() -> anyhow::Result<Vec<FindMyDevice>> {
    // Find My caches device data in JSON files.
    let home = std::env::var("HOME").unwrap_or_default();
    let cache_dir = format!("{home}/Library/Caches/com.apple.findmy.fmipcore/Items.data");

    if tokio::fs::metadata(&cache_dir).await.is_ok() {
        return fetch_from_cache(&cache_dir).await;
    }

    // Try alternate cache paths
    let alt = format!("{home}/Library/Caches/com.apple.findmy/Items.data");
    if tokio::fs::metadata(&alt).await.is_ok() {
        return fetch_from_cache(&alt).await;
    }

    anyhow::bail!(
        "Find My device cache not found. Make sure Find My is enabled and has been opened recently."
    )
}

async fn fetch_from_cache(path: &str) -> anyhow::Result<Vec<FindMyDevice>> {
    let script = format!(
        r#"
import json, sys
try:
    with open("{path}", "r") as f:
        data = json.load(f)
    devices = []
    items = data if isinstance(data, list) else data.get("items", data.get("devices", []))
    for item in items:
        if not isinstance(item, dict):
            continue
        name = item.get("name", item.get("deviceDisplayName", ""))
        if not name:
            continue
        dev = {{"name": name}}
        model = item.get("deviceModel", item.get("modelDisplayName", ""))
        if model:
            dev["model"] = model
        battery = item.get("batteryLevel")
        if battery is not None:
            dev["battery_level"] = battery
        bstatus = item.get("batteryStatus", "")
        if bstatus:
            dev["battery_status"] = bstatus
        loc = item.get("location", {{}})
        if isinstance(loc, dict):
            lat = loc.get("latitude")
            lon = loc.get("longitude")
            if lat is not None:
                dev["latitude"] = lat
            if lon is not None:
                dev["longitude"] = lon
        devices.append(dev)
    print(json.dumps(devices))
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
            Some(FindMyDevice {
                name: name.to_string(),
                model: item["model"].as_str().map(String::from),
                battery_level: item["battery_level"].as_f64(),
                battery_status: item["battery_status"].as_str().map(String::from),
                latitude: item["latitude"].as_f64(),
                longitude: item["longitude"].as_f64(),
            })
        })
        .collect())
}
