# Validation And Benchmark Protocol

**An AGZ Yazılım product.** This is step 4 of the
[documentation reading path](README.md): it defines how release claims are
validated with deterministic local gates and optional live benchmarks. The
next step is the [security policy](../SECURITY.md) and
[contributing guide](../CONTRIBUTING.md).

Release claims use deterministic local gates first. Live model benchmarks are
optional measurements and never replace compiler, protocol, package, or
security checks.

## Required Provider-Free Smokes

```bash
cargo run -p xtask -- protocol-smoke
cargo run -p xtask -- benchmark-smoke
cargo run -p xtask -- task-benchmark-smoke
```

`protocol-smoke` starts the real stdio binary and checks initialization, tool
discovery, structured/text parity, all embedded skills through the `workspace`
tool, prompts and resources (including content checksums), `2026-07-28` task
creation, progress, cancellation, terminal state, synchronous fallback, and
fixture cleanup.

`benchmark-smoke` runs frozen clean and broken Rust fixtures against an oracle.
It verifies that status and `passed` fields agree and compares current behavior
to the preserved benchmark contract.

`task-benchmark-smoke` extends that benchmark harness with the frozen
`rust-agent-tasks-v1` corpus. It replays eight Rust task classes over three
paired and position-balanced repetitions with three distinct order seeds:
A) ordinary shell/file work, B) the preserved `0.2.0` MCP surface, and C) the
Rust Change Engine contract. The task request contains only public task data;
independent oracle observations and mutation guards decide success. The replay
retains failed, timeout, and cancelled trials instead of filtering them.

The provider-free replay is a harness fixture, not measured model performance.
Its timing, Cargo-call, recompile, and host-turn numbers exist to exercise
aggregation and gate logic. Input/output/cache/schema token counts and cost are
recorded as `unknown`, never as zero. Real provider/model measurements remain
explicit opt-in work.

The frozen v1 corpus covers missing trait implementation, borrow/move repair,
exact crate API use, multi-crate signature migration, feature-only breakage,
regression-test addition, dependency upgrade, and behavior-preserving
performance work. `xtask/tests/task_benchmark.rs`, which is part of the normal
workspace CI test command, verifies corpus hashes, replay completeness,
independent scoring, unknown usage semantics, negative controls, and balanced
ordering. CI also invokes `task-benchmark-smoke` directly on the platform matrix
so transcript replay and evidence publication are exercised as a product path.

These three smokes are client-independent release gates. They run on Linux,
macOS, and Windows without installing a particular agent client or contacting
a model provider. Select a small package or test target during development;
use the complete platform and packaging gates for release evidence. A successful
filtered test is evidence only for the selected tests, never for the full
workspace or every feature combination.

### Toolchain for local Cargo measurements

The `compact_response.py`, `request_latency.py`, `scope_selection.py`, and
`verify_scope.py` scripts require Python 3.11 or later and read their default
Rust toolchain from `rust-toolchain.toml`. Pass `--toolchain <version>` to
reproduce an earlier measurement. Reports record the selected toolchain and
comparisons reject different toolchains; use the same selection for both arms.
Recorded historical reports retain the versions used when they were measured.

## Offline Workspace and Catalogue Measurements

On Linux, use an already-built binary to measure the real stdio `workspace`
inspect/search/read calls and compare the `core` and `full` tool catalogues:

```bash
python3 benchmark/workspace_context_benchmark.py \
  --binary /absolute/path/to/agz-rust-mcp \
  --out benchmark/results/workspace-context.json \
  --samples 20 --build-profile release
```

The standard-library script creates and removes a temporary eight-crate fixture
without a lockfile. It records raw samples, median/nearest-rank P95, wire bytes,
five fresh-process initialization samples, binary and fixture hashes, host
information and load averages. Cargo and Rust Analyzer are replaced by
execution-detecting sentinels; the run also verifies unchanged source, no new
lockfile/target directory, JSON-RPC framing and structured/text equivalence.
`--samples` accepts 5–200; this is an optional measurement, not a CI gate.

