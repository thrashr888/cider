//! Human-friendly table/key-value renderer for serde_json::Value.
//!
//! - Array of objects → columnar table with headers
//! - Single object → aligned key: value pairs
//! - ActionResult-like → single status line
//! - Scalars → plain text

use std::io::Write;

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";
const MAX_COL_WIDTH: usize = 50;
const MAX_COLS: usize = 8;

#[cfg(test)]
mod history_tests {
    #[test]
    fn renders_all_sources_and_keeps_json_control_characters_unchanged() {
        let value = serde_json::json!({"app":"com.test","title":"a\nb\u{001b}","body":"hello","timestamp":"now","payload":{"format":"opaque","reason":"bad"}});
        let before = value.clone();
        for source in ["notifications", "downloads", "interactions", "biome"] {
            let mut out = Vec::new();
            super::render_history(&mut out, source, std::slice::from_ref(&value)).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("com.test"));
            assert!(text.contains("1 items"));
            assert!(!text.contains("a\nb"));
        }
        assert_eq!(value, before);
    }
}

#[cfg(test)]
mod knowledge_tests {
    #[test]
    fn knowledge_table_keeps_columns_for_values_absent_from_first_event() {
        let events: Vec<crate::sources::knowledge::KnowledgeEvent> =
            serde_json::from_value(serde_json::json!([
                {"id": "a", "stream": "/app/usage", "value_string": "com.example.app"},
                {"id": "b", "stream": "/display/isBacklit", "value_integer": 1}
            ]))
            .unwrap();
        let mut output = Vec::new();
        super::render_knowledge(&mut output, &events).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("VALUE INTEGER"));
        assert!(output.contains("com.example.app"));
        assert!(output.contains("/display/isBacklit"));
        assert!(output.contains("2 items"));
    }
}

/// Show the stream and scalar values even when the first event has null fields.
pub fn render_knowledge<W: Write>(
    w: W,
    events: &[crate::sources::knowledge::KnowledgeEvent],
) -> anyhow::Result<()> {
    let items = events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    render_table_with_columns(
        w,
        &items,
        &[
            "start_date",
            "stream",
            "duration_seconds",
            "value_string",
            "value_integer",
            "value_double",
        ],
    )
}

/// Stable column selection for heterogeneous retained-history records.
pub fn render_history<W: Write>(
    w: W,
    source: &str,
    items: &[serde_json::Value],
) -> anyhow::Result<()> {
    let columns: &[&str] = match source {
        "notifications" => &["delivered_at", "app", "title", "body", "decode_error"],
        "downloads" => &["timestamp", "app", "url", "origin_url"],
        "interactions" => &[
            "start_date",
            "app",
            "sender",
            "recipients",
            "direction_code",
        ],
        "biome" => &[
            "timestamp",
            "stream",
            "app",
            "status_code",
            "crc_valid",
            "payload",
        ],
        _ => anyhow::bail!("Unknown history presentation: {source}"),
    };
    // Stored notification text can contain line breaks or terminal control codes.
    // Normalize only this presentation copy; machine-readable JSON is untouched.
    fn clean(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::String(s) => {
                *s = s
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect()
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(clean),
            serde_json::Value::Object(o) => o.values_mut().for_each(clean),
            _ => {}
        }
    }
    let mut display = items.to_vec();
    display.iter_mut().for_each(clean);
    render_table_with_columns(w, &display, columns)
}

pub fn render<W: Write>(mut w: W, value: &serde_json::Value) -> anyhow::Result<()> {
    match value {
        serde_json::Value::Array(arr) if arr.is_empty() => {
            writeln!(w, "{DIM}(no results){RESET}")?;
        }
        serde_json::Value::Array(arr)
            if arr.len() == 1
                && arr[0].is_object()
                && !is_action_result(arr[0].as_object().unwrap()) =>
        {
            render_object(&mut w, arr[0].as_object().unwrap())?;
        }
        serde_json::Value::Array(arr) if arr.iter().all(|v| v.is_object()) => {
            render_table(&mut w, arr)?;
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                writeln!(w, "  {}", format_scalar(item))?;
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.get("action").and_then(|v| v.as_str()) == Some("fetch")
                && obj.get("page").is_some_and(serde_json::Value::is_object)
            {
                render_object(&mut w, obj["page"].as_object().unwrap())?;
            } else if obj.get("action").and_then(|v| v.as_str()) == Some("request")
                && obj.contains_key("body")
            {
                render_object(&mut w, obj)?;
            } else if is_action_result(obj) {
                render_action_result(&mut w, obj)?;
            } else {
                render_object(&mut w, obj)?;
            }
        }
        other => {
            writeln!(w, "{}", format_scalar(other))?;
        }
    }
    Ok(())
}

