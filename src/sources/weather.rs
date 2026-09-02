//! `cider weather`: WeatherKit through the Cider Bridge.
//!
//! The Weather app keeps an encrypted cache on modern macOS, so there is
//! nothing on disk to read. WeatherKit is the sanctioned source, and it only
//! loads in a signed app — the same Catalyst helper HomeKit needs
//! (`docs/RFC-swift-bridge.md`). Apple requires attribution for WeatherKit
//! data, so every result carries the bridge's `attribution` block verbatim;
//! keep it when you show the numbers to a person.
//!
//! Location comes from explicit `--lat/--lon`, else from a home's geocoded
//! address in the Home app cache (`--home`, or the primary home).

use serde::Serialize;
use serde_json::{json, Value as Json};

use super::bridge::{Bridge, BridgeError};
use super::home;

/// Where the forecast is for, and how cider decided that.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    /// `args` for `--lat/--lon`, `home:<name>` for a Home app home.
    pub source: String,
}

/// Explicit coordinates, else the named (or primary) home's location.
pub async fn resolve_location(
    lat: Option<f64>,
    lon: Option<f64>,
    home_selector: Option<&str>,
) -> anyhow::Result<Location> {
    match (lat, lon) {
        (Some(lat), Some(lon)) => return location_from_args(lat, lon),
        (None, None) => {}
        _ => anyhow::bail!("invalid location: --lat and --lon must be given together"),
    }
    let homes = home::list().await.map_err(|error| {
        anyhow::anyhow!(
            "invalid location: pass --lat and --lon, or --home <name> (the Home app cache could \
             not be read: {error})"
        )
    })?;
    // Name, cache id, or bridge id (through a running bridge): all resolve
    // to the cache home's name.
    let selector = match home_selector {
        Some(selector) => Some(super::home_live::resolve_cache_selector(&homes, selector).await?),
        None => None,
    };
    location_from_homes(&homes, selector.as_deref())
}

fn location_from_args(lat: f64, lon: f64) -> anyhow::Result<Location> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        anyhow::bail!("invalid location: latitude must be within ±90 and longitude within ±180");
    }
    Ok(Location {
        lat,
        lon,
        source: "args".to_string(),
    })
}

/// The selected home's location, or the primary home's (first home when
/// none is marked primary). Pure, so it is testable on a fixture.
pub fn location_from_homes(
    homes: &[home::Home],
    selector: Option<&str>,
) -> anyhow::Result<Location> {
    let chosen = match selector {
        Some(selector) => home::find_home(homes, selector)?,
        None => homes
            .iter()
            .find(|h| h.primary)
            .or_else(|| homes.first())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid location: no homes in the Home app cache; pass --lat and --lon"
                )
            })?,
    };
    let location = chosen.location.ok_or_else(|| {
        anyhow::anyhow!(
            "invalid location: home '{}' has no location in the Home app (set its address under \
             Home Settings, or pass --lat and --lon)",
            chosen.name
        )
    })?;
    Ok(Location {
        lat: location.latitude,
        lon: location.longitude,
        source: format!("home:{}", chosen.name),
    })
}

/// `weather.current` at `location`, plus `location` itself.
pub async fn current(bridge: &mut Bridge, location: &Location) -> Result<Json, BridgeError> {
    let data = bridge
        .call(
            "weather.current",
            json!({"lat": location.lat, "lon": location.lon}),
        )
        .await?;
    Ok(with_location(data, location))
}

/// `weather.forecast` at `location` for `days` days, plus `location`.
pub async fn forecast(
    bridge: &mut Bridge,
    location: &Location,
    days: Option<u32>,
) -> Result<Json, BridgeError> {
    let mut args = json!({"lat": location.lat, "lon": location.lon});
    if let Some(days) = days {
        args["days"] = json!(days);
    }
    let data = bridge.call("weather.forecast", args).await?;
    Ok(with_location(data, location))
}

