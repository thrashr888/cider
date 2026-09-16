//! Real stdio protocol tests using an isolated synthetic download store.
#![cfg(all(feature = "mcp", feature = "cli"))]

use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cider-mcp-{}", uuid::Uuid::new_v4()));
        let path = root.join("Library/Preferences/com.apple.LaunchServices.QuarantineEventsV2");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let status = std::process::Command::new("sqlite3")
            .arg(path)
            .arg(
                "CREATE TABLE LSQuarantineEvent (
                LSQuarantineEventIdentifier TEXT, LSQuarantineTimeStamp REAL,
                LSQuarantineAgentBundleIdentifier TEXT, LSQuarantineAgentName TEXT,
                LSQuarantineDataURLString TEXT, LSQuarantineOriginURLString TEXT,
                LSQuarantineOriginTitle TEXT, LSQuarantineSenderName TEXT,
                LSQuarantineSenderAddress TEXT, LSQuarantineTypeNumber INTEGER);
             INSERT INTO LSQuarantineEvent VALUES
                ('first',1,'com.test','Test','https://example.com/a',NULL,'雪',NULL,NULL,8),
                ('second',2,'com.test','Test',NULL,NULL,NULL,NULL,NULL,NULL),
                ('other',3,'com.other','Other',NULL,NULL,NULL,NULL,NULL,NULL);",
            )
            .status()
            .unwrap();
        assert!(status.success());
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Client {
    child: Child,
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
}
impl Client {
    fn new(fixture: &Fixture) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cider"))
            .args(["mcp", "--sources", "downloads,knowledge"])
            .env("HOME", &fixture.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            input,
            output,
        }
    }
    async fn send(&mut self, value: Value) {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }
    async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .await;
        let line = tokio::time::timeout(Duration::from_secs(10), self.output.next_line())
            .await
            .expect("MCP response timeout")
            .unwrap()
            .expect("MCP stdout closed");
        let value: Value = serde_json::from_str(&line).expect("stdout must contain only JSON-RPC");
        assert_eq!(value["id"], id);
        assert_eq!(value["jsonrpc"], "2.0");
        value
    }
    async fn initialize(&mut self, version: &str, expected_version: &str) {
        let result = self
            .request(
                1,
                "initialize",
                json!({
                    "protocolVersion":version, "capabilities":{},
                    "clientInfo":{"name":"cider-test", "version":"1"}
                }),
            )
            .await;
        assert_eq!(result["result"]["protocolVersion"], expected_version);
        assert_eq!(result["result"]["serverInfo"]["name"], "cider");
        assert!(result["result"]["capabilities"]["tools"].is_object());
        self.send(json!({"jsonrpc":"2.0", "method":"notifications/initialized"}))
            .await;
    }
    async fn finish(self) {
        drop(self.input);
        let output = tokio::time::timeout(Duration::from_secs(10), self.child.wait_with_output())
            .await
            .expect("MCP must exit when stdin closes")
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn stdio_discovery_source_reads_errors_and_shutdown() {
    let fixture = Fixture::new();
    let mut client = Client::new(&fixture);
    client.initialize("2025-11-25", "2025-11-25").await;
    let listed = client.request(2, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    assert!(tools
        .iter()
        .all(|t| t["annotations"]["readOnlyHint"] == true));
    let arguments = json!({"app":"com.test", "limit":1,"offset":1,
        "since":"2001-01-01T00:00:01Z", "until":"2001-01-01T00:00:03Z"});
    let called = client
        .request(
            3,
            "tools/call",
            json!({"name":"downloads_list","arguments":arguments}),
        )
        .await;
    assert_eq!(called["result"]["isError"], false);
    let result = &called["result"]["structuredContent"];
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"][0]["id"], "first");
    assert_eq!(result["data"][0]["origin_title"], "雪");
    assert_eq!(result["data"].as_array().unwrap().len(), 1);
    let text: Value =
        serde_json::from_str(called["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, result);
    let cli = Command::new(env!("CARGO_BIN_EXE_cider"))
        .args([
            "downloads",
            "list",
            "--app",
            "com.test",
            "--limit",
            "1",
            "--offset",
            "1",
            "--since",
            "2001-01-01T00:00:01Z",
            "--until",
            "2001-01-01T00:00:03Z",
        ])
        .env("HOME", &fixture.0)
        .output()
        .await
        .unwrap();
    assert!(cli.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).unwrap(),
        result["data"]
    );
    let denied = client
        .request(
            4,
            "tools/call",
            json!({"name":"reminders_list","arguments":{}}),
        )
        .await;
    assert_eq!(denied["error"]["code"], -32602);
    let invalid = client
        .request(
            5,
            "tools/call",
            json!({"name":"downloads_list","arguments":{"limit":1001}}),
        )
        .await;
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(
        invalid["result"]["structuredContent"]["error"]["code"],
        "invalid_input"
    );
    let missing = client
        .request(
            6,
            "tools/call",
            json!({"name":"knowledge_streams","arguments":{}}),
        )
        .await;
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(
        missing["result"]["structuredContent"]["error"]["code"],
        "source_error"
    );
    let ping = client.request(7, "ping", json!({})).await;
    assert!(ping.get("result").is_some());
    client.finish().await;
}

#[tokio::test]
async fn stdio_negotiates_legacy_handshake_and_rejects_catalog_cursors() {
    let fixture = Fixture::new();
    let mut client = Client::new(&fixture);
    client.initialize("2026-07-28", "2025-11-25").await;
    let result = client
        .request(2, "tools/list", json!({"cursor":"not-a-page"}))
        .await;
    assert_eq!(result["error"]["code"], -32602);
    client.finish().await;
}

#[tokio::test]
async fn cli_output_flags_are_rejected_without_polluting_stdout() {
    for flag in ["--pretty", "--envelope", "--dry-run"] {
        let result = Command::new(env!("CARGO_BIN_EXE_cider"))
            .args(["mcp", flag])
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let error: Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(error["error"]["code"], "invalid_input");
    }
}

#[tokio::test]
async fn stdio_current_protocol_discovery_and_tool_call() {
    let fixture = Fixture::new();
    let mut client = Client::new(&fixture);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientInfo":{"name":"cider-test","version":"1"},
        "io.modelcontextprotocol/clientCapabilities":{}
    });
    let discovered = client
        .request(1, "server/discover", json!({"_meta":meta}))
        .await;
    assert!(discovered["result"]["supportedVersions"]
        .as_array()
        .unwrap()
        .contains(&json!("2026-07-28")));
    let listed = client.request(2, "tools/list", json!({"_meta":meta})).await;
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 3);
    let called = client
        .request(
            3,
            "tools/call",
            json!({"_meta":meta,
        "name":"downloads_list","arguments":{"limit":1}}),
        )
        .await;
    assert_eq!(called["result"]["resultType"], "complete");
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(
        called["result"]["structuredContent"]["data"][0]["id"],
        "other"
    );
    client.finish().await;
}
