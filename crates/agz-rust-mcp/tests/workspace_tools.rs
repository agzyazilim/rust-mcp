//! Real filesystem fixtures exercise the zero-process workspace tools.
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use agz_rust_mcp::{
    tools::workspace::{
        WorkspaceAction, WorkspaceData, WorkspaceRequest, WorkspaceStatus, execute_workspace,
    },
    workspace::{ClientRoots, RootGuard, WorkspaceRoot},
};
use tokio_util::sync::CancellationToken;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agz-workspace-tools-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(std::fs::canonicalize(path).unwrap())
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn root(&self) -> WorkspaceRoot {
        RootGuard::new([self.0.clone()], std::iter::empty())
            .unwrap()
            .snapshot(ClientRoots::unsupported())
            .unwrap()
            .select(None)
            .unwrap()
    }
    fn run(&self, request: &WorkspaceRequest) -> WorkspaceData {
        execute_workspace(&self.root(), request, &CancellationToken::new(), 49_152).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn request(action: WorkspaceAction) -> WorkspaceRequest {
    WorkspaceRequest {
        dir: None,
        action,
        path: None,
        query: None,
        case_sensitive: true,
        line: None,
        line_count: None,
        limit: None,
        cursor: None,
        byte_budget: None,
    }
}

#[test]
fn offline_virtual_workspace_preserves_inheritance_aliases_cfg_and_locked_sources() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        r#"
[workspace]
members = ["crates/*"]
[workspace.package]
version = "2.3.4"
edition = "2024"
rust-version = "1.88"
[workspace.dependencies]
json = { package = "serde_json", version = "^1", features = ["alloc"], default-features = false }
"#,
    );
    fixture.write(
        "Cargo.lock",
        r#"
version = 4
[[package]]
name = "serde_json"
version = "1.0.140"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "serde_json"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
    );
    fixture.write(
        "crates/core/Cargo.toml",
        r#"
[package]
name = "example-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
[dependencies]
json = { workspace = true, features = ["std"], optional = true }
[features]
default = ["fast"]
fast = ["dep:json"]
[target.'cfg(unix)'.build-dependencies]
cc = "1.0"
[[example]]
name = "smoke"
path = "examples/smoke.rs"
required-features = ["fast"]
"#,
    );
    fixture.write("crates/core/src/lib.rs", "pub fn hello() {}\n");
    fixture.write("crates/core/examples/smoke.rs", "fn main() {}\n");
    fixture.write("crates/core/tests/api.rs", "#[test] fn works() {}\n");
    let result = fixture.run(&request(WorkspaceAction::Inspect));
    assert_eq!(result.status, WorkspaceStatus::Ok, "{result:#?}");
    let member = result
        .manifests
        .iter()
        .find(|item| item.path == "crates/core/Cargo.toml")
        .unwrap();
    assert_eq!(member.workspace_membership, "confirmed");
    let package = member.package.as_ref().unwrap();
    assert_eq!(package.version.as_deref(), Some("2.3.4"));
    assert_eq!(package.edition.as_deref(), Some("2024"));
    assert_eq!(package.rust_version.as_deref(), Some("1.88"));
    assert!(package.unresolved_fields.is_empty());
    let dependency = member
        .dependencies
        .iter()
        .find(|item| item.name == "json")
        .unwrap();
    assert_eq!(dependency.package, "serde_json");
    assert_eq!(dependency.features, ["alloc", "std"]);
    assert!(dependency.inheritance_resolved);
    assert_eq!(dependency.locked_candidates.len(), 1);
    assert_eq!(dependency.locked_candidates[0].version, "1.0.140");
    assert_eq!(dependency.default_features, Some(false));
    assert_eq!(dependency.optional, Some(true));
    let build = member
        .dependencies
        .iter()
        .find(|item| item.name == "cc")
        .unwrap();
    assert_eq!(build.target.as_deref(), Some("cfg(unix)"));
    assert_eq!(build.kind, "build");
    assert!(member.targets.iter().any(|target| target.kind == "lib"));
    assert!(
        member
            .targets
            .iter()
            .any(|target| target.kind == "test" && target.name.as_deref() == Some("api"))
    );
    let example = member
        .targets
        .iter()
        .find(|target| target.kind == "example")
        .unwrap();
    assert_eq!(example.required_features, ["fast"]);
}

