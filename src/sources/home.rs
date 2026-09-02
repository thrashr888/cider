//! The Home app, read from its on-disk HomeKit configuration cache.
//!
//! HomeKit has no AppleScript dictionary and no CLI, but the Home app keeps a
//! full `NSKeyedArchiver` snapshot of every home it knows about under its
//! container. That archive is what `cider home` decodes: homes, rooms, zones,
//! accessories with their services, and scenes (HomeKit "action sets").
//! Nothing here talks to HomeKit itself, so the data is as fresh as the last
//! time the Home app ran.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value as Json;

use super::keyed_archive;

const CACHE_DIR: &str = "Library/Containers/com.apple.Home/Data/Library/Caches/com.apple.HomeKit/com.apple.Home/com.apple.HomeKit.configurations";

#[derive(Debug, Clone, Serialize)]
pub struct Home {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub current: bool,
    pub rooms: Vec<Room>,
    pub zones: Vec<Zone>,
    pub accessories: Vec<Accessory>,
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Room {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Accessory {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matter: Option<bool>,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    /// `user` for scenes the user created, `builtin` for the stock Good
    /// Morning / Good Night set, `trigger` for scenes owned by an automation.
    pub kind: String,
    pub action_count: usize,
}

/// One row of `cider home homes`: a home with counts instead of children.
#[derive(Debug, Clone, Serialize)]
pub struct HomeSummary {
    pub id: String,
    pub name: String,
    pub primary: bool,
    pub current: bool,
    pub rooms: usize,
    pub zones: usize,
    pub accessories: usize,
    pub scenes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoomRow {
    pub home: String,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessoryRow {
    pub home: String,
    #[serde(flatten)]
    pub accessory: Accessory,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneRow {
    pub home: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub action_count: usize,
}

/// Every home, fully nested. This is what `cider home` prints.
pub async fn list() -> anyhow::Result<Vec<Home>> {
    let path = cache_path()?;
    let decoded = keyed_archive::decode_file(&path)?;
    let homes = map_cache(&decoded);
    if homes.is_empty() {
        anyhow::bail!("Home app cache decoded but contains no homes (path: {path})");
    }
    Ok(homes)
}

pub async fn homes() -> anyhow::Result<Vec<HomeSummary>> {
    Ok(list()
        .await?
        .iter()
        .map(|h| HomeSummary {
            id: h.id.clone(),
            name: h.name.clone(),
            primary: h.primary,
            current: h.current,
            rooms: h.rooms.len(),
            zones: h.zones.len(),
            accessories: h.accessories.len(),
            scenes: h.scenes.len(),
        })
        .collect())
}

pub async fn rooms(home: Option<&str>) -> anyhow::Result<Vec<RoomRow>> {
    Ok(select_homes(list().await?, home)?
        .into_iter()
        .flat_map(|h| {
            h.rooms.into_iter().map(move |r| RoomRow {
                home: h.name.clone(),
                id: r.id,
                name: r.name,
            })
        })
        .collect())
}

pub async fn accessories(
    home: Option<&str>,
    room: Option<&str>,
) -> anyhow::Result<Vec<AccessoryRow>> {
    Ok(select_homes(list().await?, home)?
        .into_iter()
        .flat_map(|h| {
            h.accessories
                .into_iter()
                .filter(|a| match room {
                    Some(wanted) => {
                        matches_selector(a.room.as_deref(), wanted)
                            || matches_selector(a.room_id.as_deref(), wanted)
                    }
                    None => true,
                })
                .map(move |accessory| AccessoryRow {
                    home: h.name.clone(),
                    accessory,
                })
        })
        .collect())
}

pub async fn scenes(home: Option<&str>) -> anyhow::Result<Vec<SceneRow>> {
    Ok(select_homes(list().await?, home)?
        .into_iter()
        .flat_map(|h| {
            h.scenes.into_iter().map(move |s| SceneRow {
                home: h.name.clone(),
                id: s.id,
                name: s.name,
                kind: s.kind,
                action_count: s.action_count,
            })
        })
        .collect())
}

/// Find a home by name or UUID (case-insensitive).
pub fn find_home<'a>(homes: &'a [Home], selector: &str) -> anyhow::Result<&'a Home> {
    homes
        .iter()
        .find(|h| {
            matches_selector(Some(&h.name), selector) || matches_selector(Some(&h.id), selector)
        })
        .ok_or_else(|| {
            let known: Vec<&str> = homes.iter().map(|h| h.name.as_str()).collect();
            anyhow::anyhow!("no home matches '{selector}' (known: {})", known.join(", "))
        })
}

/// Find a scene in a home by name or UUID (case-insensitive).
pub fn find_scene<'a>(home: &'a Home, selector: &str) -> anyhow::Result<&'a Scene> {
    home.scenes
        .iter()
        .find(|s| {
            matches_selector(Some(&s.name), selector) || matches_selector(Some(&s.id), selector)
        })
        .ok_or_else(|| {
            let known: Vec<&str> = home.scenes.iter().map(|s| s.name.as_str()).collect();
            anyhow::anyhow!(
                "no scene matches '{selector}' in home '{}' (known: {})",
                home.name,
                known.join(", ")
            )
        })
}

fn matches_selector(value: Option<&str>, selector: &str) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case(selector))
}