fn render_table<W: Write>(w: &mut W, items: &[serde_json::Value]) -> anyhow::Result<()> {
    // Collect column names from first item, limited to MAX_COLS
    let first = items[0].as_object().unwrap();
    let columns: Vec<&str> = first.keys().map(String::as_str).take(MAX_COLS).collect();
    render_table_columns(w, items, &columns)
}

/// A table of `items` (objects) with exactly these columns, in this order.
/// `render` derives columns from the first item, which serde_json keeps
/// sorted; a caller with a meaningful order names it here.
pub fn render_table_with_columns<W: Write>(
    mut w: W,
    items: &[serde_json::Value],
    columns: &[&str],
) -> anyhow::Result<()> {
    if items.is_empty() {
        writeln!(w, "{DIM}(no results){RESET}")?;
        return Ok(());
    }
    render_table_columns(&mut w, items, columns)
}

fn render_table_columns<W: Write>(
    w: &mut W,
    items: &[serde_json::Value],
    columns: &[&str],
) -> anyhow::Result<()> {
    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for item in items {
        if let Some(obj) = item.as_object() {
            for (i, col) in columns.iter().enumerate() {
                let val = obj.get(*col).map(format_cell).unwrap_or_default();
                widths[i] = widths[i].max(val.len()).min(MAX_COL_WIDTH);
            }
        }
    }

    // Header
    let header: String = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let label = col.to_uppercase().replace('_', " ");
            format!("{label:<width$}", width = widths[i])
        })
        .collect::<Vec<_>>()
        .join("  ");
    writeln!(w, "{BOLD}{header}{RESET}")?;

    // Separator
    let sep: String = widths
        .iter()
        .map(|w| "─".repeat(*w))
        .collect::<Vec<_>>()
        .join("──");
    writeln!(w, "{DIM}{sep}{RESET}")?;

    // Rows
    for item in items {
        if let Some(obj) = item.as_object() {
            let row: String = columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let val = obj.get(*col).map(format_cell).unwrap_or_default();
                    let truncated = truncate(&val, widths[i]);
                    format!("{truncated:<width$}", width = widths[i])
                })
                .collect::<Vec<_>>()
                .join("  ");
            writeln!(w, "{row}")?;
        }
    }

    // Footer
    writeln!(w, "{DIM}{} items{RESET}", items.len())?;
    Ok(())
}

fn render_object<W: Write>(
    w: &mut W,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    let max_key_len = obj.keys().map(|k| k.len()).max().unwrap_or(0);

    for (key, value) in obj {
        let label = key.replace('_', " ");
        let val_str = match value {
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    format!("{DIM}(none){RESET}")
                } else if arr.iter().all(|v| v.is_string() || v.is_number()) {
                    arr.iter().map(format_scalar).collect::<Vec<_>>().join(", ")
                } else {
                    format!("[{} items]", arr.len())
                }
            }
            serde_json::Value::Object(inner) => {
                // Nested object — render inline
                let parts: Vec<String> = inner
                    .iter()
                    .take(5)
                    .map(|(k, v)| format!("{k}: {}", format_scalar(v)))
                    .collect();
                parts.join(", ")
            }
            other => format_scalar(other),
        };

        writeln!(
            w,
            "{BOLD}{label:>width$}{RESET}  {val_str}",
            width = max_key_len
        )?;
    }
    Ok(())
}

fn render_action_result<W: Write>(
    w: &mut W,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    let ok = obj.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("done");
    let icon = if ok {
        format!("{GREEN}✓{RESET}")
    } else {
        format!("{YELLOW}✗{RESET}")
    };

    write!(w, "{icon} {BOLD}{action}{RESET}")?;

    if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
        if !id.is_empty() {
            write!(w, " {DIM}({id}){RESET}")?;
        }
    }
    if let Some(msg) = obj.get("message").and_then(|v| v.as_str()) {
        if !msg.is_empty() {
            write!(w, " — {msg}")?;
        }
    } else if let Some(requested) = obj.get("requested").and_then(|v| v.as_u64()) {
        let succeeded = obj.get("succeeded").and_then(|v| v.as_u64()).unwrap_or(0);
        let failed = obj.get("failed").and_then(|v| v.as_u64()).unwrap_or(0);
        write!(w, " — {succeeded}/{requested} succeeded")?;
        if failed > 0 {
            write!(w, ", {failed} failed")?;
        }
    }
    writeln!(w)?;

    if let Some(results) = obj.get("results").and_then(|value| value.as_array()) {
        for result in results.iter().filter(|result| result["ok"] == false) {
            let id = result["id"].as_str().unwrap_or("unknown");
            let error = result["error"].as_str().unwrap_or("failed");
            writeln!(w, "  {YELLOW}✗{RESET} {id} — {error}")?;
        }
    }
    Ok(())
}

