//! Fast, offline workspace discovery and source access. No Cargo, shell, network,
//! or language-server process is started. Manifest declarations and lockfile
//! candidates are evidence, not a claim about Cargo's active configuration.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::workspace::{AuthorizedRoot, RootError, WorkspaceRoot};

const MAX_ENTRIES: usize = 30_000;
const MAX_FILES: usize = 4_000;
const MAX_MANIFESTS: usize = 256;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_DECLARATIONS: usize = 128;
const MAX_MATCHES: u32 = 100_000;
const MIN_BUDGET: u64 = 2_048;
const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "vendor",
    ".cache",
    ".rustup",
    ".venv",
];

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAction {
    Inspect,
    Search,
    Read,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    #[serde(default)]
    pub dir: Option<String>,
    pub action: WorkspaceAction,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub line_count: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<u32>,
    #[serde(default)]
    pub byte_budget: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Ok,
    Partial,
    Cancelled,
    NotFound,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceData {
    pub action: WorkspaceAction,
    pub status: WorkspaceStatus,
    pub scope: String,
    pub authority: String,
    pub manifests: Vec<ManifestInfo>,
    pub matches: Vec<SourceMatch>,
    pub file: Option<SourceFile>,
    pub scan: ScanReport,
    pub page: WorkspacePage,
    pub notices: Vec<String>,
    pub truncated: bool,
    pub byte_budget: u64,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub visited_entries: u32,
    pub discovered_files: u32,
    pub files_read: u32,
    pub bytes_read: u64,
    pub matching_items: u32,
    pub skipped_entries: u32,
    pub omitted_declarations: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePage {
    /// Zero-based item offset for inspect/search; read uses `nextLine` instead.
    pub cursor: u32,
    pub limit: u32,
    pub returned: u32,
    pub next_cursor: Option<u32>,
    pub next_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManifestInfo {
    pub path: String,
    pub sha256: String,
    pub package: Option<PackageInfo>,
    pub workspace_manifest: Option<String>,
    /// `confirmed`, `unresolved`, or `standalone`. No Cargo resolution runs.
    pub workspace_membership: String,
    pub members: Vec<String>,
    pub exclude: Vec<String>,
    pub targets: Vec<TargetInfo>,
    pub features: Vec<FeatureInfo>,
    pub dependencies: Vec<DependencyInfo>,
    pub omitted_declarations: u32,
    pub truncated: bool,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub edition: Option<String>,
    pub rust_version: Option<String>,
    pub inherited_fields: Vec<String>,
    pub unresolved_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub kind: String,
    pub name: Option<String>,
    pub path: Option<String>,
    /// Explicit manifest declaration or conventional file candidate.
    pub origin: String,
    pub required_features: Vec<String>,
    pub harness: Option<bool>,
    pub test: Option<bool>,
    pub bench: Option<bool>,
    pub doctest: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureInfo {
    pub name: String,
    pub enables: Vec<String>,
    pub omitted_values: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    /// Local import/dependency key, which may differ from the crate's name.
    pub name: String,
    pub package: String,
    pub requirement: Option<String>,
    pub kind: String,
    pub target: Option<String>,
    pub inherited: bool,
    pub inheritance_resolved: bool,
    pub path: Option<String>,
    pub git: Option<String>,
    pub registry: Option<String>,
    pub rev: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub optional: Option<bool>,
    pub default_features: Option<bool>,
    pub features: Vec<String>,
    pub locked_candidates: Vec<LockedPackage>,
    pub omitted_values: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub lockfile: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceMatch {
    pub path: String,
    pub line: u32,
    pub excerpt: String,
    pub excerpt_truncated: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
    pub total_lines: u32,
    pub start_line: u32,
    pub lines: Vec<SourceLine>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceLine {
    pub number: u32,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkspaceError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Access(String),
}

pub fn validate_workspace_request(request: &WorkspaceRequest) -> Result<(), WorkspaceError> {
    let invalid = |message: &str| WorkspaceError::InvalidInput(message.to_owned());
    if let Some(dir) = &request.dir
        && (dir.trim().is_empty() || dir.contains('\0'))
    {
        return Err(invalid("dir must be a nonempty directory path"));
    }
    if let Some(path) = &request.path {
        validate_path(path)?;
    }
    if request
        .limit
        .is_some_and(|limit| !(1..=200).contains(&limit))
    {
        return Err(invalid("limit must be between 1 and 200"));
    }
    if request.cursor.is_some_and(|cursor| cursor > MAX_MATCHES) {
        return Err(invalid("cursor exceeds the bounded result offset"));
    }
    if request
        .byte_budget
        .is_some_and(|budget| budget < MIN_BUDGET)
    {
        return Err(invalid("byteBudget must be at least 2048 bytes"));
    }
    match request.action {
        WorkspaceAction::Search => {
            let Some(query) = request.query.as_deref() else {
                return Err(invalid("search requires query"));
            };
            if query.is_empty() || query.len() > 512 || query.contains(['\0', '\n', '\r']) {
                return Err(invalid("query must be 1..512 bytes on a single line"));
            }
            if request.line.is_some() || request.line_count.is_some() {
                return Err(invalid("line and lineCount are only accepted for read"));
            }
        }
        WorkspaceAction::Read => {
            if request.path.as_deref().is_none_or(|path| path == ".") {
                return Err(invalid("read requires a relative file path"));
            }
            if request.line == Some(0)
                || request
                    .line_count
                    .is_some_and(|count| !(1..=500).contains(&count))
            {
                return Err(invalid(
                    "line is one-based; lineCount must be between 1 and 500",
                ));
            }
            if request.query.is_some() || request.cursor.is_some() || request.limit.is_some() {
                return Err(invalid("read uses line/lineCount, not query/cursor/limit"));
            }
        }
        WorkspaceAction::Inspect => {
            if request.query.is_some() || request.line.is_some() || request.line_count.is_some() {
                return Err(invalid("inspect does not accept query/line/lineCount"));
            }
        }
    }
    Ok(())
}

/// Run against the request-selected capability root. `dir` is only consumed by
/// the MCP adapter when selecting that root; it never widens this function's access.
pub fn execute_workspace(
    root: &WorkspaceRoot,
    request: &WorkspaceRequest,
    cancellation: &CancellationToken,
    output_limit: u64,
) -> Result<WorkspaceData, WorkspaceError> {
    validate_workspace_request(request)?;
    if output_limit < MIN_BUDGET {
        return Err(WorkspaceError::InvalidInput(
            "tool output limit is below 2048 bytes".into(),
        ));
    }
    let budget = request.byte_budget.unwrap_or(16_384).min(output_limit);
    let mut data = WorkspaceData {
        action: request.action,
        status: WorkspaceStatus::Ok,
        scope: request.path.clone().unwrap_or_else(|| ".".into()),
        authority: match request.action {
            WorkspaceAction::Inspect => "manifest_and_lockfile_declarations",
            WorkspaceAction::Search => "literal_source_matches_not_semantic_references",
            WorkspaceAction::Read => "workspace_source",
        }
        .into(),
        manifests: Vec::new(),
        matches: Vec::new(),
        file: None,
        scan: ScanReport {
            complete: true,
            ..ScanReport::default()
        },
        page: WorkspacePage {
            cursor: request.cursor.unwrap_or(0),
            limit: request.limit.unwrap_or(32),
            returned: 0,
            next_cursor: None,
            next_line: None,
        },
        notices: Vec::new(),
        truncated: false,
        byte_budget: budget,
    };
    if cancellation.is_cancelled() {
        data.status = WorkspaceStatus::Cancelled;
        data.scan.complete = false;
        return Ok(data);
    }
    let authority = root.requested_authority();
    if request.action == WorkspaceAction::Read {
        read_action(authority, request, cancellation, &mut data)?;
    } else {
        let files = discover(authority, request.path.as_deref(), cancellation, &mut data)?;
        if request.action == WorkspaceAction::Inspect {
            inspect_action(authority, &files, request, cancellation, &mut data);
        } else {
            search_action(authority, &files, request, cancellation, &mut data);
        }
    }
    if cancellation.is_cancelled() {
        data.status = WorkspaceStatus::Cancelled;
        data.scan.complete = false;
    }
    if !data.scan.complete && data.status == WorkspaceStatus::Ok {
        data.status = WorkspaceStatus::Partial;
    }
    enforce_budget(&mut data);
    Ok(data)
}

fn validate_path(path: &str) -> Result<(), WorkspaceError> {
    let path_value = Path::new(path);
    if path.is_empty()
        || path.len() > 4_096
        || path.contains('\0')
        || path_value.is_absolute()
        || path_value.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkspaceError::InvalidInput(
            "path must be relative to the selected root without '..' components".into(),
        ));
    }
    Ok(())
}

fn access_error(error: &RootError) -> WorkspaceError {
    WorkspaceError::Access(
        match error {
            RootError::Symlink(_) => "symlink paths are not followed",
            RootError::NotRegularFile(_) => "path must be a regular file",
            RootError::NotDirectory(_) => "path must be a directory",
            RootError::PathOutsideRoot(_) | RootError::InvalidPath(_) => {
                "path is outside the selected root"
            }
            RootError::TooLarge { .. } => "file exceeds the 4 MiB read limit",
            RootError::PathNotFound(_) => "path does not exist",
            _ => "path could not be accessed inside the selected root",
        }
        .into(),
    )
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn partial(data: &mut WorkspaceData, notice: &str) {
    data.scan.complete = false;
    data.truncated = true;
    if data.notices.len() < 12 && !data.notices.iter().any(|item| item == notice) {
        data.notices.push(notice.into());
    }
}

/// Directory enumeration itself is bounded. When a directory cannot be fully
/// enumerated within the entry budget, discard its unsorted partial list rather
/// than returning filesystem-order-dependent pages.
fn discover(
    authority: &Arc<AuthorizedRoot>,
    scope: Option<&str>,
    cancellation: &CancellationToken,
    data: &mut WorkspaceData,
) -> Result<Vec<PathBuf>, WorkspaceError> {
    let scope = scope
        .filter(|scope| *scope != ".")
        .map_or_else(PathBuf::new, PathBuf::from);
    let selected = if scope.as_os_str().is_empty() {
        Arc::clone(authority)
    } else {
        match authority.authorize_dir(&scope) {
            Ok(selected) => selected,
            Err(RootError::NotDirectory(_)) => {
                authority
                    .open_file(&scope, MAX_FILE_BYTES)
                    .map_err(|error| access_error(&error))?;
                data.scan.discovered_files = 1;
                return Ok(vec![scope]);
            }
            Err(RootError::PathNotFound(_)) => {
                data.status = WorkspaceStatus::NotFound;
                return Ok(Vec::new());
            }
            Err(error) => return Err(access_error(&error)),
        }
    };
    let directory = selected
        .dir()
        .try_clone()
        .map_err(|_| WorkspaceError::Access("selected directory is unavailable".into()))?;
    let mut pending = vec![(directory, scope, 0usize)];
    let mut files = Vec::new();
    while let Some((directory, relative, depth)) = pending.pop() {
        if cancellation.is_cancelled() {
            break;
        }
        let Ok(iterator) = directory.entries() else {
            data.scan.skipped_entries += 1;
            partial(data, "An unreadable directory was skipped.");
            continue;
        };
        let mut entries = Vec::new();
        let mut exhausted = false;
        for entry in iterator {
            if cancellation.is_cancelled() {
                return Ok(files);
            }
            if data.scan.visited_entries as usize >= MAX_ENTRIES {
                exhausted = true;
                break;
            }
            data.scan.visited_entries += 1;
            match entry {
                Ok(entry) => entries.push(entry),
                Err(_) => {
                    data.scan.skipped_entries += 1;
                    partial(data, "An unreadable directory entry was skipped.");
                }
            }
        }
        if exhausted {
            partial(
                data,
                "The 30000-entry scan limit was reached; narrow path to inspect additional subtrees.",
            );
            break;
        }
        entries.sort_by_key(cap_std::fs::DirEntry::file_name);
        let mut children = Vec::new();
        for entry in entries {
            if cancellation.is_cancelled() {
                return Ok(files);
            }
            let name = entry.file_name();
            let path = relative.join(&name);
            let Ok(kind) = entry.file_type() else {
                data.scan.skipped_entries += 1;
                partial(data, "An unreadable directory entry was skipped.");
                continue;
            };
            if kind.is_symlink() {
                data.scan.skipped_entries += 1;
                partial(
                    data,
                    "Symlinks are skipped; no linked source or directory was followed.",
                );
                continue;
            }
            if kind.is_dir() {
                if SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip) {
                    data.scan.skipped_entries += 1;
                    continue;
                }
                if depth >= MAX_DEPTH {
                    data.scan.skipped_entries += 1;
                    partial(data, "The directory depth limit was reached.");
                    continue;
                }
                if let Ok(child) = entry.open_dir() {
                    children.push((child, path, depth + 1));
                } else {
                    data.scan.skipped_entries += 1;
                    partial(data, "An unreadable directory was skipped.");
                }
            } else if kind.is_file() && relevant_file(&path) {
                if files.len() >= MAX_FILES {
                    partial(
                        data,
                        "The 4000-file discovery limit was reached; narrow path to continue.",
                    );
                    files.sort();
                    data.scan.discovered_files = files.len() as u32;
                    return Ok(files);
                }
                files.push(path);
            }
        }
        pending.extend(children.into_iter().rev());
    }
    files.sort();
    data.scan.discovered_files = files.len() as u32;
    Ok(files)
}

fn relevant_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
        || path
            .file_name()
            .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock")
}

fn read_text(authority: &AuthorizedRoot, path: &Path, data: &mut WorkspaceData) -> Option<String> {
    let remaining = MAX_TOTAL_BYTES.saturating_sub(data.scan.bytes_read);
    if remaining == 0 {
        partial(
            data,
            "The 32 MiB source-read budget was reached; narrow path to continue.",
        );
        return None;
    }
    let bytes = match authority.read_file(path, remaining.min(MAX_FILE_BYTES)) {
        Ok(bytes) => bytes,
        Err(_) => {
            data.scan.skipped_entries += 1;
            partial(
                data,
                "A file could not be read within the regular-file/4 MiB limit.",
            );
            return None;
        }
    };
    if data.scan.bytes_read.saturating_add(bytes.len() as u64) > MAX_TOTAL_BYTES {
        partial(
            data,
            "The 32 MiB source-read budget was reached; narrow path to continue.",
        );
        return None;
    }
    data.scan.bytes_read += bytes.len() as u64;
    data.scan.files_read += 1;
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            data.scan.skipped_entries += 1;
            partial(data, "A non-UTF-8 file was skipped.");
            None
        }
    }
}

fn read_action(
    authority: &AuthorizedRoot,
    request: &WorkspaceRequest,
    cancellation: &CancellationToken,
    data: &mut WorkspaceData,
) -> Result<(), WorkspaceError> {
    let path = request.path.as_deref().unwrap_or("");
    let bytes = match authority.read_file(Path::new(path), MAX_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(RootError::PathNotFound(_)) => {
            data.status = WorkspaceStatus::NotFound;
            return Ok(());
        }
        Err(error) => return Err(access_error(&error)),
    };
    data.scan.bytes_read = bytes.len() as u64;
    data.scan.files_read = 1;
    let hash = sha256(&bytes);
    let text =
        String::from_utf8(bytes).map_err(|_| WorkspaceError::Access("file is not UTF-8".into()))?;
    let start = request.line.unwrap_or(1);
    let count = request.line_count.unwrap_or(100);
    let total = text.lines().count() as u32;
    let mut lines = Vec::new();
    for (index, line) in text
        .lines()
        .enumerate()
        .skip(start.saturating_sub(1) as usize)
        .take(count as usize)
    {
        if cancellation.is_cancelled() {
            break;
        }
        let (text, truncated) = bounded(line, 2_000);
        lines.push(SourceLine {
            number: index as u32 + 1,
            text,
            truncated,
        });
    }
    data.scan.matching_items = total;
    data.page.limit = count;
    data.page.returned = lines.len() as u32;
    data.page.next_line = lines
        .last()
        .and_then(|line| (line.number < total).then_some(line.number + 1));
    let truncated = lines.iter().any(|line| line.truncated) || data.page.next_line.is_some();
    data.truncated = truncated;
    data.file = Some(SourceFile {
        path: display(Path::new(path)),
        sha256: hash,
        total_lines: total,
        start_line: start,
        lines,
        truncated,
    });
    Ok(())
}

fn search_action(
    authority: &AuthorizedRoot,
    files: &[PathBuf],
    request: &WorkspaceRequest,
    cancellation: &CancellationToken,
    data: &mut WorkspaceData,
) {
    let query = request.query.as_deref().unwrap_or("");
    let needle = if request.case_sensitive {
        query.into()
    } else {
        query.to_lowercase()
    };
    let cursor = data.page.cursor;
    let limit = data.page.limit;
    for path in files {
        if cancellation.is_cancelled() || data.scan.matching_items >= MAX_MATCHES {
            break;
        }
        if path.file_name().is_some_and(|name| name == "Cargo.lock") {
            continue;
        }
        if data.scan.bytes_read >= MAX_TOTAL_BYTES {
            partial(
                data,
                "The 32 MiB source-read budget was reached; narrow path to continue.",
            );
            break;
        }
        let Some(text) = read_text(authority, path, data) else {
            continue;
        };
        let mut hash = None;
        for (index, line) in text.lines().enumerate() {
            if cancellation.is_cancelled() {
                return;
            }
            let matched = if request.case_sensitive {
                line.contains(&needle)
            } else {
                line.to_lowercase().contains(&needle)
            };
            if !matched {
                continue;
            }
            let offset = data.scan.matching_items;
            data.scan.matching_items += 1;
            if offset >= cursor && offset < cursor.saturating_add(limit) {
                let (excerpt, excerpt_truncated) =
                    match_excerpt(line, query, request.case_sensitive);
                data.matches.push(SourceMatch {
                    path: display(path),
                    line: index as u32 + 1,
                    excerpt,
                    excerpt_truncated,
                    sha256: hash.get_or_insert_with(|| sha256(text.as_bytes())).clone(),
                });
            }
            if data.scan.matching_items >= MAX_MATCHES {
                partial(
                    data,
                    "The 100000-match scan limit was reached; use a more specific query.",
                );
                break;
            }
        }
    }
    data.page.returned = data.matches.len() as u32;
    data.page.next_cursor = (cursor + data.page.returned < data.scan.matching_items)
        .then_some(cursor + data.page.returned);
    data.truncated |= data.page.next_cursor.is_some();
    data.notices.push("Literal search of .rs and Cargo.toml files. Generated/cache directories and symlinks are skipped; .gitignore is not interpreted. Pages are recomputed from current files.".into());
}

fn match_excerpt(line: &str, query: &str, case_sensitive: bool) -> (String, bool) {
    // For insensitive matches Unicode lowercasing can change byte offsets;
    // character windows are safer than indexing the original string with those offsets.
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= 600 {
        return (line.into(), false);
    }
    let character_offset = if case_sensitive {
        line.find(query)
            .map_or(0, |byte| line[..byte].chars().count())
    } else {
        let lowered = line.to_lowercase();
        let offset = lowered.find(&query.to_lowercase()).unwrap_or(0);
        let mut folded_bytes = 0;
        let mut position = 0;
        for character in line.chars() {
            if folded_bytes >= offset {
                break;
            }
            folded_bytes += character.to_lowercase().map(char::len_utf8).sum::<usize>();
            position += 1;
        }
        position
    };
    let start = character_offset.saturating_sub(8);
    (chars.iter().skip(start).take(600).collect(), true)
}

#[derive(Debug)]
struct ParsedManifest {
    value: toml::Value,
    hash: String,
}

type LockIndex = BTreeMap<String, Vec<LockedPackage>>;

fn inspect_action(
    authority: &AuthorizedRoot,
    files: &[PathBuf],
    request: &WorkspaceRequest,
    cancellation: &CancellationToken,
    data: &mut WorkspaceData,
) {
    let mut manifests = BTreeMap::new();
    let mut locks = BTreeMap::new();
    for (file_index, path) in files.iter().enumerate() {
        if cancellation.is_cancelled() {
            return;
        }
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            if manifests.len() >= MAX_MANIFESTS {
                partial(
                    data,
                    "The 256-manifest limit was reached; narrow path to inspect remaining packages.",
                );
                break;
            }
            if let Some(text) = read_text(authority, path, data) {
                match toml::from_str::<toml::Value>(&text) {
                    Ok(value) => {
                        manifests.insert(
                            path.clone(),
                            ParsedManifest {
                                value,
                                hash: sha256(text.as_bytes()),
                            },
                        );
                    }
                    Err(_) => partial(
                        data,
                        "An invalid Cargo.toml was skipped; use read to inspect its contents.",
                    ),
                }
            }
        } else if path.file_name().is_some_and(|name| name == "Cargo.lock")
            && let Some(text) = read_text(authority, path, data)
        {
            match text.parse::<cargo_lock::Lockfile>() {
                Ok(lock) => {
                    let mut index = LockIndex::new();
                    for package in lock.packages {
                        let name = package.name.to_string();
                        index.entry(name.clone()).or_default().push(LockedPackage {
                            name,
                            version: package.version.to_string(),
                            source: package.source.as_ref().map(ToString::to_string),
                            lockfile: display(path),
                        });
                    }
                    locks.insert(path.clone(), index);
                }
                Err(_) => partial(
                    data,
                    "An invalid Cargo.lock was skipped; dependency declarations remain available.",
                ),
            }
        }
        if data.scan.bytes_read >= MAX_TOTAL_BYTES {
            if files[file_index + 1..].iter().any(|path| {
                path.file_name()
                    .is_some_and(|name| name == "Cargo.toml" || name == "Cargo.lock")
            }) {
                partial(
                    data,
                    "The 32 MiB source-read budget was reached; narrow path to continue.",
                );
            }
            break;
        }
    }
    // A narrowed member inspection can still inherit from ancestors inside the
    // selected capability root. Never look above that root or run Cargo.
    let initial: Vec<PathBuf> = manifests.keys().cloned().collect();
    let mut attempted: BTreeSet<PathBuf> = manifests.keys().cloned().collect();
    for path in &initial {
        for parent in path
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .skip(1)
            .take(MAX_DEPTH)
        {
            let manifest = parent.join("Cargo.toml");
            if cancellation.is_cancelled() {
                return;
            }
            if attempted.insert(manifest.clone())
                && manifests.len() < MAX_MANIFESTS
                && let Ok(bytes) = authority.read_file(
                    &manifest,
                    MAX_FILE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(data.scan.bytes_read)),
                )
                && data.scan.bytes_read.saturating_add(bytes.len() as u64) <= MAX_TOTAL_BYTES
            {
                data.scan.bytes_read += bytes.len() as u64;
                data.scan.files_read += 1;
                if let Ok(text) = String::from_utf8(bytes)
                    && let Ok(value) = toml::from_str::<toml::Value>(&text)
                {
                    manifests.insert(
                        manifest,
                        ParsedManifest {
                            value,
                            hash: sha256(text.as_bytes()),
                        },
                    );
                }
            }
        }
    }
    let ancestor_locks: BTreeSet<PathBuf> = manifests
        .keys()
        .filter_map(|path| path.parent().map(|parent| parent.join("Cargo.lock")))
        .collect();
    for path in ancestor_locks {
        if cancellation.is_cancelled() {
            return;
        }
        if !locks.contains_key(&path)
            && let Ok(bytes) = authority.read_file(
                &path,
                MAX_FILE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(data.scan.bytes_read)),
            )
        {
            data.scan.bytes_read += bytes.len() as u64;
            data.scan.files_read += 1;
            if let Ok(text) = String::from_utf8(bytes)
                && let Ok(lock) = text.parse::<cargo_lock::Lockfile>()
            {
                let mut index = LockIndex::new();
                for package in lock.packages {
                    let name = package.name.to_string();
                    index.entry(name.clone()).or_default().push(LockedPackage {
                        name,
                        version: package.version.to_string(),
                        source: package.source.as_ref().map(ToString::to_string),
                        lockfile: display(&path),
                    });
                }
                locks.insert(path, index);
            }
        }
    }
    let source_files: BTreeSet<&Path> = files.iter().map(PathBuf::as_path).collect();
    data.scan.matching_items = initial.len() as u32;
    for path in initial
        .iter()
        .skip(data.page.cursor as usize)
        .take(data.page.limit as usize)
    {
        if cancellation.is_cancelled() {
            break;
        }
        let Some(parsed) = manifests.get(path) else {
            continue;
        };
        let (workspace_path, confirmed) = workspace_for(path, &parsed.value, &manifests);
        let workspace = workspace_path
            .as_ref()
            .and_then(|path| manifests.get(path))
            .map(|parsed| &parsed.value);
        let inherited = confirmed.then_some(workspace).flatten();
        let lock_path = workspace_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|path| path.join("Cargo.lock"));
        let index = lock_path
            .as_ref()
            .and_then(|path| locks.get(path))
            .or_else(|| {
                path.parent()
                    .into_iter()
                    .flat_map(Path::ancestors)
                    .find_map(|parent| locks.get(&parent.join("Cargo.lock")))
            });
        let mut info = manifest_info(path, parsed, inherited, index, &source_files);
        info.workspace_manifest = workspace_path.as_ref().map(|path| display(path));
        info.workspace_membership = if confirmed {
            "confirmed"
        } else if workspace_path.is_some() {
            "unresolved"
        } else {
            "standalone"
        }
        .into();
        if workspace_path.is_some() && !confirmed {
            info.notices.push("Ancestor workspace membership could not be confirmed without Cargo; inherited fields remain unresolved.".into());
        }
        if index.is_none() {
            info.notices.push(
                "No parsed lockfile is available; version requirements are not exact resolutions."
                    .into(),
            );
        }
        data.scan.omitted_declarations += info.omitted_declarations;
        if info.truncated {
            data.truncated = true;
            data.status = WorkspaceStatus::Partial;
        }
        data.manifests.push(info);
        data.page.returned = data.manifests.len() as u32;
        let intended_count = data.manifests.len();
        enforce_budget(data);
        if data.manifests.len() < intended_count {
            break;
        }
    }
    data.page.returned = data.manifests.len() as u32;
    data.page.next_cursor = (data.page.returned > 0
        && data.page.cursor + data.page.returned < data.scan.matching_items)
        .then_some(data.page.cursor + data.page.returned);
    data.truncated |= data.page.next_cursor.is_some() || data.scan.omitted_declarations > 0;
    data.notices.push("Offline manifest/lockfile inspection, not Cargo resolution. Features and targets are declarations/candidates; cfg, build scripts, target support and installed toolchains are not evaluated. Generated/cache directories and symlinks are skipped; .gitignore is not interpreted.".into());
    let _ = request;
}

