//! Capture actual Safari network traffic through Safari 27's native MCP server.
//! Each capture owns one short-lived MCP session and one tab. No DOM extraction,
//! token reconstruction, or request replay is involved.
use super::{safari, util::ActionResult};
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

const MAX_MESSAGE: usize = 32 * 1024 * 1024;
const MAX_TOTAL_BODY: usize = 16 * 1024 * 1024;
const SETUP: &str = "Safari network capture requires Safari 27 or newer with native MCP support (/usr/bin/safaridriver --mcp). In Safari Settings → Advanced enable Show features for web developers, then Settings → Developer enable Allow remote automation and external agents. Open Safari, allow its external-agent prompt if shown, and complete any sign-in in the automation session. A regular signed-in Safari tab may not share its login with this session. Retry with --timeout 120 if loading is slow. JavaScript from Apple Events is not required for network capture.";

#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub url: String,
    pub filter: Option<String>,
    pub bodies: bool,
    pub wait: u32,
    pub timeout: u32,
    pub limit: usize,
    pub max_body_bytes: usize,
}
impl CaptureOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        safari::validate_url(&self.url)?;
        safari::validate_timeout(self.timeout)?;
        if self.wait > 60 || self.wait >= self.timeout {
            bail!("invalid input: wait must be 0–60 seconds and less than timeout");
        }
        if !(1..=1000).contains(&self.limit) {
            bail!("invalid input: limit must be 1–1000");
        }
        if !(1..=1_000_000).contains(&self.max_body_bytes) {
            bail!("invalid input: max-body-bytes must be 1–1000000");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub request_id: String,
    pub url: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_iso8601: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_size_bytes: Option<u64>,
    /// Decoded text exactly as Safari returned it, never parsed/reconstructed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    /// not_requested, captured, unavailable, over_limit, or error.
    #[serde(default = "not_requested")]
    pub body_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_error: Option<String>,
}
fn not_requested() -> String {
    "not_requested".into()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CapturePage {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct NetworkCapture {
    #[serde(flatten)]
    pub result: ActionResult,
    pub backend: String,
    pub opened_tab: bool,
    pub url: String,
    pub page: CapturePage,
    pub matched_count: usize,
    pub requests_truncated: bool,
    pub requests: Vec<NetworkRequest>,
    pub tab_closed: bool,
}

/// Open a new Safari tab, enable network inspection before navigation, and
/// capture a bounded snapshot. The new tab is closed afterward when possible.
/// Safari controls automation-session storage; regular-tab logins may not carry
/// over. Existing user tabs are untouched.
pub async fn capture(options: &CaptureOptions) -> anyhow::Result<NetworkCapture> {
    options.validate()?;
    let mut client = Client::spawn().map_err(recovery_error)?;
    let result = tokio::time::timeout(
        Duration::from_secs(options.timeout.into()),
        collect(&mut client, options),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Safari network capture timed out. {SETUP}"))
    .and_then(|v| v.map_err(recovery_error));
    // Even after a protocol failure or timeout, attempt to close only our tab.
    let closed = if let Some(handle) = client.tab.take() {
        matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                client.tool("close_tab", json!({"handle":handle}))
            )
            .await,
            Ok(Ok(_))
        )
    } else {
        true
    };
    let _ = client.child.kill().await;
    let _ = client.child.wait().await;
    result.map(|mut capture| {
        capture.tab_closed = closed;
        if !closed {
            capture.result.message = Some(
                "Safari did not confirm closing the capture tab; close it manually if still open."
                    .into(),
            );
        }
        capture
    })
}

fn recovery_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("{error:#}. {SETUP}")
}

async fn collect(client: &mut Client, options: &CaptureOptions) -> anyhow::Result<NetworkCapture> {
    client.initialize().await?;
    let tab = client.tool("create_tab", json!({})).await?;
    let handle = tab["handle"]
        .as_str()
        .context("Safari did not return a tab handle")?
        .to_owned();
    client.tab = Some(handle.clone());
    // Resource collection must be enabled before the page starts its requests.
    client
        .tool("list_network_requests", json!({"tab_handle":handle}))
        .await?;
    client
        .tool(
            "navigate_to_url",
            json!({"url":options.url,"tab_uuid":handle}),
        )
        .await?;
    tokio::time::sleep(Duration::from_secs(options.wait.into())).await;
    let page: CapturePage = serde_json::from_value(client.tool("page_info", json!({})).await?)
        .context("Safari returned invalid page info")?;
    let mut args = json!({"tab_handle":handle});
    if let Some(filter) = &options.filter {
        args["filter"] = json!({"url_substring":filter});
    }
    let listed = client.tool("list_network_requests", args).await?;
    let mut requests: Vec<NetworkRequest> = serde_json::from_value(listed["requests"].clone())
        .context("Safari returned an invalid network request list")?;
    let matched_count = requests.len();
    requests.truncate(options.limit);
    // Only get_network_request supplies bodies; list metadata is not a body capture.
    for request in &mut requests {
        request.response_body = None;
        request.body_state = not_requested();
        request.body_error = None;
    }
    let mut remaining = MAX_TOTAL_BODY;
    if options.bodies && !requests.is_empty() {
        client.tool("switch_tab", json!({"handle":handle})).await?;
        for request in &mut requests {
            match client
                .tool(
                    "get_network_request",
                    json!({"request_id":request.request_id}),
                )
                .await
            {
                Ok(detail) => attach_body(request, &detail, options.max_body_bytes, &mut remaining),
                Err(error) => {
                    request.body_state = "error".into();
                    request.body_error = Some(error.to_string());
                }
            }
        }
    }
    Ok(NetworkCapture {
        result: ActionResult {
            ok: true,
            action: "network".into(),
            id: None,
            message: None,
        },
        backend: "safari_mcp".into(),
        opened_tab: true,
        url: options.url.clone(),
        page,
        matched_count,
        requests_truncated: matched_count > requests.len(),
        requests,
        tab_closed: false,
    })
}

