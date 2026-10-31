# AGZ Rust MCP

[![CI](https://github.com/agzyazilim/rust-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/agzyazilim/rust-mcp/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/agz-rust-mcp.svg)](https://crates.io/crates/agz-rust-mcp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

English | [Türkçe](README.tr.md)

**An AGZ Yazılım product.** `agz-rust-mcp` is a standalone MCP server with stdio and Streamable HTTP transports for
general-purpose Rust development grounded in Cargo and rustc. Discover a project
offline, collect focused source context and compiler diagnostics, select individual
tests, and run binaries/examples. Exact-version dependency docs, Rust Analyzer
navigation and write-free change/refactor packages support deeper work.

## Everyday workflow

The same workflow works with a library, CLI, service or multi-package Cargo
workspace. No framework-specific project structure or particular MCP client is
required.

| Need | Tool call |
| --- | --- |
| Map packages, declared targets, features and dependencies without compiling | `workspace(action="inspect")` |
| Locate code and read bounded, numbered source | `workspace(action="search", query="parse_record")`, then `workspace(action="read", path="src/lib.rs", line=1, lineCount=80)` |
| Get fresh compiler diagnostics | `check(target="check", options={"packages":["my_crate"]})` |
| Discover actual test names | `test(action="list", packages=["my_crate"])` |
| Run one library test by its full name | `test(action="run", packages=["my_crate"], cargoTarget={"kind":"lib"}, filter="tests::parses_record", exact=true)` |
| Compile tests without executing them | `test(action="compile", packages=["my_crate"])` |
| Execute a binary or example | `run(package="my_crate", target={"kind":"example","name":"demo"}, args=["input.json"], timeoutMs=30000)` |
| Resolve an unfamiliar dependency API | `docs` with its exact version; `api` for deeper resolution/probing |

Pass an absolute `dir` for the active project when the client starts elsewhere.
A function is exercised through a test or executable example. `run` is a typed
Cargo runner; program arguments remain literal, including spaces and shell
metacharacters. Tests and applications execute project code with its ordinary
side effects. Empty selections return `NO_TESTS`, not a passing test verdict.

Use `agz-rust-mcp --toolset core` for a compact catalog of six everyday tools:
`workspace`, `check`, `test`, `run`, `docs`, and `crate_lookup`. The default
`--toolset full` preserves every existing tool plus the new additions. Individual
`--tools-<name> false` switches can disable tools further. See the
[full reference](docs/tools.md#everyday-rust-workflow) for limits and result states.

## Install

```bash
# installer script (Linux x86_64): download install.sh and SHA256SUMS from the
# latest release, verify the script, then run it
bash install.sh

# crates.io (requires Rust 1.91.0 or newer)
cargo install agz-rust-mcp --locked
agz-rust-mcp --version
```

Prebuilt `.tar.gz` archives with `.sha256` files are available for Linux
x86_64, macOS arm64, and Windows x86_64. Releases up to `0.2.0` use the legacy
`agz-rust-coder-*` asset names; from `0.3.0` onward the names are
`agz-rust-mcp-*`.

**Step-by-step instructions, per-OS notes, checksum verification, and
troubleshooting: [docs/install.md](docs/install.md) ·
[Türkçe](docs/install.tr.md).**

## MCP Client Setup

Use the installed executable with any client supporting local stdio MCP. Supply
these fields in the client's own server configuration format:

```json
{
  "command": "agz-rust-mcp",
  "args": ["--allow-root", "/absolute/path/to/project"],
  "env": {"AGZ_RUST_MCP_SERVER__TOOLSET": "core"}
}
```

For a client that uses HTTP, start the same binary as a local Streamable HTTP
server and configure `http://127.0.0.1:8787/mcp` as its MCP URL:

```bash
agz-rust-mcp --transport http --http-bind 127.0.0.1:8787 \
  --allow-root /absolute/path/to/project
```

The HTTP listener is loopback-only. It supports the session lifecycle used by
`2025-11-25` clients and the stateless `2026-07-28` protocol. Client-side display
prefixes, configuration file locations, and automatic prompt selection belong
to the client; the server depends on none of them. The canonical working
directory is the default authorized root; client-provided roots can only narrow
configured access. See [connection details](docs/install.md#mcp-client-setup).

## Built-in skills

Four offline skills ship inside the executable: `agz-rust-workflow`,
`agz-rust-repair`, `agz-rust-refactor`, and `agz-rust-performance`.
They are available immediately through all three standard MCP interfaces:

- Tools: `workspace(action="skills")` lists the catalog;
  `workspace(action="skills", skill="agz-rust-workflow")` reads a skill.
- Prompts: `prompts/list` and `prompts/get` expose `workflow`, `repair`,
  `refactor`, and `performance`.
- Resources: `resources/read` exposes `agz-rust-mcp://skills/<skill-name>`.

A client that only exposes tools can use the complete skill content. Version and
SHA-256 metadata identify the exact bundled instructions. No skill installation
or separate MCP is required. Optional `agz-rust-mcp skills export --dir <new-directory>`
creates files for clients that also support filesystem skills; existing files
are never overwritten. See [built-in skills](docs/install.md#built-in-skills).

## Documentation

Read the documentation in this order:

1. [Install and client setup](docs/install.md) - every install method and
   client configuration.
2. [Tool and configuration reference](docs/tools.md) - tools, actions, result
   semantics, and all configuration keys.
3. [Architecture](docs/architecture.md) - process model, protocol lifecycle,
   and authority boundaries.
4. [Validation and benchmark protocol](docs/benchmark.md) - smokes, gates, and
   evidence.
5. [Security policy](SECURITY.md) and [Contributing](CONTRIBUTING.md).

The bilingual index with the full reading path is
[docs/README.md](docs/README.md) / [docs/README.tr.md](docs/README.tr.md).

## Identity

| Contract | Value |
| --- | --- |
| Crate, binary, server | `agz-rust-mcp` |
| MCP Registry | `io.github.agzyazilim/rust-mcp` |
| Source version | `0.6.0` |
| First release | `0.1.0` |
| Release tag | `agz-rust-mcp-v<version>` |
| Rust edition / MSRV | `2024` / `1.91.0` |
| Rust MCP SDK | `rmcp` `3.8.0` |
| Default / discovered protocol | `2025-11-25` / `2026-07-28` |

MCP package ownership marker: `mcp-name: io.github.agzyazilim/rust-mcp`.

## Tools

These are the exact server tool names. Use the names returned by `tools/list`;
a client may display its own namespace or prefix.

| MCP tool | Default | Purpose |
| --- | --- | --- |
| `workspace` | `enabled` | Read built-in skills, inspect manifests/lockfiles, search source and read excerpts offline. |
| `test` | `enabled` | Discover, compile or run selected tests with exact names, ignored-test controls and bounded execution. |
| `run` | `enabled` | Run an exact Cargo binary/example with literal arguments, a deadline and bounded output. |
| `check` | `enabled` | Run bounded Cargo check, Clippy, tests, docs, or the full gate. |
| `profile` | `enabled` | Analyze observed Cargo rebuild behavior and compare bounded build evidence without claiming unmeasured speedups. |
| `audit` | `enabled` | Scan Rust source for bounded static findings. |
| `crate_lookup` | `enabled` | Verify a crate and optional exact version on crates.io. |
| `docs` | `enabled` | Resolve exact-version docs from cache, local sources, or docs.rs. |
| `context` | `enabled` | Prepare, expand, or delta a revision-bound semantic context capsule with per-item reasons. |
| `api` | `enabled` | Resolve an API signature from bounded analyzer evidence or type-check a candidate snippet in an isolated copy of the workspace configuration. |
| `explain` | `enabled` | Explain macro expansion provenance, trait obligations, or cfg enablement. |
| `verify` | `enabled` | Plan or run a bounded feature, target, toolchain, and stage matrix. |
| `symbol` | `enabled` | Read Rust Analyzer hover data for one symbol. |
| `references` | `enabled` | Find bounded references. |
| `definition` | `enabled` | Find the selected definition. |
| `symbols` | `enabled` | List symbols in one Rust file. |
| `implementations` | `enabled` | Find implementations. |
| `hierarchy` | `enabled` | Trace a bounded call hierarchy. |
| `rename` | `enabled` | Produce a verified rename edit package without applying it. |
| `refactor` | `enabled` | Produce a verified refactor edit package without applying it. |
| `change` | `enabled` | Create, stage, migrate, and validate a revision-bound changeset in server-owned scratch without writing the workspace. |
| `repair` | `enabled` | Analyze, try, compare, and minimize compiler-driven repair candidates for a failing change revision without writing the workspace. |
| `work` | `enabled` | Drive a typed intent through change/validate with explicit gates and budgets, returning honest gate evidence or a bounded single-use handoff. |

Every tool returns deterministic structured data plus an equivalent bounded text
fallback. External data stays under `untrustedData`. Expected domain outcomes
such as compiler failure, missing crates, or unavailable docs are typed results;
invalid input, authorization failure, resource exhaustion, and unavailable
semantic infrastructure are protocol errors.

## Configuration

Precedence is CLI, then `AGZ_RUST_MCP_*` environment variables, then the
explicit `--config` TOML file, then defaults. Environment keys use `__` between
sections, for example `AGZ_RUST_MCP_GATE__HARD_TIMEOUT_MS=600000`.

| Key | Default | Meaning |
| --- | --- | --- |
| `server.transport` | `stdio` | MCP transport: `stdio` or `http`. |
| `server.http_bind` | `127.0.0.1:8787` | Loopback address for the HTTP `/mcp` endpoint. |
| `server.allow_roots` | canonical CWD | Workspace read/command boundary. |
| `server.allow_dependency_roots` | empty | Explicit external path-dependency roots. |
| `gate.hard_timeout_ms` | `600000` | Cargo operation deadline. |
| `gate.scope` | `shadow` | Validation target: `workspace`, `shadow`, or `affected`. |
| `gate.cache` | `auto` | Cache policy: `auto`, `project`, or `isolated`. |
| `rust_analyzer.workspace_code` | `deny` | Reject RA startup unless workspace code is disabled. |
| `docs.fallback` | `auto` | Documentation source policy. |
| `profile.max_report_bytes` | `4194304` | Bounded read/store cap for one Cargo timing artifact. |
| `profile.max_runs` | `4` | Fresh Cargo runs available to one `profile` call. |
| `profile.compare_samples` | `3` | Required samples per side before any speed claim. |
| `limits.tool_output_bytes` | `49152` | Maximum serialized tool result size. |
| `change.max_bytes` | `268435456` | Maximum captured candidate bytes per changeset. |
| `repair.max_candidates` | `4` | Candidates one `repair` action may try. |
| `repair.max_compiles` | `4` | Cargo validations one `repair` action may run. |
| `repair.wall_time_ms` | `120000` | Wall-clock budget for one `repair` action. |
| `repair.minimize_max_candidates` | `32` | Compile-evaluated reduction attempts one `repair(action=minimize)` may try. |
| `repair.minimize_max_compiles` | `16` | Cargo runs one `repair(action=minimize)` may execute, including reproduction and export verification. |
| `work.max_candidates` | `4` | Host candidate revisions one work item may stage. |
| `work.max_compiles` | `12` | Gate validations one work item may run. |
| `work.wall_time_ms` | `600000` | Wall-time budget for one work item. |
| `telemetry.enabled` | `true` | Bounded local activity records without prompts or source. |

`profile` evidence is bounded: the latest 64 records stay in memory, and
persisted timing artifacts under the server-owned `profile-evidence` directory
are pruned after 24 hours or beyond 64 files. The `profile` result reports this
retention policy, so expired evidence is visible instead of silently reused.

Run `agz-rust-mcp --help` for every CLI field. The complete behavior and
default table is in [docs/tools.md](docs/tools.md#configuration-reference).

## Security

The server does not modify workspace source, but it is not an operating-system
sandbox. Cargo build scripts, tests, procedural macros, local rustdoc, and
opted-in Rust Analyzer workspace code execute with the server user's authority.
Use a container or OS sandbox when that boundary is required. Workspace,
dependency, cache, lease, journal, docs, and telemetry paths are canonicalized,
bounded, and fail closed, and stdout is reserved for MCP framing. Report
vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## Links

- Repository: https://github.com/agzyazilim/rust-mcp
- Crate: https://crates.io/crates/agz-rust-mcp
- SDK docs: https://docs.rs/rmcp/3.8.0/rmcp/
- MCP `2025-11-25`: https://modelcontextprotocol.io/specification/2025-11-25
- MCP `2026-07-28`: https://modelcontextprotocol.io/specification/2026-07-28

## License

[MIT](LICENSE), Copyright (c) 2026 Ugur Murat Altintas.