fn workspace_for(
    path: &Path,
    value: &toml::Value,
    manifests: &BTreeMap<PathBuf, ParsedManifest>,
) -> (Option<PathBuf>, bool) {
    if value.get("workspace").is_some() {
        return (Some(path.into()), true);
    }
    let base = path.parent().unwrap_or(Path::new(""));
    if let Some(explicit) = value
        .get("package")
        .and_then(|package| package.get("workspace"))
        .and_then(toml::Value::as_str)
    {
        let Some(root) = normalize_inside(&base.join(explicit)) else {
            return (None, false);
        };
        let manifest = root.join("Cargo.toml");
        let confirmed = manifests
            .get(&manifest)
            .is_some_and(|parsed| parsed.value.get("workspace").is_some());
        return (Some(manifest), confirmed);
    }
    for parent in base.ancestors().skip(1) {
        let manifest = parent.join("Cargo.toml");
        let Some(workspace) = manifests
            .get(&manifest)
            .and_then(|parsed| parsed.value.get("workspace"))
        else {
            continue;
        };
        let relative = base.strip_prefix(parent).map(display).unwrap_or_default();
        let excluded = workspace
            .get("exclude")
            .and_then(toml::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .any(|pattern| glob_matches(pattern, &relative))
            });
        let member = workspace
            .get("members")
            .and_then(toml::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .any(|pattern| glob_matches(pattern, &relative))
            });
        let uncertain_exclusion = workspace
            .get("exclude")
            .and_then(toml::Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .any(|pattern| !supported_glob(pattern))
            });
        return (Some(manifest), member && !excluded && !uncertain_exclusion);
    }
    (None, false)
}

