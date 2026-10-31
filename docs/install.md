# Install And Client Setup

**An AGZ Yazılım product.** This is step 1 of the
[documentation reading path](README.md). It covers every install method,
per-OS notes, MCP client configuration, verification, and troubleshooting.
The next step is the [tool and configuration reference](tools.md).

`agz-rust-mcp` is one executable with standard stdio and local Streamable HTTP
MCP transports. No agent harness, model provider, client plugin, or separate
skill package is required. Every method below installs the same executable.

## Built-in skills

Four versioned `SKILL.md` files are compiled into the binary and work offline:
`agz-rust-workflow`, `agz-rust-repair`, `agz-rust-refactor`, and
`agz-rust-performance`. The content is available immediately after connecting.

| MCP interface | Discovery | Read one skill |
| --- | --- | --- |
| Tools, including tools-only clients | `workspace(action="skills")` | `workspace(action="skills", skill="agz-rust-workflow")` |
| Prompts | `prompts/list` | `prompts/get` with `name="workflow"` |
| Resources | `resources/list` | `resources/read` with `uri="agz-rust-mcp://skills/agz-rust-workflow"` |

The tool accepts the full skill name or its short prompt name (`workflow`,
`repair`, `refactor`, `performance`). Its catalog includes version and SHA-256
metadata; selected content supports `line`, `lineCount`, and `byteBudget` paging.
A skill read does not require project roots or start Cargo/Rust Analyzer. MCP
prompts accept an optional `task` string bounded to 4,000 sanitized characters.
Clients control when guidance is selected; supporting MCP resources does not
imply that a client automatically invokes them.

CLI inspection and export are optional:

```bash
agz-rust-mcp skills list
agz-rust-mcp skills show agz-rust-workflow
# Existing trusted parent; destination must not exist:
agz-rust-mcp skills export --dir /path/to/new-skills
```

Export creates one folder and `SKILL.md` per skill, refuses existing destinations,
and reports partial output on failure. It does not change client settings or
install into any client's configuration directory. These CLI commands do not
start MCP/Cargo or load server configuration. Updating the binary updates its
embedded guidance; an earlier export is a snapshot and needs an explicit refresh.

## Requirements

- Linux, macOS, or Windows on x86_64, or macOS on arm64.
- A client supporting local stdio MCP or local Streamable HTTP MCP.
- Rust `1.91.0` or newer to compile this server through `cargo install` or from source.
- Cargo and the target project's Rust toolchain for metadata, compiler diagnostics,
  test execution, and binary/example execution, including with a prebuilt server.
- Rust Analyzer only for semantic navigation and edit proposals. Nextest and
  sccache are optional accelerators; the ordinary Cargo workflow remains available.

## 1. Installer Script (`install.sh`)

The installer downloads a release archive, verifies its published SHA-256
checksum before extraction, refuses symlink destinations, verifies the staged
binary, and installs atomically. It currently supports **Linux x86_64 only**;
other platforms get a clear error and should use method 2, 3, or 4.

Download the installer and checksum manifest from the release page, verify the
script itself, then run it:

```bash
curl -fsSL -O \
  https://github.com/agzyazilim/rust-mcp/releases/latest/download/install.sh
curl -fsSL -O \
  https://github.com/agzyazilim/rust-mcp/releases/latest/download/SHA256SUMS
grep ' install.sh$' SHA256SUMS | sha256sum --check
# macOS: grep ' install.sh$' SHA256SUMS | shasum -a 256 -c -

bash install.sh
```

Do not run the installer if the script checksum fails; re-download from the
release page instead. Environment overrides:

| Variable | Default | Meaning |
| --- | --- | --- |
| `AGZ_RUST_MCP_VERSION` | installer release version (`0.6.0` in this source) | Exact version to install. |
| `AGZ_RUST_MCP_INSTALL_DIR` | `$HOME/.local/bin` | Absolute install directory. |

```bash
AGZ_RUST_MCP_VERSION=0.6.0 AGZ_RUST_MCP_INSTALL_DIR="$HOME/.local/bin" bash install.sh
```

The installer also verifies the archive layout and the extracted binary's
`--version` before replacing an existing file. A `SHA256SUMS` file covers the
crate, source bundle, and installer; each archive ships its own `.tar.gz.sha256`.

## 2. Prebuilt Archives

Release pages provide `.tar.gz` archives with a matching `.sha256` file:

| Platform | Archive (0.3.0 and later) |
| --- | --- |
| Linux x86_64 | `agz-rust-mcp-linux-x86_64.tar.gz` |
| macOS arm64 | `agz-rust-mcp-macos-arm64.tar.gz` |
| Windows x86_64 | `agz-rust-mcp-windows-x86_64.tar.gz` |

**Legacy 0.1.0-0.2.0 assets.** Releases up to and including `0.2.0` were
published under the former product name and use the old asset and executable
names. The files remain available and `install.sh` maps them automatically:

| Release line | Archive name | Archived executable |
| --- | --- | --- |
| `0.1.0`-`0.2.0` | `agz-rust-coder-<platform>-<arch>.tar.gz` | `agz-rust-coder` |
| `0.3.0` and later | `agz-rust-mcp-<platform>-<arch>.tar.gz` | `agz-rust-mcp` |