/// The bridge's data verbatim with `location` added. Attribution comes from
/// the bridge; if it ever arrives without one the block is still present so
/// a consumer's "show the attribution" code has something to point at.
pub fn with_location(data: Json, location: &Location) -> Json {
    let mut out = match data {
        Json::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("data".into(), other);
            map
        }
    };
    out.insert("location".into(), json!(location));
    out.entry("attribution").or_insert_with(|| {
        json!({
            "service_name": "Apple Weather",
            "legal_url": "https://weatherkit.apple.com/legal-attribution.html"
        })
    });
    Json::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::home::{Home, HomeLocation};

    fn fixture(primary_located: bool) -> Vec<Home> {
        let make = |name: &str, primary: bool, location: Option<HomeLocation>| Home {
            id: format!("{}-ID", name.to_uppercase()),
            name: name.to_string(),
            primary,
            current: false,
            cache_updated_at: None,
            location,
            rooms: vec![],
            zones: vec![],
            accessories: vec![],
            scenes: vec![],
        };
        vec![
            make(
                "Cabin",
                false,
                Some(HomeLocation {
                    latitude: 39.0,
                    longitude: -120.0,
                }),
            ),
            make(
                "Casa",
                true,
                primary_located.then_some(HomeLocation {
                    latitude: 37.75,
                    longitude: -122.49,
                }),
            ),
        ]
    }

    #[test]
    fn primary_home_is_the_default_and_selector_matches_name_or_id() {
        let homes = fixture(true);
        let primary = location_from_homes(&homes, None).unwrap();
        assert_eq!(
            primary,
            Location {
                lat: 37.75,
                lon: -122.49,
                source: "home:Casa".into()
            }
        );
        let cabin = location_from_homes(&homes, Some("cabin")).unwrap();
        assert_eq!(cabin.source, "home:Cabin");
        assert_eq!(cabin.lat, 39.0);
        assert_eq!(
            location_from_homes(&homes, Some("CABIN-ID")).unwrap().lon,
            -120.0
        );
        let missing = location_from_homes(&homes, Some("Nope"))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("no home matches"), "{missing}");
    }

    #[test]
    fn homes_without_location_or_no_homes_are_invalid_input() {
        let error = location_from_homes(&fixture(false), None)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("invalid location"), "{error}");
        assert!(error.contains("Casa"), "{error}");
        assert!(error.contains("--lat"), "{error}");
        let none = location_from_homes(&[], None).unwrap_err().to_string();
        assert!(none.starts_with("invalid location"), "{none}");
    }

    #[tokio::test]
    async fn explicit_coordinates_win_and_are_range_checked() {
        let location = resolve_location(Some(1.5), Some(-2.5), Some("ignored"))
            .await
            .unwrap();
        assert_eq!(location.source, "args");
        assert_eq!((location.lat, location.lon), (1.5, -2.5));
        let half = resolve_location(Some(1.0), None, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            half.contains("--lat and --lon must be given together"),
            "{half}"
        );
        let range = resolve_location(Some(91.0), Some(0.0), None)
            .await
            .unwrap_err()
            .to_string();
        assert!(range.starts_with("invalid location"), "{range}");
    }

    #[test]
    fn output_keeps_bridge_data_and_adds_location_and_attribution() {
        let location = Location {
            lat: 37.75,
            lon: -122.49,
            source: "args".into(),
        };
        let data = json!({
            "temperature_c": 18.2, "condition": "Clear",
            "attribution": {"service_name": "Apple Weather", "legal_url": "https://x"}
        });
        let out = with_location(data, &location);
        assert_eq!(out["temperature_c"], 18.2);
        assert_eq!(out["location"]["lat"], 37.75);
        assert_eq!(out["location"]["source"], "args");
        assert_eq!(out["attribution"]["legal_url"], "https://x");

        let bare = with_location(json!({"days": []}), &location);
        assert_eq!(bare["attribution"]["service_name"], "Apple Weather");
        assert!(bare["attribution"]["legal_url"]
            .as_str()
            .unwrap()
            .contains("weatherkit.apple.com"));

        let scalar = with_location(json!(null), &location);
        assert_eq!(scalar["location"]["lon"], -122.49);
    }
}