fn normalize_inside(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => (),
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(result)
}

/// Conservative workspace member matching: `*`, `?`, and whole `**` path
/// segments are supported. Bracket/escape patterns stay unresolved, rather than
/// claiming Cargo membership from an approximate glob implementation.
fn supported_glob(pattern: &str) -> bool {
    pattern.len() <= 4096
        && !pattern.contains(['[', ']', '\\'])
        && pattern.split('/').count() <= MAX_DEPTH
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if !supported_glob(pattern) || value.len() > 4096 {
        return false;
    }
    let pattern: Vec<_> = pattern.trim_end_matches('/').split('/').collect();
    let value: Vec<_> = value.trim_end_matches('/').split('/').collect();
    if value.len() > MAX_DEPTH {
        return false;
    }
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for segment in pattern {
        let mut current = vec![false; value.len() + 1];
        if segment == "**" {
            current[0] = previous[0];
        }
        for index in 1..=value.len() {
            current[index] = if segment == "**" {
                previous[index] || current[index - 1]
            } else {
                previous[index - 1] && wildcard(segment, value[index - 1])
            };
        }
        previous = current;
    }
    previous[value.len()]
}

fn wildcard(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut p, mut v, mut star, mut saved) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            saved = v;
        } else if let Some(previous) = star {
            saved += 1;
            v = saved;
            p = previous + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn manifest_info(
    path: &Path,
    parsed: &ParsedManifest,
    workspace: Option<&toml::Value>,
    lock: Option<&LockIndex>,
    files: &BTreeSet<&Path>,
) -> ManifestInfo {
    let value = &parsed.value;
    let mut info = ManifestInfo {
        path: display(path),
        sha256: parsed.hash.clone(),
        package: None,
        workspace_manifest: None,
        workspace_membership: "standalone".into(),
        members: Vec::new(),
        exclude: Vec::new(),
        targets: Vec::new(),
        features: Vec::new(),
        dependencies: Vec::new(),
        omitted_declarations: 0,
        truncated: false,
        notices: Vec::new(),
    };
    if let Some(package) = value.get("package") {
        let mut inherited_fields = Vec::new();
        let mut unresolved_fields = Vec::new();
        for key in ["name", "version", "edition", "rust-version"] {
            info.truncated |= package.get(key).is_some_and(has_long_text);
            info.truncated |= workspace
                .and_then(|value| value.get("workspace"))
                .and_then(|value| value.get("package"))
                .and_then(|value| value.get(key))
                .is_some_and(has_long_text);
        }
        let mut field = |name: &str| {
            if package
                .get(name)
                .and_then(|value| value.get("workspace"))
                .and_then(toml::Value::as_bool)
                == Some(true)
            {
                inherited_fields.push(name.into());
                let result = workspace
                    .and_then(|value| value.get("workspace"))
                    .and_then(|value| value.get("package"))
                    .and_then(|value| value.get(name))
                    .and_then(toml::Value::as_str)
                    .map(short);
                if result.is_none() {
                    unresolved_fields.push(name.into());
                }
                result
            } else {
                string(package, name)
            }
        };
        let name = field("name");
        let version = field("version");
        let edition = field("edition");
        let rust_version = field("rust-version");
        info.package = Some(PackageInfo {
            name,
            version,
            edition,
            rust_version,
            inherited_fields,
            unresolved_fields,
        });
    }
    if let Some(workspace) = value.get("workspace") {
        info.truncated |= workspace.get("members").is_some_and(has_long_text)
            || workspace.get("exclude").is_some_and(has_long_text);
        info.members = strings(workspace.get("members"), &mut info.omitted_declarations);
        info.exclude = strings(workspace.get("exclude"), &mut info.omitted_declarations);
    }
    if let Some(features) = value.get("features").and_then(toml::Value::as_table) {
        for (name, enables) in features.iter().take(MAX_DECLARATIONS) {
            info.truncated |= name.chars().count() > 512 || has_long_text(enables);
            let mut omitted_values = 0;
            let enables = strings(Some(enables), &mut omitted_values);
            info.features.push(FeatureInfo {
                name: short(name),
                enables,
                omitted_values,
            });
            info.omitted_declarations += omitted_values;
        }
        info.omitted_declarations += features.len().saturating_sub(MAX_DECLARATIONS) as u32;
    }
    for (key, kind) in [
        ("dependencies", "normal"),
        ("dev-dependencies", "dev"),
        ("build-dependencies", "build"),
    ] {
        add_dependencies(&mut info, value.get(key), kind, None, workspace, lock);
    }
    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        for (target, value) in targets {
            for (key, kind) in [
                ("dependencies", "normal"),
                ("dev-dependencies", "dev"),
                ("build-dependencies", "build"),
            ] {
                add_dependencies(
                    &mut info,
                    value.get(key),
                    kind,
                    Some(target),
                    workspace,
                    lock,
                );
            }
        }
    }
    collect_targets(
        &mut info,
        value,
        path.parent().unwrap_or(Path::new("")),
        files,
    );
    if info.truncated {
        info.notices.push("Some declaration text exceeds 512 characters and is truncated; use read for the full declaration.".into());
    }
    info.truncated |= info.omitted_declarations > 0;
    info
}

