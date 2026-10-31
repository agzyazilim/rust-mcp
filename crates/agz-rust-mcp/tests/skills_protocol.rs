//! Exercise embedded guidance through the same tools-only surface clients use.
//! The configured project has no manifest/toolchain and client roots fail.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use agz_rust_mcp::{Config, RustMcpServer, config::Toolset, skills::BUNDLED_SKILLS};
use anyhow::{Context, Result};
use rmcp::{
    ClientHandler, ErrorData as McpError, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ClientCapabilities, ClientConfig, Implementation,
    },
    service::{MaybeSendFuture, Peer, RequestContext, RoleClient},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
mod support;

#[derive(Clone)]
struct UnavailableRoots(Arc<AtomicUsize>);

impl ClientHandler for UnavailableRoots {
    #[allow(deprecated)]
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::new("generic-tools-client", "1"),
        )
    }

    #[allow(deprecated)]
    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListRootsResult, McpError>>
    + MaybeSendFuture
    + '_ {
        self.0.fetch_add(1, Ordering::Relaxed);
        async { Err(McpError::internal_error("roots unavailable", None)) }
    }
}

async fn call(peer: &Peer<RoleClient>, arguments: Value) -> Result<Value> {
    let response = peer
        .call_tool_once(
            CallToolRequestParams::new("workspace")
                .with_arguments(arguments.as_object().context("object input")?.clone()),
        )
        .await?;
    let CallToolResponse::Complete(result) = response else {
        anyhow::bail!("embedded skill lookup cannot become a background task");
    };
    assert_ne!(result.is_error, Some(true));
    let value = result
        .structured_content
        .context("structured skill result")?;
    let text = result
        .content
        .first()
        .and_then(|item| item.as_text())
        .context("text fallback")?;
    assert_eq!(serde_json::from_str::<Value>(&text.text)?, value);
    assert_eq!(
        value["untrustedData"], false,
        "guidance is bundled, not workspace input"
    );
    assert_eq!(value["data"]["authority"], "bundled_skill_guidance");
    assert_eq!(value["data"]["scan"]["visitedEntries"], 0);
    assert_eq!(value["data"]["scan"]["filesRead"], 0);
    Ok(value)
}

#[tokio::test]
async fn tools_only_clients_read_complete_versioned_skills_without_workspace_roots() -> Result<()> {
    for toolset in [Toolset::Core, Toolset::Full] {
        let root = support::TestRoot::new("embedded-skills");
        let mut config = Config::defaults_at(root.dir("empty-project"));
        config.server.toolset = toolset;
        config.cargo.path = Some(root.path().join("missing-cargo"));
        config.gate.cache_dir = root.path().join("state/gate");
        config.gate.lease_dir = root.path().join("state/leases");
        config.docs.cache_dir = root.path().join("state/docs");
        config.change.scratch_dir = root.path().join("state/change");
        config.telemetry.enabled = false;
        config.telemetry.path = root.path().join("state/activity.jsonl");
        let (server_io, client_io) = tokio::io::duplex(1 << 20);
        let server_task = tokio::spawn(async move {
            let server = Box::pin(RustMcpServer::new(config)?.serve(server_io)).await?;
            server.waiting().await?;
            anyhow::Ok(())
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let client = UnavailableRoots(Arc::clone(&calls))
            .serve(client_io)
            .await?;
        let catalog = call(client.peer(), json!({"action":"skills"})).await?;
        assert_eq!(catalog["status"], "OK");
        let entries = catalog["data"]["skills"].as_array().context("catalog")?;
        assert_eq!(entries.len(), BUNDLED_SKILLS.len());
        for (entry, skill) in entries.iter().zip(BUNDLED_SKILLS) {
            assert_eq!(entry["name"], skill.name);
            assert_eq!(entry["version"], env!("CARGO_PKG_VERSION"));
            assert!(entry.get("markdown").is_none(), "list is metadata-only");
            let digest = hex::encode(Sha256::digest(skill.markdown.as_bytes()));
            assert_eq!(entry["sha256"], digest);
            for name in [skill.name, skill.prompt] {
                let selected = call(client.peer(), json!({"action":"skills","skill":name})).await?;
                assert_eq!(selected["status"], "OK");
                assert_eq!(selected["data"]["skills"][0]["markdown"], skill.markdown);
                assert_eq!(selected["data"]["skills"][0]["sha256"], digest);
                assert_eq!(selected["data"]["page"]["nextLine"], Value::Null);
            }
        }

        // A tools-only client can reconstruct a long skill under a small wire budget.
        let mut line = 1;
        let mut reconstructed = String::new();
        loop {
            let page = call(
                client.peer(),
                json!({"action":"skills","skill":"workflow","line":line,"byteBudget":2048}),
            )
            .await?;
            assert!(serde_json::to_vec(&page["data"])?.len() <= 2048);
            reconstructed.push_str(
                page["data"]["skills"][0]["markdown"]
                    .as_str()
                    .context("skill page")?,
            );
            if let Some(next) = page["data"]["page"]["nextLine"].as_u64() {
                assert!(next > line, "pagination must make progress");
                assert_eq!(page["status"], "PARTIAL");
                assert_eq!(page["truncated"], true);
                line = next;
            } else {
                assert_eq!(page["status"], "OK");
                break;
            }
        }
        assert_eq!(reconstructed, BUNDLED_SKILLS[0].markdown);
        let first = call(client.peer(), json!({"action":"skills","limit":1})).await?;
        assert_eq!(first["data"]["skills"][0]["name"], BUNDLED_SKILLS[0].name);
        assert_eq!(first["data"]["page"]["nextCursor"], 1);
        let remainder = call(client.peer(), json!({"action":"skills","cursor":1})).await?;
        assert_eq!(
            remainder["data"]["skills"].as_array().unwrap().len(),
            BUNDLED_SKILLS.len() - 1
        );
        assert_eq!(remainder["data"]["page"]["nextCursor"], Value::Null);

        for arguments in [
            json!({"action":"skills","skill":"missing"}),
            json!({"action":"skills","dir":"/unrelated"}),
            json!({"action":"skills","path":"../SKILL.md"}),
            json!({"action":"skills","line":1}),
            json!({"action":"skills","skill":"repair","line":0}),
            json!({"action":"skills","skill":"repair","limit":1}),
            json!({"action":"inspect","skill":"repair"}),
        ] {
            assert!(call(client.peer(), arguments).await.is_err());
        }
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "skills never request client roots"
        );
        assert!(!root.path().join("empty-project/Cargo.lock").exists());
        client.cancel().await?;
        server_task.await??;
    }
    Ok(())
}
