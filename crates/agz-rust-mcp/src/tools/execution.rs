//! Focused Cargo test discovery/execution and binary/example execution.
//!
//! Every call starts a fresh bounded operation. Cargo owns compilation and
//! target resolution; this service never constructs a shell command, modifies
//! source, or treats successful compilation as proof that tests ran.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::{
    config::Config,
    diagnostics::{CargoStream, Diagnostic, EvidenceStats, sanitize_text},
    gate::{
        CargoTargetSelection, GateMode, GateTarget, GateTargetId, ValidationOptions,
        select_gate_cache,
    },
    process::{ProcessError, ProcessRunOptions, ProcessSupervisor},
    workspace::{AuthorizedRoot, ClientRoots, WorkspaceSnapshot},
};

use super::CheckService;

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TestAction {
    List,
    #[default]
    Run,
    Compile,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IgnoredTests {
    #[default]
    Exclude,
    Include,
    Only,
}

/// Cargo test selection, independent of MCP authorization and framing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestRequest {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub action: TestAction,
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub packages: Vec<String>,
    #[serde(default)]
    pub cargo_target: Option<CargoTargetSelection>,
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub features: Vec<String>,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub no_default_features: bool,
    #[serde(default)]
    pub target_triple: Option<String>,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub ignored: IgnoredTests,
    #[serde(default)]
    #[schemars(range(min = 1, max = 256))]
    pub threads: Option<u16>,
    #[serde(default)]
    pub no_fail_fast: bool,
    #[serde(default)]
    pub release: bool,
    #[serde(default)]
    #[schemars(range(min = 1, max = 3_600_000))]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_max_items")]
    #[schemars(range(min = 1, max = 1000))]
    pub max_items: usize,
}

const fn default_max_items() -> usize {
    100
}

impl Default for TestRequest {
    fn default() -> Self {
        Self {
            dir: None,
            action: TestAction::Run,
            packages: Vec::new(),
            cargo_target: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target_triple: None,
            filter: None,
            exact: false,
            ignored: IgnoredTests::Exclude,
            threads: None,
            no_fail_fast: false,
            release: false,
            timeout_ms: None,
            max_items: default_max_items(),
        }
    }
}

impl TestRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_common(self.dir.as_deref(), self.timeout_ms)?;
        self.selection().validate(GateTargetId::Test)?;
        if !(1..=1000).contains(&self.max_items) {
            return Err("maxItems must be between 1 and 1000".into());
        }
        if self.exact && self.filter.is_none() {
            return Err("exact requires a nonempty test filter".into());
        }
        if self
            .threads
            .is_some_and(|threads| !(1..=256).contains(&threads))
        {
            return Err("threads must be between 1 and 256".into());
        }
        if self.action != TestAction::Run && (self.threads.is_some() || self.no_fail_fast) {
            return Err("threads and noFailFast apply only to action=run".into());
        }
        if self.action == TestAction::Compile
            && (self.filter.is_some() || self.exact || self.ignored != IgnoredTests::Exclude)
        {
            return Err("action=compile builds test executables; filter, exact and ignored apply only when a harness runs".into());
        }
        Ok(())
    }

    fn selection(&self) -> ValidationOptions {
        ValidationOptions {
            packages: self.packages.clone(),
            cargo_target: self.cargo_target.clone(),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target_triple: self.target_triple.clone(),
            test_filter: self.filter.clone(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum RunTarget {
    Bin { name: String },
    Example { name: String },
}

impl RunTarget {
    fn selection(&self) -> CargoTargetSelection {
        match self {
            Self::Bin { name } => CargoTargetSelection::Bin { name: name.clone() },
            Self::Example { name } => CargoTargetSelection::Example { name: name.clone() },
        }
    }
}

/// Run one exact Cargo binary or example, with arguments passed after `--`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    #[serde(default)]
    pub dir: Option<String>,
    pub package: String,
    pub target: RunTarget,
    #[serde(default)]
    #[schemars(length(max = 128))]
    pub args: Vec<String>,
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub features: Vec<String>,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub no_default_features: bool,
    #[serde(default)]
    pub target_triple: Option<String>,
    #[serde(default)]
    pub release: bool,
    #[serde(default)]
    #[schemars(range(min = 1, max = 3_600_000))]
    pub timeout_ms: Option<u64>,
}

impl RunRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_common(self.dir.as_deref(), self.timeout_ms)?;
        self.selection().validate(GateTargetId::Build)?;
        if self.args.len() > 128
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
            || self.args.iter().map(String::len).sum::<usize>() > MAX_ARGUMENT_BYTES
        {
            return Err("args must contain at most 128 arguments, 4096 bytes per argument and 16384 bytes in total, without NUL".into());
        }
        Ok(())
    }

    fn selection(&self) -> ValidationOptions {
        ValidationOptions {
            packages: vec![self.package.clone()],
            cargo_target: Some(self.target.selection()),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target_triple: self.target_triple.clone(),
            ..Default::default()
        }
    }
}

