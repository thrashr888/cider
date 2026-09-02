//! NSKeyedArchiver decoding in pure Rust.
//!
//! Apple's Home app (and plenty of other Cocoa code) persists object graphs
//! with `NSKeyedArchiver`: a binary plist whose `$objects` table is a flat
//! list of values, with every reference between them expressed as a
//! `plist::Value::Uid` index into that table. This module flattens such an
//! archive back into a plain `serde_json::Value` tree.
//!
//! Resolution is memoized by index and guarded against cycles: an object that
//! is already being resolved higher up the stack becomes a shallow stub —
//! its scalar fields plus a `"$ref": n` marker — instead of recursing. The
//! Home archive has parent back-pointers everywhere (room → home → rooms → …),
//! and without both the memo and the guard a traversal never finishes.
//!
//! Special Cocoa shapes are normalized as they are met:
//!
//! - `NS.keys` + `NS.objects` → JSON object (keys resolved to strings)
//! - `NS.objects` alone → JSON array
//! - `NS.string` → string
//! - `NS.time` / plist dates → `{"$date": seconds since 2001-01-01}`
//! - `NS.uuidbytes` → uppercase hyphenated UUID string
//! - `NS.data` / raw `Data` → `{"$data_len": n}` — bytes are never embedded,
//!   except that a `Data` blob which is itself a keyed archive (the Home
//!   cache's `kHomeDataKey`) is decoded recursively
//! - `$class` → its `$classname`, emitted as `"$class": "HMHome"`
//! - `$null` → `null`

use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use plist::{Dictionary, Value};
use serde_json::{json, Map, Value as Json};

use super::util::APPLE_EPOCH;

const BPLIST_MAGIC: &[u8] = b"bplist00";

/// Parse a binary plist file and decode it (see [`decode_value`]).
pub fn decode_file(path: &str) -> anyhow::Result<Json> {
    let value =
        Value::from_file(path).map_err(|e| anyhow::anyhow!("failed to parse plist {path}: {e}"))?;
    Ok(decode_value(&value))
}

/// Parse plist bytes and decode them (see [`decode_value`]).
pub fn decode_bytes(bytes: &[u8]) -> anyhow::Result<Json> {
    let value = Value::from_reader(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("failed to parse plist bytes: {e}"))?;
    Ok(decode_value(&value))
}

/// Decode a parsed plist. A keyed archive is resolved from its `$top` root;
/// anything else is converted structurally, still decoding any keyed archive
/// nested inside a `Data` value.
pub fn decode_value(value: &Value) -> Json {
    match archive_parts(value) {
        Some((objects, top)) => {
            let mut resolver = Resolver::new(objects);
            let root = resolver.convert(top);
            resolver.expand_refs(root)
        }
        None => Resolver::new(&[]).convert(value),
    }
}

/// True when the plist is an `NSKeyedArchiver` archive.
pub fn is_keyed_archive(value: &Value) -> bool {
    archive_parts(value).is_some()
}

fn archive_parts(value: &Value) -> Option<(&[Value], &Value)> {
    let dict = value.as_dictionary()?;
    if dict.get("$archiver").and_then(Value::as_string) != Some("NSKeyedArchiver") {
        return None;
    }
    let objects = dict.get("$objects")?.as_array()?;
    let top = dict.get("$top")?.as_dictionary()?;
    let root = top.get("root").or_else(|| top.values().next())?;
    Some((objects.as_slice(), root))
}

struct Resolver<'a> {
    objects: &'a [Value],
    memo: Vec<Option<Json>>,
    on_stack: Vec<bool>,
}

impl<'a> Resolver<'a> {
    fn new(objects: &'a [Value]) -> Self {
        Self {
            objects,
            memo: vec![None; objects.len()],
            on_stack: vec![false; objects.len()],
        }
    }

    fn resolve_uid(&mut self, index: usize) -> Json {
        if index >= self.objects.len() {
            return json!({ "$bad_uid": index });
        }
        if let Some(done) = &self.memo[index] {
            return done.clone();
        }
        if self.on_stack[index] {
            return json!({ "$ref": index });
        }
        self.on_stack[index] = true;
        let out = self.convert(&self.objects[index]);
        self.on_stack[index] = false;
        self.memo[index] = Some(out.clone());
        out
    }

