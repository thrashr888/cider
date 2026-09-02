//! `cider home` over the Cider Bridge: live HomeKit reads and writes.
//!
//! [`super::home`] decodes the Home app's on-disk cache, which is as fresh as
//! the last time the app ran and knows nothing about characteristic values.
//! This module is the same surface backed by `HomeKit.framework` through the
//! bridge (`docs/RFC-swift-bridge.md`), plus what only HomeKit can do: live
//! state, running scenes, setting characteristics, and timer triggers that
//! fire on the home hub with the Mac asleep.

use serde::Serialize;
use serde_json::{json, Map, Value as Json};

use super::bridge::{self, Bridge, BridgeError};
use super::util::ActionResult;

/// Which backend answered a `cider home` read; `--envelope` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Bridge,
    Cache,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Bridge => "bridge",
            Source::Cache => "cache",
        }
    }
}

/// The bridge for a read subcommand: mandatory with `--live` (launching the
/// app if needed), otherwise only if it is installed and already answering.
pub async fn bridge_for(live: bool) -> Result<Option<Bridge>, BridgeError> {
    if live {
        return Bridge::connect().await.map(Some);
    }
    if !bridge::is_installed() {
        return Ok(None);
    }
    Ok(Bridge::connect_running().await.ok())
}

fn args(pairs: &[(&str, Option<&str>)]) -> Json {
    let mut map = Map::new();
    for (key, value) in pairs {
        if let Some(value) = value {
            map.insert((*key).to_string(), json!(value));
        }
    }
    Json::Object(map)
}

pub async fn homes(bridge: &mut Bridge) -> Result<Json, BridgeError> {
    bridge.call("home.homes", json!({})).await
}

pub async fn rooms(bridge: &mut Bridge, home: Option<&str>) -> Result<Json, BridgeError> {
    bridge.call("home.rooms", args(&[("home", home)])).await
}

pub async fn accessories(
    bridge: &mut Bridge,
    home: Option<&str>,
    room: Option<&str>,
) -> Result<Json, BridgeError> {
    bridge
        .call("home.accessories", args(&[("home", home), ("room", room)]))
        .await
}

pub async fn scenes(bridge: &mut Bridge, home: Option<&str>) -> Result<Json, BridgeError> {
    bridge.call("home.scenes", args(&[("home", home)])).await
}

/// Every home with rooms, accessories, and scenes nested, like the cache's
/// `list` but live. One `home.*` call per home per collection.
pub async fn list(bridge: &mut Bridge) -> Result<Json, BridgeError> {
    let homes = homes(bridge).await?;
    let mut out = Vec::new();
    for home in homes.as_array().into_iter().flatten() {
        let Some(fields) = home.as_object() else {
            continue;
        };
        let Some(selector) = fields
            .get("id")
            .or_else(|| fields.get("name"))
            .and_then(Json::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let mut nested = fields.clone();
        nested.insert("rooms".into(), rooms(bridge, Some(&selector)).await?);
        nested.insert(
            "accessories".into(),
            accessories(bridge, Some(&selector), None).await?,
        );
        nested.insert("scenes".into(), scenes(bridge, Some(&selector)).await?);
        out.push(Json::Object(nested));
    }
    Ok(Json::Array(out))
}

/// One characteristic of one service of one accessory, with its live value.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StateRow {
    pub accessory: String,
    pub accessory_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
    pub name: String,
    pub value: Json,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub writable: bool,
    pub readable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub characteristic_id: Option<String>,
}

/// Live characteristic values, one row each, optionally narrowed to one
/// accessory by name or UUID.
pub async fn state(
    bridge: &mut Bridge,
    home: Option<&str>,
    room: Option<&str>,
    accessory: Option<&str>,
) -> anyhow::Result<Vec<StateRow>> {
    let data = accessories(bridge, home, room).await?;
    let rows = flatten_state(&data, accessory);
    if rows.is_empty() {
        if let Some(wanted) = accessory {
            let known: Vec<&str> = data
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.get("name").and_then(Json::as_str))
                .collect();
            anyhow::bail!(
                "accessory '{wanted}' not found (known: {})",
                known.join(", ")
            );
        }
    }
    Ok(rows)
}

