use std::{collections::BTreeMap, fs, path::Path};

use agz_rust_mcp::Config;

const ENGLISH: &str = include_str!("../../../README.md");
const TURKISH: &str = include_str!("../../../README.tr.md");
const TOOLS_ENGLISH: &str = include_str!("../../../docs/tools.md");
const TOOLS_TURKISH: &str = include_str!("../../../docs/tools.tr.md");
const INSTALL_ENGLISH: &str = include_str!("../../../docs/install.md");
const INSTALL_TURKISH: &str = include_str!("../../../docs/install.tr.md");
const ARCHITECTURE_ENGLISH: &str = include_str!("../../../docs/architecture.md");
const ARCHITECTURE_TURKISH: &str = include_str!("../../../docs/architecture.tr.md");
const BENCHMARK_ENGLISH: &str = include_str!("../../../docs/benchmark.md");
const BENCHMARK_TURKISH: &str = include_str!("../../../docs/benchmark.tr.md");
const PACKAGE_README: &str = include_str!("../README.md");

#[test]
fn bilingual_public_contract_matches_runtime_and_package_versions() {
    assert_contract(ENGLISH, TURKISH).expect("bilingual README contract");
    assert!(PACKAGE_README.contains(&format!("`{}`", env!("CARGO_PKG_VERSION"))));
    for tool in Config::defaults_at("/workspace").enabled_tool_names() {
        assert!(
            PACKAGE_README.contains(&format!("`{tool}`")),
            "package README omits {tool}"
        );
    }
}

#[test]
fn drift_validator_rejects_version_config_tool_link_and_duplicate_changes() {
    let current_version = format!("`{}`", env!("CARGO_PKG_VERSION"));
    let wrong_version = TURKISH.replacen(&current_version, "`9.9.9`", 1);
    assert!(assert_contract(ENGLISH, &wrong_version).is_err());

    let wrong_config = TURKISH.replacen("`49152`", "`49151`", 1);
    assert!(assert_contract(ENGLISH, &wrong_config).is_err());
    assert!(assert_contract(&wrong_config, &wrong_config).is_err());

    let wrong_tool = TURKISH.replacen("| `check` |", "| `validate` |", 1);
    assert!(assert_contract(ENGLISH, &wrong_tool).is_err());

    let (sdk_version, _) = manifest_versions();
    let wrong_link = TURKISH.replacen(
        &format!("https://docs.rs/rmcp/{sdk_version}/rmcp/"),
        "https://docs.rs/rmcp/latest/rmcp/",
        1,
    );
    assert!(assert_contract(ENGLISH, &wrong_link).is_err());

    let duplicate = format!("{TURKISH}\n| `server.transport` | `stdio` | duplicate |\n");
    assert!(assert_contract(ENGLISH, &duplicate).is_err());
}

#[test]
fn paired_public_docs_preserve_protocol_and_workflow_contracts() {
    assert_shared_markers(
        TOOLS_ENGLISH,
        TOOLS_TURKISH,
        &[
            "`workspace`",
            "`test`",
            "`run`",
            "`check`",
            "`audit`",
            "`crate_lookup`",
            "`docs`",
            "`context`",
            "`symbol`",
            "`references`",
            "`definition`",
            "`symbols`",
            "`implementations`",
            "`hierarchy`",
            "`rename`",
            "`refactor`",
            "`change`",
            "`repair`",
            "`repair.max_candidates`",
            "`change.scratch_dir`",
            "`gate.scope`",
            "`rust_analyzer.workspace_code`",
            "`telemetry.enabled`",
            "`context.max_capsules`",
            "`context.capsule_ttl_ms`",
            "`context.max_items`",
        ],
    );
    assert_shared_markers(
        INSTALL_ENGLISH,
        INSTALL_TURKISH,
        &[
            "--transport http --http-bind 127.0.0.1:8787",
            "http://127.0.0.1:8787/mcp",
            "workspace(action=\"skills\")",
            "AGZ_RUST_MCP_SERVER__TRANSPORT=http",
            "AGZ_RUST_MCP_SERVER__HTTP_BIND=127.0.0.1:8787",
            "skills export --dir /path/to/new-skills",
        ],
    );
    assert_shared_markers(
        ARCHITECTURE_ENGLISH,
        ARCHITECTURE_TURKISH,
        &["`CreateTaskResult`", "`2025-11-25`", "`2026-07-28`"],
    );
    assert_shared_markers(
        BENCHMARK_ENGLISH,
        BENCHMARK_TURKISH,
        &[
            "cargo run -p xtask -- protocol-smoke",
            "cargo run -p xtask -- benchmark-smoke",
            "cargo run -p xtask -- task-benchmark-smoke",
            "`run_id`",
            "`source_commit`",
            "`source_checksum`",
            "`provider` / `model` / `variant`",
            "`non_inferiority_margin`",
        ],
    );
}