fn select_homes(homes: Vec<Home>, selector: Option<&str>) -> anyhow::Result<Vec<Home>> {
    match selector {
        Some(selector) => {
            let wanted = find_home(&homes, selector)?.id.clone();
            Ok(homes.into_iter().filter(|h| h.id == wanted).collect())
        }
        None => Ok(homes),
    }
}

/// The newest `homeData.*.config` in the Home app's cache directory.
fn cache_path() -> anyhow::Result<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = format!("{home}/{CACHE_DIR}");
    let missing = || {
        anyhow::anyhow!(
            "Home app cache not found; open the Home app once (path: {dir}/homeData.*.config)"
        )
    };
    let entries = std::fs::read_dir(&dir).map_err(|_| missing())?;
    let mut candidates: Vec<(std::time::SystemTime, String)> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("homeData.") && name.ends_with(".config")
        })
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, e.path().to_string_lossy().into_owned())
        })
        .collect();
    candidates.sort();
    candidates.pop().map(|(_, path)| path).ok_or_else(missing)
}

/// Map the decoded outer archive (the file's root dict) to homes.
pub fn map_cache(outer: &Json) -> Vec<Home> {
    let primary = outer.get("kPrimaryHomeUUIDKey").and_then(Json::as_str);
    let current = outer.get("kCurrentHomeUUIDKey").and_then(Json::as_str);
    let homes = match outer.get("kHomeDataKey") {
        Some(Json::Array(items)) => items.as_slice(),
        _ => &[],
    };
    homes
        .iter()
        .filter_map(|h| map_home(h, primary, current))
        .collect()
}

fn map_home(value: &Json, primary: Option<&str>, current: Option<&str>) -> Option<Home> {
    let id = string(value, "homeUUID")?;
    let name = string(value, "homeName").unwrap_or_default();

    let rooms: Vec<Room> = array(value, "rooms")
        .iter()
        .filter_map(|r| {
            Some(Room {
                id: string(r, "roomUUID")?,
                name: string(r, "roomName").unwrap_or_default(),
            })
        })
        .collect();

    let mut room_names: HashMap<String, String> = rooms
        .iter()
        .map(|r| (r.id.clone(), r.name.clone()))
        .collect();
    if let Some(whole) = value.get("roomForEntireHome") {
        if let Some(id) = string(whole, "roomUUID") {
            room_names
                .entry(id)
                .or_insert_with(|| string(whole, "roomName").unwrap_or_default());
        }
    }

    let zones = array(value, "zones")
        .iter()
        .filter_map(|z| {
            Some(Zone {
                id: string(z, "zoneUUID")?,
                name: string(z, "zoneName").unwrap_or_default(),
            })
        })
        .collect();

    let accessories = array(value, "accessories")
        .iter()
        .filter_map(|a| map_accessory(a, &room_names))
        .collect();

    let mut scenes = Vec::new();
    for (key, kind) in [
        ("actionSets", "user"),
        ("builtinActionSets", "builtin"),
        ("HM.triggerOwnedActionSets", "trigger"),
    ] {
        scenes.extend(array(value, key).iter().filter_map(|s| map_scene(s, kind)));
    }

    Some(Home {
        primary: primary.is_some_and(|p| p.eq_ignore_ascii_case(&id)),
        current: current.is_some_and(|c| c.eq_ignore_ascii_case(&id)),
        id,
        name,
        rooms,
        zones,
        accessories,
        scenes,
    })
}