fn add_dependencies(
    info: &mut ManifestInfo,
    table: Option<&toml::Value>,
    kind: &str,
    target: Option<&str>,
    workspace: Option<&toml::Value>,
    lock: Option<&LockIndex>,
) {
    let Some(table) = table.and_then(toml::Value::as_table) else {
        return;
    };
    for (name, declaration) in table {
        if info.dependencies.len() >= MAX_DECLARATIONS {
            info.omitted_declarations += 1;
            continue;
        }
        let inherited = declaration.get("workspace").and_then(toml::Value::as_bool) == Some(true);
        let workspace_declaration = workspace
            .and_then(|value| value.get("workspace"))
            .and_then(|value| value.get("dependencies"))
            .and_then(|value| value.get(name));
        let value = if inherited {
            workspace_declaration
        } else {
            Some(declaration)
        };
        info.truncated |= name.chars().count() > 512
            || target.is_some_and(|target| target.chars().count() > 512)
            || value.is_some_and(has_long_text)
            || has_long_text(declaration);
        let package = value
            .and_then(|value| string(value, "package"))
            .unwrap_or_else(|| short(name));
        let requirement = value.and_then(|value| {
            value
                .as_str()
                .map(short)
                .or_else(|| string(value, "version"))
        });
        let mut omitted = 0;
        let mut features = strings(value.and_then(|value| value.get("features")), &mut omitted);
        if inherited {
            features.extend(strings(declaration.get("features"), &mut omitted));
            features.sort();
            features.dedup();
        }
        if features.len() > MAX_DECLARATIONS {
            omitted += (features.len() - MAX_DECLARATIONS) as u32;
            features.truncate(MAX_DECLARATIONS);
        }
        let get = |key| value.and_then(|value| string(value, key));
        let path = get("path");
        let git = get("git");
        let requirement_parsed = requirement
            .as_deref()
            .and_then(|requirement| cargo_metadata::semver::VersionReq::parse(requirement).ok());
        let candidates: Vec<_> = lock
            .into_iter()
            .filter_map(|index| index.get(&package))
            .flatten()
            .filter(|candidate| {
                let version_matches = requirement_parsed.as_ref().is_none_or(|requirement| {
                    cargo_metadata::semver::Version::parse(&candidate.version)
                        .is_ok_and(|version| requirement.matches(&version))
                });
                let source_matches = if path.is_some() {
                    candidate.source.is_none()
                } else if let Some(git) = &git {
                    candidate.source.as_ref().is_some_and(|source| {
                        source.strip_prefix("git+").is_some_and(|source| {
                            source.split(['?', '#']).next() == Some(git.as_str())
                        })
                    })
                } else {
                    true
                };
                version_matches && source_matches
            })
            .collect();
        omitted += candidates.len().saturating_sub(16) as u32;
        let default_features = declaration
            .get("default-features")
            .and_then(toml::Value::as_bool)
            .or_else(|| {
                value
                    .and_then(|value| value.get("default-features"))
                    .and_then(toml::Value::as_bool)
            });
        info.dependencies.push(DependencyInfo {
            name: short(name),
            package,
            requirement,
            kind: kind.into(),
            target: target.map(short),
            inherited,
            inheritance_resolved: !inherited || workspace_declaration.is_some(),
            path,
            git,
            registry: get("registry"),
            rev: get("rev"),
            tag: get("tag"),
            branch: get("branch"),
            optional: declaration.get("optional").and_then(toml::Value::as_bool),
            default_features,
            features,
            locked_candidates: candidates.into_iter().take(16).cloned().collect(),
            omitted_values: omitted,
        });
        info.omitted_declarations += omitted;
    }
}