fn assert_shared_markers(english: &str, turkish: &str, markers: &[&str]) {
    for marker in markers {
        assert!(
            english.contains(marker),
            "English document missing {marker}"
        );
        assert!(
            turkish.contains(marker),
            "Turkish document missing {marker}"
        );
    }
}

fn manifest_versions() -> (String, String) {
    let manifest: toml::Value =
        toml::from_str(include_str!("../../../Cargo.toml")).expect("workspace manifest");
    let sdk = manifest["workspace"]["dependencies"]["rmcp"]["version"]
        .as_str()
        .expect("rmcp version")
        .trim_start_matches('=')
        .to_owned();
    let rust = manifest["workspace"]["package"]["rust-version"]
        .as_str()
        .expect("MSRV");
    let msrv = if rust.split('.').count() == 2 {
        format!("{rust}.0")
    } else {
        rust.to_owned()
    };
    (sdk, msrv)
}

fn assert_contract(english: &str, turkish: &str) -> Result<(), String> {
    let (sdk_version, msrv) = manifest_versions();
    for value in [
        env!("CARGO_PKG_VERSION"),
        sdk_version.as_str(),
        msrv.as_str(),
        "2025-11-25",
        "2026-07-28",
        "agz-rust-mcp-v<version>",
    ] {
        let marker = format!("`{value}`");
        let count = english.matches(&marker).count();
        if count == 0 || count != turkish.matches(&marker).count() {
            return Err(format!("version/protocol marker drift: {marker}"));
        }
    }

    let config = Config::defaults_at("/workspace");
    let tools = config.enabled_tool_names();
    let english_tools = keyed_table(english, &tools)?;
    let turkish_tools = keyed_table(turkish, &tools)?;
    for tool in tools {
        for rows in [&english_tools, &turkish_tools] {
            let row = &rows[tool];
            if row.len() != 3 || row[1] != "enabled" {
                return Err(format!("tool name/default drift: {tool}"));
            }
        }
    }

    let defaults = BTreeMap::from([
        (
            "server.transport",
            format!("{:?}", config.server.transport).to_ascii_lowercase(),
        ),
        ("server.http_bind", config.server.http_bind.to_string()),
        ("server.allow_roots", "canonical CWD".to_owned()),
        ("server.allow_dependency_roots", "empty".to_owned()),
        (
            "gate.hard_timeout_ms",
            config.gate.hard_timeout_ms.to_string(),
        ),
        (
            "gate.scope",
            format!("{:?}", config.gate.scope).to_ascii_lowercase(),
        ),
        (
            "gate.cache",
            format!("{:?}", config.gate.cache).to_ascii_lowercase(),
        ),
        (
            "rust_analyzer.workspace_code",
            format!("{:?}", config.rust_analyzer.workspace_code).to_ascii_lowercase(),
        ),
        (
            "docs.fallback",
            format!("{:?}", config.docs.fallback).to_ascii_lowercase(),
        ),
        (
            "limits.tool_output_bytes",
            config.limits.tool_output_bytes.to_string(),
        ),
        ("change.max_bytes", config.change.max_bytes.to_string()),
        (
            "repair.max_candidates",
            config.repair.max_candidates.to_string(),
        ),
        (
            "repair.max_compiles",
            config.repair.max_compiles.to_string(),
        ),
        (
            "repair.wall_time_ms",
            config.repair.wall_time_ms.to_string(),
        ),
        (
            "repair.minimize_max_candidates",
            config.repair.minimize_max_candidates.to_string(),
        ),
        (
            "repair.minimize_max_compiles",
            config.repair.minimize_max_compiles.to_string(),
        ),
        (
            "profile.max_report_bytes",
            config.profile.max_report_bytes.to_string(),
        ),
        ("profile.max_runs", config.profile.max_runs.to_string()),
        (
            "profile.compare_samples",
            config.profile.compare_samples.to_string(),
        ),
        (
            "work.max_candidates",
            config.work.max_candidates.to_string(),
        ),
        ("work.max_compiles", config.work.max_compiles.to_string()),
        ("work.wall_time_ms", config.work.wall_time_ms.to_string()),
        ("telemetry.enabled", config.telemetry.enabled.to_string()),
    ]);
    let config_keys = defaults.keys().copied().collect::<Vec<_>>();
    for document in [english, turkish] {
        let rows = keyed_table(document, &config_keys)?;
        for (key, default) in &defaults {
            if rows[*key].get(1) != Some(default) {
                return Err(format!("configuration differs from runtime default: {key}"));
            }
        }
    }

    for link in [
        "https://github.com/agzyazilim/rust-mcp".to_owned(),
        format!("https://docs.rs/rmcp/{sdk_version}/rmcp/"),
        "https://modelcontextprotocol.io/specification/2025-11-25".to_owned(),
        "https://modelcontextprotocol.io/specification/2026-07-28".to_owned(),
    ] {
        if !english.contains(&link) || !turkish.contains(&link) {
            return Err(format!("canonical link drift: {link}"));
        }
    }
    Ok(())
}