/// Flatten `home.accessories` data into [`StateRow`]s.
pub fn flatten_state(accessories: &Json, accessory: Option<&str>) -> Vec<StateRow> {
    let text = |value: &Json, key: &str| value.get(key).and_then(Json::as_str).map(str::to_string);
    let flag = |value: &Json, key: &str| value.get(key).and_then(Json::as_bool).unwrap_or(false);
    let wanted = |value: &Json| match accessory {
        None => true,
        Some(selector) => ["name", "id"].iter().any(|key| {
            value
                .get(key)
                .and_then(Json::as_str)
                .is_some_and(|v| v.eq_ignore_ascii_case(selector))
        }),
    };

    let mut rows = Vec::new();
    for item in accessories.as_array().into_iter().flatten() {
        if !wanted(item) {
            continue;
        }
        let accessory_name = text(item, "name").unwrap_or_default();
        let accessory_id = text(item, "id").unwrap_or_default();
        let room = text(item, "room");
        for service in item
            .get("services")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let service_name = text(service, "name").unwrap_or_default();
            let service_type = text(service, "type");
            for characteristic in service
                .get("characteristics")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                rows.push(StateRow {
                    accessory: accessory_name.clone(),
                    accessory_id: accessory_id.clone(),
                    room: room.clone(),
                    service: service_name.clone(),
                    service_type: service_type.clone(),
                    name: text(characteristic, "name")
                        .or_else(|| text(characteristic, "type"))
                        .unwrap_or_default(),
                    value: characteristic.get("value").cloned().unwrap_or(Json::Null),
                    unit: text(characteristic, "unit"),
                    writable: flag(characteristic, "writable"),
                    readable: flag(characteristic, "readable"),
                    characteristic_id: text(characteristic, "id"),
                });
            }
        }
    }
    rows
}

pub async fn run_scene(
    bridge: &mut Bridge,
    home: Option<&str>,
    scene: &str,
) -> Result<ActionResult, BridgeError> {
    bridge
        .call(
            "home.run_scene",
            args(&[("home", home), ("scene", Some(scene))]),
        )
        .await?;
    Ok(ActionResult::success_with_id("home.run", scene))
}

/// `--value` is JSON when it parses as JSON (`true`, `50`, `"warm"`) and a
/// plain string otherwise (`warm`), so scalars need no quoting.
pub fn parse_value(raw: &str) -> Json {
    serde_json::from_str(raw).unwrap_or_else(|_| Json::String(raw.to_string()))
}

/// Set one characteristic; the bridge echoes `{accessory, characteristic, value}`.
pub async fn set(
    bridge: &mut Bridge,
    home: Option<&str>,
    accessory: &str,
    characteristic: &str,
    value: Json,
    service: Option<&str>,
) -> Result<Json, BridgeError> {
    let mut request = args(&[
        ("home", home),
        ("accessory", Some(accessory)),
        ("characteristic", Some(characteristic)),
        ("service", service),
    ]);
    request["value"] = value;
    bridge.call("home.set", request).await
}

pub async fn triggers(bridge: &mut Bridge, home: Option<&str>) -> Result<Json, BridgeError> {
    bridge.call("home.triggers", args(&[("home", home)])).await
}

/// `--repeat`: `daily`, `weekly`, or `<minutes>m` (e.g. `90m`).
pub fn parse_repeat(raw: &str) -> anyhow::Result<Json> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "daily" => Ok(json!("daily")),
        "weekly" => Ok(json!("weekly")),
        other => {
            let minutes = other
                .strip_suffix('m')
                .and_then(|n| n.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --repeat {raw:?}: expected daily, weekly, or <minutes>m (e.g. 90m)"
                    )
                })?;
            Ok(json!({"minutes": minutes}))
        }
    }
}

/// `--at` must be RFC 3339 with an offset; HomeKit stores an instant, and
/// the bridge interprets the offset, so a bare local time is ambiguous.
pub fn validate_fire_at(raw: &str) -> anyhow::Result<()> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|_| ())
        .map_err(|error| {
            anyhow::anyhow!(
                "invalid --at {raw:?}: expected RFC 3339 like 2026-09-01T19:30:00-07:00 ({error})"
            )
        })
}

/// Create a timer trigger that runs `scenes`; returns the trigger row.
pub async fn create_timer(
    bridge: &mut Bridge,
    home: Option<&str>,
    name: &str,
    fire_at: &str,
    recurrence: Option<Json>,
    scenes: &[String],
) -> Result<Json, BridgeError> {
    let mut request = args(&[
        ("home", home),
        ("name", Some(name)),
        ("fire_at", Some(fire_at)),
    ]);
    if let Some(recurrence) = recurrence {
        request["recurrence"] = recurrence;
    }
    request["scenes"] = json!(scenes);
    bridge.call("home.trigger_create_timer", request).await
}

/// Enable or disable a trigger; returns the trigger row.
pub async fn set_trigger_enabled(
    bridge: &mut Bridge,
    home: Option<&str>,
    trigger: &str,
    enabled: bool,
) -> Result<Json, BridgeError> {
    let mut request = args(&[("home", home), ("trigger", Some(trigger))]);
    request["enabled"] = json!(enabled);
    bridge.call("home.trigger_set_enabled", request).await
}

