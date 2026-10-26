#!/usr/bin/env python3
"""Build the in-repository plugins, then execute a development command."""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEV_ROOT = ROOT / "target" / "plugin-dev"
CATALOG_DIRECTORY = DEV_ROOT / "plugins"
HOST_TRIPLES = {
    ("darwin", "arm64"): "aarch64-apple-darwin",
    ("darwin", "x86_64"): "x86_64-apple-darwin",
    ("win32", "x86_64"): "x86_64-pc-windows-msvc",
}
PLATFORM_KEYS = {
    "aarch64-apple-darwin": "macos-aarch64",
    "x86_64-apple-darwin": "macos-x86_64",
    "x86_64-pc-windows-msvc": "windows-x86_64",
}


class DevPluginError(RuntimeError):
    pass


def host_triple():
    platform = sys.platform
    machine = os.uname().machine if hasattr(os, "uname") else os.environ.get("PROCESSOR_ARCHITECTURE")
    return host_triple_for(platform, machine)


def host_triple_for(platform, machine):
    normalized = (machine or "").lower()
    if normalized == "amd64":
        normalized = "x86_64"
    triple = HOST_TRIPLES.get((platform, normalized))
    if triple is None:
        raise DevPluginError(
            f"unsupported host {platform}-{normalized or 'unknown'}; "
            "BongoCat plugin development supports Windows x64 and macOS arm64/x64"
        )
    return triple


def prepare_catalog():
    triple = host_triple()
    CATALOG_DIRECTORY.mkdir(parents=True, exist_ok=True)
    packaging = ["cargo", "run", "--locked", "-q", "-p", "bongocat-plugin-packaging", "--"]
    command = packaging + [
        "catalog",
        "dev",
        "--root",
        ".",
        "--target",
        triple,
        "--output",
        str(CATALOG_DIRECTORY),
    ]
    try:
        subprocess.run(command, cwd=ROOT, check=True)
    except FileNotFoundError as error:
        raise DevPluginError("Cargo is required to build the official plugins") from error
    except subprocess.CalledProcessError as error:
        raise DevPluginError(
            f"cannot build official plugins for {triple}"
        ) from error

    catalog = CATALOG_DIRECTORY / "catalog.json"
    if not catalog.is_file():
        raise DevPluginError(f"plugin build did not produce {catalog}")
    prune_catalog_output(CATALOG_DIRECTORY, catalog)
    return triple, CATALOG_DIRECTORY


def prune_catalog_output(output, catalog_path):
    """Remove only artifacts no longer named by the newly written catalog."""
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    referenced = {"catalog.json"}
    for entry in catalog["plugins"]:
        referenced.update(entry["downloads"].values())
        icon = entry.get("icon_url")
        if icon:
            referenced.add(Path(icon).name)
    for stale in output.iterdir():
        if (
            stale.is_file()
            and stale.suffix in {".zip", ".png"}
            and stale.name not in referenced
        ):
            stale.unlink()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    command = arguments.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("a command to run after preparing plugins is required")

    try:
        triple, catalog_directory = prepare_catalog()
        environment = os.environ.copy()
        environment["BONGOCAT_PLUGIN_DEV_DIRECTORY"] = str(catalog_directory)
        environment.setdefault("BONGOCAT_PLUGIN_TEST_ROOT", str(ROOT))
        environment.setdefault("BONGOCAT_PLUGIN_SMOKE_REQUIRE_CARDS", "1")
        print(
            f"Using in-repository plugins ({triple})",
            file=sys.stderr,
        )
        started = time.monotonic()
        result = subprocess.run(command, cwd=ROOT, env=environment)
        if result.returncode == 0 and "--settings-window-smoke" in command:
            if time.monotonic() - started < 1:
                print(
                    "plugin Development smoke exited before the settings window could render",
                    file=sys.stderr,
                )
                return 1
    except DevPluginError as error:
        print(f"plugin development setup failed: {error}", file=sys.stderr)
        return 1
    except FileNotFoundError as error:
        print(f"cannot execute plugin development command: {error}", file=sys.stderr)
        return 1
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