fn collect_targets(
    info: &mut ManifestInfo,
    value: &toml::Value,
    base: &Path,
    files: &BTreeSet<&Path>,
) {
    for kind in ["lib", "bin", "example", "test", "bench"] {
        let declarations: Vec<&toml::Value> = if kind == "lib" {
            value.get(kind).into_iter().collect()
        } else {
            value
                .get(kind)
                .and_then(toml::Value::as_array)
                .map_or_else(Vec::new, |values| values.iter().collect())
        };
        for declaration in declarations {
            info.truncated |= has_long_text(declaration);
            if info.targets.len() >= MAX_DECLARATIONS {
                info.omitted_declarations += 1;
                continue;
            }
            let required_features = strings(
                declaration.get("required-features"),
                &mut info.omitted_declarations,
            );
            info.targets.push(TargetInfo {
                kind: kind.into(),
                name: string(declaration, "name"),
                path: string(declaration, "path"),
                origin: "declared".into(),
                required_features,
                harness: boolean(declaration, "harness"),
                test: boolean(declaration, "test"),
                bench: boolean(declaration, "bench"),
                doctest: boolean(declaration, "doctest"),
            });
        }
    }
    let package_name = info
        .package
        .as_ref()
        .and_then(|package| package.name.clone());
    if package_name.is_none() {
        return;
    }
    for (kind, candidate, flag) in [
        ("lib", "src/lib.rs", "autolib"),
        ("bin", "src/main.rs", "autobins"),
    ] {
        if value
            .get("package")
            .and_then(|package| boolean(package, flag))
            == Some(false)
        {
            continue;
        }
        let candidate_name = if kind == "lib" {
            package_name.as_ref().map(|name| name.replace('-', "_"))
        } else {
            package_name.clone()
        };
        if files.contains(base.join(candidate).as_path())
            && !info.targets.iter().any(|target| {
                target.kind == kind && (kind == "lib" || target.name == candidate_name)
            })
        {
            info.targets.push(TargetInfo {
                kind: kind.into(),
                name: candidate_name,
                path: Some(candidate.into()),
                origin: "conventional_candidate".into(),
                required_features: Vec::new(),
                harness: None,
                test: None,
                bench: None,
                doctest: None,
            });
        }
    }
    for (kind, directory, flag) in [
        ("bin", "src/bin", "autobins"),
        ("example", "examples", "autoexamples"),
        ("test", "tests", "autotests"),
        ("bench", "benches", "autobenches"),
    ] {
        if value
            .get("package")
            .and_then(|package| boolean(package, flag))
            == Some(false)
        {
            continue;
        }
        let prefix = base.join(directory);
        for path in files.range(prefix.as_path()..) {
            if !path.starts_with(&prefix) {
                break;
            }
            let Ok(relative) = path.strip_prefix(&prefix) else {
                continue;
            };
            let count = relative.components().count();
            let name = if count == 1 && path.extension().is_some_and(|ext| ext == "rs") {
                path.file_stem()
            } else if count == 2 && path.file_name().is_some_and(|name| name == "main.rs") {
                path.parent().and_then(Path::file_name)
            } else {
                None
            };
            let Some(name) = name.and_then(|name| name.to_str()) else {
                continue;
            };
            if info
                .targets
                .iter()
                .any(|target| target.kind == kind && target.name.as_deref() == Some(name))
            {
                continue;
            }
            if info.targets.len() >= MAX_DECLARATIONS {
                info.omitted_declarations += 1;
                continue;
            }
            info.targets.push(TargetInfo {
                kind: kind.into(),
                name: Some(short(name)),
                path: path.strip_prefix(base).ok().map(display),
                origin: "conventional_candidate".into(),
                required_features: Vec::new(),
                harness: None,
                test: None,
                bench: None,
                doctest: None,
            });
        }
    }
    info.targets.sort_by(|left, right| {
        (&left.kind, &left.name, &left.path).cmp(&(&right.kind, &right.name, &right.path))
    });
}

