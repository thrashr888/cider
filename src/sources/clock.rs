use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ClockInfo {
    pub local_time: String,
    pub timezone: String,
    pub utc_offset: String,
    pub world_clocks: Vec<WorldClock>,
    pub alarms: Vec<Alarm>,
}

#[derive(Debug, Serialize)]
pub struct WorldClock {
    pub city: String,
    pub timezone: String,
    pub time: String,
}

#[derive(Debug, Serialize)]
pub struct Alarm {
    pub time: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<ClockInfo>> {
    let timeout = std::time::Duration::from_secs(5);

    // Get local time info
    let local_time = run_command_with_timeout("date", &["+%Y-%m-%d %H:%M:%S"], timeout)
        .await
        .unwrap_or_default();
    let timezone = run_command_with_timeout("date", &["+%Z"], timeout)
        .await
        .unwrap_or_default();
    let utc_offset = run_command_with_timeout("date", &["+%z"], timeout)
        .await
        .unwrap_or_default();

    // Read world clocks from Clock app preferences
    let home = std::env::var("HOME").unwrap_or_default();
    let world_clocks = fetch_world_clocks(&home).await;
    let alarms = fetch_alarms(&home).await;

    Ok(vec![ClockInfo {
        local_time: local_time.trim().to_string(),
        timezone: timezone.trim().to_string(),
        utc_offset: utc_offset.trim().to_string(),
        world_clocks,
        alarms,
    }])
}

async fn fetch_world_clocks(home: &str) -> Vec<WorldClock> {
    let plist_path = format!(
        "{home}/Library/Containers/com.apple.clock/Data/Library/Preferences/com.apple.clock.plist"
    );

    let script = format!(
        r#"
import plistlib, json, subprocess, sys
from datetime import datetime
import zoneinfo

clocks = []
try:
    with open("{plist_path}", "rb") as f:
        data = plistlib.load(f)

    cities = data.get("WorldClockCities", data.get("selectedCities", []))
    for city in cities:
        if isinstance(city, dict):
            name = city.get("name", city.get("city", ""))
            tz = city.get("timezone", city.get("timeZone", ""))
            if name and tz:
                try:
                    zi = zoneinfo.ZoneInfo(tz)
                    now = datetime.now(zi)
                    clocks.append({{"city": name, "timezone": tz, "time": now.strftime("%H:%M")}})
                except Exception:
                    clocks.append({{"city": name, "timezone": tz, "time": ""}})
except Exception:
    pass
print(json.dumps(clocks))
"#
    );

    if let Ok(output) = super::util::run_command_with_timeout(
        "python3",
        &["-c", &script],
        std::time::Duration::from_secs(5),
    )
    .await
    {
        if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&output) {
            return items
                .iter()
                .filter_map(|item| {
                    Some(WorldClock {
                        city: item["city"].as_str()?.to_string(),
                        timezone: item["timezone"].as_str().unwrap_or("").to_string(),
                        time: item["time"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect();
        }
    }

    vec![]
}

async fn fetch_alarms(home: &str) -> Vec<Alarm> {
    // Alarms are synced from iPhone and stored in the Clock container
    let plist_path = format!(
        "{home}/Library/Containers/com.apple.clock/Data/Library/Preferences/com.apple.clock.plist"
    );

    let script = format!(
        r#"
import plistlib, json
alarms = []
try:
    with open("{plist_path}", "rb") as f:
        data = plistlib.load(f)
    alarm_list = data.get("alarms", data.get("Alarms", []))
    for a in alarm_list:
        if isinstance(a, dict):
            hour = a.get("hour", 0)
            minute = a.get("minute", 0)
            enabled = a.get("enabled", a.get("isEnabled", True))
            label = a.get("title", a.get("label", ""))
            entry = {{"time": f"{{hour:02d}}:{{minute:02d}}", "enabled": bool(enabled)}}
            if label:
                entry["label"] = str(label)
            alarms.append(entry)
except Exception:
    pass
print(json.dumps(alarms))
"#
    );

    if let Ok(output) = run_command_with_timeout(
        "python3",
        &["-c", &script],
        std::time::Duration::from_secs(5),
    )
    .await
    {
        if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&output) {
            return items
                .iter()
                .filter_map(|item| {
                    Some(Alarm {
                        time: item["time"].as_str()?.to_string(),
                        enabled: item["enabled"].as_bool().unwrap_or(true),
                        label: item["label"].as_str().map(String::from),
                    })
                })
                .collect();
        }
    }

    vec![]
}