    fn convert(&mut self, value: &Value) -> Json {
        match value {
            Value::Uid(uid) => self.resolve_uid(uid.get() as usize),
            Value::Dictionary(dict) => self.convert_dict(dict),
            Value::Array(items) => Json::Array(items.iter().map(|v| self.convert(v)).collect()),
            Value::String(s) if s == "$null" => Json::Null,
            Value::Data(bytes) => convert_data(bytes),
            other => plist_to_json(other, 0),
        }
    }

    fn class_name(&mut self, dict: &Dictionary) -> Option<String> {
        let class = self.convert(dict.get("$class")?);
        class
            .get("$classname")
            .and_then(Json::as_str)
            .map(str::to_string)
    }

    fn convert_dict(&mut self, dict: &Dictionary) -> Json {
        // The class descriptor itself: leave `$classname`/`$classes` intact so
        // `class_name` can read them.
        if dict.contains_key("$classname") {
            return plist_to_json(&Value::Dictionary(dict.clone()), 0);
        }

        if let Some(s) = dict.get("NS.string") {
            return self.convert(s);
        }
        if let Some(t) = dict.get("NS.time") {
            return json!({ "$date": self.convert(t) });
        }
        if let Some(bytes) = dict.get("NS.uuidbytes").and_then(Value::as_data) {
            return match uuid::Uuid::from_slice(bytes) {
                Ok(id) => Json::String(id.hyphenated().to_string().to_uppercase()),
                Err(_) => json!({ "$data_len": bytes.len() }),
            };
        }
        if let Some(d) = dict.get("NS.data") {
            return match d {
                Value::Data(bytes) => json!({ "$data_len": bytes.len() }),
                other => self.convert(other),
            };
        }

        let class = self.class_name(dict);

        if let (Some(keys), Some(values)) = (
            dict.get("NS.keys").and_then(Value::as_array),
            dict.get("NS.objects").and_then(Value::as_array),
        ) {
            let mut out = Map::new();
            if let Some(class) = class {
                out.insert("$class".into(), Json::String(class));
            }
            for (k, v) in keys.iter().zip(values) {
                let key = match self.convert(k) {
                    Json::String(s) => s,
                    other => other.to_string(),
                };
                let value = self.convert(v);
                out.insert(key, value);
            }
            return Json::Object(out);
        }
        if let Some(values) = dict.get("NS.objects").and_then(Value::as_array) {
            return Json::Array(values.iter().map(|v| self.convert(v)).collect());
        }

        let mut out = Map::new();
        if let Some(class) = class {
            out.insert("$class".into(), Json::String(class));
        }
        for (k, v) in dict {
            if k == "$class" {
                continue;
            }
            let value = self.convert(v);
            out.insert(k.clone(), value);
        }
        Json::Object(out)
    }

    /// Replace every `{"$ref": n}` cycle marker with a shallow stub of the
    /// object it points at: its scalar fields (name, UUID, dates, …) plus the
    /// marker itself. Callers get identity without the recursion.
    fn expand_refs(&self, value: Json) -> Json {
        match value {
            Json::Object(map) => {
                if map.len() == 1 {
                    if let Some(index) = map.get("$ref").and_then(Json::as_u64) {
                        return self.stub(index as usize);
                    }
                }
                Json::Object(
                    map.into_iter()
                        .map(|(k, v)| (k, self.expand_refs(v)))
                        .collect(),
                )
            }
            Json::Array(items) => {
                Json::Array(items.into_iter().map(|v| self.expand_refs(v)).collect())
            }
            other => other,
        }
    }