pub async fn delete_trigger(
    bridge: &mut Bridge,
    home: Option<&str>,
    trigger: &str,
) -> Result<ActionResult, BridgeError> {
    bridge
        .call(
            "home.trigger_delete",
            args(&[("home", home), ("trigger", Some(trigger))]),
        )
        .await?;
    Ok(ActionResult::success_with_id(
        "home.triggers.delete",
        trigger,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accessories_fixture() -> Json {
        json!([
            {
                "id": "ACC-1", "name": "Desk Lamp", "room": "Office",
                "manufacturer": "Signify", "model": "LCA001", "reachable": true,
                "services": [{
                    "id": "SVC-1", "name": "Light", "type": "lightbulb",
                    "characteristics": [
                        {"id": "CH-1", "type": "power-state", "name": "Power", "value": true,
                         "writable": true, "readable": true},
                        {"id": "CH-2", "type": "brightness", "name": "Brightness", "value": 60,
                         "unit": "%", "writable": true, "readable": true}
                    ]
                }]
            },
            {
                "id": "ACC-2", "name": "Thermostat", "room": "Hall",
                "services": [{
                    "id": "SVC-2", "name": "Thermostat", "type": "thermostat",
                    "characteristics": [
                        {"id": "CH-3", "type": "current-temperature", "value": 21.5,
                         "unit": "celsius", "writable": false, "readable": true}
                    ]
                }]
            }
        ])
    }

    #[test]
    fn state_rows_are_one_per_characteristic() {
        let rows = flatten_state(&accessories_fixture(), None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].accessory, "Desk Lamp");
        assert_eq!(rows[0].room.as_deref(), Some("Office"));
        assert_eq!(rows[0].service, "Light");
        assert_eq!(rows[0].name, "Power");
        assert_eq!(rows[0].value, json!(true));
        assert!(rows[0].writable);
        assert_eq!(rows[1].unit.as_deref(), Some("%"));
        // A characteristic without a display name falls back to its type.
        assert_eq!(rows[2].name, "current-temperature");
        assert!(!rows[2].writable);
        assert_eq!(rows[2].value, json!(21.5));

        let json = serde_json::to_value(&rows[2]).unwrap();
        assert_eq!(json["accessory_id"], "ACC-2");
        assert_eq!(json["characteristic_id"], "CH-3");
        assert!(json.get("room").is_some());
    }

    #[test]
    fn state_filters_by_accessory_name_or_id_case_insensitively() {
        let by_name = flatten_state(&accessories_fixture(), Some("desk lamp"));
        assert_eq!(by_name.len(), 2);
        let by_id = flatten_state(&accessories_fixture(), Some("acc-2"));
        assert_eq!(by_id.len(), 1);
        assert!(flatten_state(&accessories_fixture(), Some("Nope")).is_empty());
        assert!(flatten_state(&json!({"not": "an array"}), None).is_empty());
    }

    #[test]
    fn set_values_parse_as_json_first_then_string() {
        assert_eq!(parse_value("true"), json!(true));
        assert_eq!(parse_value("50"), json!(50));
        assert_eq!(parse_value("21.5"), json!(21.5));
        assert_eq!(parse_value("\"warm\""), json!("warm"));
        assert_eq!(parse_value("warm"), json!("warm"));
        assert_eq!(parse_value("{\"h\":1}"), json!({"h": 1}));
    }

    #[test]
    fn repeat_accepts_daily_weekly_or_minutes() {
        assert_eq!(parse_repeat("daily").unwrap(), json!("daily"));
        assert_eq!(parse_repeat("Weekly").unwrap(), json!("weekly"));
        assert_eq!(parse_repeat("90m").unwrap(), json!({"minutes": 90}));
        assert!(parse_repeat("0m").is_err());
        assert!(parse_repeat("hourly").is_err());
        assert!(parse_repeat("90").is_err());
    }

    #[test]
    fn fire_at_must_be_rfc3339_with_offset() {
        assert!(validate_fire_at("2026-09-01T19:30:00-07:00").is_ok());
        assert!(validate_fire_at("2026-09-01T19:30:00Z").is_ok());
        assert!(validate_fire_at("2026-09-01 19:30").is_err());
        assert!(validate_fire_at("tomorrow").is_err());
    }

    #[test]
    fn args_drop_absent_selectors() {
        assert_eq!(
            args(&[("home", None), ("scene", Some("x"))]),
            json!({"scene": "x"})
        );
        assert_eq!(args(&[("home", None)]), json!({}));
    }
}
