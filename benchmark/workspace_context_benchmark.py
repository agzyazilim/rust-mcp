#!/usr/bin/env python3
"""Measure real stdio workspace latency and core/full catalogue bytes on Linux.

Uses an already-built executable, a temporary eight-crate fixture, and no Rust
commands. Measurements include Python transport/JSON overhead, not model time.
"""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shlex
import statistics
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from mcp_stdio import McpSession

ACTIONS = {
    "inspect": {"action": "inspect", "limit": 32, "byteBudget": 16384},
    "search": {"action": "search", "query": "BENCH_NEEDLE", "caseSensitive": True,
               "limit": 16, "byteBudget": 16384},
    "read": {"action": "read", "path": "crates/core4/src/module06.rs",
             "line": 42, "lineCount": 24, "byteBudget": 16384},
}


def digest(path):
    with path.open("rb") as source:
        hasher = hashlib.sha256()
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def snapshot(root):
    return {str(path.relative_to(root)): digest(path)
            for path in sorted(root.rglob("*")) if path.is_file()}


def make_fixture(root):
    def write(relative, text):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    write("Cargo.toml", '[workspace]\nmembers=["crates/*"]\nresolver="2"\n'
          '[workspace.package]\nversion="0.1.0"\nedition="2021"\n')
    for crate in range(8):
        prefix = f"crates/core{crate}"
        manifest = (f'[package]\nname="bench-core{crate}"\nversion.workspace=true\n'
                    'edition.workspace=true\n[features]\ndefault=["fast"]\nfast=[]\n')
        if crate:
            manifest += (f'[dependencies]\nprevious={{package="bench-core{crate-1}",'
                         f'path="../core{crate-1}"}}\n')
        write(f"{prefix}/Cargo.toml", manifest)
        modules = "\n".join(f"pub mod module{module:02d};" for module in range(12))
        write(f"{prefix}/src/lib.rs", modules +
              "\n#[cfg(test)] mod tests { #[test] fn smoke() { assert_eq!(2 + 2, 4); } }\n")
        for module in range(12):
            lines = [f"//! Fixture crate {crate}, module {module}: deterministic arithmetic helpers."]
            for function in range(100):
                if function == 50:
                    lines.append("// BENCH_NEEDLE: shared packet checksum context — Türkçe açıklama.")
                lines.append(f"pub fn operation_{function:03d}(value: u64) -> u64 {{ "
                             f"value.wrapping_add({crate*1200 + module*100 + function}) }}")
            write(f"{prefix}/src/module{module:02d}.rs", "\n".join(lines) + "\n")
        write(f"{prefix}/tests/public_api.rs", f"#[test] fn public_value() {{ "
              f"assert_eq!(bench_core{crate}::module00::operation_000(0), {crate*1200}); }}\n")


class CheckedSession(McpSession):
    """Also validate EOF trailers, which the request reader need not consume."""
    def close(self):
        try:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
                raise AssertionError("MCP did not terminate after stdio EOF") from None
            trailer = bytes(self.pending) + self.process.stdout.read(8 * 1024 * 1024 + 1)
            assert len(trailer) <= 8 * 1024 * 1024, "oversized stdout trailer"
            assert not trailer or trailer.endswith(b"\n"), "unframed stdout trailer"
            for line in trailer.splitlines():
                assert json.loads(line).get("jsonrpc") == "2.0", "non-JSON-RPC stdout"
            assert self.process.returncode == 0, self.stderr.read_text()[-2000:]
        finally:
            self.selector.close()
            self.process.stdout.close()
            self.log.close()


def open_session(binary, base, root, profile, label):
    state = base / f"state-{profile}-{label}"
    state.mkdir()
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("AGZ_RUST_MCP_")}
    env.update(PATH=str(base / "empty-path"), CARGO_NET_OFFLINE="true")
    env.pop("CARGO_TARGET_DIR", None)
    sentinel = str(base / "unavailable-rust-tool")
    command = [str(binary), "--toolset", profile, "--allow-root", str(root),
               "--cargo-path", sentinel, "--rust-analyzer-path", sentinel,
               "--gate-cache-dir", str(state / "gate"), "--gate-lease-dir", str(state / "leases"),
               "--docs-cache-dir", str(state / "docs"), "--change-scratch-dir", str(state / "change"),
               "--telemetry-enabled", "false"]
    return CheckedSession(command, cwd=root, env=env, stderr=state / "stderr.log", timeout=30)


def distribution(values):
    ordered = sorted(values)
    return {"samples": len(values), "medianMs": round(statistics.median(values), 3),
            "p95Ms": round(ordered[math.ceil(0.95 * len(values)) - 1], 3),
            "minMs": round(min(values), 3), "maxMs": round(max(values), 3),
            "rawMs": [round(value, 6) for value in values]}


def validate_output(output, action):
    assert not output.get("isError"), output
    structured = output["structuredContent"]
    assert structured == json.loads(output["content"][0]["text"]), "structured/text mismatch"
    data = structured["data"]
    assert data.get("status") == "ok" and data["scan"]["complete"], data
    if action == "inspect":
        assert len(data["manifests"]) == 9, data
    elif action == "search":
        assert data["scan"]["matchingItems"] == 96 and len(data["matches"]) == 16, data
    else:
        assert len(data["file"]["lines"]) == 24, data
    return {"status": structured["status"], "scan": data["scan"],
            "page": data["page"], "truncated": data["truncated"]}