fn attach_body(request: &mut NetworkRequest, detail: &Value, limit: usize, remaining: &mut usize) {
    // Headers (including credentials) and other unsolicited fields are not exported.
    match detail["request"]["response_body"].as_str() {
        Some(body) if body.len() <= limit && body.len() <= *remaining => {
            *remaining -= body.len();
            request.response_body = Some(body.to_owned());
            request.body_state = "captured".into();
        }
        Some(_) => request.body_state = "over_limit".into(),
        None => request.body_state = "unavailable".into(),
    }
}

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
    tab: Option<String>,
}
impl Client {
    fn spawn() -> anyhow::Result<Self> {
        let child = Command::new("/usr/bin/safaridriver")
            .arg("--mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Cannot launch safaridriver --mcp")?;
        Ok(Self::from_child(child))
    }
    fn from_child(mut child: Child) -> Self {
        Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            id: 0,
            tab: None,
        }
    }
    async fn send(&mut self, value: Value) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn rpc(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let response = read_message(&mut self.output, MAX_MESSAGE).await?;
            // Safari can send notifications, or a prior response after cancellation.
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                bail!("Safari MCP {method}: {error}");
            }
            return response
                .get("result")
                .cloned()
                .context("Safari MCP response has no result");
        }
    }
    async fn initialize(&mut self) -> anyhow::Result<()> {
        let initialized = self.rpc("initialize", json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"cider","version":env!("CARGO_PKG_VERSION")}})).await?;
        if initialized["protocolVersion"] != "2024-11-05" {
            bail!("Unsupported Safari MCP protocol version");
        }
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        let tools = self.rpc("tools/list", json!({})).await?;
        for required in [
            "create_tab",
            "close_tab",
            "navigate_to_url",
            "switch_tab",
            "page_info",
            "list_network_requests",
            "get_network_request",
        ] {
            if !tools["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == required))
            {
                bail!("Safari MCP does not provide {required}; update Safari");
            }
        }
        Ok(())
    }
    async fn tool(&mut self, name: &str, args: Value) -> anyhow::Result<Value> {
        let result = self
            .rpc("tools/call", json!({"name":name,"arguments":args}))
            .await?;
        tool_value(&result)
    }
}

fn tool_value(result: &Value) -> anyhow::Result<Value> {
    let text = result["content"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["text"].as_str()));
    if result["isError"] == true {
        bail!("{}", text.unwrap_or("Safari tool failed"));
    }
    // Navigation/close may return plain text; callers needing JSON validate it.
    Ok(text
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(Value::Null))
}

async fn read_message<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> anyhow::Result<Value> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            bail!("Safari MCP exited before returning a complete response");
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |end| end + 1);
        if line.len() + count > limit {
            bail!("Safari MCP response exceeds the {limit}-byte message limit; use a narrower --filter");
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return serde_json::from_slice(&line).context("Invalid JSON from Safari MCP");
        }
    }
}

/// Observe future fetch/XHR traffic in an existing, signed-in tab. No navigation
/// or requests are initiated. The page's wrappers are restored after the window.
#[derive(Debug, Clone)]
pub struct MonitorOptions {
    pub target: safari::TabTarget,
    pub filter: Option<String>,
    pub bodies: bool,
    pub seconds: u32,
    pub limit: usize,
    pub max_body_bytes: usize,
}
impl MonitorOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.target.validate()?;
        if !(1..=60).contains(&self.seconds) {
            bail!("invalid input: seconds must be 1–60");
        }
        if !(1..=1000).contains(&self.limit) {
            bail!("invalid input: limit must be 1–1000");
        }
        if !(1..=1_000_000).contains(&self.max_body_bytes) {
            bail!("invalid input: max-body-bytes must be 1–1000000");
        }
        Ok(())
    }
}