fn validate_common(dir: Option<&str>, timeout_ms: Option<u64>) -> Result<(), String> {
    if dir.is_some_and(|dir| dir.trim().is_empty() || dir.len() > 4096 || dir.contains('\0')) {
        return Err("dir must be a nonempty bounded path without NUL".into());
    }
    if timeout_ms.is_some_and(|timeout| !(1..=MAX_TIMEOUT_MS).contains(&timeout)) {
        return Err(
            "timeoutMs must be between 1 and 3600000; the server deadline may lower it".into(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionDiagnostic {
    pub level: String,
    pub code: Option<String>,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub package_id: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredTest {
    /// Names are scoped to the requested packages/targets. Equal names in
    /// different harnesses remain separate entries, never silently deduplicated.
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestSummary {
    pub listed: u64,
    pub listed_omitted: u64,
    pub executed: Option<u64>,
    pub passed: Option<u64>,
    pub failed: Option<u64>,
    pub ignored: Option<u64>,
    pub filtered_out: Option<u64>,
    pub harness_summaries: u64,
    /// Custom harness output is not assumed to follow libtest's format.
    pub counts_known: bool,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCoverage {
    pub packages: Vec<String>,
    pub cargo_target: Option<CargoTargetSelection>,
    /// Custom harnesses may ignore libtest flags or use a different format.
    pub custom_harnesses: Vec<String>,
    /// Declared custom harnesses with no executable artifact in this invocation
    /// (for example because their required features were not selected).
    pub unbuilt_custom_harnesses: Vec<String>,
    pub manifest_inspection_complete: bool,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutput {
    pub diagnostics: Vec<ExecutionDiagnostic>,
    pub stdout: String,
    pub stderr: String,
    pub tests: Vec<DiscoveredTest>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    /// LISTED, COMPILED, PASSED, COMPLETED, FAILED, NO_TESTS, INCONCLUSIVE,
    /// INVALID_INPUT, CANCELLED, TIMEOUT, or UNAVAILABLE.
    pub status: String,
    pub reason: String,
    pub action: String,
    /// Logical command for display, sanitized by the protocol adapter. The
    /// guard may bind Cargo paths relative to its verified cwd; program
    /// arguments are executed literally. This is not a raw argv transcript or
    /// a runnable shell script.
    pub command: Vec<String>,
    pub workspace_root: Option<String>,
    pub elapsed_ms: u64,
    pub metadata_ms: u64,
    pub execution_ms: u64,
    pub first_diagnostic_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub output_truncated: bool,
    pub cleanup_complete: bool,
    pub drain_complete: bool,
    pub diagnostics_omitted: u64,
    pub evidence: EvidenceStats,
    pub tests: Option<TestSummary>,
    pub test_coverage: Option<TestCoverage>,
    /// The actual result describes this invocation, not an unchanged-source
    /// or full-workspace correctness certificate. Use `check` for gate evidence.
    pub revision_bound: bool,
    pub untrusted_data: ExecutionOutput,
    pub warnings: Vec<String>,
}

impl ExecutionResult {
    fn terminal(action: &str, status: &str, reason: String, started: Instant) -> Self {
        Self {
            status: status.into(),
            reason,
            action: action.into(),
            command: Vec::new(),
            workspace_root: None,
            elapsed_ms: elapsed_ms(started),
            metadata_ms: 0,
            execution_ms: 0,
            first_diagnostic_ms: None,
            exit_code: None,
            signal: None,
            timed_out: status == "TIMEOUT",
            cancelled: status == "CANCELLED",
            output_truncated: false,
            cleanup_complete: true,
            drain_complete: true,
            diagnostics_omitted: 0,
            evidence: EvidenceStats::default(),
            tests: None,
            test_coverage: None,
            revision_bound: false,
            untrusted_data: ExecutionOutput::default(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct ExecutionService {
    config: Config,
    check: Arc<CheckService>,
    supervisor: ProcessSupervisor,
    slots: Semaphore,
}

impl ExecutionService {
    pub fn new(config: Config, check: Arc<CheckService>, supervisor: ProcessSupervisor) -> Self {
        let slots = Semaphore::new(
            usize::try_from(config.gate.host_concurrency)
                .unwrap_or(1)
                .max(1),
        );
        Self {
            config,
            check,
            supervisor,
            slots,
        }
    }

    pub async fn test(
        &self,
        request: TestRequest,
        directory: Option<PathBuf>,
        client_roots: ClientRoots,
        cancellation: CancellationToken,
    ) -> ExecutionResult {
        self.execute(
            Operation::Test(request),
            directory,
            client_roots,
            cancellation,
        )
        .await
    }

    pub async fn run(
        &self,
        request: RunRequest,
        directory: Option<PathBuf>,
        client_roots: ClientRoots,
        cancellation: CancellationToken,
    ) -> ExecutionResult {
        self.execute(
            Operation::Run(request),
            directory,
            client_roots,
            cancellation,
        )
        .await
    }

    async fn execute(
        &self,
        operation: Operation,
        directory: Option<PathBuf>,
        client_roots: ClientRoots,
        cancellation: CancellationToken,
    ) -> ExecutionResult {
        let started = Instant::now();
        let action = operation.action();
        let terminal = |status: &str, message: String| {
            ExecutionResult::terminal(action, status, message, started)
        };
        if let Err(error) = operation.validate() {
            return terminal("INVALID_INPUT", error);
        }
        let timeout_ms = operation
            .timeout_ms()
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(self.config.gate.hard_timeout_ms);
        let deadline = started + Duration::from_millis(timeout_ms);
        let _permit = tokio::select! {
            biased;
            () = cancellation.cancelled() => return terminal("CANCELLED", "execution cancelled before admission".into()),
            () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return terminal("TIMEOUT", "execution deadline elapsed during admission".into()),
            permit = self.slots.acquire() => match permit {
                Ok(permit) => permit,
                Err(_) => return terminal("UNAVAILABLE", "execution admission is closed".into()),
            }
        };
        let metadata_started = Instant::now();
        let plan = match self
            .check
            .plan_snapshot_with_deadline(
                directory,
                client_roots,
                Some(cancellation.clone()),
                deadline,
            )
            .await
        {
            Ok(plan) => plan,
            Err((status, message)) => {
                let mut message = message.replace("verify", "execution");
                if message.contains("lock file")
                    || message.contains("lockfile")
                    || message.contains("--locked")
                {
                    message.push_str(". Prepare the project's Cargo.lock with the project's normal cargo generate-lockfile workflow, then retry; workspace inspect/search/read remains available without a lockfile. This tool does not generate or update the project lockfile.");
                }
                return terminal(status.as_str(), message);
            }
        };
        let metadata_ms = elapsed_ms(metadata_started);
        let test_coverage = if let Operation::Test(request) = &operation {
            Some(inspect_test_coverage(
                &plan.snapshot,
                &plan.workspace_authority,
                request,
                deadline,
                &cancellation,
            ))
        } else {
            None
        };
        let mut args = match operation.arguments(&plan.snapshot) {
            Ok(args) => args,
            Err(error) => return terminal("INVALID_INPUT", error),
        };
        let cache = match select_gate_cache(&plan.snapshot, &self.config.gate, GateMode::Fast) {
            Ok(cache) => cache,
            Err(error) => {
                return terminal(
                    "UNAVAILABLE",
                    format!("execution cache selection failed: {error}"),
                );
            }
        };
        let mut environment = cache.environment;
        environment.insert(
            "CARGO".into(),
            self.check.cargo_path().as_os_str().to_owned(),
        );
        let output_budget = usize::try_from(self.config.limits.tool_output_bytes / 5)
            .unwrap_or(8192)
            .clamp(512, 8192);
        let capture = Arc::new(Mutex::new(ExecutionCapture::new(
            operation.test_action(),
            operation.max_items(),
            output_budget,
        )));
        let observed = Arc::clone(&capture);
        let execution_started = Instant::now();
        let options = ProcessRunOptions::new(&plan.snapshot.workspace_root)
            .with_environment(environment)
            .with_deadline(deadline)
            .with_timeout(Duration::from_millis(timeout_ms))
            .with_cancellation(cancellation)
            .with_max_output_bytes(
                usize::try_from(self.config.limits.process_output_bytes)
                    .unwrap_or(256 * 1024)
                    .min(256 * 1024),
            )
            .on_stdout(move |bytes| {
                observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(bytes)
            });
        let command = std::iter::once(self.check.cargo_path().to_string_lossy().into_owned())
            .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
            .collect();
        let tail_start = args
            .iter()
            .position(|arg| arg == "--")
            .map_or(args.len(), |index| index + 1);
        let process = match self
            .supervisor
            .run_authorized_preserving_tail(
                self.check.cargo_path(),
                args.drain(..),
                options,
                plan.workspace_authority,
                tail_start,
            )
            .await
        {
            Ok(process) => process,
            Err(error) => {
                let status = match error {
                    ProcessError::Cancelled => "CANCELLED",
                    ProcessError::TimedOut => "TIMEOUT",
                    _ => "UNAVAILABLE",
                };
                let mut result = terminal(status, error.to_string());
                result.command = command;
                result.metadata_ms = metadata_ms;
                return result;
            }
        };
        let captured = std::mem::replace(
            &mut *capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ExecutionCapture::new(None, 1, 512),
        )
        .finish();
        let mut result = terminal("INCONCLUSIVE", String::new());
        result.command = command;
        result.workspace_root = Some(plan.snapshot.workspace_root.to_string_lossy().into_owned());
        result.metadata_ms = metadata_ms;
        result.execution_ms = process.duration_ms;
        result.elapsed_ms = elapsed_ms(started);
        result.first_diagnostic_ms = process
            .first_diagnostic_ms
            .map(|ms| ms.saturating_add(elapsed_between(started, execution_started)));
        result.exit_code = Some(process.exit_code);
        result.signal = process.signal;
        result.timed_out = process.timed_out;
        result.cancelled = process.cancelled;
        result.cleanup_complete = process.cleanup_complete;
        result.drain_complete = process.drain_complete;
        result.output_truncated =
            process.output_truncated || captured.truncated || process.stderr.len() > output_budget;
        result.diagnostics_omitted = captured.evidence.omitted_records;
        result.evidence = captured.evidence;
        result.tests = operation.test_action().map(|_| captured.summary);
        result.test_coverage = test_coverage.map(|mut inspection| {
            inspection.coverage.custom_harnesses.clear();
            for (label, key) in inspection.custom_artifacts {
                if captured.executable_targets.contains(&key) {
                    inspection.coverage.custom_harnesses.push(label);
                } else {
                    inspection.coverage.unbuilt_custom_harnesses.push(label);
                }
            }
            if captured.artifact_inventory_truncated {
                inspection.coverage.manifest_inspection_complete = false;
            }
            inspection.coverage
        });
        if let (Some(summary), Some(coverage)) = (&mut result.tests, &result.test_coverage)
            && (!coverage.manifest_inspection_complete
                || !coverage.custom_harnesses.is_empty()
                || summary.harness_summaries < result.evidence.test_binaries)
        {
            summary.counts_known = false;
        }
        result.untrusted_data = ExecutionOutput {
            diagnostics: captured.diagnostics,
            stdout: captured.stdout,
            stderr: tail_bytes(&process.stderr, output_budget),
            tests: captured.tests,
        };
        result.warnings = process.warnings;
        if operation.test_action() == Some(TestAction::List) {
            result.warnings.push("libtest lists ignored tests as well; ignored=only narrows to ignored tests. Names are scoped by testCoverage, and equal names in different harnesses remain separate. Select one package and cargoTarget when exact target provenance is required.".into());
        }
        if let Some(summary) = result.tests.as_ref()
            && summary.listed_omitted > 0
        {
            result.warnings.push("test inventory was bounded; narrow packages, cargoTarget or filter to inspect omitted entries".into());
        }
        if matches!(operation, Operation::Run(_)) {
            result.warnings.push("the selected program ran with the server user's permissions and may have its own side effects".into());
        }
        let (status, reason) = if result.cancelled {
            (
                "CANCELLED",
                "execution was cancelled; no success is claimed",
            )
        } else if result.timed_out {
            (
                "TIMEOUT",
                "execution exceeded its deadline, including metadata and compilation",
            )
        } else if !result.cleanup_complete || !result.drain_complete {
            (
                "UNAVAILABLE",
                "process cleanup or output drainage was incomplete",
            )
        } else if process.exit_code != 0 || result.evidence.build_success == Some(false) {
            (
                "FAILED",
                "Cargo, the test harness, or the selected program returned a failure",
            )
        } else if result
            .tests
            .as_ref()
            .and_then(|summary| summary.failed)
            .is_some_and(|failed| failed > 0)
        {
            ("FAILED", "the test harness reported failed tests")
        } else if !result.evidence.build_finished {
            ("INCONCLUSIVE", "Cargo build completion was not observed")
        } else if operation
            .test_action()
            .is_some_and(|action| action != TestAction::Compile)
            && result.test_coverage.as_ref().is_some_and(|coverage| {
                !coverage.manifest_inspection_complete || !coverage.custom_harnesses.is_empty()
            })
        {
            (
                "INCONCLUSIVE",
                "one or more selected test harnesses use an opaque format or could not be inspected; other libtest summaries cannot certify their results",
            )
        } else if operation.test_action() == Some(TestAction::Run)
            && result
                .tests
                .as_ref()
                .is_some_and(|summary| summary.harness_summaries < result.evidence.test_binaries)
        {
            (
                "INCONCLUSIVE",
                "not every observed test executable produced a libtest summary; execution counts are partial",
            )
        } else {
            operation.verdict(result.tests.as_ref())
        };
        result.status = status.into();
        result.reason = reason.into();
        result
    }
}

#[derive(Debug)]
enum Operation {
    Test(TestRequest),
    Run(RunRequest),
}

struct CoverageInspection {
    coverage: TestCoverage,
    custom_artifacts: Vec<(String, String)>,
}

fn inspect_test_coverage(
    snapshot: &WorkspaceSnapshot,
    authority: &AuthorizedRoot,
    request: &TestRequest,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> CoverageInspection {
    let metadata = &snapshot.metadata;
    let root_manifest = snapshot.workspace_root.join("Cargo.toml");
    let mut packages: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| {
            if !metadata.workspace_members.contains(&package.id) {
                return false;
            }
            if !request.packages.is_empty() {
                return request
                    .packages
                    .iter()
                    .any(|name| name == package.name.as_str());
            }
            if snapshot.manifest_path != root_manifest {
                return package.manifest_path.as_std_path() == snapshot.manifest_path;
            }
            metadata.workspace_default_members.is_available()
                && metadata.workspace_default_members.contains(&package.id)
        })
        .collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    let mut coverage = TestCoverage {
        packages: packages
            .iter()
            .take(64)
            .map(|package| package.name.to_string())
            .collect(),
        cargo_target: request.cargo_target.clone(),
        custom_harnesses: Vec::new(),
        unbuilt_custom_harnesses: Vec::new(),
        manifest_inspection_complete: !packages.is_empty() && packages.len() <= 64,
    };
    let mut remaining_bytes = 8 * 1024 * 1024;
    let mut custom_artifacts = Vec::new();
    for package in packages.into_iter().take(64) {
        if cancellation.is_cancelled() || Instant::now() >= deadline || remaining_bytes == 0 {
            coverage.manifest_inspection_complete = false;
            break;
        }
        let parsed = authority
            .read_file(
                package.manifest_path.as_std_path(),
                remaining_bytes.min(1024 * 1024),
            )
            .ok()
            .and_then(|bytes| {
                remaining_bytes = remaining_bytes.saturating_sub(bytes.len() as u64);
                String::from_utf8(bytes).ok()
            })
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok());
        let Some(manifest) = parsed else {
            coverage.manifest_inspection_complete = false;
            continue;
        };
        if let Some(lib) = manifest.get("lib")
            && lib.get("harness").and_then(toml::Value::as_bool) == Some(false)
            && (matches!(request.cargo_target, Some(CargoTargetSelection::Lib {}))
                || (request.cargo_target.is_none()
                    && lib.get("test").and_then(toml::Value::as_bool) != Some(false)))
        {
            let label = format!("{}:lib", package.name);
            bind_custom_targets(package, "lib", None, &label, &mut custom_artifacts);
        }
        for kind in ["bin", "test", "example", "bench"] {
            if let Some(targets) = manifest.get(kind).and_then(toml::Value::as_array) {
                for target in targets {
                    if target.get("harness").and_then(toml::Value::as_bool) != Some(false) {
                        continue;
                    }
                    let name = target
                        .get("name")
                        .and_then(toml::Value::as_str)
                        .or_else(|| {
                            target
                                .get("path")
                                .and_then(toml::Value::as_str)
                                .and_then(|path| Path::new(path).file_stem())
                                .and_then(|name| name.to_str())
                        });
                    let selected = match &request.cargo_target {
                        Some(CargoTargetSelection::Bin { name: expected }) => {
                            kind == "bin" && name.is_none_or(|name| name == expected)
                        }
                        Some(CargoTargetSelection::Test { name: expected }) => {
                            kind == "test" && name.is_none_or(|name| name == expected)
                        }
                        Some(CargoTargetSelection::Example { name: expected }) => {
                            kind == "example" && name.is_none_or(|name| name == expected)
                        }
                        Some(CargoTargetSelection::Bench { name: expected }) => {
                            kind == "bench" && name.is_none_or(|name| name == expected)
                        }
                        Some(CargoTargetSelection::Lib {}) => false,
                        None => match kind {
                            "bin" | "test" => {
                                target.get("test").and_then(toml::Value::as_bool) != Some(false)
                            }
                            "example" => {
                                target.get("test").and_then(toml::Value::as_bool) == Some(true)
                            }
                            _ => false,
                        },
                    };
                    if selected {
                        if custom_artifacts.len() < 64 {
                            let label =
                                format!("{}:{kind}:{}", package.name, name.unwrap_or("<unnamed>"));
                            bind_custom_targets(package, kind, name, &label, &mut custom_artifacts);
                        } else {
                            coverage.manifest_inspection_complete = false;
                        }
                    }
                }
            }
        }
    }
    CoverageInspection {
        coverage,
        custom_artifacts,
    }
}

fn bind_custom_targets(
    package: &cargo_metadata::Package,
    kind: &str,
    name: Option<&str>,
    label: &str,
    output: &mut Vec<(String, String)>,
) {
    for target in &package.targets {
        let kind_matches = if kind == "lib" {
            CargoTargetSelection::Lib {}.matches(target)
        } else {
            target.kind.iter().any(|target_kind| {
                matches!(
                    (kind, target_kind),
                    ("bin", cargo_metadata::TargetKind::Bin)
                        | ("test", cargo_metadata::TargetKind::Test)
                        | ("example", cargo_metadata::TargetKind::Example)
                        | ("bench", cargo_metadata::TargetKind::Bench)
                )
            })
        };
        if kind_matches && name.is_none_or(|name| name == target.name) {
            let kinds = serde_json::to_string(&target.kind).unwrap_or_default();
            output.push((
                label.into(),
                format!("{}|{}|{kinds}", package.id, target.name),
            ));
        }
    }
}

impl Operation {
    fn action(&self) -> &'static str {
        match self {
            Self::Test(request) => match request.action {
                TestAction::List => "list",
                TestAction::Run => "test",
                TestAction::Compile => "compile",
            },
            Self::Run(_) => "run",
        }
    }
    fn test_action(&self) -> Option<TestAction> {
        match self {
            Self::Test(request) => Some(request.action),
            Self::Run(_) => None,
        }
    }
    fn max_items(&self) -> usize {
        match self {
            Self::Test(request) => request.max_items,
            Self::Run(_) => 1,
        }
    }
    fn timeout_ms(&self) -> Option<u64> {
        match self {
            Self::Test(request) => request.timeout_ms,
            Self::Run(request) => request.timeout_ms,
        }
    }
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Test(request) => request.validate(),
            Self::Run(request) => request.validate(),
        }
    }

    fn arguments(&self, snapshot: &WorkspaceSnapshot) -> Result<Vec<OsString>, String> {
        let (selection, release, id, verb) = match self {
            Self::Test(request) => (
                request.selection(),
                request.release,
                GateTargetId::Test,
                "test",
            ),
            Self::Run(request) => (
                request.selection(),
                request.release,
                GateTargetId::Build,
                "run",
            ),
        };
        let packages = crate::gate::selection::resolve(snapshot, &selection)?;
        let mut target = GateTarget {
            id,
            label: "focused Cargo execution",
            timeout: Duration::from_secs(60),
            args: vec![
                verb.into(),
                "--manifest-path".into(),
                snapshot.manifest_path.as_os_str().to_owned(),
                "--locked".into(),
                "--message-format=json".into(),
            ],
        };
        if let Some((_, package_args)) = packages {
            target.args.extend(package_args);
        }
        selection.apply(&mut target);
        if release {
            target.args.push("--release".into());
        }
        match self {
            Self::Test(request) => {
                if request.no_fail_fast {
                    target.args.push("--no-fail-fast".into());
                }
                if request.action == TestAction::Compile {
                    target.args.push("--no-run".into());
                } else {
                    target.args.push("--".into());
                    if request.action == TestAction::List {
                        target
                            .args
                            .extend(["--list".into(), "--format=terse".into()]);
                    }
                    if request.exact {
                        target.args.push("--exact".into());
                    }
                    match request.ignored {
                        IgnoredTests::Exclude => {}
                        IgnoredTests::Include => target.args.push("--include-ignored".into()),
                        IgnoredTests::Only => target.args.push("--ignored".into()),
                    }
                    if let Some(threads) = request.threads {
                        target.args.push(format!("--test-threads={threads}").into());
                    }
                }
            }
            Self::Run(request) => {
                target.args.push("--".into());
                target.args.extend(request.args.iter().map(OsString::from));
            }
        }
        Ok(target.args)
    }

    fn verdict(&self, summary: Option<&TestSummary>) -> (&'static str, &'static str) {
        match self {
            Self::Run(_) => ("COMPLETED", "the selected program exited successfully"),
            Self::Test(request) => {
                let summary = summary.expect("test operations carry a test summary");
                match request.action {
                    TestAction::Compile => (
                        "COMPILED",
                        "Cargo compiled the selected test executables without running tests",
                    ),
                    TestAction::List if summary.listed > 0 => (
                        "LISTED",
                        "libtest test names were discovered; test bodies were not requested to run",
                    ),
                    TestAction::List => (
                        "NO_TESTS",
                        "no libtest test names were observed; the selection may be empty or use a custom harness",
                    ),
                    TestAction::Run if summary.executed.is_some_and(|count| count > 0) => (
                        "PASSED",
                        "Cargo succeeded and libtest reported executed tests",
                    ),
                    TestAction::Run if summary.executed == Some(0) => (
                        "NO_TESTS",
                        "libtest executed zero tests; this is not a passing test run",
                    ),
                    TestAction::Run => (
                        "INCONCLUSIVE",
                        "the process succeeded but no standard libtest execution counts were observed; custom harness counts remain unknown",
                    ),
                }
            }
        }
    }
}

#[derive(Debug)]
struct ExecutionCapture {
    cargo: CargoStream,
    line: Vec<u8>,
    discard_line: bool,
    build_finished: bool,
    stdout: String,
    tests: Vec<DiscoveredTest>,
    summary: TestSummary,
    action: Option<TestAction>,
    max_items: usize,
    output_budget: usize,
    inventory_bytes: usize,
    truncated: bool,
    executable_targets: BTreeSet<String>,
    artifact_inventory_truncated: bool,
}

impl ExecutionCapture {
    fn new(action: Option<TestAction>, max_items: usize, output_budget: usize) -> Self {
        Self {
            // Raw compiler records include redundant rendered spans. Retain a
            // bounded parser budget, then compact to the much smaller result
            // budget so a long source line does not erase the primary error.
            cargo: CargoStream::new(MAX_LINE_BYTES, 32, 1024 * 1024),
            line: Vec::new(),
            discard_line: false,
            build_finished: false,
            stdout: String::new(),
            tests: Vec::new(),
            summary: TestSummary::default(),
            action,
            max_items,
            output_budget,
            inventory_bytes: 0,
            truncated: false,
            executable_targets: BTreeSet::new(),
            artifact_inventory_truncated: false,
        }
    }

    fn push(&mut self, mut bytes: &[u8]) -> bool {
        let mut diagnostic = false;
        while !bytes.is_empty() {
            let end = bytes.iter().position(|byte| *byte == b'\n');
            let segment = &bytes[..end.unwrap_or(bytes.len())];
            if !self.discard_line {
                if self.line.len().saturating_add(segment.len()) > MAX_LINE_BYTES {
                    self.line.clear();
                    self.discard_line = true;
                    self.truncated = true;
                } else {
                    self.line.extend_from_slice(segment);
                }
            }
            if let Some(end) = end {
                diagnostic |= self.consume_line();
                self.discard_line = false;
                bytes = &bytes[end + 1..];
            } else {
                break;
            }
        }
        diagnostic
    }

    fn consume_line(&mut self) -> bool {
        if self.discard_line {
            return false;
        }
        let bytes = std::mem::take(&mut self.line);
        if bytes.is_empty() {
            return false;
        }
        let line = String::from_utf8_lossy(&bytes);
        if !self.build_finished
            && line.starts_with('{')
            && let Ok(message) = serde_json::from_str::<serde_json::Value>(&line)
            && matches!(
                message["reason"].as_str(),
                Some(
                    "compiler-message"
                        | "compiler-artifact"
                        | "build-script-executed"
                        | "build-finished"
                )
            )
        {
            if message["reason"] == "compiler-artifact" && message["executable"].is_string() {
                if let (Some(package), Some(name)) = (
                    message["package_id"].as_str(),
                    message["target"]["name"].as_str(),
                ) {
                    let kinds = message["target"]["kind"].to_string();
                    let key = format!("{package}|{name}|{kinds}");
                    if key.len() <= 4096
                        && (self.executable_targets.len() < 512
                            || self.executable_targets.contains(&key))
                    {
                        self.executable_targets.insert(key);
                    } else {
                        self.artifact_inventory_truncated = true;
                        self.truncated = true;
                    }
                } else {
                    self.artifact_inventory_truncated = true;
                }
            }
            let diagnostic = self.cargo.push(&bytes);
            let diagnostic = self.cargo.push(b"\n") || diagnostic;
            if message["reason"] == "build-finished" {
                self.build_finished = true;
            }
            return diagnostic;
        }
        let line = sanitize_text(line.trim_end_matches('\r'));
        if self.action == Some(TestAction::List) {
            let entry = line
                .strip_suffix(": test")
                .map(|name| (name, "test"))
                .or_else(|| {
                    line.strip_suffix(": benchmark")
                        .map(|name| (name, "benchmark"))
                });
            if let Some((name, kind)) = entry {
                self.summary.listed = self.summary.listed.saturating_add(1);
                if self.tests.len() < self.max_items
                    && name.len() <= 1024
                    && self.inventory_bytes.saturating_add(name.len() + 64) <= self.output_budget
                {
                    self.inventory_bytes += name.len() + 64;
                    self.tests.push(DiscoveredTest {
                        name: name.into(),
                        kind: kind.into(),
                    });
                } else {
                    self.summary.listed_omitted = self.summary.listed_omitted.saturating_add(1);
                    self.truncated = true;
                }
            }
        }
        if self.action == Some(TestAction::Run) {
            self.observe_summary(&line);
        }
        if self.stdout.len().saturating_add(line.len() + 1) > self.output_budget {
            self.truncated = true;
        }
        self.stdout.push_str(&line);
        self.stdout.push('\n');
        self.stdout = tail_bytes(&self.stdout, self.output_budget);
        false
    }

    fn observe_summary(&mut self, line: &str) {
        let Some(fields) = line
            .strip_prefix("test result: ")
            .and_then(|s| s.split_once(". ").map(|(_, s)| s))
        else {
            return;
        };
        let count = |suffix: &str| {
            fields.split(';').find_map(|field| {
                field
                    .trim()
                    .strip_suffix(suffix)
                    .and_then(|number| number.parse::<u64>().ok())
            })
        };
        let (Some(passed), Some(failed)) = (count(" passed"), count(" failed")) else {
            return;
        };
        self.summary.counts_known = true;
        self.summary.harness_summaries = self.summary.harness_summaries.saturating_add(1);
        add_count(&mut self.summary.passed, passed);
        add_count(&mut self.summary.failed, failed);
        add_count(&mut self.summary.executed, passed.saturating_add(failed));
        if let Some(ignored) = count(" ignored") {
            add_count(&mut self.summary.ignored, ignored);
        }
        if let Some(filtered) = count(" filtered out") {
            add_count(&mut self.summary.filtered_out, filtered);
        }
    }

    fn finish(mut self) -> CapturedExecution {
        self.consume_line();
        let (mut cargo, mut evidence) = self.cargo.finish();
        evidence.tests_executed = self.summary.executed;
        evidence.test_summaries = self.summary.harness_summaries;
        if self.summary.executed == Some(0) {
            evidence.empty_test_summaries = self.summary.harness_summaries;
        }
        cargo.diagnostics.sort_by_key(|diagnostic| {
            diagnostic.level != crate::diagnostics::DiagnosticLevel::Error
        });
        let mut diagnostics = Vec::new();
        let mut diagnostic_bytes = 0usize;
        for diagnostic in cargo.diagnostics {
            self.truncated |= diagnostic.message.len() > 2048
                || diagnostic
                    .file
                    .as_ref()
                    .is_some_and(|file| file.len() > 1024)
                || diagnostic
                    .package_id
                    .as_ref()
                    .is_some_and(|package| package.len() > 1024)
                || diagnostic
                    .target_name
                    .as_ref()
                    .is_some_and(|target| target.len() > 256);
            let diagnostic = compact_diagnostic(diagnostic);
            let bytes = serde_json::to_vec(&diagnostic).map_or(usize::MAX, |value| value.len());
            if diagnostics.len() < 8 && diagnostic_bytes.saturating_add(bytes) <= self.output_budget
            {
                diagnostic_bytes += bytes;
                diagnostics.push(diagnostic);
            } else {
                evidence.omitted_records = evidence.omitted_records.saturating_add(1);
                self.truncated = true;
            }
        }
        CapturedExecution {
            evidence,
            diagnostics,
            stdout: self.stdout,
            tests: self.tests,
            summary: self.summary,
            truncated: self.truncated || cargo.truncated,
            executable_targets: self.executable_targets,
            artifact_inventory_truncated: self.artifact_inventory_truncated,
        }
    }
}

struct CapturedExecution {
    evidence: EvidenceStats,
    diagnostics: Vec<ExecutionDiagnostic>,
    stdout: String,
    tests: Vec<DiscoveredTest>,
    summary: TestSummary,
    truncated: bool,
    executable_targets: BTreeSet<String>,
    artifact_inventory_truncated: bool,
}

fn compact_diagnostic(diagnostic: Diagnostic) -> ExecutionDiagnostic {
    let primary = diagnostic.spans.iter().find(|span| span.is_primary);
    ExecutionDiagnostic {
        level: diagnostic.level.to_string(),
        code: diagnostic.code,
        message: prefix_bytes(&diagnostic.message, 2048),
        file: diagnostic.file.map(|file| prefix_bytes(&file, 1024)),
        line: diagnostic.line,
        column: primary.map(|span| span.column_start),
        package_id: diagnostic
            .package_id
            .map(|package| prefix_bytes(&package, 1024)),
        target: diagnostic
            .target_name
            .map(|target| prefix_bytes(&target, 256)),
    }
}

fn add_count(target: &mut Option<u64>, value: u64) {
    *target = Some(target.unwrap_or(0).saturating_add(value));
}
fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
fn elapsed_between(started: Instant, later: Instant) -> u64 {
    later
        .duration_since(started)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn prefix_bytes(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}
fn tail_bytes(value: &str, max: usize) -> String {
    let mut start = value.len().saturating_sub(max);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_keeps_program_json_out_of_compiler_evidence() {
        let mut capture = ExecutionCapture::new(None, 1, 1024);
        capture.push(b"{\"reason\":\"build-finished\",\"success\":true}\n");
        capture.push(b"{\"reason\":\"build-finished\",\"success\":false}\n");
        let output = capture.finish();
        assert_eq!(output.evidence.build_success, Some(true));
        assert!(output.stdout.contains("false"));
    }

    #[test]
    fn inventory_keeps_duplicate_names_and_reports_omitted_entries() {
        let mut capture = ExecutionCapture::new(Some(TestAction::List), 2, 1024);
        capture.push(b"same: test\nsame: test\nthird: test\n");
        let output = capture.finish();
        assert_eq!(output.tests.len(), 2);
        assert_eq!(output.summary.listed, 3);
        assert_eq!(output.summary.listed_omitted, 1);
        assert!(output.truncated);
    }

    #[test]
    fn test_validation_rejects_ambiguous_ignored_compile_and_flag_filters() {
        assert!(
            TestRequest {
                action: TestAction::Compile,
                filter: Some("one".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            TestRequest {
                exact: true,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            TestRequest {
                filter: Some("--help".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            TestRequest {
                threads: Some(0),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            TestRequest {
                dir: Some(" ".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<TestRequest>("{\"action\":\"run\",\"arbitraryArgs\":[]}")
                .is_err()
        );
    }
}