def measure_profile(binary, base, root, profile, samples):
    startup, catalogue = [], None
    for trial in range(5):
        started = time.perf_counter()
        with open_session(binary, base, root, profile, f"startup-{trial}") as client:
            startup.append((time.perf_counter() - started) * 1000)
            listing, wire_bytes = client.request("tools/list", {})
            names = [tool["name"] for tool in listing["tools"]]
            current = {"tools": len(names), "names": names, "responseWireBytes": wire_bytes + 1,
                       "toolsJsonBytes": len(json.dumps(listing["tools"], ensure_ascii=False,
                                                        separators=(",", ":")).encode())}
            if catalogue is not None:
                assert current == catalogue, "unstable catalogue"
            catalogue = current
    observations = {}
    with open_session(binary, base, root, profile, "warm") as client:
        for action, arguments in ACTIONS.items():
            validate_output(client.call("workspace", arguments)[0], action)
            timings, sizes = [], []
            for _ in range(samples):
                started = time.perf_counter()
                output, size = client.call("workspace", arguments)
                timings.append((time.perf_counter() - started) * 1000)
                sizes.append(size + 1)  # McpSession excludes the JSONL newline.
                result = validate_output(output, action)
            observations[action] = {**distribution(timings), "medianWireBytes": statistics.median(sizes),
                                    "request": arguments, "result": result}
    return {"startupInitialize": distribution(startup), "catalogue": catalogue,
            "workspace": observations}


def read_optional(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def measure(binary, samples, build_profile):
    binary_hash = digest(binary)
    cpu_info = read_optional("/proc/cpuinfo") or ""
    host = {"platform": platform.platform(), "logicalCpuCount": os.cpu_count(),
            "cpuQuota": read_optional("/sys/fs/cgroup/cpu.max"),
            "memoryLimit": read_optional("/sys/fs/cgroup/memory.max"),
            "cpuModel": next((line.split(":", 1)[1].strip() for line in cpu_info.splitlines()
                              if line.startswith("model name")), None)}
    report = {"measuredAtUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "host": host,
              "build": {"profile": build_profile, "binarySha256": binary_hash,
                        "binaryBytes": binary.stat().st_size,
                        "version": subprocess.check_output([str(binary), "--version"], text=True, timeout=10).strip()},
              "conditions": {"loadBefore": os.getloadavg(), "profileOrder": ["core", "full"],
                             "concurrentLoad": "not controlled; inspect host load and run conditions",
                             "timing": "Python stdio round trip including transport and JSON parsing",
                             "percentile": "nearest-rank p95", "warmupCallsPerAction": 1,
                             "startup": "fresh process and state setup; OS page cache is not flushed",
                             "cargoAndAnalyzer": "execution-detecting sentinel; no Rust tools in PATH"},
              "profiles": {}}
    with tempfile.TemporaryDirectory(prefix="agz-workspace-benchmark-") as directory:
        base, root = Path(directory), Path(directory) / "fixture"
        root.mkdir()
        make_fixture(root)
        before = snapshot(root)
        files = [path for path in root.rglob("*") if path.is_file()]
        report["fixture"] = {"workspacePackages": 8, "files": len(files),
                             "rustFiles": sum(path.suffix == ".rs" for path in files),
                             "bytes": sum(path.stat().st_size for path in files),
                             "lines": sum(len(path.read_text().splitlines()) for path in files),
                             "sha256": hashlib.sha256(json.dumps(before, sort_keys=True).encode()).hexdigest(),
                             "lockfile": False, "dependencies": "workspace-internal path dependencies only"}
        marker = base / "unexpected-tool-execution.log"
        sentinel = base / "unavailable-rust-tool"
        sentinel.write_text('#!/bin/sh\nprintf "unexpected subprocess\\n" >> '
                            + shlex.quote(str(marker)) + "\nexit 99\n")
        sentinel.chmod(0o755)
        (base / "empty-path").mkdir()
        for profile in ("core", "full"):
            report["profiles"][profile] = measure_profile(binary, base, root, profile, samples)
        assert not marker.exists(), "workspace launched Cargo or rust-analyzer"
        assert snapshot(root) == before, "fixture files changed"
        assert not (root / "Cargo.lock").exists() and not (root / "target").exists()
    assert digest(binary) == binary_hash, "binary changed during measurement; rerun on an immutable copy"
    report["verification"] = {"stdioFramingAndTextParity": "passed", "cargoOrAnalyzerInvocations": 0,
                              "fixtureHashesUnchanged": True, "lockfileCreated": False,
                              "targetDirectoryCreated": False, "binaryUnchanged": True}
    report["conditions"]["loadAfter"] = os.getloadavg()
    core, full = (report["profiles"][profile]["catalogue"]["responseWireBytes"]
                  for profile in ("core", "full"))
    report["catalogueReductionPercent"] = round((1 - core / full) * 100, 2)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="absolute path to an already-built executable")
    parser.add_argument("--out", type=Path, required=True, help="JSON output path")
    parser.add_argument("--samples", type=int, default=20, help="warm samples per action/profile (5..200)")
    parser.add_argument("--build-profile", choices=("debug", "release", "unknown"), default="unknown")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("this benchmark currently supports Linux only (POSIX sentinel and pipe selector)")
    if not args.binary.is_absolute() or not args.binary.is_file() or not os.access(args.binary, os.X_OK):
        parser.error("--binary must be an absolute path to an executable file")
    if not 5 <= args.samples <= 200:
        parser.error("--samples must be between 5 and 200")
    report = measure(args.binary, args.samples, args.build_profile)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.out), "catalogueReductionPercent": report["catalogueReductionPercent"],
                      "profiles": {profile: {"tools": value["catalogue"]["tools"],
                                             "toolsListBytes": value["catalogue"]["responseWireBytes"],
                                             "medianMs": {action: row["medianMs"] for action, row in value["workspace"].items()}}
                                   for profile, value in report["profiles"].items()}}, ensure_ascii=False))


if __name__ == "__main__":
    main()