#[test]
fn no_lockfile_and_unconfirmed_workspace_inheritance_are_honest() {
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[workspace]\nmembers = [\"a\"]\nexclude=[\"b\"]\n[workspace.package]\nversion = \"5.0.0\"\n");
    fixture.write("b/Cargo.toml", "[package]\nname = \"b\"\nversion.workspace = true\n[dependencies]\nrenamed = { package = \"serde\", version = \"1\" }\n");
    let result = fixture.run(&request(WorkspaceAction::Inspect));
    let member = result
        .manifests
        .iter()
        .find(|item| item.path == "b/Cargo.toml")
        .unwrap();
    assert_eq!(member.workspace_membership, "unresolved");
    assert!(member.package.as_ref().unwrap().version.is_none());
    assert_eq!(
        member.package.as_ref().unwrap().unresolved_fields,
        ["version"]
    );
    assert!(member.dependencies[0].locked_candidates.is_empty());
    assert!(
        member
            .notices
            .iter()
            .any(|notice| notice.contains("No parsed lockfile"))
    );
}

#[test]
fn narrowed_member_reads_ancestor_metadata_only_within_selected_root() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers=[\"crates/*\"]\n[workspace.package]\nversion=\"3.0.0\"\n",
    );
    fixture.write(
        "crates/member/Cargo.toml",
        "[package]\nname=\"member\"\nversion.workspace=true\n",
    );
    let mut input = request(WorkspaceAction::Inspect);
    input.path = Some("crates/member".into());
    let result = fixture.run(&input);
    assert_eq!(result.manifests.len(), 1);
    assert_eq!(
        result.manifests[0]
            .package
            .as_ref()
            .unwrap()
            .version
            .as_deref(),
        Some("3.0.0")
    );
    let narrow_root = RootGuard::new([fixture.0.join("crates/member")], std::iter::empty())
        .unwrap()
        .snapshot(ClientRoots::unsupported())
        .unwrap()
        .select(None)
        .unwrap();
    input.path = None;
    let result =
        execute_workspace(&narrow_root, &input, &CancellationToken::new(), 49_152).unwrap();
    assert!(
        result.manifests[0]
            .package
            .as_ref()
            .unwrap()
            .version
            .is_none()
    );
}

#[test]
fn search_paginates_sorted_current_sources_without_generated_directories() {
    let fixture = Fixture::new();
    fixture.write("src/z.rs", "pub fn shared() {}\n// shared\n");
    fixture.write("src/a.rs", "pub fn shared() {}\n");
    fixture.write("target/generated.rs", "shared\n");
    fixture.write("vendor/dependency/lib.rs", "shared\n");
    fixture.write("node_modules/pkg/lib.rs", "shared\n");
    fixture.write(".git/hidden.rs", "shared\n");
    let mut input = request(WorkspaceAction::Search);
    input.query = Some("shared".into());
    input.limit = Some(1);
    let first = fixture.run(&input);
    assert_eq!(first.scan.matching_items, 3);
    assert_eq!(first.matches[0].path, "src/a.rs");
    assert_eq!(first.page.next_cursor, Some(1));
    input.cursor = first.page.next_cursor;
    let second = fixture.run(&input);
    assert_eq!(second.matches[0].path, "src/z.rs");
    assert_eq!(second.matches[0].line, 1);
    input.cursor = second.page.next_cursor;
    let third = fixture.run(&input);
    assert_eq!(third.matches[0].line, 2);
    assert!(third.page.next_cursor.is_none());
    fixture.write("src/z.rs", "pub fn replaced() {}\n");
    input.cursor = None;
    let fresh = fixture.run(&input);
    assert_eq!(fresh.scan.matching_items, 1);
}