fn map_accessory(value: &Json, room_names: &HashMap<String, String>) -> Option<Accessory> {
    let id = string(value, "accessoryUUID")?;
    let name = string(value, "accessoryConfiguredName")
        .or_else(|| string(value, "accessoryName"))
        .unwrap_or_default();

    let room_ref = value.get("accessoryRoom");
    let room_id = room_ref.and_then(|r| string(r, "roomUUID"));
    let room = room_id
        .as_ref()
        .and_then(|id| room_names.get(id).cloned())
        .or_else(|| room_ref.and_then(|r| string(r, "roomName")));

    let services = array(value, "services")
        .iter()
        .filter_map(|s| {
            Some(Service {
                id: string(s, "serviceUUID")?,
                name: string(s, "serviceConfiguredName")
                    .or_else(|| string(s, "serviceName"))
                    .unwrap_or_default(),
                service_type: string(s, "serviceType"),
            })
        })
        .collect();

    Some(Accessory {
        id,
        name,
        room,
        room_id,
        manufacturer: string(value, "HM.manufacturer"),
        model: string(value, "HM.model"),
        category: value
            .get("HM.accessoryCategory")
            .and_then(|c| string(c, "HM.accessoryCategoryName")),
        reachable: boolean(value, "reachable"),
        bridged: boolean(value, "isBridged"),
        matter: boolean(value, "HMA.supportsCHIP"),
        services,
    })
}

fn map_scene(value: &Json, kind: &str) -> Option<Scene> {
    Some(Scene {
        id: string(value, "actionSetUUID")?,
        name: string(value, "actionSetName").unwrap_or_default(),
        kind: kind.to_string(),
        action_count: array(value, "actionSetActions").len(),
    })
}

