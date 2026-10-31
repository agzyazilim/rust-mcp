use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
        ClientCapabilities, ClientConfig, ExtensionCapabilities, GetPromptRequestParams,
        GetPromptResponse, GetTaskParams, Implementation, JsonObject, ProtocolVersion,
        ReadResourceRequestParams, ReadResourceResponse, TASKS_EXTENSION_ID, TaskPayload,
        UpdateTaskParams,
    },
    transport::child_process::TokioChildProcess,
};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio::process::Command;

#[derive(Debug, Serialize)]
pub struct ProtocolEvidence {
    pub status: &'static str,
    pub default_protocol: &'static str,
    pub discovered_protocol: &'static str,
    pub tool_names: Vec<String>,
    pub prompt_count: usize,
    pub resource_count: usize,
    pub embedded_skill_count: usize,
    pub embedded_skill_interfaces_equivalent: bool,
    pub synchronous_status: String,
    pub task_status: String,
    pub structured_text_equivalent: bool,
    pub task_equivalent: bool,
    pub task_lifecycle: TaskLifecycleEvidence,
    pub provider_requests: u32,
}

#[derive(Debug, Serialize)]
pub struct TaskLifecycleEvidence {
    pub update_acknowledged: bool,
    pub cancelled: bool,
    pub cleanup_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckObservation {
    pub status: String,
    pub is_error: bool,
    pub structured_text_equivalent: bool,
    pub reason: String,
    pub duration_ms: u128,
}

#[allow(clippy::too_many_lines)]
///
/// # Errors
/// Returns an error if the server cannot be launched or any protocol, skill, task, or cleanup check fails.
pub async fn protocol_smoke(root: &Path) -> Result<ProtocolEvidence> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/stage7/protocol/expectations.json"
    ))
    .context("parse protocol smoke fixture")?;
    let expected_tools = fixture
        .pointer("/tools")
        .and_then(Value::as_array)
        .context("protocol fixture tools")?;

    let transport = TokioChildProcess::new(server_command(root))
        .context("spawn MCP stdio server for protocol smoke")?;
    let client = client_info(ClientCapabilities::default())
        .serve(transport)
        .await
        .context("initialize capability-free MCP client")?;
    let peer_info = client.peer_info().context("missing server peer info")?;
    if peer_info.protocol_version != ProtocolVersion::V_2025_11_25 {
        bail!("unexpected default protocol version");
    }
    if peer_info
        .server_info
        .as_ref()
        .map(|info| info.name.as_ref())
        != Some("agz-rust-mcp")
    {
        bail!("unexpected server implementation name");
    }
    if !peer_info
        .instructions
        .as_deref()
        .is_some_and(|instructions| instructions.contains("write-free"))
    {
        bail!("write-free guidance is missing from initialize");
    }

    let tools = client
        .peer()
        .list_tools(None)
        .await
        .context("list MCP tools")?;
    let tool_names: Vec<_> = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let expected_tool_names: Vec<_> = expected_tools
        .iter()
        .map(|tool| tool.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .context("protocol fixture tool names")?;
    if tool_names != expected_tool_names
        || tools.tools.iter().any(|tool| tool.output_schema.is_none())
    {
        bail!("MCP tool catalog does not match the frozen protocol fixture");
    }

    let prompts = client.peer().list_prompts(None).await?;
    let prompt_names: Vec<_> = prompts
        .prompts
        .iter()
        .map(|prompt| prompt.name.as_str())
        .collect();
    let expected_prompts: Vec<_> = fixture["prompts"]
        .as_array()
        .context("protocol fixture prompt names")?
        .iter()
        .map(|name| name.as_str().context("prompt name must be a string"))
        .collect::<Result<_>>()?;
    if prompt_names != expected_prompts {
        bail!("MCP prompt catalog does not match the frozen protocol fixture");
    }
    let resources = client.peer().list_resources(None).await?;
    let expected_resource_count = usize::try_from(
        fixture["resource_count"]
            .as_u64()
            .context("protocol fixture resource count")?,
    )
    .context("protocol fixture resource count does not fit usize")?;
    if resources.resources.len() != expected_resource_count {
        bail!("MCP resource catalog does not match the frozen protocol fixture");
    }
    let embedded_skill_count = verify_embedded_skills(&client, &expected_prompts).await?;

    let synchronous = call_and_extract(&client, CallToolRequestParams::new("check")).await?;
    if synchronous.status != "FAST_PASS" || synchronous.is_error {
        bail!(
            "capability-free check returned status={} is_error={} instead of FAST_PASS: {}",
            synchronous.status,
            synchronous.is_error,
            synchronous.reason
        );
    }
    client.cancel().await.context("close protocol client")?;

    let task_transport = TokioChildProcess::new(server_command(root))
        .context("spawn MCP stdio server for task smoke")?;
    let task_client = client_info(task_capabilities())
        .serve_with_lifecycle(
            task_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .context("initialize task-capable MCP client")?;
    if task_client
        .peer_info()
        .context("missing discovered server peer info")?
        .protocol_version
        != ProtocolVersion::V_2026_07_28
    {
        bail!("task discovery did not negotiate protocol version 2026-07-28");
    }
    let task_response = task_client
        .call_tool_once(CallToolRequestParams::new("check"))
        .await
        .context("call check with task capability")?;
    let task = match task_response {
        CallToolResponse::Task(task) => task,
        other => bail!("expected MCP task response, got {other:?}"),
    };
    let task_result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = task_client
                .peer()
                .get_task(GetTaskParams::new(task.task.task_id.clone()))
                .await?;
            match current.task.payload {
                TaskPayload::Completed { result } => {
                    let result = serde_json::from_value::<CallToolResult>(Value::Object(result))?;
                    break Ok::<_, anyhow::Error>(result);
                }
                TaskPayload::Failed { error } => bail!("MCP task failed: {error:?}"),
                TaskPayload::Cancelled => bail!("MCP task was cancelled"),
                TaskPayload::Working | TaskPayload::InputRequired { .. } => {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                _ => bail!("MCP task returned an unsupported payload"),
            }
        }
    })
    .await
    .context("MCP task completion timeout")??;
    let task_status = result_status(&task_result)?;
    if task_status != synchronous.status
        || task_result.is_error != Some(synchronous.is_error)
        || !same_structured_and_text(&task_result)?
    {
        bail!("task and synchronous MCP results differ");
    }
    task_client.cancel().await.context("close task client")?;
    let (task_update_acknowledged, task_cancelled, task_cleanup_verified) =
        task_cancel_smoke(root).await?;

    Ok(ProtocolEvidence {
        status: "PASS",
        default_protocol: "2025-11-25",
        discovered_protocol: "2026-07-28",
        tool_names,
        prompt_count: prompts.prompts.len(),
        resource_count: resources.resources.len(),
        embedded_skill_count,
        embedded_skill_interfaces_equivalent: true,
        synchronous_status: synchronous.status,
        task_status,
        structured_text_equivalent: synchronous.structured_text_equivalent,
        task_equivalent: true,
        task_lifecycle: TaskLifecycleEvidence {
            update_acknowledged: task_update_acknowledged,
            cancelled: task_cancelled,
            cleanup_verified: task_cleanup_verified,
        },
        provider_requests: 0,
    })
}

