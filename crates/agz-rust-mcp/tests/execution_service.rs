//! Real Cargo fixtures for the focused execution tools. No registry packages
//! or optional test runners are needed.

use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use agz_rust_mcp::{
    Config,
    gate::CargoTargetSelection,
    process::ProcessSupervisor,
    tools::{
        CheckService,
        execution::{
            ExecutionResult, ExecutionService, IgnoredTests, RunRequest, RunTarget, TestAction,
            TestRequest,
        },
    },
    workspace::{ClientRoots, RootGuard},
};
use tokio_util::sync::CancellationToken;

const LIB: &str = r#"
pub fn double(value: u64) -> u64 { value * 2 }
#[cfg(test)] mod tests {
    #[test] fn sample() { assert_eq!(super::double(2), 4); }
    #[test] fn sample_longer() { assert_eq!(super::double(3), 6); }
    #[test] #[ignore] fn ignored_case() { assert_eq!(super::double(4), 8); }
    #[test] fn fails() { panic!("intentional test failure"); }
    #[test] fn side_effect() { std::fs::write("test-body-ran", "yes").unwrap(); }
}
"#;

const BIN: &str = r##"
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("fail") => { eprintln!("expected program failure"); std::process::exit(7); }
        Some("sleep") => { std::thread::sleep(std::time::Duration::from_secs(30)); }
        Some("flood") => { println!("{}", "x".repeat(400_000)); println!("after-flood"); }
        _ => {}
    }
    for arg in args { println!("ARG={arg}"); }
    println!("{}", r#"{"reason":"build-finished","success":false}"#);
}
"##;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    #[cfg(unix)]
    config: Config,
    #[cfg(unix)]
    guard: Arc<RootGuard>,
    supervisor: ProcessSupervisor,
    check: Arc<CheckService>,
    execution: ExecutionService,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("agz-exec-{label}-{}-{stamp}", std::process::id()));
        let root = base.join("workspace");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir(root.join("examples")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname=\"exec-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[features]\nextra=[]\n").unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n[[package]]\nname=\"exec-fixture\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), LIB).unwrap();
        fs::write(root.join("src/main.rs"), BIN).unwrap();
        fs::write(
            root.join("examples/demo.rs"),
            "fn main() { println!(\"example={}\", exec_fixture::double(10)); }\n",
        )
        .unwrap();
        let mut config = Config::defaults_at(&root);
        config.gate.cache_dir = base.join("cache");
        config.gate.lease_dir = base.join("leases");
        config.gate.hard_timeout_ms = 60_000;
        config.gate.host_concurrency = 2;
        config.telemetry.enabled = false;
        let guard = Arc::new(RootGuard::new([root.clone()], std::iter::empty()).unwrap());
        let supervisor = ProcessSupervisor::without_journal();
        let check = Arc::new(CheckService::with_shared_supervisor(
            config.clone(),
            Arc::clone(&guard),
            supervisor.clone(),
        ));
        let execution =
            ExecutionService::new(config.clone(), Arc::clone(&check), supervisor.clone());
        Self {
            base,
            root,
            #[cfg(unix)]
            config,
            #[cfg(unix)]
            guard,
            supervisor,
            check,
            execution,
        }
    }

    async fn test(&self, request: TestRequest) -> ExecutionResult {
        self.execution
            .test(
                request,
                Some(self.root.clone()),
                ClientRoots::unsupported(),
                CancellationToken::new(),
            )
            .await
    }

    async fn run(&self, request: RunRequest) -> ExecutionResult {
        self.execution
            .run(
                request,
                Some(self.root.clone()),
                ClientRoots::unsupported(),
                CancellationToken::new(),
            )
            .await
    }

    async fn close(&self) {
        self.check.close().await;
        let report = self.supervisor.close().await;
        assert_eq!(report.remaining, 0);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn focused_test() -> TestRequest {
    TestRequest {
        packages: vec!["exec-fixture".into()],
        cargo_target: Some(CargoTargetSelection::Lib {}),
        ..Default::default()
    }
}

fn run_request() -> RunRequest {
    RunRequest {
        dir: None,
        package: "exec-fixture".into(),
        target: RunTarget::Bin {
            name: "exec-fixture".into(),
        },
        args: Vec::new(),
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        target_triple: None,
        release: false,
        timeout_ms: None,
    }
}

#[tokio::test]
async fn discovers_and_compiles_tests_without_running_their_bodies() {
    let fixture = Fixture::new("discover");
    let listed = fixture
        .test(TestRequest {
            action: TestAction::List,
            max_items: 2,
            ..focused_test()
        })
        .await;
    assert_eq!(listed.status, "LISTED", "{listed:#?}");
    assert_eq!(listed.tests.as_ref().unwrap().listed, 5);
    assert_eq!(listed.tests.as_ref().unwrap().listed_omitted, 3);
    assert_eq!(listed.untrusted_data.tests.len(), 2);
    assert!(!fixture.root.join("test-body-ran").exists());
    let compiled = fixture
        .test(TestRequest {
            action: TestAction::Compile,
            ..focused_test()
        })
        .await;
    assert_eq!(compiled.status, "COMPILED", "{compiled:#?}");
    assert_eq!(compiled.tests.as_ref().unwrap().executed, None);
    assert!(!fixture.root.join("test-body-ran").exists());
    fixture.close().await;
}

#[tokio::test]
async fn exact_filters_ignored_cases_and_zero_matches_have_honest_verdicts() {
    let fixture = Fixture::new("selection");
    let substring = fixture
        .test(TestRequest {
            filter: Some("tests::sample".into()),
            ..focused_test()
        })
        .await;
    assert_eq!(substring.status, "PASSED", "{substring:#?}");
    assert_eq!(substring.tests.as_ref().unwrap().executed, Some(2));
    let exact = fixture
        .test(TestRequest {
            filter: Some("tests::sample".into()),
            exact: true,
            threads: Some(1),
            ..focused_test()
        })
        .await;
    assert_eq!(exact.status, "PASSED", "{exact:#?}");
    assert_eq!(exact.tests.as_ref().unwrap().executed, Some(1));
    let missing = fixture
        .test(TestRequest {
            filter: Some("missing_test".into()),
            exact: true,
            ..focused_test()
        })
        .await;
    assert_eq!(missing.status, "NO_TESTS", "{missing:#?}");
    let ignored = fixture
        .test(TestRequest {
            filter: Some("ignored_case".into()),
            ..focused_test()
        })
        .await;
    assert_eq!(ignored.status, "NO_TESTS", "{ignored:#?}");
    assert_eq!(ignored.tests.as_ref().unwrap().ignored, Some(1));
    let only = fixture
        .test(TestRequest {
            ignored: IgnoredTests::Only,
            ..focused_test()
        })
        .await;
    assert_eq!(only.status, "PASSED", "{only:#?}");
    assert_eq!(only.tests.as_ref().unwrap().executed, Some(1));
    let include = fixture
        .test(TestRequest {
            filter: Some("ignored_case".into()),
            ignored: IgnoredTests::Include,
            ..focused_test()
        })
        .await;
    assert_eq!(include.status, "PASSED", "{include:#?}");
    let failed = fixture
        .test(TestRequest {
            filter: Some("tests::fails".into()),
            exact: true,
            no_fail_fast: true,
            ..focused_test()
        })
        .await;
    assert_eq!(failed.status, "FAILED", "{failed:#?}");
    assert_eq!(failed.tests.as_ref().unwrap().failed, Some(1));
    fixture.close().await;
}

#[tokio::test]
async fn program_argv_is_literal_and_examples_and_failures_are_observed() {
    let fixture = Fixture::new("run");
    let absolute = fixture
        .root
        .join("a path with spaces")
        .to_string_lossy()
        .into_owned();
    let args = vec![
        "two words".into(),
        "--config".into(),
        "$(touch should-not-exist)".into(),
        absolute.clone(),
    ];
    let result = fixture
        .run(RunRequest {
            args: args.clone(),
            ..run_request()
        })
        .await;
    assert_eq!(result.status, "COMPLETED", "{result:#?}");
    for arg in &args {
        assert!(
            result.untrusted_data.stdout.contains(&format!("ARG={arg}")),
            "{result:#?}"
        );
    }
    assert_eq!(result.evidence.build_success, Some(true));
    assert!(result.untrusted_data.stdout.contains("\"success\":false"));
    assert!(!fixture.root.join("should-not-exist").exists());
    let example = fixture
        .run(RunRequest {
            target: RunTarget::Example {
                name: "demo".into(),
            },
            ..run_request()
        })
        .await;
    assert_eq!(example.status, "COMPLETED", "{example:#?}");
    assert!(example.untrusted_data.stdout.contains("example=20"));
    let failed = fixture
        .run(RunRequest {
            args: vec!["fail".into()],
            ..run_request()
        })
        .await;
    assert_eq!(failed.status, "FAILED", "{failed:#?}");
    assert_eq!(failed.exit_code, Some(7));
    assert!(
        failed
            .untrusted_data
            .stderr
            .contains("expected program failure")
    );
    fixture.close().await;
}

#[tokio::test]
async fn execution_is_fresh_and_compiler_errors_remain_structured() {
    let fixture = Fixture::new("fresh");
    let first = fixture.run(run_request()).await;
    assert_eq!(first.status, "COMPLETED", "{first:#?}");
    fs::write(
        fixture.root.join("src/main.rs"),
        "fn main() { let value: u32 = \"wrong\"; println!(\"{value}\"); }",
    )
    .unwrap();
    let second = fixture.run(run_request()).await;
    assert_eq!(second.status, "FAILED", "{second:#?}");
    assert!(
        second
            .untrusted_data
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("E0308"))
    );
    assert!(second.first_diagnostic_ms.is_some());
    fixture.close().await;
}