fn string(value: &Json, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn boolean(value: &Json, key: &str) -> Option<bool> {
    match value.get(key)? {
        Json::Bool(b) => Some(*b),
        Json::Number(n) => Some(n.as_f64().unwrap_or(0.0) != 0.0),
        _ => None,
    }
}

fn array<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value
        .get(key)
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_cache() -> Json {
        json!({
            "kPrimaryHomeUUIDKey": "HOME-1",
            "kCurrentHomeUUIDKey": "HOME-2",
            "kHomeDataKey": [
                {
                    "$class": "HMHome",
                    "homeName": "Casa",
                    "homeUUID": "HOME-1",
                    "rooms": [
                        {"$class": "HMRoom", "roomName": "Kitchen", "roomUUID": "ROOM-K"},
                        {"$class": "HMRoom", "roomName": "Bedroom", "roomUUID": "ROOM-B"}
                    ],
                    "roomForEntireHome": {"roomName": "Default Room", "roomUUID": "ROOM-D"},
                    "zones": [{"zoneName": "Upstairs", "zoneUUID": "ZONE-1"}],
                    "accessories": [
                        {
                            "accessoryName": "Hue Bulb",
                            "accessoryConfiguredName": "Counter Light",
                            "accessoryUUID": "ACC-1",
                            // A cycle stub: only scalar fields survived.
                            "accessoryRoom": {"$ref": 12, "roomUUID": "ROOM-K", "roomName": "Kitchen"},
                            "HM.manufacturer": "Signify",
                            "HM.model": "LCA001",
                            "reachable": true,
                            "isBridged": 1,
                            "HMA.supportsCHIP": false,
                            "HM.accessoryCategory": {"HM.accessoryCategoryName": "Lightbulb"},
                            "services": [
                                {"serviceName": "Light", "serviceUUID": "SVC-1",
                                 "serviceType": "00000043-0000-1000-8000-0026BB765291", "HM.primary": true}
                            ]
                        },
                        {
                            "accessoryName": "Hub",
                            "accessoryUUID": "ACC-2",
                            "accessoryRoom": {"roomUUID": "ROOM-D"},
                            "services": []
                        },
                        {
                            "accessoryName": "Orphan",
                            "accessoryUUID": "ACC-3",
                            "accessoryRoom": {"$ref": 99, "roomUUID": "ROOM-X", "roomName": "Garage"}
                        }
                    ],
                    "actionSets": [
                        {"actionSetName": "Movie Time", "actionSetUUID": "SCENE-U",
                         "actionSetType": "HMActionSetTypeUserDefined", "actionSetActions": [{}, {}, {}]}
                    ],
                    "builtinActionSets": [
                        {"actionSetName": "Good Night", "actionSetUUID": "SCENE-B",
                         "actionSetType": "HMActionSetTypeSleep", "actionSetActions": [{}]}
                    ],
                    "HM.triggerOwnedActionSets": [
                        {"actionSetName": "Trigger 1", "actionSetUUID": "SCENE-T"}
                    ]
                },
                {
                    "$class": "HMHome",
                    "homeName": "Cabin",
                    "homeUUID": "home-2"
                },
                {"$class": "HMHome", "homeName": "no uuid, dropped"}
            ]
        })
    }

    #[test]
    fn maps_homes_rooms_accessories_and_room_membership() {
        let homes = map_cache(&sample_cache());
        assert_eq!(homes.len(), 2);

        let casa = &homes[0];
        assert_eq!(casa.id, "HOME-1");
        assert_eq!(casa.name, "Casa");
        assert!(casa.primary);
        assert!(!casa.current);
        assert_eq!(casa.rooms.len(), 2);
        assert_eq!(casa.rooms[1].name, "Bedroom");
        assert_eq!(casa.zones[0].name, "Upstairs");

        let light = &casa.accessories[0];
        assert_eq!(light.id, "ACC-1");
        assert_eq!(light.name, "Counter Light");
        assert_eq!(light.room.as_deref(), Some("Kitchen"));
        assert_eq!(light.room_id.as_deref(), Some("ROOM-K"));
        assert_eq!(light.manufacturer.as_deref(), Some("Signify"));
        assert_eq!(light.category.as_deref(), Some("Lightbulb"));
        assert_eq!(light.reachable, Some(true));
        assert_eq!(light.bridged, Some(true));
        assert_eq!(light.matter, Some(false));
        assert_eq!(light.services.len(), 1);
        assert_eq!(
            light.services[0].service_type.as_deref(),
            Some("00000043-0000-1000-8000-0026BB765291")
        );

        let hub = &casa.accessories[1];
        assert_eq!(hub.room.as_deref(), Some("Default Room"));
        assert_eq!(hub.room_id.as_deref(), Some("ROOM-D"));
        assert!(hub.reachable.is_none());

        // Unknown room UUID falls back to the stub's own name.
        let orphan = &casa.accessories[2];
        assert_eq!(orphan.room.as_deref(), Some("Garage"));

        let cabin = &homes[1];
        assert_eq!(cabin.name, "Cabin");
        assert!(cabin.current, "UUID match is case-insensitive");
        assert!(!cabin.primary);
        assert!(cabin.accessories.is_empty());
    }

    #[test]
    fn scene_kind_follows_source_list() {
        let homes = map_cache(&sample_cache());
        let scenes = &homes[0].scenes;
        let by_name: HashMap<&str, &Scene> = scenes.iter().map(|s| (s.name.as_str(), s)).collect();
        assert_eq!(by_name["Movie Time"].kind, "user");
        assert_eq!(by_name["Movie Time"].action_count, 3);
        assert_eq!(by_name["Good Night"].kind, "builtin");
        assert_eq!(by_name["Good Night"].action_count, 1);
        assert_eq!(by_name["Trigger 1"].kind, "trigger");
        assert_eq!(by_name["Trigger 1"].action_count, 0);
    }

    #[test]
    fn selectors_match_name_or_id_case_insensitively() {
        let homes = map_cache(&sample_cache());
        assert_eq!(find_home(&homes, "casa").unwrap().id, "HOME-1");
        assert_eq!(find_home(&homes, "HOME-2").unwrap().name, "Cabin");
        assert!(find_home(&homes, "Nope").is_err());
        let casa = find_home(&homes, "Casa").unwrap();
        assert_eq!(find_scene(casa, "good night").unwrap().id, "SCENE-B");
        assert_eq!(find_scene(casa, "scene-u").unwrap().name, "Movie Time");
        assert!(find_scene(casa, "Missing").is_err());
    }

    #[test]
    fn empty_cache_maps_to_no_homes() {
        assert!(map_cache(&json!({})).is_empty());
        assert!(map_cache(&json!({"kHomeDataKey": {"not": "an array"}})).is_empty());
    }
}