Whatever the archive name, the installed executable is always `agz-rust-mcp`;
`install.sh` renames a legacy archive executable during installation.

Linux x86_64:

```bash
sha256sum --check agz-rust-mcp-linux-x86_64.tar.gz.sha256
tar -xzf agz-rust-mcp-linux-x86_64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 agz-rust-mcp "$HOME/.local/bin/agz-rust-mcp"
```

macOS arm64:

```bash
shasum -a 256 -c agz-rust-mcp-macos-arm64.tar.gz.sha256
tar -xzf agz-rust-mcp-macos-arm64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 agz-rust-mcp "$HOME/.local/bin/agz-rust-mcp"
```

Windows PowerShell:

```powershell
$archive = "agz-rust-mcp-windows-x86_64.tar.gz"
$expected = ((Get-Content "$archive.sha256" -Raw) -split '\s+')[0].ToLowerInvariant()
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -notmatch '^[a-f0-9]{64}$' -or $actual -ne $expected) {
    throw "Release archive checksum mismatch"
}
tar -xzf $archive
# Add the extraction folder to PATH.
```

The commands verify the archive against its `.sha256` file before extracting.
Do not proceed if verification fails.

## 3. crates.io (`cargo install`)

Requires Rust `1.91.0` or newer:

```bash
rustup toolchain install 1.91.0
cargo install agz-rust-mcp --locked
agz-rust-mcp --version
```

`--locked` builds with the exact dependency versions recorded in `Cargo.lock`.
Pin a release with `cargo install agz-rust-mcp --version 0.6.0 --locked`.
Cargo installs to `$HOME/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows);
ensure that directory is on `PATH`.

## 4. From Source

```bash
git clone https://github.com/agzyazilim/rust-mcp
cd rust-mcp
cargo +1.91.0 build --release -p agz-rust-mcp --locked
./target/release/agz-rust-mcp --version
```

MSRV is Rust `1.91.0` with edition 2024; `rust-toolchain.toml` pins the toolchain
used by CI. Copy `target/release/agz-rust-mcp` (`.exe` on Windows) to a directory
on `PATH`, or point the client at its absolute path. See
[CONTRIBUTING.md](../CONTRIBUTING.md) for the full development gate.

## Per-OS Notes

- **Linux.** `install.sh` is the shortest verified path (x86_64 only). Add
  `$HOME/.local/bin` to `PATH` when the installer reports it is missing.
- **macOS.** Use the `macos-arm64` archive or `cargo install`. Intel Macs have
  no prebuilt archive in this release; build from source. If the OS blocks the
  unsigned binary, allow it under System Settings -> Privacy & Security.
- **Windows.** Use the `windows-x86_64` archive or `cargo install`; `install.sh`
  does not support Windows. Add the extraction directory to `PATH`.

## MCP Client Setup

Check `agz-rust-mcp --version` before connecting. Use an absolute binary path when
the client's `PATH` differs from your development shell, and give the server
access to the project's Rust/Cargo toolchain. The invocation fields below are
adapted to the client's own configuration format; they are not a universal
client configuration file.

### Stdio

```json
{
  "command": "agz-rust-mcp",
  "args": ["--allow-root", "/absolute/path/to/project"],
  "env": {"AGZ_RUST_MCP_SERVER__TOOLSET": "core"}
}
```

Stdio is the default. The client starts the process and exchanges newline-delimited
JSON-RPC on stdin/stdout. Diagnostics go to stderr. No shell wrapper or model API
key is required.

### Streamable HTTP

Start the process separately:

```bash
agz-rust-mcp --transport http --http-bind 127.0.0.1:8787 \
  --allow-root /absolute/path/to/project