#[test]
fn utf8_read_and_search_report_actual_lines_and_changed_hashes() {
    let fixture = Fixture::new();
    fixture.write(
        "src/örnek.rs",
        "// Türkçe\r\npub fn çalış() {}\r\n// son\r\n",
    );
    let mut input = request(WorkspaceAction::Read);
    input.path = Some("src/örnek.rs".into());
    input.line = Some(2);
    input.line_count = Some(1);
    let result = fixture.run(&input);
    let file = result.file.unwrap();
    assert_eq!(file.total_lines, 3);
    assert_eq!(file.lines[0].number, 2);
    assert_eq!(file.lines[0].text, "pub fn çalış() {}");
    assert_eq!(result.page.next_line, Some(3));
    fixture.write("src/örnek.rs", "// Türkçe\npub fn değişti() {}\n");
    assert_ne!(file.sha256, fixture.run(&input).file.unwrap().sha256);
    input = request(WorkspaceAction::Search);
    input.query = Some("TÜRKÇE".into());
    input.case_sensitive = false;
    let result = fixture.run(&input);
    assert_eq!(result.matches[0].line, 1);
}

#[test]
fn byte_budget_retains_useful_long_line_and_manifest_results() {
    let fixture = Fixture::new();
    fixture.write("src/lib.rs", &format!("// {} needle\n", "ü".repeat(2_000)));
    let mut input = request(WorkspaceAction::Read);
    input.path = Some("src/lib.rs".into());
    input.byte_budget = Some(2_048);
    let result = fixture.run(&input);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 2_048);
    assert!(!result.file.as_ref().unwrap().lines.is_empty());
    assert!(result.file.as_ref().unwrap().lines[0].truncated);
    input = request(WorkspaceAction::Search);
    input.query = Some("needle".into());
    input.byte_budget = Some(2_048);
    let result = fixture.run(&input);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 2_048);
    assert_eq!(result.matches.len(), 1);
    let mut manifest = "[package]\nname=\"many\"\nversion=\"0.1.0\"\n[dependencies]\n".to_owned();
    for index in 0..90 {
        manifest.push_str(&format!("dependency_{index}=\"1\"\n"));
    }
    fixture.write("Cargo.toml", &manifest);
    input = request(WorkspaceAction::Inspect);
    input.byte_budget = Some(2_048);
    let result = fixture.run(&input);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 2_048);
    assert_eq!(result.manifests.len(), 1);
    assert!(result.manifests[0].omitted_declarations > 0);
    assert_eq!(result.status, WorkspaceStatus::Partial);
}

