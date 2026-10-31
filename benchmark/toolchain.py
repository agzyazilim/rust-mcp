"""Select a benchmark toolchain without duplicating the repository's Rust pin."""
from pathlib import Path
import tomllib


def selected_toolchain(override=None):
    if override is None:
        path = Path(__file__).resolve().parents[1] / "rust-toolchain.toml"
        with path.open("rb") as source:
            override = tomllib.load(source).get("toolchain", {}).get("channel")
    if not isinstance(override, str) or not override.strip():
        raise ValueError("a nonempty --toolchain or rust-toolchain.toml channel is required")
    return override
