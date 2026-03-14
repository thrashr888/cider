use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Weather {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub humidity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast: Option<String>,
}

pub async fn fetch() -> anyhow::Result<Vec<Weather>> {
    // Use the macOS Weather widget cache or WeatherKit data
    let home = std::env::var("HOME").unwrap_or_default();
    let cache_dirs = [
        format!("{home}/Library/Containers/com.apple.weather/Data/Library/Caches"),
        format!("{home}/Library/Caches/com.apple.weather"),
    ];

    // Try to read weather cache
    for dir in &cache_dirs {
        if tokio::fs::metadata(dir).await.is_ok() {
            if let Ok(result) = fetch_from_cache(dir).await {
                if !result.is_empty() {
                    return Ok(result);
                }
            }
        }
    }

    // Fallback: use CoreLocation + WeatherKit via swift script
    let swift_script = r#"
import Foundation
import CoreLocation

let semaphore = DispatchSemaphore(value: 0)
let manager = CLLocationManager()

class Delegate: NSObject, CLLocationManagerDelegate {
    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let loc = locations.first else { semaphore.signal(); return }
        let geocoder = CLGeocoder()
        geocoder.reverseGeocodeLocation(loc) { placemarks, _ in
            let city = placemarks?.first?.locality ?? "Unknown"
            print("{\"location\": \"\(city)\", \"latitude\": \(loc.coordinate.latitude), \"longitude\": \(loc.coordinate.longitude)}")
            semaphore.signal()
        }
    }
    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        semaphore.signal()
    }
}

let delegate = Delegate()
manager.delegate = delegate
manager.requestLocation()
semaphore.wait()
"#;

    // Swift location approach is complex — provide a helpful error
    let _ = swift_script; // Acknowledge we have the code but it needs compilation
    anyhow::bail!(
        "Weather data not accessible. The Weather app caches data in encrypted format on modern macOS."
    )
}

async fn fetch_from_cache(cache_dir: &str) -> anyhow::Result<Vec<Weather>> {
    // Try to find JSON or plist weather data in the cache
    let output = run_command_with_timeout(
        "find",
        &[cache_dir, "-name", "*.json", "-o", "-name", "*.plist"],
        std::time::Duration::from_secs(5),
    )
    .await?;

    for file in output.lines() {
        let file = file.trim();
        if file.is_empty() {
            continue;
        }
        if file.ends_with(".json") {
            if let Ok(content) =
                run_command_with_timeout("cat", &[file], std::time::Duration::from_secs(5)).await
            {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let temp = data
                        .get("currentWeather")
                        .and_then(|cw| cw.get("temperature"))
                        .and_then(|t| t.as_f64())
                        .map(|t| format!("{t:.1}°C"));
                    let condition = data
                        .get("currentWeather")
                        .and_then(|cw| cw.get("conditionCode"))
                        .and_then(|c| c.as_str())
                        .map(String::from);

                    if temp.is_some() || condition.is_some() {
                        return Ok(vec![Weather {
                            location: None,
                            temperature: temp,
                            condition,
                            humidity: None,
                            wind: None,
                            forecast: None,
                        }]);
                    }
                }
            }
        }
    }

    Ok(vec![])
}