#[tokio::test]
async fn unknown_targets_custom_harnesses_and_output_floods_are_bounded() {
    let fixture = Fixture::new("boundaries");
    let unknown = fixture
        .run(RunRequest {
            target: RunTarget::Bin {
                name: "missing".into(),
            },
            ..run_request()
        })
        .await;
    assert_eq!(unknown.status, "INVALID_INPUT", "{unknown:#?}");
    let missing_package = fixture
        .run(RunRequest {
            package: "not-a-member".into(),
            ..run_request()
        })
        .await;
    assert_eq!(
        missing_package.status, "INVALID_INPUT",
        "{missing_package:#?}"
    );
    fs::create_dir(fixture.root.join("tests")).unwrap();
    fs::write(
        fixture.root.join("tests/custom.rs"),
        "fn main() { println!(\"custom harness completed\"); }",
    )
    .unwrap();
    let manifest = fixture.root.join("Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[[test]]\nname=\"custom\"\nharness=false\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let custom = fixture
        .test(TestRequest {
            cargo_target: Some(CargoTargetSelection::Test {
                name: "custom".into(),
            }),
            ..focused_test()
        })
        .await;
    assert_eq!(custom.status, "INCONCLUSIVE", "{custom:#?}");
    assert!(!custom.tests.as_ref().unwrap().counts_known);
    let mixed = fixture
        .test(TestRequest {
            packages: vec!["exec-fixture".into()],
            filter: Some("sample".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(mixed.status, "INCONCLUSIVE", "{mixed:#?}");
    assert_eq!(mixed.tests.as_ref().unwrap().executed, Some(2));
    assert!(!mixed.tests.as_ref().unwrap().counts_known);
    assert_eq!(
        mixed.test_coverage.as_ref().unwrap().custom_harnesses,
        ["exec-fixture:test:custom"]
    );
    let flood = fixture
        .run(RunRequest {
            args: vec!["flood".into()],
            ..run_request()
        })
        .await;
    assert_eq!(flood.status, "COMPLETED", "{flood:#?}");
    assert!(flood.output_truncated);
    assert!(flood.untrusted_data.stdout.contains("after-flood"));
    assert!(serde_json::to_vec(&flood).unwrap().len() < 49152);
    fixture.close().await;
}

#[tokio::test]
async fn program_timeout_and_cancellation_reap_children() {
    let fixture = Fixture::new("timeout");
    let warm = fixture.run(run_request()).await;
    assert_eq!(warm.status, "COMPLETED", "{warm:#?}");
    let start = Instant::now();
    let timed_out = fixture
        .run(RunRequest {
            args: vec!["sleep".into()],
            timeout_ms: Some(500),
            ..run_request()
        })
        .await;
    assert_eq!(timed_out.status, "TIMEOUT", "{timed_out:#?}");
    assert!(timed_out.cleanup_complete);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(fixture.supervisor.active_count(), 0);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let cancelling = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.cancel();
    });
    let cancelled = fixture
        .execution
        .run(
            RunRequest {
                args: vec!["sleep".into()],
                ..run_request()
            },
            Some(fixture.root.clone()),
            ClientRoots::unsupported(),
            token,
        )
        .await;
    cancelling.await.unwrap();
    assert_eq!(cancelled.status, "CANCELLED", "{cancelled:#?}");
    assert!(cancelled.cleanup_complete);
    assert_eq!(fixture.supervisor.active_count(), 0);
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn request_deadline_also_bounds_cargo_metadata_preflight() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("metadata-timeout");
    let cargo = fixture.base.join("slow-cargo");
    fs::write(&cargo, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = fixture.config.clone();
    config.cargo.path = Some(cargo);
    let check = Arc::new(CheckService::with_shared_supervisor(
        config.clone(),
        Arc::clone(&fixture.guard),
        fixture.supervisor.clone(),
    ));
    let execution = ExecutionService::new(config, Arc::clone(&check), fixture.supervisor.clone());
    let started = Instant::now();
    let result = execution
        .test(
            TestRequest {
                timeout_ms: Some(200),
                ..focused_test()
            },
            Some(fixture.root.clone()),
            ClientRoots::unsupported(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.status, "TIMEOUT", "{result:#?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(fixture.supervisor.active_count(), 0);
    check.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn client_roots_cannot_authorize_a_different_workspace() {
    let fixture = Fixture::new("roots");
    let other = Fixture::new("other-roots");
    let result = fixture
        .execution
        .run(
            run_request(),
            Some(other.root.clone()),
            ClientRoots::unsupported(),
            CancellationToken::new(),
        )
        .await;
    assert_ne!(result.status, "COMPLETED", "{result:#?}");
    assert!(result.command.is_empty());
    assert_eq!(fixture.supervisor.active_count(), 0);
    fixture.close().await;
    other.close().await;
}

#[tokio::test]
async fn explicit_package_selection_skips_a_broken_workspace_sibling() {
    let fixture = Fixture::new("members");
    let manifest = fixture.root.join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("[workspace]", "[workspace]\nmembers=[\"sibling\"]"),
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join("sibling/src")).unwrap();
    fs::write(
        fixture.root.join("sibling/Cargo.toml"),
        "[package]\nname=\"broken-sibling\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        fixture.root.join("sibling/src/lib.rs"),
        "compile_error!(\"this sibling must not be compiled\");\n",
    )
    .unwrap();
    let lock = fixture.root.join("Cargo.lock");
    fs::write(
        &lock,
        format!(
            "{}\n[[package]]\nname=\"broken-sibling\"\nversion=\"0.1.0\"\n",
            fs::read_to_string(&lock).unwrap()
        ),
    )
    .unwrap();
    let tested = fixture
        .test(TestRequest {
            filter: Some("tests::sample".into()),
            exact: true,
            features: vec!["extra".into()],
            ..focused_test()
        })
        .await;
    assert_eq!(tested.status, "PASSED", "{tested:#?}");
    assert_eq!(tested.tests.as_ref().unwrap().executed, Some(1));
    let run = fixture.run(run_request()).await;
    assert_eq!(run.status, "COMPLETED", "{run:#?}");
    fixture.close().await;
}

#[tokio::test]
async fn no_fail_fast_continues_to_later_test_executables() {
    let fixture = Fixture::new("keep-going");
    fs::create_dir(fixture.root.join("tests")).unwrap();
    fs::write(fixture.root.join("tests/zz_after.rs"), "#[test] fn after_failure() { std::fs::write(\"continued-after-failure\", \"yes\").unwrap(); }\n").unwrap();
    let request = TestRequest {
        packages: vec!["exec-fixture".into()],
        ..Default::default()
    };
    let fail_fast = fixture.test(request.clone()).await;
    assert_eq!(fail_fast.status, "FAILED", "{fail_fast:#?}");
    assert!(!fixture.root.join("continued-after-failure").exists());
    let continued = fixture
        .test(TestRequest {
            no_fail_fast: true,
            ..request
        })
        .await;
    assert_eq!(continued.status, "FAILED", "{continued:#?}");
    assert!(fixture.root.join("continued-after-failure").exists());
    fixture.close().await;
}

#[tokio::test]
async fn missing_lockfile_explains_project_preparation_without_writing_it() {
    let fixture = Fixture::new("lockfile");
    fs::remove_file(fixture.root.join("Cargo.lock")).unwrap();
    let result = fixture.run(run_request()).await;
    assert_eq!(result.status, "INCONCLUSIVE", "{result:#?}");
    assert!(
        result.reason.contains("cargo generate-lockfile"),
        "{result:#?}"
    );
    assert!(!fixture.root.join("Cargo.lock").exists());
    assert!(result.command.is_empty());
    fixture.close().await;
}

#[tokio::test]
async fn a_feature_gated_custom_harness_only_limits_coverage_when_built() {
    let fixture = Fixture::new("custom-features");
    fs::create_dir(fixture.root.join("tests")).unwrap();
    fs::write(
        fixture.root.join("tests/custom.rs"),
        "fn main() { println!(\"opaque custom success\"); }\n",
    )
    .unwrap();
    let manifest = fixture.root.join("Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[[test]]\nname=\"custom\"\nharness=false\nrequired-features=[\"extra\"]\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let request = TestRequest {
        packages: vec!["exec-fixture".into()],
        filter: Some("sample".into()),
        ..Default::default()
    };
    let disabled = fixture.test(request.clone()).await;
    assert_eq!(disabled.status, "PASSED", "{disabled:#?}");
    assert!(
        disabled
            .test_coverage
            .as_ref()
            .unwrap()
            .custom_harnesses
            .is_empty()
    );
    assert_eq!(
        disabled
            .test_coverage
            .as_ref()
            .unwrap()
            .unbuilt_custom_harnesses,
        ["exec-fixture:test:custom"]
    );
    let enabled = fixture
        .test(TestRequest {
            features: vec!["extra".into()],
            ..request
        })
        .await;
    assert_eq!(enabled.status, "INCONCLUSIVE", "{enabled:#?}");
    assert_eq!(
        enabled.test_coverage.as_ref().unwrap().custom_harnesses,
        ["exec-fixture:test:custom"]
    );
    assert_eq!(enabled.tests.as_ref().unwrap().executed, Some(2));
    assert!(!enabled.tests.as_ref().unwrap().counts_known);
    fixture.close().await;
}