#[test]
fn cancellation_missing_paths_and_invalid_arguments_have_bounded_results() {
    let fixture = Fixture::new();
    let token = CancellationToken::new();
    token.cancel();
    let result = execute_workspace(
        &fixture.root(),
        &request(WorkspaceAction::Inspect),
        &token,
        49_152,
    )
    .unwrap();
    assert_eq!(result.status, WorkspaceStatus::Cancelled);
    assert_eq!(result.scan.visited_entries, 0);
    let mut input = request(WorkspaceAction::Read);
    input.path = Some("missing.rs".into());
    assert_eq!(fixture.run(&input).status, WorkspaceStatus::NotFound);
    for invalid in ["../outside.rs", "/outside.rs", "a/../../escape", "\0"] {
        input.path = Some(invalid.into());
        assert!(
            execute_workspace(&fixture.root(), &input, &CancellationToken::new(), 49_152).is_err()
        );
    }
    input = request(WorkspaceAction::Search);
    assert!(execute_workspace(&fixture.root(), &input, &CancellationToken::new(), 49_152).is_err());
    input.query = Some("a".into());
    input.limit = Some(201);
    assert!(execute_workspace(&fixture.root(), &input, &CancellationToken::new(), 49_152).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_file_and_directory_are_never_followed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write("secret.rs", "private_target\n");
    symlink(outside.0.join("secret.rs"), fixture.0.join("linked.rs")).unwrap();
    symlink(&outside.0, fixture.0.join("linked-dir")).unwrap();
    let mut input = request(WorkspaceAction::Search);
    input.query = Some("private_target".into());
    let result = fixture.run(&input);
    assert!(result.matches.is_empty());
    assert_eq!(result.status, WorkspaceStatus::Partial);
    input = request(WorkspaceAction::Read);
    input.path = Some("linked.rs".into());
    assert!(execute_workspace(&fixture.root(), &input, &CancellationToken::new(), 49_152).is_err());
    input.path = Some("linked-dir/secret.rs".into());
    assert!(execute_workspace(&fixture.root(), &input, &CancellationToken::new(), 49_152).is_err());
    assert!(outside.0.exists());
}

#[test]
fn inherited_dependency_outside_workspace_is_not_resolved_from_unrelated_ancestor() {
    let fixture = Fixture::new();
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers=[\"other\"]\n[workspace.dependencies]\nserde=\"9\"\n",
    );
    fixture.write(
        "unlisted/Cargo.toml",
        "[package]\nname=\"unlisted\"\nversion=\"1.0.0\"\n[dependencies]\nserde.workspace=true\n",
    );
    let result = fixture.run(&request(WorkspaceAction::Inspect));
    let member = result
        .manifests
        .iter()
        .find(|item| item.path == "unlisted/Cargo.toml")
        .unwrap();
    assert!(!member.dependencies[0].inheritance_resolved);
    assert!(member.dependencies[0].requirement.is_none());
}

#[test]
fn exact_read_budget_never_reports_an_unscanned_search_as_complete() {
    let fixture = Fixture::new();
    let four_mib = "x".repeat(4 * 1024 * 1024);
    for index in 0..8 {
        fixture.write(&format!("a{index}.rs"), &four_mib);
    }
    fixture.write("z.rs", "needle\n");
    let mut input = request(WorkspaceAction::Search);
    input.query = Some("needle".into());
    let capped = fixture.run(&input);
    assert!(capped.matches.is_empty());
    assert_eq!(capped.scan.bytes_read, 32 * 1024 * 1024);
    assert_eq!(capped.status, WorkspaceStatus::Partial);
    assert!(!capped.scan.complete);
    assert!(capped.truncated);
    // The same byte cap is complete when the last seven bytes actually cover
    // the final file; reaching the cap alone is not evidence of omission.
    fixture.write("a7.rs", &four_mib[..four_mib.len() - 7]);
    let complete = fixture.run(&input);
    assert_eq!(complete.scan.bytes_read, 32 * 1024 * 1024);
    assert_eq!(complete.status, WorkspaceStatus::Ok);
    assert!(complete.scan.complete);
    assert_eq!(complete.matches.len(), 1);
    assert_eq!(complete.matches[0].path, "z.rs");
}

#[test]
fn invalid_manifests_and_oversized_declarations_are_reported_as_partial() {
    let fixture = Fixture::new();
    fixture.write("bad/Cargo.toml", "[package\nname = ???\n");
    fixture.write(
        "valid/Cargo.toml",
        "[package]\nname=\"valid\"\nversion=\"0.1.0\"\n",
    );
    let result = fixture.run(&request(WorkspaceAction::Inspect));
    assert_eq!(result.status, WorkspaceStatus::Partial);
    assert_eq!(result.manifests.len(), 1);
    assert!(!result.scan.complete);
    let long = "x".repeat(600);
    fixture.write("valid/Cargo.toml", &format!("[package]\nname=\"valid\"\nversion=\"0.1.0\"\n[dependencies]\nlocal={{path=\"{long}\"}}\n"));
    let mut input = request(WorkspaceAction::Inspect);
    input.path = Some("valid".into());
    let result = fixture.run(&input);
    assert_eq!(result.status, WorkspaceStatus::Partial);
    assert!(result.manifests[0].truncated);
    assert!(result.truncated);
}

#[test]
fn conventional_targets_preserve_library_name_and_multiple_binary_candidates() {
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[package]\nname=\"example-app\"\nversion=\"0.1.0\"\n[[bin]]\nname=\"custom\"\npath=\"src/custom.rs\"\n");
    fixture.write("src/lib.rs", "pub fn shared() {}\n");
    fixture.write("src/main.rs", "fn main() {}\n");
    fixture.write("src/custom.rs", "fn main() {}\n");
    let result = fixture.run(&request(WorkspaceAction::Inspect));
    let targets = &result.manifests[0].targets;
    assert!(
        targets
            .iter()
            .any(|target| target.kind == "lib" && target.name.as_deref() == Some("example_app"))
    );
    assert!(
        targets
            .iter()
            .any(|target| target.kind == "bin" && target.name.as_deref() == Some("example-app"))
    );
    assert!(
        targets
            .iter()
            .any(|target| target.kind == "bin" && target.name.as_deref() == Some("custom"))
    );
}
