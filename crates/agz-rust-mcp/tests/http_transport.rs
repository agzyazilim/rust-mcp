//! Protocol compatibility and access boundaries over a real HTTP socket.
use std::{sync::Arc, time::Duration};

use agz_rust_mcp::{
    AppState, Config,
    config::{CliOptions, Transport},
};
use anyhow::{Context, Result};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[allow(dead_code)]
mod support;

struct HttpFixture {
    _root: support::TestRoot,
    endpoint: String,
    client: Client,
    state: Arc<AppState>,
    cancellation: CancellationToken,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl HttpFixture {
    async fn new() -> Result<Self> {
        let root = support::TestRoot::new("http");
        let project = support::write_minimal_package(&root, "project", "http_fixture");
        let mut config = Config::defaults_at(project);
        config.server.transport = Transport::Http;
        config.gate.cache_dir = root.path().join("state/gate");
        config.gate.lease_dir = root.path().join("state/leases");
        config.docs.cache_dir = root.path().join("state/docs");
        config.change.scratch_dir = root.path().join("state/change");
        config.rust_analyzer.path = Some(root.path().join("no-rust-analyzer"));
        config.telemetry.enabled = false;
        config.telemetry.path = root.path().join("state/activity.jsonl");
        let state = Arc::new(AppState::new(config)?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/mcp", listener.local_addr()?);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(agz_rust_mcp::transport::serve_http(
            listener,
            Arc::clone(&state),
            cancellation.clone(),
        ));
        Ok(Self {
            _root: root,
            endpoint,
            state,
            cancellation,
            task: Some(task),
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }

    fn request(&self, method: &str, mut params: Value, modern: bool) -> RequestBuilder {
        let version = if modern { "2026-07-28" } else { "2025-11-25" };
        if modern {
            params["_meta"] = json!({
                "io.modelcontextprotocol/protocolVersion": version,
                "io.modelcontextprotocol/clientInfo": {"name":"generic-http-client","version":"1"},
                "io.modelcontextprotocol/clientCapabilities": {}
            });
        }
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", version)
            .header("Mcp-Method", method);
        if let Some(name) = match method {
            "tools/call" | "prompts/get" => params.get("name"),
            "resources/read" => params.get("uri"),
            _ => None,
        }
        .and_then(Value::as_str)
        {
            request = request.header("Mcp-Name", name);
        }
        request.body(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let value = decode(self.request(method, params, true).send().await?).await?;
        assert!(value.get("error").is_none(), "{value:#}");
        Ok(value["result"].clone())
    }

    async fn stop(mut self) -> Result<()> {
        self.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(5), self.task.take().unwrap()).await???;
        self.state.shutdown_async().await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while Arc::strong_count(&self.state) != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("HTTP worker retained application state after shutdown")?;
        Ok(())
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn decode(response: Response) -> Result<Value> {
    let status = response.status();
    let text = response.text().await?;
    assert!(status.is_success(), "HTTP {status}: {text}");
    if let Ok(value) = serde_json::from_str(&text) {
        return Ok(value);
    }
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("id").is_some())
        .context("JSON-RPC response missing from SSE stream")
}

#[tokio::test]
async fn legacy_sessions_and_modern_stateless_requests_expose_the_same_mcp() -> Result<()> {
    let fixture = HttpFixture::new().await?;
    let initialized = fixture
        .request(
            "initialize",
            json!({
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"legacy-http-client","version":"1"}
            }),
            false,
        )
        .send()
        .await?;
    let session = initialized
        .headers()
        .get("Mcp-Session-Id")
        .context("legacy session ID")?
        .to_str()?
        .to_owned();
    assert_eq!(
        decode(initialized).await?["result"]["protocolVersion"],
        "2025-11-25"
    );
    let notification = fixture
        .client
        .post(&fixture.endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Mcp-Session-Id", &session)
        .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .send()
        .await?;
    assert_eq!(notification.status(), StatusCode::ACCEPTED);
    let legacy_tools = decode(
        fixture
            .request("tools/list", json!({}), false)
            .header("Mcp-Session-Id", &session)
            .send()
            .await?,
    )
    .await?;
    let modern_response = fixture
        .request("tools/list", json!({}), true)
        .send()
        .await?;
    assert!(!modern_response.headers().contains_key("Mcp-Session-Id"));
    let modern_tools = decode(modern_response).await?;
    assert_eq!(
        legacy_tools["result"]["tools"],
        modern_tools["result"]["tools"]
    );
    assert_eq!(
        modern_tools["result"]["tools"].as_array().unwrap().len(),
        23
    );

    let discovered = fixture.rpc("server/discover", json!({})).await?;
    assert!(discovered.to_string().contains("2026-07-28"));
    let skills = fixture
        .rpc(
            "tools/call",
            json!({"name":"workspace", "arguments":{"action":"skills"}}),
        )
        .await?;
    assert_eq!(
        skills["structuredContent"]["data"]["skills"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    for skill in agz_rust_mcp::skills::BUNDLED_SKILLS {
        let content = fixture
            .rpc("resources/read", json!({"uri":skill.uri}))
            .await?;
        assert_eq!(content["contents"][0]["text"], skill.markdown);
    }
    let prompts = fixture.rpc("prompts/list", json!({})).await?;
    assert_eq!(prompts["prompts"].as_array().unwrap().len(), 4);
    let deleted = fixture
        .client
        .delete(&fixture.endpoint)
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Mcp-Session-Id", &session)
        .send()
        .await?;
    assert!(deleted.status().is_success());
    assert_eq!(
        fixture
            .request("tools/list", json!({}), false)
            .header("Mcp-Session-Id", &session)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    fixture.stop().await
}

#[tokio::test]
async fn stateless_http_preserves_context_between_separate_requests() -> Result<()> {
    let fixture = HttpFixture::new().await?;
    let prepared = fixture
        .rpc(
            "tools/call",
            json!({"name":"context", "arguments":{
                "action":"prepare", "anchors":[{"kind":"file","file":"src/lib.rs"}]
            }}),
        )
        .await?;
    let capsule = prepared["structuredContent"]["data"]["capsuleId"]
        .as_str()
        .with_context(|| format!("capsule missing: {prepared:#}"))?;
    let expanded = fixture
        .rpc(
            "tools/call",
            json!({"name":"context", "arguments":{
                "action":"expand", "capsuleId":capsule, "pageSize":1
            }}),
        )
        .await?;
    assert_ne!(expanded["isError"], true, "{expanded:#}");
    assert_eq!(
        expanded["structuredContent"]["status"], "OK",
        "{expanded:#}"
    );
    assert_eq!(expanded["structuredContent"]["data"]["capsuleId"], capsule);
    assert!(
        !expanded["structuredContent"]["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fixture.stop().await
}

#[tokio::test]
async fn http_rejects_rebinding_browser_origins_and_oversized_requests() -> Result<()> {
    let fixture = HttpFixture::new().await?;
    for (header, value) in [
        ("Host", "attacker.invalid"),
        ("Host", "localhost:1"),
        ("Origin", "https://attacker.invalid"),
        ("Origin", "null"),
    ] {
        assert_eq!(
            fixture
                .request("tools/list", json!({}), true)
                .header(header, value)
                .send()
                .await?
                .status(),
            StatusCode::FORBIDDEN,
            "{header}: {value}"
        );
    }
    assert_eq!(
        fixture
            .request(
                "tools/list",
                json!({"padding":"x".repeat(1024 * 1024)}),
                true
            )
            .send()
            .await?
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // A rejected request must not poison the listener or authority state.
    assert!(fixture.rpc("tools/list", json!({})).await?["tools"].is_array());
    fixture.stop().await
}

#[tokio::test]
async fn shutdown_closes_a_legacy_session_without_waiting_for_its_idle_timeout() -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let fixture = HttpFixture::new().await?;
    let response = fixture
        .request(
            "initialize",
            json!({
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"idle-client","version":"1"}
            }),
            false,
        )
        .send()
        .await?;
    assert!(response.headers().contains_key("Mcp-Session-Id"));
    decode(response).await?;
    let address = fixture
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut stalled = tokio::net::TcpStream::connect(address).await?;
    stalled.write_all(format!(
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: 1000\r\n\r\n{{"
    ).as_bytes()).await?;
    fixture.rpc("tools/list", json!({})).await?;
    // Intentionally leave the session open. stop() proves that the transport
    // and every SDK handler release state even with an incomplete request body.
    fixture.stop().await
}

#[test]
fn transport_configuration_obeys_precedence_and_rejects_public_listeners() -> Result<()> {
    let root = support::TestRoot::new("transport-config");
    let config = Config::from_sources(
        root.path(),
        Some("[server]\ntransport='http'\nhttp_bind='127.0.0.1:8787'"),
        [
            ("AGZ_RUST_MCP_SERVER__TRANSPORT", "stdio"),
            ("AGZ_RUST_MCP_SERVER__HTTP_BIND", "[::1]:8788"),
        ],
        &CliOptions {
            transport: Some(Transport::Http),
            ..Default::default()
        },
    )?;
    assert_eq!(config.server.transport, Transport::Http);
    assert_eq!(config.server.http_bind.to_string(), "[::1]:8788");
    assert_eq!(
        Config::defaults_at(root.path()).server.transport,
        Transport::Stdio
    );
    for bind in [
        "0.0.0.0:8787",
        "[::]:8787",
        "192.0.2.1:8787",
        "not-an-address",
    ] {
        assert!(
            Config::from_sources(
                root.path(),
                None,
                [("AGZ_RUST_MCP_SERVER__HTTP_BIND", bind)],
                &CliOptions::default()
            )
            .is_err(),
            "{bind}"
        );
    }
    Ok(())
}
