#!/usr/bin/env python3
"""Write reproducible, path-free metadata for a Native workspace build."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def run(*command: str) -> str:
    return subprocess.check_output(command, text=True).strip()


def rustc_metadata() -> dict[str, str]:
    values: dict[str, str] = {}
    lines = run("rustc", "-vV").splitlines()
    if lines and lines[0].startswith("rustc "):
        values["rustc"] = lines[0].removeprefix("rustc ")
    for line in lines[1:]:
        key, separator, value = line.partition(": ")
        if separator and key in {"rustc", "binary", "commit-hash", "commit-date", "host"}:
            values[key.replace("-", "_")] = value
    required = {"rustc", "commit_hash", "host"}
    missing = required - values.keys()
    if missing:
        raise RuntimeError(f"rustc -vV omitted required fields: {sorted(missing)}")
    return values


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=Path("native"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", help="target triple; defaults to rustc host")
    parser.add_argument("--profile", default="release")
    parser.add_argument("--features", default="default")
    parser.add_argument("--environment", required=True, choices=("development", "production"))
    args = parser.parse_args()

    workspace = args.workspace.resolve()
    lockfile = workspace / "Cargo.lock"
    if not lockfile.is_file():
        raise SystemExit(f"missing Cargo.lock: {lockfile}")

    metadata = {
        "schema_version": 1,
        "source_commit": run("git", "rev-parse", "HEAD"),
        "cargo_lock_sha256": sha256(lockfile),
        "rust_toolchain": rustc_metadata(),
        "target": args.target or rustc_metadata()["host"],
        "profile": args.profile,
        "features": args.features,
        "build_environment": args.environment,
    }

    output = args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp")
    temporary.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output)


if __name__ == "__main__":
    main()
