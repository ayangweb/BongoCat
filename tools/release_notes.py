#!/usr/bin/env python3
"""Validate the plugin changelog and render one plugin release batch."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


VERSION = re.compile(
    r"^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
LANGUAGE_SEPARATOR = "---"


def fail(path: Path, message: str) -> ValueError:
    return ValueError(f"{path}: {message}")


def plugins(root: Path) -> dict[str, dict[str, str]]:
    result = {}
    for directory in sorted((root / "plugins").iterdir()):
        manifest = directory / "plugin.json"
        if not manifest.is_file():
            continue
        try:
            import json

            metadata = json.loads(manifest.read_text(encoding="utf-8"))["metadata"]
            plugin_id = metadata["id"]
            version = metadata["version"]
            english = metadata["name"]
            chinese = metadata["localizations"]["zh-CN"]["name"]
        except (OSError, KeyError, TypeError, ValueError) as error:
            raise fail(manifest, f"invalid plugin metadata: {error}") from error
        if not all(isinstance(value, str) and value.strip() for value in [plugin_id, version, english, chinese]):
            raise fail(manifest, "plugin identity, version, and names must be non-empty")
        if not VERSION.fullmatch(version):
            raise fail(manifest, f"invalid plugin version {version!r}")
        if plugin_id in result:
            raise fail(manifest, f"duplicate plugin id {plugin_id!r}")
        result[plugin_id] = {
            "version": version,
            "en": english,
            "zh-CN": chinese,
        }
    if not result:
        raise fail(root / "plugins", "no plugin manifests found")
    return result


def parse(path: Path) -> dict[str, dict[str, object]]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise fail(path, f"cannot read changelog: {error}") from error
    if not lines or not lines[0].startswith("# "):
        raise fail(path, "changelog must start with a title")
    current = None
    versions: dict[str, dict[str, object]] = {}
    section = None
    for number, line in enumerate(lines[1:], 2):
        if not line.strip():
            if current is not None:
                versions[current]["lines"].append(line)
            continue
        if line.startswith("## "):
            heading = line[3:].strip()
            version = "Unreleased" if heading == "Unreleased" else heading.split(" - ", 1)[0]
            if version != "Unreleased" and not VERSION.fullmatch(version):
                raise fail(path, f"line {number}: invalid version heading {heading!r}")
            if version in versions:
                raise fail(path, f"duplicate changelog version {version}")
            if version != "Unreleased" and not DATE.fullmatch(heading.split(" - ", 1)[-1]):
                raise fail(path, f"line {number}: version heading needs a YYYY-MM-DD date")
            current = version
            section = None
            versions[current] = {"lines": [], "plugins": {}}
        elif line.startswith("### "):
            if current is None:
                raise fail(path, f"line {number}: plugin heading outside a version")
            section = line[4:].strip()
            plugin_entries = versions[current]["plugins"]
            if not section or section in plugin_entries:
                raise fail(path, f"line {number}: duplicate or empty plugin heading")
            plugin_entries[section] = {"categories": [], "changes": 0}
            versions[current]["lines"].append(line)
        elif line.startswith("#### "):
            if section is None:
                raise fail(path, f"line {number}: change category outside a plugin")
            category = line[5:].strip()
            if not category:
                raise fail(path, f"line {number}: empty change category")
            versions[current]["plugins"][section]["categories"].append(category)
            versions[current]["lines"].append(line)
        elif line.startswith("- "):
            if section is None:
                raise fail(path, f"line {number}: change outside a plugin")
            versions[current]["plugins"][section]["changes"] += 1
            versions[current]["lines"].append(line)
        else:
            raise fail(path, f"line {number}: unsupported changelog syntax")
    for version, data in versions.items():
        data["text"] = "\n".join(data.pop("lines")).strip()
        for name, plugin in data["plugins"].items():
            if not plugin["categories"] or plugin["changes"] == 0:
                raise fail(path, f"{version} has no change entry for {name}")
    return versions


def validate(root: Path) -> dict[str, dict[str, str]]:
    names = plugins(root)
    english = parse(root / "plugins" / "CHANGELOG.md")
    chinese = parse(root / "plugins" / "CHANGELOG.zh-CN.md")
    if english.keys() != chinese.keys():
        raise fail(root, "bilingual changelog versions differ")
    english_names = {data["en"]: plugin_id for plugin_id, data in names.items()}
    chinese_names = {data["zh-CN"]: plugin_id for plugin_id, data in names.items()}
    if len(english_names) != len(names) or len(chinese_names) != len(names):
        raise fail(root / "plugins", "plugin display names must be unique")
    changed = set()
    for version in english:
        try:
            english_order = [
                english_names.get(name, name) for name in english[version]["plugins"]
            ]
            chinese_order = [
                chinese_names.get(name, name) for name in chinese[version]["plugins"]
            ]
        except (KeyError, TypeError, AttributeError) as error:
            raise fail(root, f"invalid plugin heading in {version}") from error
        if english_order != chinese_order:
            raise fail(root, f"bilingual plugins differ in {version}")
        if any(plugin_id not in names for plugin_id in english_order):
            raise fail(root, f"unknown plugin heading in {version}")
        changed.update(english_order)
    for plugin_id, data in names.items():
        if plugin_id not in changed:
            raise fail(
                root,
                f"{plugin_id} {data['version']} has no changelog entry",
            )
    return names


def render(root: Path, tag: str) -> str:
    validate(root)
    version = tag[1:] if tag.startswith("v") else tag
    if not VERSION.fullmatch(version):
        raise ValueError(f"invalid release tag {tag!r}")
    english = parse(root / "plugins" / "CHANGELOG.md")
    chinese = parse(root / "plugins" / "CHANGELOG.zh-CN.md")
    if version not in english or version not in chinese:
        raise fail(root, f"changelogs have no release notes for {version}")
    return (
        f"## Changelog\n\n{english[version]['text']}\n\n"
        f"{LANGUAGE_SEPARATOR}\n\n"
        f"## 更新日志\n\n{chinese[version]['text']}\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--tag", default="v0.0.0")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        notes = render(args.root.resolve(), args.tag)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(notes, encoding="utf-8")
        else:
            print(notes, end="")
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