fn string(value: &toml::Value, key: &str) -> Option<String> {
    value.get(key).and_then(toml::Value::as_str).map(short)
}
fn boolean(value: &toml::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(toml::Value::as_bool)
}
fn has_long_text(value: &toml::Value) -> bool {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            toml::Value::String(text) if text.chars().count() > 512 => return true,
            toml::Value::Array(values) => pending.extend(values),
            toml::Value::Table(values) => {
                if values.keys().any(|key| key.chars().count() > 512) {
                    return true;
                }
                pending.extend(values.values());
            }
            _ => (),
        }
    }
    false
}

fn short(value: &str) -> String {
    bounded(value, 512).0
}
fn bounded(value: &str, chars: usize) -> (String, bool) {
    let text: String = value.chars().take(chars).collect();
    let truncated = text.len() < value.len();
    (text, truncated)
}
fn strings(value: Option<&toml::Value>, omitted: &mut u32) -> Vec<String> {
    let Some(values) = value.and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    *omitted += values.len().saturating_sub(MAX_DECLARATIONS) as u32;
    values
        .iter()
        .take(MAX_DECLARATIONS)
        .filter_map(toml::Value::as_str)
        .map(short)
        .collect()
}

/// Counts serialization without allocating its output and stops immediately at
/// the response budget, including when one nested declaration is very large.
fn fits_wire_budget(data: &WorkspaceData) -> bool {
    struct Counter {
        remaining: u64,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() as u64 > self.remaining {
                return Err(std::io::Error::other("workspace response byte budget"));
            }
            self.remaining -= bytes.len() as u64;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(
        &mut Counter {
            remaining: data.byte_budget,
        },
        data,
    )
    .is_ok()
}

fn enforce_budget(data: &mut WorkspaceData) {
    while !fits_wire_budget(data) {
        data.truncated = true;
        if let Some(file) = data.file.as_mut() {
            file.truncated = true;
            if file.lines.len() == 1 && file.lines[0].text.chars().count() > 32 {
                let line = &mut file.lines[0];
                line.text = bounded(&line.text, line.text.chars().count() / 2).0;
                line.truncated = true;
                continue;
            }
            if let Some(line) = file.lines.pop() {
                data.page.next_line = Some(line.number);
                data.page.returned = file.lines.len() as u32;
                continue;
            }
        }
        if data.matches.len() == 1 && data.matches[0].excerpt.chars().count() > 32 {
            let item = &mut data.matches[0];
            item.excerpt = bounded(&item.excerpt, item.excerpt.chars().count() / 2).0;
            item.excerpt_truncated = true;
            continue;
        }
        if data.matches.pop().is_some() {
            data.page.returned = data.matches.len() as u32;
            data.page.next_cursor = if data.page.returned > 0 {
                Some(data.page.cursor + data.page.returned)
            } else {
                None
            };
            if data.page.returned == 0 {
                if data.status != WorkspaceStatus::Cancelled {
                    data.status = WorkspaceStatus::Partial;
                }
                data.notices.push(
                    "A matching item exceeds byteBudget; increase the budget or narrow path/query."
                        .into(),
                );
            }
            continue;
        }
        if data.manifests.len() > 1 {
            data.manifests.pop();
            data.page.returned = data.manifests.len() as u32;
            data.page.next_cursor = Some(data.page.cursor + data.page.returned);
            continue;
        }
        if let Some(manifest) = data.manifests.first_mut() {
            let reduced = manifest.dependencies.pop().is_some()
                || manifest.features.pop().is_some()
                || manifest.targets.pop().is_some()
                || manifest.members.pop().is_some()
                || manifest.exclude.pop().is_some();
            if reduced {
                manifest.omitted_declarations += 1;
                manifest.truncated = true;
                data.scan.omitted_declarations += 1;
                if data.status != WorkspaceStatus::Cancelled {
                    data.status = WorkspaceStatus::Partial;
                }
                continue;
            }
        }
        if data.notices.pop().is_some() {
            continue;
        }
        if let Some(manifest) = data.manifests.first_mut()
            && manifest.notices.pop().is_some()
        {
            continue;
        }
        // Even paths and package metadata can be large. Keep a bounded, honest
        // result instead of violating the negotiated tool output cap.
        if !data.manifests.is_empty() {
            data.manifests.clear();
            data.page.returned = 0;
            data.page.next_cursor = None;
            if data.status != WorkspaceStatus::Cancelled {
                data.status = WorkspaceStatus::Partial;
            }
            data.notices.push("The requested item exceeds byteBudget; increase the budget or use read with a smaller range.".into());
            continue;
        }
        if data.file.is_some() {
            data.file = None;
            data.page.returned = 0;
            data.page.next_line = None;
            continue;
        }
        data.scope = bounded(&data.scope, 128).0;
        if data.status != WorkspaceStatus::Cancelled {
            data.status = WorkspaceStatus::Partial;
        }
        break;
    }
}
