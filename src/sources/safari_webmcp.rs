use super::{content, eval, ContentFormat, EvalOptions, TabTarget};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const MAX_SOURCE_CHARS: u32 = 1_000_000;
const MAX_REGISTRY_CHARS: u32 = 65_536;
const MAX_TOOLS: usize = 100;
const MAX_LABEL_BYTES: usize = 4_096;
const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebMcpBackend {
    DeclarativeHtml,
    PageRegistry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebMcpStatus {
    /// The selected backend returned a complete list, which may be empty.
    Complete,
    /// Some entries may be missing because a local output bound was reached.
    Partial,
    /// The page has no compatible registry or registry discovery failed.
    Unavailable,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WebMcpTool {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
pub struct WebMcpList {
    pub window: u32,
    pub tab: u32,
    pub backend: WebMcpBackend,
    pub status: WebMcpStatus,
    pub tools: Vec<WebMcpTool>,
}

#[derive(Deserialize)]
struct RegistryReply {
    available: bool,
    truncated: bool,
    tools: Vec<WebMcpTool>,
}

/// Discover WebMCP metadata in one existing Safari tab; never invoke a tool.
/// The default reads Safari's native HTML source. Explicit `allow_eval`
/// instead queries a compatible page-loaded registry via the gated eval path.
/// As with any page-side JavaScript, an untrusted page can redefine that API.
pub async fn webmcp_list(target: TabTarget, allow_eval: bool) -> anyhow::Result<WebMcpList> {
    target.validate()?;
    let (backend, status, tools) = if allow_eval {
        let reply = eval(
            include_str!("safari_webmcp.js"),
            target,
            &EvalOptions {
                allow_eval,
                max_chars: MAX_REGISTRY_CHARS,
                timeout: 10,
            },
        )
        .await?;
        let (status, tools) = read_registry(reply.value, reply.truncated);
        (WebMcpBackend::PageRegistry, status, tools)
    } else {
        let page = content(target, ContentFormat::Html, MAX_SOURCE_CHARS).await?;
        let (status, tools) = read_declarations(&page.content, page.truncated);
        (WebMcpBackend::DeclarativeHtml, status, tools)
    };
    Ok(WebMcpList {
        window: target.window,
        tab: target.tab,
        backend,
        status,
        tools,
    })
}

fn read_declarations(source: &str, mut partial: bool) -> (WebMcpStatus, Vec<WebMcpTool>) {
    let html = Html::parse_document(source);
    let selector = Selector::parse("form[toolname][tooldescription]").expect("static selector");
    let mut tools = Vec::new();
    for form in html.select(&selector) {
        if form.value().name.ns.as_ref() != HTML_NAMESPACE
            || form
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|ancestor| matches!(ancestor.value().name(), "template" | "noscript"))
        {
            continue;
        }
        let name = form.attr("toolname").unwrap_or_default().trim();
        let description = form.attr("tooldescription").unwrap_or_default().trim();
        if name.is_empty() || description.is_empty() {
            continue;
        }
        if tools.len() == MAX_TOOLS {
            partial = true;
            break;
        }
        if name.len() > MAX_LABEL_BYTES || description.len() > MAX_LABEL_BYTES {
            partial = true;
            continue;
        }
        tools.push(WebMcpTool {
            name: name.into(),
            description: description.into(),
            // Native source is not a registry and cannot supply the browser's
            // derived schema. Do not scrape form controls, values, or actions.
            input_schema: None,
        });
    }
    (completeness(partial), tools)
}

fn read_registry(value: Value, truncated: bool) -> (WebMcpStatus, Vec<WebMcpTool>) {
    if truncated {
        return (WebMcpStatus::Partial, Vec::new());
    }
    let Ok(reply) = serde_json::from_value::<RegistryReply>(value) else {
        return (WebMcpStatus::Unavailable, Vec::new());
    };
    if !reply.available {
        return (WebMcpStatus::Unavailable, Vec::new());
    }
    let mut partial = reply.truncated || reply.tools.len() > MAX_TOOLS;
    let tools = reply
        .tools
        .into_iter()
        .take(MAX_TOOLS)
        .filter(|tool| {
            let valid = !tool.name.trim().is_empty()
                && !tool.description.trim().is_empty()
                && tool.name.len() <= MAX_LABEL_BYTES
                && tool.description.len() <= MAX_LABEL_BYTES;
            partial |= !valid;
            valid
        })
        .collect();
    (completeness(partial), tools)
}

fn completeness(partial: bool) -> WebMcpStatus {
    if partial {
        WebMcpStatus::Partial
    } else {
        WebMcpStatus::Complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarative_metadata_handles_html_and_does_not_return_form_content() {
        let (status, tools) = read_declarations(
            r#"<!doctype html>
            <!-- <form toolname="comment" tooldescription="ignore"></form> -->
            <script>const fake = '<form toolname="script" tooldescription="ignore"></form>';</script>
            <textarea><form toolname="text" tooldescription="ignore"></form></textarea>
            <template><form toolname="template" tooldescription="ignore"></form></template>
            <noscript><form toolname="noscript" tooldescription="ignore"></form></noscript>
            <iframe srcdoc='<form toolname="frame" tooldescription="ignore"></form>'></iframe>
            <form action="/ordinary"><input value="private"></form>
            <FORM ACTION="/private" TOOLNAME='lookup' TOOLDESCRIPTION=" Find &amp; filter ☕ "><input name="secret" value="private"></FORM>
            <form toolname=search tooldescription=Search></form>
            <form toolname="" tooldescription="Missing name"></form>"#,
            false,
        );
        assert_eq!(status, WebMcpStatus::Complete);
        assert_eq!(
            serde_json::to_value(tools).unwrap(),
            serde_json::json!([
                {"name":"lookup", "description":"Find & filter ☕"},
                {"name":"search", "description":"Search"}
            ])
        );
    }

    #[test]
    fn bounded_html_distinguishes_empty_from_incomplete() {
        let (status, tools) = read_declarations("", false);
        assert_eq!(status, WebMcpStatus::Complete);
        assert!(tools.is_empty());
        assert_eq!(read_declarations("", true).0, WebMcpStatus::Partial);

        let form = "<form toolname=lookup tooldescription=Lookup></form>";
        let (status, tools) = read_declarations(&form.repeat(MAX_TOOLS + 1), false);
        assert_eq!(status, WebMcpStatus::Partial);
        assert_eq!(tools.len(), MAX_TOOLS);
    }

    #[test]
    fn registry_distinguishes_unavailable_empty_and_partial() {
        assert_eq!(
            read_registry(Value::Null, false).0,
            WebMcpStatus::Unavailable
        );
        assert_eq!(read_registry(Value::Null, true).0, WebMcpStatus::Partial);
        assert_eq!(
            read_registry(
                serde_json::json!({"available":true,"truncated":false,"tools":[]}),
                false
            )
            .0,
            WebMcpStatus::Complete
        );
        let (status, tools) = read_registry(
            serde_json::json!({"available":true,"truncated":false,"tools":[
                {"name":"find","description":"Find","input_schema":{"type":"object"},"execute":"untrusted"}
            ]}),
            false,
        );
        assert_eq!(status, WebMcpStatus::Complete);
        assert_eq!(
            serde_json::to_value(tools).unwrap(),
            serde_json::json!([{"name":"find","description":"Find","input_schema":{"type":"object"}}])
        );
    }
}