fn is_action_result(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    obj.contains_key("ok") && obj.contains_key("action")
}

fn format_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => {
            if *b {
                format!("{GREEN}✓{RESET}")
            } else {
                "✗".to_string()
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 1_000_000.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{f:.1}")
                }
            } else {
                n.to_string()
            }
        }
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => format!("[{}]", arr.len()),
        serde_json::Value::Object(_) => "{…}".to_string(),
    }
}

fn format_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => format!("{DIM}null{RESET}"),
        serde_json::Value::Bool(true) => format!("{GREEN}true{RESET}"),
        serde_json::Value::Bool(false) => "false".to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
        serde_json::Value::Object(obj) => format!("{{{} keys}}", obj.len()),
    }
}

fn truncate(s: &str, max: usize) -> String {
    // Strip ANSI escape codes for length calculation
    let visible_len = strip_ansi_len(s);
    if visible_len <= max {
        s.to_string()
    } else {
        // Find byte position for truncation, accounting for ANSI codes
        let mut visible = 0;
        let mut byte_pos = 0;
        let mut in_escape = false;
        for (i, c) in s.char_indices() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c.is_ascii_alphabetic() {
                    in_escape = false;
                }
            } else {
                visible += 1;
                if visible >= max.saturating_sub(1) {
                    byte_pos = i + c.len_utf8();
                    break;
                }
            }
            byte_pos = i + c.len_utf8();
        }
        format!("{}…", &s[..byte_pos])
    }
}

fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_array_of_objects() {
        let value = serde_json::json!([
            {"name": "Alice", "email": "alice@example.com"},
            {"name": "Bob", "email": "bob@test.com"},
        ]);
        let mut buf = Vec::new();
        render(&mut buf, &value).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("NAME"));
        assert!(output.contains("Alice"));
        assert!(output.contains("2 items"));
    }

    #[test]
    fn test_render_table_with_columns_keeps_the_given_order() {
        let value = serde_json::json!([
            {"detail": "d1", "permission": "calendars", "status": "ok"},
            {"detail": "d2", "permission": "contacts", "status": "denied"},
        ]);
        let mut buf = Vec::new();
        render_table_with_columns(
            &mut buf,
            value.as_array().unwrap(),
            &["permission", "status"],
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        let header = output.lines().next().unwrap();
        assert!(header.find("PERMISSION").unwrap() < header.find("STATUS").unwrap());
        assert!(!output.contains("DETAIL"));
        assert!(output.contains("denied"));

        let mut buf = Vec::new();
        render_table_with_columns(&mut buf, &[], &["permission"]).unwrap();
        assert!(String::from_utf8(buf).unwrap().contains("no results"));
    }

    #[test]
    fn test_render_action_result() {
        let value = serde_json::json!({"ok": true, "action": "created", "id": "abc"});
        let mut buf = Vec::new();
        render(&mut buf, &value).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("created"));
        assert!(output.contains("abc"));
    }

    #[test]
    fn test_render_batch_result_keeps_failure_details() {
        let value = serde_json::json!({
            "ok": false,
            "action": "batch-delete",
            "requested": 2,
            "succeeded": 1,
            "failed": 1,
            "results": [
                {"id": "a", "ok": true},
                {"id": "b", "ok": false, "error": "not found"}
            ]
        });
        let mut buf = Vec::new();
        render(&mut buf, &value).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("1/2 succeeded"));
        assert!(output.contains("b"));
        assert!(output.contains("not found"));
    }

    #[test]
    fn test_render_empty_array() {
        let value = serde_json::json!([]);
        let mut buf = Vec::new();
        render(&mut buf, &value).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("no results"));
    }

    #[test]
    fn test_render_single_object() {
        let value = serde_json::json!({"computer_name": "Paul's Mac", "os_version": "15.0"});
        let mut buf = Vec::new();
        render(&mut buf, &value).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("computer name"));
        assert!(output.contains("Paul's Mac"));
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("a very long string here", 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_format_cell_bool() {
        let t = format_cell(&serde_json::json!(true));
        assert!(t.contains('✓'));
        let f = format_cell(&serde_json::json!(false));
        assert!(f.contains('✗'));
    }
}

#[cfg(test)]
mod safari_tests {
    #[test]
    fn fetched_and_requested_bodies_are_visible() {
        for value in [
            serde_json::json!({"ok":true,"action":"fetch","page":{"content":"page body","truncated":true}}),
            serde_json::json!({"ok":false,"action":"request","status":401,"body":"response body"}),
        ] {
            let before = value.clone();
            let mut out = Vec::new();
            super::render(&mut out, &value).unwrap();
            assert!(String::from_utf8(out).unwrap().contains("body"));
            assert_eq!(value, before);
        }
    }
}
