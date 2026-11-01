# Architecture

**An AGZ Yazılım product.** This is step 3 of the
[documentation reading path](README.md): it explains the process model behind
the [tool and configuration reference](tools.md). Continue with the
[validation and benchmark protocol](benchmark.md).

`agz-rust-mcp` is one Rust process with standard MCP stdio and Streamable HTTP
adapters, bounded domain services, and supervised external processes. HTTP binds
only to a loopback address. The server never writes workspace source.

## Data Flow

1. `main` loads strict configuration and starts RMCP on stdin/stdout by default,
   or serves Streamable HTTP at `/mcp` when `server.transport="http"`.
2. The server negotiates protocol `2025-11-25` by default and can discover
   `2026-07-28` support.
3. Client roots narrow the configured root set; every workspace and dependency
   path is checked by the root guard.
4. Admission control bounds concurrent tools and tasks.
5. Domain services run Cargo, static audit, documentation, or Rust Analyzer
   operations through shared process supervision.
6. Responses are normalized into deterministic structured data and equivalent
   text under one wire-size limit.
7. Shutdown closes admission, cancels tasks, stops Rust Analyzer, terminates
   supervised process groups/jobs, flushes telemetry, and reports incomplete
   cleanup as failure.

## Components

| Component | Responsibility |
| --- | --- |
| `server` | MCP tools, resources, prompts, tasks, progress, response parity. |
| `transport` | Loopback Streamable HTTP, Host/Origin validation, service and body limits. |
| `skills` | Versioned offline workflows embedded in the executable; tools, prompts, resources and optional export. |
| `workspace` | Root authorization, package selection, metadata, input identity. |
| `gate` | Cargo targets, preflight, singleflight, cache, host leases. |
| `process` | Bounded output, deadlines, process groups/Job Objects, recovery journal. |
| `lsp` | Rust Analyzer lifecycle, document sync, navigation, write-free edits. |
| `docs` | Lockfile resolution, cache, source/docs.rs/local fallback. |
| `tools` | Validation, audit, crate lookup, semantic domain operations. |
| `telemetry` | Bounded local activity records and atomic rotation. |

## Protocol Lifecycle

The server exposes tools, resources, prompts, roots, progress, cancellation, and
tasks. Task-capable requests return `CreateTaskResult`; polling and
`tasks/cancel` use RMCP's negotiated task model. A client without task support
receives the same domain operation synchronously.

Root changes increment an epoch. Work tied to an older root generation is
cancelled, and root-sensitive cache authority is invalidated. Client roots are
not authentication and cannot authorize a path absent from configured roots.

HTTP keeps domain state for the process lifetime so modern stateless requests can
continue a task or expand an existing context capsule. Each legacy session or
modern request has a separate client-root coordinator. A bounded authority cache
keeps unrelated root sets from invalidating each other's evidence; a client's
root-change notification still revokes its previous authority generation.

Each process serves one trusted user's domain. Task IDs and tool state are shared
within that domain; an HTTP session is not a separate user account. Use a separate
server process for each security principal. Remote access requires an authenticated
HTTPS proxy in front of the loopback listener. Browser Origin headers are rejected,
and the proxy must preserve an accepted local Host authority. The standard MCP
SDK handles legacy session IDs and modern per-request protocol metadata.

## Authority Model

Cargo and rustc output decides validation. Rust Analyzer and source audit are
advisory. A successful result binds to the observed worktree/input identity and
is not reused for a later explicit validation request.

Semantic rename/refactor responses contain bounded edit packages. The server
verifies containment, file versions, overlap, and original text but does not
apply changes.

## Process And Storage Boundaries

Unix commands use process groups; Windows commands use Job Objects. Timeout and
shutdown attempt graceful stop, then bounded force termination and reap. A
journal retains entries when cleanup cannot be proven complete.

Server-owned state lives outside authorized roots under the platform
`agz-rust-mcp` namespace. Cache publication uses lock-protected,
same-directory atomic replacement. Symlink or parent-identity uncertainty fails
closed.

## Residual Risk

This architecture is not a sandbox. Cargo build scripts, tests, procedural
macros, local rustdoc, and explicitly permitted Rust Analyzer workspace code can
use the operating-system account's filesystem, process, and network rights.
Unix descendants can deliberately escape a process group. Stronger guarantees
require a container or OS sandbox.

## Distribution

The crate and binary are `agz-rust-mcp`. Release tags use
`agz-rust-mcp-v<version>`. The official MCP Registry identity is
`io.github.agzyazilim/rust-mcp`, backed by the exact crates.io package
version and the repository's `server.json`.
