# Contributing

_An AGZ Yazılım product._

Contributions to `agz-rust-mcp` should preserve its bounded, source-write-free
MCP contract.

## Setup

Install Rust `1.88.0` with rustfmt, Clippy, and Rust Analyzer. Clone the
repository and run:

```bash
cargo +1.88.0 test --workspace --all-targets --all-features --locked
```

Do not edit generated files under `target/` or benchmark results. Keep new
dependencies minimal and exact-pin security- or release-sensitive tools.

## Change Rules

- Keep stdout exclusive to MCP stdio frames.
- Treat Cargo/rustc output as authority and semantic output as advisory.
- Do not make `rename`, `refactor`, suggestions, or `fmt --check` write source.
- Reject workspace/dependency path escapes and unverifiable symlink state.
- Bound processes, HTTP bodies, filesystem walks, tasks, edits, and responses.
- Keep structured and text response status equivalent.
- Update English and Turkish public documents together when behavior, defaults,
  identifiers, commands, versions, or links change.
- Add regression tests for every corrected failure mode.

## Validation

Run focused tests while developing, then the complete gate:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
cargo +1.88.0 check --workspace --all-targets --all-features --locked
cargo build --release --locked
cargo run -p xtask -- protocol-smoke
cargo run -p xtask -- benchmark-smoke
cargo run -p xtask -- task-benchmark-smoke
```

Use the ignored real Rust Analyzer and documentation tests when those adapters
change. Run `cargo deny check` for dependency or policy changes.

The core CI runs on Linux, macOS, and Windows and does not require a particular
MCP client or model provider. When changing the legacy OpenCode2 adapter, install
the pinned client and run `cargo run -p xtask -- opencode-smoke` separately. The
CI workflow's manual `legacy_opencode` input enables the same optional
compatibility job; it is not part of the release regression gate.

## Pull Requests

Keep commits reviewable and explain user-visible behavior, preserved invariants,
tests, and residual risk. Never include credentials, private source, prompts,
absolute paths, session IDs, or unbounded logs. Report vulnerabilities through
the private process in [SECURITY.md](SECURITY.md), not a public issue or PR.

## Releases

Releases are immutable and use `agz-rust-mcp-v<version>`. A release requires a
clean reviewed commit, exact package/tag checksum agreement, platform CI, a
crates.io package whose README exposes the MCP ownership marker, and matching
`server.json` metadata. Never move a published tag or overwrite a package
version.

Prepare the version, changelog, package README, and `server.json` in one reviewed
commit. Push that exact commit to `release/<version>` to request publication, or
dispatch the Release workflow with its exact tag and `publish=true`. The workflow
creates the tag after all verification gates; do not create the release tag by
hand. The `release` environment must supply `CRATES_IO_TOKEN`, and GitHub OIDC
must authorize the MCP Registry namespace recorded in `server.json`. The publish
job checks those prerequisites before the first package publication and never
prints credentials.