async fn verify_embedded_skills<S>(
    client: &rmcp::service::RunningService<rmcp::service::RoleClient, S>,
    expected_prompts: &[&str],
) -> Result<usize>
where
    S: rmcp::service::Service<rmcp::service::RoleClient>,
{
    let catalog = call_skill_tool(client, None).await?;
    let skills = catalog
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/skills"))
        .and_then(Value::as_array)
        .context("embedded skill catalog has no skills")?;
    if skills.len() != expected_prompts.len() {
        bail!("embedded skill catalog and prompt catalog differ");
    }
    for (skill, expected_prompt) in skills.iter().zip(expected_prompts) {
        let name = skill["name"].as_str().context("embedded skill name")?;
        let prompt_name = skill["prompt"].as_str().context("embedded skill prompt")?;
        let uri = skill["uri"].as_str().context("embedded skill URI")?;
        if prompt_name != *expected_prompt || skill.get("markdown").is_some() {
            bail!("embedded skill catalog must contain ordered metadata without bodies");
        }
        let result = call_skill_tool(client, Some(name)).await?;
        let body = result
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/skills/0/markdown"))
            .and_then(Value::as_str)
            .context("embedded skill tool has no markdown body")?;
        let expected_hash = skill["sha256"]
            .as_str()
            .context("embedded skill checksum")?;
        if hex::encode(Sha256::digest(body.as_bytes())) != expected_hash {
            bail!("embedded skill body does not match its catalog checksum");
        }

        let prompt = client
            .peer()
            .get_prompt_once(GetPromptRequestParams::new(prompt_name))
            .await?;
        let GetPromptResponse::Complete(prompt) = prompt else {
            bail!("embedded skill prompt did not complete");
        };
        let resource = client
            .peer()
            .read_resource_once(ReadResourceRequestParams::new(uri))
            .await?;
        let ReadResourceResponse::Complete(resource) = resource else {
            bail!("embedded skill resource did not complete");
        };
        if serde_json::to_value(prompt)?["messages"][0]["content"]["text"] != body
            || serde_json::to_value(resource)?["contents"][0]["text"] != body
        {
            bail!("embedded skill tool, prompt, and resource contents differ");
        }
    }
    Ok(skills.len())
}