```

Configure the client's Streamable HTTP connection with:

```json
{
  "url": "http://127.0.0.1:8787/mcp"
}
```

The same endpoint supports `2025-11-25` session clients and the stateless
`2026-07-28` protocol. Binding is restricted to loopback addresses; a local native
client connects directly. HTTP requests with an `Origin` header are rejected,
and `Host` must match the listener address or `localhost` with its port. Browser
page scripts therefore cannot call this local service directly.

One server process is one trusted user's workspace domain: changes, capsules,
and tasks share its configured access and lifetime. Remote use requires an
authenticated HTTPS proxy that rewrites `Host` to the loopback listener; use a
separate server process for each security principal. No HTTP-to-stdio bridge is
needed.

The server's TOML configuration can express the same settings:

```toml
[server]
transport = "http"
http_bind = "127.0.0.1:8787"
allow_roots = ["/absolute/path/to/project"]
toolset = "core"
```

Load it with `agz-rust-mcp --config /absolute/path/to/server.toml`. CLI flags take
precedence over `AGZ_RUST_MCP_*` environment variables, then explicit TOML, then
defaults. Environment equivalents are `AGZ_RUST_MCP_SERVER__TRANSPORT=http` and
`AGZ_RUST_MCP_SERVER__HTTP_BIND=127.0.0.1:8787`.

### Tool catalog and project roots

The compact `core` catalog contains `workspace`, `test`, `run`, `check`,
`crate_lookup`, and `docs`. Select it with `--toolset core` or
`AGZ_RUST_MCP_SERVER__TOOLSET=core`. The default `full` catalog also exposes the
advanced semantic, change, repair, verification, and profiling tools. Explicit
tool enable/disable settings further restrict the selected catalog; they do not
add advanced tools to `core`. Restart the server after changing its catalog.

The canonical working directory is the default authorized root. For additional
checkouts or worktrees, pass repeated `--allow-root` arguments and use an absolute
`dir` in each tool call. External path dependencies need an explicit
`--allow-dependency-root`. For example:

```text
--allow-root /projects --allow-root /client-worktrees --allow-dependency-root /projects
```

Client-provided MCP roots can narrow configured access but never widen it.
Dependencies inherited from member/source directories resolve against the
selected workspace. Different checkouts retain separate cache keys. Unsafe or
external project target directories fall back to an isolated target in auto
cache mode; project-only mode requires a safe in-workspace target. Missing or
inaccessible inputs return a bounded reason, not a passing verdict.

Use the exact tool names from `tools/list`; any namespace displayed by the
client belongs to that client. See the [configuration reference](tools.md#configuration-reference)
for all settings and their defaults.

## Verification

1. Check the executable:

   ```bash
   agz-rust-mcp --version
   agz-rust-mcp --help
   ```

2. Check the MCP handshake and tool catalog over stdio (newline-delimited
   JSON-RPC):

   ```bash
   {
     printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"verify","version":"1.0.0"}}}'
     printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
     printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
   } | agz-rust-mcp
   ```

   Expect an `initialize` result naming the negotiated protocol and a
   `tools/list` result containing the `workspace`, `test`, `run`, `check`, `profile`, `audit`,
   `crate_lookup`, `docs`, `context`, `api`, `explain`, `verify`, `symbol`,
   `references`, `definition`, `symbols`, `implementations`, `hierarchy`,
   `rename`, `refactor`, `change`, `repair`, and `work` tools (tools disabled by
   configuration are omitted). Stdout is MCP framing only; diagnostics go to
   stderr.

3. From a source checkout, run the repository protocol smoke:

   ```bash
   cargo run -p xtask -- protocol-smoke
   ```

## First Development Task

Use the same sequence for a library, service, command-line application, or
multi-package workspace:

| Goal | Tool and action | Result to use |
| --- | --- | --- |
| Discover the project without compiling | `workspace` → `inspect` | Manifests, packages, targets, features, and available lockfile versions. |
| Find relevant source | `workspace` → `search`, then `read` | Bounded text matches and line ranges; these are lexical results, not semantic references. |
| Locate the first compiler failure | `check` with the selected package/target | Actual Cargo/rustc diagnostics and their source locations. |
| Retrieve dependency evidence | `crate_lookup`, `docs`, or `context` | Evidence tied to the actual dependency version and source. |
| List and isolate a regression test | `test` → `list`, then `run` | Selected test names and fresh test results. Use an exact name when appropriate. |
| Compile tests without running them | `test` → `compile` | Compilation evidence only. |
| Exercise a function through a test/example | `run` with an explicit binary/example target | Exit status, bounded output, timing, and cleanup status. |

Plain Rust functions need a test, example, or binary entry point to execute.
The `run` tool accepts a Cargo target and structured arguments; it does not take
a shell command. Cargo build scripts, tests, and programs execute local project
code and may perform filesystem or network operations. The server's root guards
and process limits are not an operating-system sandbox for that code. Review the
tool's exact selected scope, truncation notices, and timeout/cancellation state;
a result with no matching tests is not a passing regression test.

## Troubleshooting

| Symptom | Check and fix |
| --- | --- |
| `agz-rust-mcp: command not found` | Add the install directory (`$HOME/.local/bin` or `$HOME/.cargo/bin`) to `PATH`, or use an absolute path in the client config. |
| Checksum mismatch | Re-download the archive and its `.sha256` from the release page; do not bypass verification. |
| Installer rejects the OS or architecture | `install.sh` supports Linux x86_64 only; use a prebuilt archive, `cargo install`, or a source build. |
| Client shows no tools | For stdio, verify the binary path and startup timeout. For HTTP, confirm the server is running and use the `/mcp` URL. |
| HTTP address rejected | `--http-bind` must be a loopback address, for example `127.0.0.1:8787` or `[::1]:8787`. |
| Client has no prompt/resource browser | Read guidance through `workspace(action="skills")`; no filesystem skill installation is required. |
| Path or root authorization errors | Start the client in the workspace or pass repeated `--allow-root <path>` flags. |
| Semantic tools return unavailable | Install the pinned Rust Analyzer (`rustup component add rust-analyzer --toolchain 1.91.0`) and review the workspace-code policy in [docs/tools.md](tools.md#rust-analyzer-policy). |
| `cargo install` fails on an old toolchain | Install Rust `1.91.0` or newer with `rustup toolchain install 1.91.0`. |

Related: [documentation index](README.md) - [tool reference](tools.md) -
[architecture](architecture.md) - [security policy](../SECURITY.md).
