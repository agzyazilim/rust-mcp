//! Exercise the everyday workflow through the public MCP adapter, not direct
//! service calls. This catches schema, authorization, envelope and routing drift.
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use agz_rust_mcp::{
    Config, RustMcpServer,
    config::{CliOptions, Toolset},
};
use anyhow::{Context, Result};
use rmcp::{
    ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ClientCapabilities, ClientInfo, Implementation,
    },
};
use serde_json::{Value, json};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let base = fs::canonicalize(std::env::temp_dir())?
            .join(format!("agz-everyday-{}-{stamp}", std::process::id()));
        fs::create_dir_all(base.join("project/src"))?;
        fs::create_dir_all(base.join("project/examples"))?;
        fs::write(
            base.join("project/Cargo.toml"),
            "[package]\nname='everyday_fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
        )?;
        fs::write(
            base.join("project/Cargo.lock"),
            "version = 4\n[[package]]\nname = \"everyday_fixture\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(
            base.join("project/src/lib.rs"),
            "pub fn answer() -> usize { 42 }\n#[cfg(test)] mod tests { #[test] fn exact_case() { assert_eq!(super::answer(), 42); } #[test] fn exact_case_suffix() { assert!(true); } }\n",
        )?;
        fs::write(
            base.join("project/examples/echo.rs"),
            "fn main() { println!(\"{}\", std::env::args().skip(1).collect::<Vec<_>>().join(\"|\")); }\n",
        )?;
        Ok(Self(base))
    }

    fn config(&self) -> Config {
        let mut config = Config::defaults_at(self.0.join("project"));
        config.server.toolset = Toolset::Core;
        config.gate.cache_dir = self.0.join("state/gate");
        config.gate.lease_dir = self.0.join("state/leases");
        config.docs.cache_dir = self.0.join("state/docs");
        config.change.scratch_dir = self.0.join("state/change");
        config.telemetry.enabled = false;
        config.telemetry.path = self.0.join("state/activity.jsonl");
        config
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn data(response: CallToolResponse) -> Result<Value> {
    let CallToolResponse::Complete(result) = response else {
        anyhow::bail!("expected complete everyday result");
    };
    let value = result.structured_content.context("structured result")?;
    let text = result
        .content
        .first()
        .and_then(|item| item.as_text())
        .context("text fallback")?;
    assert_eq!(serde_json::from_str::<Value>(&text.text)?, value);
    assert_eq!(value["untrustedData"], true);
    Ok(value)
}

#[tokio::test]
async fn core_catalog_runs_the_complete_focused_workflow() -> Result<()> {
    let fixture = Fixture::new()?;
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let config = fixture.config();
    let server_task = tokio::spawn(async move {
        let service = Box::pin(RustMcpServer::new(config)?.serve(server_io)).await?;
        service.waiting().await?;
        anyhow::Ok(())
    });
    let client = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("everyday-test", "1"),
    )
    .serve(client_io)
    .await?;
    let catalog = client.peer().list_tools(None).await?;
    let names: Vec<_> = catalog
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect();
    assert_eq!(
        names,
        ["workspace", "test", "run", "check", "crate_lookup", "docs"]
    );
    assert!(
        !fixture.0.join("state/change").exists(),
        "core must not initialize hidden change storage"
    );

    macro_rules! call {
        ($tool:expr, $args:expr) => {
            data(
                client
                    .call_tool_once(
                        CallToolRequestParams::new($tool)
                            .with_arguments($args.as_object().unwrap().clone()),
                    )
                    .await?,
            )?
        };
    }
    let inspected = call!("workspace", json!({"action":"inspect"}));
    assert_eq!(inspected["status"], "OK");
    assert_eq!(
        inspected["data"]["manifests"][0]["package"]["name"],
        "everyday_fixture"
    );
    let found = call!(
        "workspace",
        json!({"action":"search", "query":"pub fn answer"})
    );
    assert_eq!(found["data"]["matches"][0]["path"], "src/lib.rs");
    let read = call!(
        "workspace",
        json!({"action":"read", "path":"src/lib.rs", "line":1, "lineCount":1})
    );
    assert_eq!(read["data"]["file"]["path"], "src/lib.rs");

    let listed = call!(
        "test",
        json!({"action":"list", "packages":["everyday_fixture"]})
    );
    assert_eq!(listed["status"], "LISTED", "{listed:#}");
    let tested = call!(
        "test",
        json!({"action":"run", "packages":["everyday_fixture"], "filter":"tests::exact_case", "exact":true})
    );
    assert_eq!(tested["status"], "PASSED", "{tested:#}");
    assert_eq!(tested["data"]["tests"]["executed"], 1);
    let empty = call!(
        "test",
        json!({"action":"run", "filter":"does_not_exist", "exact":true})
    );
    assert_eq!(empty["status"], "NO_TESTS", "{empty:#}");
    let ran = call!(
        "run",
        json!({"package":"everyday_fixture", "target":{"kind":"example","name":"echo"}, "args":["hello world", "a;echo", "--config"]})
    );
    assert_eq!(ran["status"], "COMPLETED", "{ran:#}");
    assert!(
        ran.to_string().contains("hello world|a;echo|--config"),
        "arguments are literal data"
    );

    for args in [
        json!({"action":"read", "path":"../outside.rs"}),
        json!({"action":"inspect", "unknown":true}),
    ] {
        assert!(
            client
                .call_tool_once(
                    CallToolRequestParams::new("workspace")
                        .with_arguments(args.as_object().unwrap().clone())
                )
                .await
                .is_err()
        );
    }
    assert!(
        client
            .call_tool_once(CallToolRequestParams::new("work"))
            .await
            .is_err()
    );
    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[test]
fn core_configuration_respects_source_precedence_and_explicit_disables() -> Result<()> {
    let fixture = Fixture::new()?;
    let cwd = fixture.0.join("project");
    let cli = CliOptions {
        toolset: Some(Toolset::Core),
        tools_run: Some(false),
        ..Default::default()
    };
    let config = Config::from_sources(
        &cwd,
        Some("[server]\ntoolset='core'\n[tools]\ntest=false\n"),
        [
            ("AGZ_RUST_MCP_SERVER__TOOLSET", "full"),
            ("AGZ_RUST_MCP_TOOLS__TEST", "true"),
        ],
        &cli,
    )?;
    assert_eq!(
        config.enabled_tool_names(),
        ["workspace", "test", "check", "crate_lookup", "docs"]
    );
    assert!(!config.tools.change);
    assert!(
        Config::from_sources(
            &cwd,
            None,
            [("AGZ_RUST_MCP_SERVER__TOOLSET", "typo")],
            &CliOptions::default()
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn offline_workspace_works_without_a_lockfile_at_a_small_wire_budget() -> Result<()> {
    let fixture = Fixture::new()?;
    fs::remove_file(fixture.0.join("project/Cargo.lock"))?;
    let mut config = fixture.config();
    config.limits.tool_output_bytes = 4096;
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server_task = tokio::spawn(async move {
        let service = Box::pin(RustMcpServer::new(config)?.serve(server_io)).await?;
        service.waiting().await?;
        anyhow::Ok(())
    });
    let client = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("offline-test", "1"),
    )
    .serve(client_io)
    .await?;
    let response = client
        .call_tool_once(
            CallToolRequestParams::new("workspace")
                .with_arguments(json!({"action":"inspect"}).as_object().unwrap().clone()),
        )
        .await?;
    let CallToolResponse::Complete(result) = &response else {
        anyhow::bail!("expected complete result");
    };
    assert!(serde_json::to_vec(result)?.len() <= 4096);
    assert_eq!(result.is_error, Some(false));
    let value = data(response)?;
    assert!(matches!(value["status"].as_str(), Some("OK" | "PARTIAL")));
    assert!(!fixture.0.join("project/Cargo.lock").exists());
    assert!(!fixture.0.join("project/target").exists());
    client.cancel().await?;
    server_task.await??;
    Ok(())
}