async fn call_skill_tool<S>(
    client: &rmcp::service::RunningService<rmcp::service::RoleClient, S>,
    skill: Option<&str>,
) -> Result<CallToolResult>
where
    S: rmcp::service::Service<rmcp::service::RoleClient>,
{
    let mut arguments = Map::new();
    arguments.insert("action".to_owned(), Value::String("skills".to_owned()));
    if let Some(skill) = skill {
        arguments.insert("skill".to_owned(), Value::String(skill.to_owned()));
    }
    let response = client
        .call_tool_once(CallToolRequestParams::new("workspace").with_arguments(arguments))
        .await?;
    let CallToolResponse::Complete(result) = response else {
        bail!("capability-free embedded skill call unexpectedly returned a task");
    };
    if result.is_error == Some(true)
        || result_status(&result)? != "OK"
        || !same_structured_and_text(&result)?
        || result
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/authority"))
            .and_then(Value::as_str)
            != Some("bundled_skill_guidance")
    {
        bail!("embedded skill tool did not return complete, consistent bundled guidance");
    }
    Ok(result)
}

async fn task_cancel_smoke(root: &Path) -> Result<(bool, bool, bool)> {
    let fixture = ProtocolTaskFixture::new()?;
    let mut command = server_command(root);
    command
        .arg("--allow-root")
        .arg(&fixture.workspace)
        .arg("--docs-fallback")
        .arg("local")
        .arg("--docs-cache-dir")
        .arg(&fixture.cache)
        .arg("--docs-timeout-ms")
        .arg("30000");
    let transport = TokioChildProcess::new(command)
        .context("spawn MCP stdio server for task cancellation smoke")?;
    let client = client_info(task_capabilities())
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .context("initialize task cancellation client")?;
    let mut arguments = Map::new();
    arguments.insert(
        "crate".to_owned(),
        Value::String("stage7-task-cancel".to_owned()),
    );
    arguments.insert("expensiveFallback".to_owned(), Value::Bool(true));
    let task = match client
        .call_tool_once(CallToolRequestParams::new("docs").with_arguments(arguments))
        .await?
    {
        CallToolResponse::Task(task) => task,
        other => bail!("expected cancellable docs task, got {other:?}"),
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if fixture.started.is_file() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("cancellable docs build script did not start")?;
    let task_id = task.task.task_id;
    let current = client
        .peer()
        .get_task(GetTaskParams::new(task_id.clone()))
        .await?;
    if current.task.payload.status().is_terminal() {
        bail!("cancellable docs task completed before update/cancel smoke");
    }
    client
        .peer()
        .update_task(UpdateTaskParams::new(task_id.clone(), BTreeMap::new()))
        .await
        .context("tasks/update acknowledgement")?;
    client
        .peer()
        .cancel_task(CancelTaskParams::new(task_id.clone()))
        .await
        .context("tasks/cancel acknowledgement")?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = client
                .peer()
                .get_task(GetTaskParams::new(task_id.clone()))
                .await?;
            match current.task.payload {
                TaskPayload::Cancelled => break Ok::<_, anyhow::Error>(()),
                TaskPayload::Completed { result } => {
                    bail!("cancelled docs task completed instead: {result:?}")
                }
                TaskPayload::Failed { error } => {
                    bail!("cancelled docs task failed instead: {error:?}")
                }
                TaskPayload::Working | TaskPayload::InputRequired { .. } => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                _ => bail!("cancelled docs task returned an unsupported payload"),
            }
        }
    })
    .await
    .context("cancelled docs task did not reach a terminal state")??;
    client
        .cancel()
        .await
        .context("close task cancellation client")?;
    fs::remove_dir_all(&fixture.base).context("remove task cancellation fixture")?;
    Ok((true, true, !fixture.base.exists()))
}

struct ProtocolTaskFixture {
    base: PathBuf,
    workspace: PathBuf,
    cache: PathBuf,
    started: PathBuf,
}

