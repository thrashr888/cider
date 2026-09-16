//! Lossless protobuf wire fields without inventing schemas for unknown streams.
use crate::sources::keyed_archive::{decode_bytes, hex_encode};
use anyhow::{ensure, Context};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Field {
    pub number: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'static str>,
    pub wire_type: u8,
    pub value: Value,
}
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Unsigned(u64),
    Fixed64Hex(String),
    Fixed32Hex(String),
    Text(String),
    BytesHex(String),
    Plist(serde_json::Value),
}
fn varint(b: &[u8], pos: &mut usize) -> anyhow::Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *b.get(*pos).context("Truncated protobuf varint")?;
        *pos += 1;
        ensure!(shift < 63 || byte <= 1, "Protobuf varint overflow");
        value |= ((byte & 127) as u64) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("Protobuf varint overflow")
}
fn take<'a>(b: &'a [u8], pos: &mut usize, len: usize) -> anyhow::Result<&'a [u8]> {
    let end = pos.checked_add(len).context("Protobuf length overflow")?;
    let value = b.get(*pos..end).context("Truncated protobuf field")?;
    *pos = end;
    Ok(value)
}
pub(super) fn parse(stream: &str, b: &[u8]) -> anyhow::Result<Vec<Field>> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        ensure!(fields.len() < 4096, "Protobuf payload exceeds 4096 fields");
        let key = varint(b, &mut pos)?;
        ensure!(
            key >> 3 > 0 && key >> 3 <= 0x1fffffff,
            "Invalid protobuf field number"
        );
        let number = (key >> 3) as u32;
        let wire_type = (key & 7) as u8;
        let value = match wire_type {
            0 => Value::Unsigned(varint(b, &mut pos)?),
            1 => Value::Fixed64Hex(hex_encode(take(b, &mut pos, 8)?)),
            2 => {
                let len =
                    usize::try_from(varint(b, &mut pos)?).context("Protobuf length overflow")?;
                let bytes = take(b, &mut pos, len)?;
                if bytes.starts_with(b"bplist00") {
                    match decode_bytes(bytes) {
                        Ok(value) => Value::Plist(value),
                        Err(_) => Value::BytesHex(hex_encode(bytes)),
                    }
                } else {
                    match std::str::from_utf8(bytes) {
                        Ok(s)
                            if s.chars()
                                .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')) =>
                        {
                            Value::Text(s.to_owned())
                        }
                        _ => Value::BytesHex(hex_encode(bytes)),
                    }
                }
            }
            5 => Value::Fixed32Hex(hex_encode(take(b, &mut pos, 4)?)),
            _ => anyhow::bail!("Unsupported protobuf wire type {wire_type}"),
        };
        fields.push(Field {
            number,
            name: field_name(stream, number, wire_type),
            wire_type,
            value,
        });
    }
    Ok(fields)
}
// Names from mac_apt's Biome parser; unknown fields retain their wire numbers.
fn field_name(stream: &str, number: u32, wire: u8) -> Option<&'static str> {
    match (stream, number, wire) {
        ("App.InFocus", 3, 0)
        | ("App.WebUsage", 3, 0)
        | ("ScreenTime.AppUsage", 1, 0)
        | ("Device.Wireless.WiFi", 2, 0)
        | ("Device.Wireless.Bluetooth", 4, 0) => Some("status"),
        ("App.InFocus", 6, 2)
        | ("ScreenTime.AppUsage", 3, 2)
        | ("App.WebUsage", 6, 2)
        | ("Notification.Usage", 4, 2) => Some("app"),
        ("App.InFocus", 9, 2) => Some("app_version"),
        ("App.InFocus", 10, 2) => Some("app_build"),
        ("App.WebUsage", 4, 2) => Some("url"),
        ("App.WebUsage", 5, 2) => Some("domain"),
        ("Device.Wireless.WiFi", 1, 2) => Some("ssid"),
        ("Device.Wireless.Bluetooth", 1, 2) => Some("address"),
        ("Device.Wireless.Bluetooth", 2, 2) => Some("product_name"),
        ("Device.Wireless.Bluetooth", 3, 0) => Some("product_id"),
        ("Notification.Usage", 8, 2) => Some("title"),
        ("Notification.Usage", 9, 2) => Some("subtitle"),
        ("SystemSettings.SearchTerms", 1, 2) => Some("search_term"),
        _ => None,
    }
}
pub(super) fn app(fields: &[Field]) -> Option<String> {
    fields.iter().find_map(|f| match (&f.name, &f.value) {
        (Some("app"), Value::Text(s)) => Some(s.clone()),
        _ => None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_embedded_binary_plists_without_guessing_other_binary_fields() {
        let mut dict = plist::Dictionary::new();
        dict.insert("name".into(), plist::Value::String("document".into()));
        let mut payload = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut payload)
            .unwrap();
        assert!(payload.len() < 128);
        let mut wire = vec![0x0a, payload.len() as u8];
        wire.extend(payload);
        let fields = parse("App.DocumentInteraction", &wire).unwrap();
        assert!(matches!(&fields[0].value,Value::Plist(v) if v["name"]=="document"));
    }

    #[test]
    fn keeps_repeated_fields_unknown_bytes_and_unsigned_values() {
        let fields = parse(
            "Device.Wireless.WiFi",
            b"\x0a\x03abc\x10\x01\x10\x00\x1a\x02\xff\x00",
        )
        .unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name, Some("ssid"));
        assert_eq!(fields[2].number, 2);
        assert!(matches!(fields[3].value, Value::BytesHex(_)));
        for invalid in [
            &b"\0"[..],
            &b"\x08\x80"[..],
            &b"\x0a\xff\xff\xff\xff\xff\xff\xff\xff\xff\x7f"[..],
            &b"\x0a\x05ab"[..],
        ] {
            assert!(parse("x", invalid).is_err());
        }
    }
}