    fn stub(&self, index: usize) -> Json {
        let mut out = Map::new();
        out.insert("$ref".into(), json!(index));
        if let Some(Json::Object(target)) = self.memo.get(index).and_then(Option::as_ref) {
            for (k, v) in target {
                let scalar = match v {
                    Json::Array(_) => false,
                    Json::Object(inner) => inner.keys().all(|k| k.starts_with('$')),
                    _ => true,
                };
                if scalar {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        Json::Object(out)
    }
}

/// A raw `Data` blob: decoded recursively when it is itself a plist (the Home
/// cache nests a whole second archive this way), otherwise just its length.
fn convert_data(bytes: &[u8]) -> Json {
    if bytes.starts_with(BPLIST_MAGIC) {
        if let Ok(inner) = Value::from_reader(Cursor::new(bytes)) {
            return decode_value(&inner);
        }
    }
    json!({ "$data_len": bytes.len() })
}

/// Structural plist → JSON conversion, with no archive semantics.
///
/// `Data` becomes `{"$data_len": n}`, plus `"$hex"` when the blob is at most
/// `hex_limit` bytes (0 never embeds bytes). `Uid` becomes `{"$uid": n}` and
/// dates become `{"$date": seconds since 2001-01-01}`.
pub fn plist_to_json(value: &Value, hex_limit: usize) -> Json {
    match value {
        Value::Array(items) => {
            Json::Array(items.iter().map(|v| plist_to_json(v, hex_limit)).collect())
        }
        Value::Dictionary(dict) => Json::Object(
            dict.iter()
                .map(|(k, v)| (k.clone(), plist_to_json(v, hex_limit)))
                .collect(),
        ),
        Value::Boolean(b) => Json::Bool(*b),
        Value::Data(bytes) => {
            let mut out = Map::new();
            out.insert("$data_len".into(), json!(bytes.len()));
            if hex_limit > 0 && bytes.len() <= hex_limit {
                out.insert("$hex".into(), Json::String(hex_encode(bytes)));
            }
            Json::Object(out)
        }
        Value::Date(date) => {
            let unix = SystemTime::from(*date)
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            json!({ "$date": unix - APPLE_EPOCH as f64 })
        }
        Value::Real(f) => json!(f),
        Value::Integer(i) => match (i.as_signed(), i.as_unsigned()) {
            (Some(s), _) => json!(s),
            (None, Some(u)) => json!(u),
            (None, None) => Json::Null,
        },
        Value::String(s) => Json::String(s.clone()),
        Value::Uid(uid) => json!({ "$uid": uid.get() }),
        _ => Json::Null,
    }
}

/// Lowercase hex of a byte slice.
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode a hex string (either case, no separators) into bytes.
pub fn hex_decode(hex: &str) -> anyhow::Result<Vec<u8>> {
    let hex = hex.trim();
    if hex.len() % 2 != 0 {
        anyhow::bail!("odd-length hex string");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("bad hex at offset {i}: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::Uid;

    fn uid(n: u64) -> Value {
        Value::Uid(Uid::new(n))
    }

    fn dict(entries: Vec<(&str, Value)>) -> Value {
        let mut d = Dictionary::new();
        for (k, v) in entries {
            d.insert(k.to_string(), v);
        }
        Value::Dictionary(d)
    }

    /// A tiny archive: root is an `HMHome`-ish dict with a name, a uuid, a
    /// room, and the room points back at the home (a cycle).
    fn sample_archive() -> Value {
        let objects = vec![
            Value::String("$null".into()),
            // 1: home
            dict(vec![
                ("$class", uid(6)),
                ("homeName", uid(2)),
                ("homeUUID", uid(3)),
                ("rooms", uid(4)),
                ("when", uid(9)),
            ]),
            // 2: mutable string
            dict(vec![
                ("$class", uid(7)),
                ("NS.string", Value::String("Casa".into())),
            ]),
            // 3: uuid
            dict(vec![
                ("$class", uid(8)),
                (
                    "NS.uuidbytes",
                    Value::Data(vec![
                        0xcb, 0x31, 0x86, 0x5c, 0x3c, 0xae, 0x44, 0xe4, 0x87, 0xfe, 0xac, 0x8f,
                        0x9c, 0x9b, 0xd8, 0x1a,
                    ]),
                ),
            ]),
            // 4: NSArray of rooms
            dict(vec![("NS.objects", Value::Array(vec![uid(5)]))]),
            // 5: room, pointing back at the home
            dict(vec![
                ("$class", uid(10)),
                ("roomName", Value::String("Kitchen".into())),
                ("home", uid(1)),
            ]),
            dict(vec![("$classname", Value::String("HMHome".into()))]),
            dict(vec![(
                "$classname",
                Value::String("NSMutableString".into()),
            )]),
            dict(vec![("$classname", Value::String("NSUUID".into()))]),
            // 9: NSDate
            dict(vec![("NS.time", Value::Real(1.5))]),
            dict(vec![("$classname", Value::String("HMRoom".into()))]),
        ];
        let mut top = Dictionary::new();
        top.insert("root".into(), uid(1));
        dict(vec![
            ("$archiver", Value::String("NSKeyedArchiver".into())),
            ("$version", Value::Integer(100000.into())),
            ("$objects", Value::Array(objects)),
            ("$top", Value::Dictionary(top)),
        ])
    }

    #[test]
    fn decodes_archive_with_strings_uuids_dates_and_cycles() {
        let decoded = decode_value(&sample_archive());
        assert_eq!(decoded["$class"], "HMHome");
        assert_eq!(decoded["homeName"], "Casa");
        assert_eq!(decoded["homeUUID"], "CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A");
        assert_eq!(decoded["when"]["$date"], 1.5);
        let room = &decoded["rooms"][0];
        assert_eq!(room["$class"], "HMRoom");
        assert_eq!(room["roomName"], "Kitchen");
        // The back-pointer is a stub carrying the home's scalar fields.
        assert_eq!(room["home"]["$ref"], 1);
        assert_eq!(room["home"]["homeName"], "Casa");
        assert_eq!(
            room["home"]["homeUUID"],
            "CB31865C-3CAE-44E4-87FE-AC8F9C9BD81A"
        );
        assert!(room["home"].get("rooms").is_none());
    }

    #[test]
    fn nested_archive_in_data_is_decoded() {
        let mut inner = Vec::new();
        sample_archive().to_writer_binary(&mut inner).unwrap();
        assert!(inner.starts_with(BPLIST_MAGIC));

        let mut root = Dictionary::new();
        root.insert("kHomeDataKey".into(), Value::Data(inner));
        root.insert("blob".into(), Value::Data(vec![1, 2, 3]));
        let outer = Value::Dictionary(root);

        let decoded = decode_value(&outer);
        assert_eq!(decoded["kHomeDataKey"]["homeName"], "Casa");
        assert_eq!(decoded["blob"]["$data_len"], 3);
    }

    #[test]
    fn plain_plist_conversion_embeds_short_hex_only() {
        let value = dict(vec![
            ("short", Value::Data(vec![0xab, 0xcd])),
            ("long", Value::Data(vec![0; 300])),
            ("n", Value::Integer(7.into())),
            ("big", Value::Integer(u64::MAX.into())),
            ("f", Value::Real(2.5)),
            ("b", Value::Boolean(true)),
            ("s", Value::String("x".into())),
            ("u", uid(3)),
            ("list", Value::Array(vec![Value::String("a".into())])),
        ]);
        let json = plist_to_json(&value, 256);
        assert_eq!(json["short"]["$hex"], "abcd");
        assert_eq!(json["short"]["$data_len"], 2);
        assert!(json["long"].get("$hex").is_none());
        assert_eq!(json["long"]["$data_len"], 300);
        assert_eq!(json["n"], 7);
        assert_eq!(json["big"], u64::MAX);
        assert_eq!(json["f"], 2.5);
        assert_eq!(json["b"], true);
        assert_eq!(json["s"], "x");
        assert_eq!(json["u"]["$uid"], 3);
        assert_eq!(json["list"][0], "a");
        assert!(plist_to_json(&value, 0)["short"].get("$hex").is_none());
    }

    #[test]
    fn hex_round_trips() {
        let bytes = hex_decode("2210E8ae").unwrap();
        assert_eq!(bytes, vec![0x22, 0x10, 0xe8, 0xae]);
        assert_eq!(hex_encode(&bytes), "2210e8ae");
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
    }
}