impl ProtocolTaskFixture {
    fn new() -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = fs::canonicalize(std::env::temp_dir())
            .context("canonical protocol task temp directory")?
            .join(format!(
                "agz-rust-mcp-protocol-task-{}-{timestamp}",
                std::process::id()
            ));
        let workspace = base.join("workspace");
        let cache = base.join("cache");
        let started = base.join("build-script.pid");
        fs::create_dir_all(workspace.join("src")).context("create task cancellation fixture")?;
        fs::create_dir(&cache).context("create task cancellation cache")?;
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"stage7-task-cancel\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )?;
        fs::write(
            workspace.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"stage7-task-cancel\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(workspace.join("src/lib.rs"), "pub struct TaskCancel;\n")?;
        let started_literal =
            serde_json::to_string(&started.to_string_lossy()).context("encode marker path")?;
        fs::write(
            workspace.join("build.rs"),
            format!(
                "fn main() {{ std::fs::write({started_literal}, std::process::id().to_string()).unwrap(); std::thread::sleep(std::time::Duration::from_secs(30)); }}\n"
            ),
        )?;
        Ok(Self {
            base,
            workspace,
            cache,
            started,
        })
    }
}

impl Drop for ProtocolTaskFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

///
/// # Errors
/// Returns an error if the server cannot be launched or the check response violates the protocol contract.
pub async fn check(root: &Path, directory: &Path) -> Result<CheckObservation> {
    let mut command = server_command(root);
    command.arg("--allow-root").arg(directory);
    let transport =
        TokioChildProcess::new(command).context("spawn MCP stdio server for benchmark arm")?;
    let client = client_info(ClientCapabilities::default())
        .serve(transport)
        .await
        .context("initialize benchmark MCP client")?;
    let mut arguments = Map::new();
    arguments.insert(
        "dir".to_owned(),
        Value::String(directory.to_string_lossy().into_owned()),
    );
    arguments.insert("target".to_owned(), Value::String("check".to_owned()));
    arguments.insert("detail".to_owned(), Value::String("compact".to_owned()));
    let started = Instant::now();
    let mut observation = call_and_extract(
        &client,
        CallToolRequestParams::new("check").with_arguments(arguments),
    )
    .await?;
    observation.duration_ms = started.elapsed().as_millis();
    client
        .cancel()
        .await
        .context("close benchmark MCP client")?;
    Ok(observation)
}

fn server_command(root: &Path) -> Command {
    if let Some(binary) = server_binary(root) {
        let mut command = Command::new(binary);
        command.current_dir(root).env_remove("AGZ_RUST_MCP_BIN");
        return command;
    }

    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["run", "--quiet", "--locked", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .args(["-p", "agz-rust-mcp", "--"])
        .env_remove("AGZ_RUST_MCP_BIN");
    command
}

fn server_binary(root: &Path) -> Option<PathBuf> {
    let configured = std::env::var_os("AGZ_RUST_MCP_BIN").map(PathBuf::from);
    let mut candidates = configured.into_iter().chain([
        root.join("target/debug/agz-rust-mcp"),
        root.join("target/release/agz-rust-mcp"),
    ]);
    candidates.find(|path| {
        fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    })
}

fn client_info(capabilities: ClientCapabilities) -> ClientConfig {
    ClientConfig::new(
        capabilities,
        Implementation::new("stage7-xtask", env!("CARGO_PKG_VERSION")),
    )
}

fn task_capabilities() -> ClientCapabilities {
    let mut capabilities = ClientCapabilities::default();
    let mut extensions = ExtensionCapabilities::new();
    extensions.insert(TASKS_EXTENSION_ID.to_owned(), JsonObject::new());
    capabilities.extensions = Some(extensions);
    capabilities
}

async fn call_and_extract<S>(
    client: &rmcp::service::RunningService<rmcp::service::RoleClient, S>,
    request: CallToolRequestParams,
) -> Result<CheckObservation>
where
    S: rmcp::service::Service<rmcp::service::RoleClient>,
{
    let response = client.call_tool_once(request).await?;
    let CallToolResponse::Complete(result) = response else {
        bail!("unexpected task response for capability-free call");
    };
    let status = result_status(&result)?;
    let reason = result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/reason"))
        .and_then(Value::as_str)
        .unwrap_or("no reason supplied")
        .to_owned();
    Ok(CheckObservation {
        status,
        is_error: result.is_error == Some(true),
        structured_text_equivalent: same_structured_and_text(&result)?,
        reason,
        duration_ms: 0,
    })
}

fn result_status(result: &CallToolResult) -> Result<String> {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("MCP result has no typed status")
}

fn same_structured_and_text(result: &CallToolResult) -> Result<bool> {
    let structured = result
        .structured_content
        .as_ref()
        .context("MCP result has no structured content")?;
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .context("MCP result has no text fallback")?;
    Ok(serde_json::from_str::<Value>(&text.text)? == *structured)
}