pub async fn monitor(options: &MonitorOptions) -> anyhow::Result<NetworkCapture> {
    options.validate()?;
    let key = format!("__cider_monitor_{}", uuid::Uuid::new_v4().simple());
    let config = json!({"key":key,"filter":options.filter,"bodies":options.bodies,"seconds":options.seconds,"limit":options.limit,"max_body_bytes":options.max_body_bytes});
    let install = format!("(()=>{{try{{return ({} )({config});}}catch(e){{return JSON.stringify({{error:String(e)}});}}}})()", include_str!("safari_monitor.js"));
    let stop = format!("window[{key}].stop()", key = serde_json::to_string(&key)?);
    let script = format!(
        r#"{}
const installed = app.doJavaScript({}, {{in:target}});
if (installed !== "started") throw new Error("Safari monitor could not start: " + installed);
try {{
    delay({});
    const result = app.doJavaScript({}, {{in:target}});
    if (typeof result !== "string" || !result.startsWith("{{")) throw new Error("Safari monitor lost its page. Keep the same document open; use in-page navigation instead of reloading. Retry.");
    result;
}} finally {{
    try {{ app.doJavaScript({}, {{in:target}}); }} catch (_) {{}}
}}
"#,
        options.target.script()?,
        serde_json::to_string(&install)?,
        options.seconds,
        serde_json::to_string(&stop)?,
        serde_json::to_string(&format!(
            "if(window[{key}])window[{key}].stop()",
            key = serde_json::to_string(&key)?
        ))?
    );
    let output = super::util::run_jxa_with_timeout(
        &script,
        Duration::from_secs(u64::from(options.seconds) + 20),
    )
    .await
    .map_err(safari::safari_error)?;
    // Monitor owns the output schema, unlike native Safari detail payloads.
    let raw: Value =
        serde_json::from_str(&output).context("Safari monitor returned invalid output")?;
    let requests: Vec<NetworkRequest> = serde_json::from_value(raw["requests"].clone())?;
    Ok(NetworkCapture {
        result: ActionResult::success("monitor"),
        backend: "page_fetch_xhr".into(),
        url: raw["url"].as_str().context("Monitor URL missing")?.into(),
        page: serde_json::from_value(raw["page"].clone())?,
        matched_count: raw["matched_count"]
            .as_u64()
            .context("Monitor count missing")? as usize,
        requests_truncated: raw["requests_truncated"] == true,
        requests,
        opened_tab: false,
        tab_closed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rpc_ignores_notifications_and_surfaces_protocol_errors() {
        // A deterministic stdio peer, without Safari or private account data.
        let script = r#"
IFS= read -r request
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/message","params":{}}' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{\"requests\":[]}"}]}}'
IFS= read -r request
printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"unsupported"}}'
"#;
        let child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut client = Client::from_child(child);
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            client.tool("list_network_requests", json!({})),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result["requests"], json!([]));
        let error = client.tool("missing", json!({})).await.unwrap_err();
        assert!(error.to_string().contains("unsupported"));
        client.child.wait().await.unwrap();
    }
    #[test]
    fn validates_bounds_and_retains_error_cause_with_setup_instructions() {
        let mut options = CaptureOptions {
            url: "https://example.com".into(),
            filter: None,
            bodies: false,
            wait: 5,
            timeout: 60,
            limit: 100,
            max_body_bytes: 1000,
        };
        assert!(options.validate().is_ok());
        options.wait = 60;
        assert!(options.validate().is_err());
        options.wait = 0;
        options.limit = 0;
        assert!(options.validate().is_err());
        options.limit = 1;
        options.max_body_bytes = 1_000_001;
        assert!(options.validate().is_err());
        let message = recovery_error(anyhow::anyhow!("remote automation disabled")).to_string();
        assert!(message.contains("remote automation disabled"));
        assert!(message.contains("Allow remote automation and external agents"));
    }
    #[tokio::test]
    async fn framing_rejects_oversize_eof_and_invalid_json() {
        for mut bytes in [b"123456789".as_slice(), b"{}", b"oops\n"] {
            assert!(read_message(&mut bytes, 8).await.is_err());
        }
        let mut bytes = b"{\"id\":1}\n{\"id\":2}\n".as_slice();
        assert_eq!(read_message(&mut bytes, 32).await.unwrap()["id"], 1);
        assert_eq!(read_message(&mut bytes, 32).await.unwrap()["id"], 2);
    }
    #[test]
    fn original_body_is_preserved_and_headers_are_not_exported() {
        let mut request: NetworkRequest = serde_json::from_value(json!({"request_id":"1","url":"https://example.com/api","method":"GET","request_headers":{"Authorization":"secret"}})).unwrap();
        let body = "{ \"items\" : [1, 2], \"text\": \"🐈\" }\n";
        let detail = json!({"request":{"response_body":body}});
        let mut remaining = 1000;
        attach_body(&mut request, &detail, 1000, &mut remaining);
        assert_eq!(request.response_body.as_deref(), Some(body));
        let round_trip: NetworkRequest =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        assert_eq!(round_trip.response_body.as_deref(), Some(body));
        assert_eq!(remaining, 1000 - body.len());
        assert!(!serde_json::to_string(&request).unwrap().contains("secret"));
        request.response_body = None;
        attach_body(&mut request, &detail, 4, &mut remaining);
        assert_eq!(request.body_state, "over_limit");
        assert!(request.response_body.is_none());
        attach_body(&mut request, &json!({"request":{}}), 1000, &mut remaining);
        assert_eq!(request.body_state, "unavailable");
        assert!(
            tool_value(&json!({"isError":true,"content":[{"text":"disabled"}]}))
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
    }
}
