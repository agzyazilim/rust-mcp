# agz-rust-mcp

_An AGZ Yazılım product._

`agz-rust-mcp` is a general-purpose Rust development MCP server with bounded
source context, compiler diagnostics, focused tests and Cargo target execution.
Cargo and rustc remain the authority; Rust Analyzer and static audit results are
advisory.

- MCP Registry name: `mcp-name: io.github.agzyazilim/rust-mcp`
- Version: `0.5.0`
- MSRV: Rust `1.88.0`
- Transport: stdio

```bash
cargo install agz-rust-mcp --locked
agz-rust-mcp --version
```

The server exposes `workspace`, `test`, `run`, `check`, `profile`, `audit`, `crate_lookup`, `docs`,
`context`, `api`, `explain`, `verify`, `symbol`, `references`, `definition`,
`symbols`, `implementations`, `hierarchy`, `rename`, `refactor`, `change`,
`repair`, and `work`.
Start with offline `workspace` discovery, then focused `check`, `test` or `run`.
Use `--toolset core` for six everyday tools, or the default full catalog.
Edit tools return bounded packages and never modify source. Project code run
by Cargo tests, build scripts or `run` may have side effects. The `context` tool
builds revision-bound capsules with per-item reasons, bounded pagination, and
stale/delta tracking. The `api` tool resolves signatures from bounded analyzer
evidence and type-checks candidate snippets only in a discarded candidate copy.
The `change` tool keeps a revision-bound candidate copy with validation evidence
and never writes workspace source.

The current source also bundles `agz-rust-workflow`, `agz-rust-repair`,
`agz-rust-refactor`, and `agz-rust-performance` as offline MCP prompts/resources.
Use `agz-rust-mcp skills list`, `skills show <name>`, or
`skills export --dir <new-directory>` for filesystem-based skill discovery.
Export never replaces an existing directory. Bundled skills are available from
`0.4.0` and are not present in `0.3.0` binaries.

See the [project repository](https://github.com/agzyazilim/rust-mcp)
for client configuration, security boundaries, and release artifacts.

[MIT](LICENSE)