Timings include Python transport and JSON parsing. OS page caches are not
flushed, concurrent host load is not controlled, and profiles run in the recorded
order. Compare like build profiles and conditions; fresh-process startup is not
a cold-filesystem measurement. Catalogue byte reductions are not token counts,
and these fixture measurements do not establish general Rust project or model
performance. The current harness is Linux-only; this does not narrow the
server's supported platforms.

## Predeclared Task Benchmark Gates

Before Change Engine implementation, the corpus fixes these comparison rules:

- quality: C may not fall below B; the `non_inferiority_margin` is 0 percentage
  points for v1;
- efficiency: C targets at least 20% fewer host turns and 15% fewer Cargo calls
  than B;
- wall time: C may regress by at most 10% versus B.

Every arm reports sample count, success rate with a Wilson 95% interval, wall
time dispersion, CPU time, snapshot preparation, Cargo/recompile counts,
host turns, cold/warm cache strata, token fields, cost, timeout and
cancellation outcomes. Provider-free replay can prove the harness evaluates
these gates, but it can never make Change Engine default-on; comparable opt-in
live runs are required for that decision.

Raw replay observations live in
`benchmark/task-corpus/provider-free-replay.json`. The frozen task manifest,
embedded `fixtures.json` workspace snapshots, and independent oracle catalog
live beside it. Fixture and settings hashes bind results to the exact corpus.
The recorded provenance includes provider, model, harness version, MCP SHA,
fixture hash, toolchain, OS/hardware, cache state, and settings hash.

## Evidence Layout

Each run is atomically published under `benchmark/results/stage7/` with:

- `run.json`: `run_id`, mode, fixture, protocol and adapter metadata;
- `results.json`: observations and pass/fail assertions;
- `report.md`: human-readable bounded summary;
- `provenance.json`: `source_commit`, `source_checksum`, dirty state, and command
  identity.

No report contains prompts, absolute workspace paths, session IDs, credentials,
or private source. Concurrent publishers use a lock and unique final directory.

## Live Mode

Live mode requires an explicitly reviewed adapter and an explicit operator
decision because it may incur cost:

```bash
AGZ_RUST_MCP_LIVE_ADAPTER=/absolute/path/to/reviewed-adapter \
  cargo run -p xtask -- benchmark-smoke --live
```

The manifest records `provider` / `model` / `variant`, repetitions, fixtures,
cost when available, and the `non_inferiority_margin`. Adapter output is rejected
if its boolean pass field contradicts its typed status. Results from different
source or adapter checksums are not pooled.

## Release Gate

The minimum local release gate is:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
cargo +1.91.0 check --workspace --all-targets --all-features --locked
cargo build --release --locked
cargo package -p agz-rust-mcp --locked
cargo publish -p agz-rust-mcp --dry-run --locked
```

The three required provider-free smokes, the real pinned Rust Analyzer smoke,
and `cargo deny check` complete the automated release gate. Run the real docs
adapter checks when changing that adapter. Platform CI supplies Linux, macOS,
and Windows process and path coverage. Report any additional manual checks with
their actual results; a configured or skipped job is not validation evidence.

The release workflow verifies fresh installation from the packaged crate, source
bundle identity, archive checksums, and exact package/tag/registry version
agreement. Before publishing a new crate, it checks that `CRATES_IO_TOKEN` is
configured and authenticates to the MCP Registry using GitHub OIDC. A missing
credential or failed Registry login stops publication before that first write.
The token's presence does not prove that crates.io will authorize a publish;
the publish response remains authoritative. Existing crate versions must match
the verified checksum, and an existing release tag is never moved or reused.

## Input-identity comparisons

Build `crates/agz-rust-mcp/examples/identity_measure.rs` against the baseline and
candidate using the same toolchain/profile. Keep both binaries, then run
`python3 benchmark/identity_compare.py BASELINE CANDIDATE --output comparison.json`.
The script generates identical fixtures, alternates measurement order, warms
each case three times and records 15 samples by default. Source edits and manifest
edits are separate scenarios. Unequal input hashes invalidate a comparison.
Run `python3 -m unittest discover -s benchmark -p test_identity_compare.py` to test
comparison rejection. No LLM, network or Cargo execution is involved in the
measured stage. See [recorded evidence](rust-efficiency-evidence.md).