fn keyed_table(document: &str, keys: &[&str]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut rows = BTreeMap::new();
    for line in document.lines().filter(|line| line.starts_with('|')) {
        let columns = line
            .trim_matches('|')
            .split('|')
            .map(|column| column.trim().trim_matches('`').to_owned())
            .collect::<Vec<_>>();
        let Some(key) = columns.first() else { continue };
        if keys.contains(&key.as_str()) && rows.insert(key.clone(), columns.clone()).is_some() {
            return Err(format!("duplicate table row {key}"));
        }
    }
    for key in keys {
        if !rows.contains_key(*key) {
            return Err(format!("missing table row {key}"));
        }
    }
    Ok(rows)
}

#[test]
fn package_readme_and_license_are_in_the_publish_allowlist() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("read package manifest");
    assert!(manifest.contains("README.md"));
    assert!(manifest.contains("LICENSE"));
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("README.md")
            .is_file()
    );
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("LICENSE")
            .is_file()
    );

    let package_readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
            .expect("read package README");
    assert!(package_readme.contains("mcp-name: io.github.agzyazilim/rust-mcp"));
}

#[test]
fn mcp_registry_metadata_matches_the_cargo_package() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("server.json")).expect("read server.json"),
    )
    .expect("parse server.json");

    let expected = serde_json::json!({
        "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
        "name": "io.github.agzyazilim/rust-mcp",
        "title": "AGZ Rust MCP",
        "description": "Rust development tools for offline context, compiler diagnostics, focused tests and Cargo execution.",
        "repository": {
            "url": "https://github.com/agzyazilim/rust-mcp",
            "source": "github"
        },
        "version": env!("CARGO_PKG_VERSION"),
        "packages": [{
            "registryType": "cargo",
            "registryBaseUrl": "https://crates.io",
            "identifier": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "transport": { "type": "stdio" }
        }]
    });

    assert_eq!(metadata, expected);
}
