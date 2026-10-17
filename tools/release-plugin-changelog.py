#!/usr/bin/env python3
"""Validate and publish the shared bilingual plugin release notes.

Unlike the product changelog, this file intentionally describes only plugin changes.
The latest entry is inserted into the existing product GitHub Release by the
independent plugin workflow, so publishing a plugin does not require rebuilding the
application.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


VERSION = re.compile(
    r"^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
# Keep plugin release notes on the same vocabulary as the product changelog. A plugin
# can use any of these categories without adding a second translation convention.
SECTION_VOCABULARY = (
    ("⚠️", "Upgrade Notice", "升级说明", "upgrade_notice"),
    ("✨", "Features", "新功能", "features"),
    ("🐛", "Bug Fixes", "问题修复", "bug_fixes"),
    ("⚡️", "Performance", "性能优化", "performance"),
    ("🔐", "Security", "安全", "security"),
    ("⬆️", "Dependencies", "依赖更新", "dependencies"),
    ("🗑️", "Removals", "移除", "removals"),
    ("🎨", "Interface", "界面", "interface"),
    ("🌍", "Localization", "本地化", "localization"),
    ("⚙️", "Configuration", "配置", "configuration"),
    ("🧪", "Testing", "测试", "testing"),
    ("♻️", "Refactoring", "重构", "refactoring"),
    ("🔧", "Maintenance", "维护", "maintenance"),
    ("📝", "Documentation", "文档", "documentation"),
    ("⏪", "Reverts", "回退", "reverts"),
    ("💻", "Support Changes", "支持范围变化", "support_changes"),
)
VARIATION_SELECTOR_16 = "️"
SECTIONS = {
    "en": {
        f"{emoji} {english}": key
        for emoji, english, _chinese, key in SECTION_VOCABULARY
    },
    "zh-CN": {
        f"{emoji} {chinese}": key
        for emoji, _english, chinese, key in SECTION_VOCABULARY
    },
}
START_MARKER = "<!-- BONGOCAT_PLUGIN_CHANGELOG:START -->"
END_MARKER = "<!-- BONGOCAT_PLUGIN_CHANGELOG:END -->"


def fail(path: Path, message: str) -> ValueError:
    return ValueError(f"{path}: {message}")


def plugin_names(root: Path) -> dict[str, dict[str, str]]:
    result = {}
    for directory in sorted((root / "plugins").iterdir()):
        manifest_path = directory / "plugin.json"
        if not manifest_path.is_file():
            continue
        try:
            metadata = json.loads(manifest_path.read_text(encoding="utf-8"))["metadata"]
            plugin_id = metadata["id"]
            english = metadata["name"]
            chinese = metadata["localizations"]["zh-CN"]["name"]
            version = metadata["version"]
        except (OSError, KeyError, TypeError, json.JSONDecodeError) as exc:
            raise fail(manifest_path, f"invalid plugin metadata: {exc}") from exc
        if not all(isinstance(value, str) and value.strip() for value in [plugin_id, english, chinese, version]):
            raise fail(manifest_path, "id, names, and version must be non-empty strings")
        if not VERSION.fullmatch(version):
            raise fail(manifest_path, f"invalid plugin version {version!r}")
        result[plugin_id] = {"en": english, "zh-CN": chinese, "version": version}
    if not result:
        raise fail(root / "plugins", "no plugin manifests found")
    return result


def parse(path: Path, language: str) -> list[dict]:
    title = "# Plugin Changelog" if language == "en" else "# 插件更新日志"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise fail(path, f"cannot read changelog: {exc}") from exc
    if not lines or lines[0].strip() != title:
        raise fail(path, f"first line must be {title!r}")

    entries = []
    entry = None
    plugin = None
    section = None
    section_has_item = False

    def finish_section() -> None:
        nonlocal section_has_item
        if section is not None and not section_has_item:
            raise fail(path, f"section {section!r} is empty")
        section_has_item = False

    def finish_plugin() -> None:
        nonlocal plugin, section
        if plugin is None:
            return
        finish_section()
        if not plugin["sections"]:
            raise fail(path, f"plugin {plugin['name']!r} has no change sections")
        entry["plugins"].append(plugin)
        plugin = None
        section = None

    def finish_entry() -> None:
        nonlocal entry
        if entry is None:
            return
        finish_plugin()
        if not entry["plugins"]:
            raise fail(path, f"version {entry['version']} has no plugin changes")
        entries.append(entry)
        entry = None

    for line_number, line in enumerate(lines[1:], start=2):
        if not line.strip():
            continue
        if line.startswith("## "):
            finish_entry()
            heading = line[3:].strip()
            version, separator, date = heading.partition(" - ")
            if not VERSION.fullmatch(version) or (separator and not DATE.fullmatch(date)):
                raise fail(path, f"line {line_number}: invalid release heading {heading!r}")
            entry = {"version": version, "date": date if separator else "", "plugins": []}
            continue
        if line.startswith("### "):
            if entry is None:
                raise fail(path, f"line {line_number}: plugin is outside a release")
            finish_plugin()
            plugin = {"name": line[4:].strip(), "sections": []}
            if not plugin["name"]:
                raise fail(path, f"line {line_number}: plugin name is empty")
            continue
        if line.startswith("#### "):
            if plugin is None:
                raise fail(path, f"line {line_number}: section is outside a plugin")
            finish_section()
            name = line[5:].strip()
            normalized_name = name.replace(VARIATION_SELECTOR_16, "")
            section_names = {
                key.replace(VARIATION_SELECTOR_16, ""): value
                for key, value in SECTIONS[language].items()
            }
            key = section_names.get(normalized_name)
            if key is None:
                raise fail(path, f"line {line_number}: unsupported section {name!r}")
            if key in plugin["sections"]:
                raise fail(path, f"line {line_number}: duplicate section {name!r}")
            plugin["sections"].append(key)
            section = name
            continue
        if line.startswith("- "):
            if section is None:
                raise fail(path, f"line {line_number}: bullet is outside a section")
            if not line[2:].strip():
                raise fail(path, f"line {line_number}: empty bullet")
            section_has_item = True
            continue
        raise fail(path, f"line {line_number}: only blank lines, headings, and bullets are allowed")

    finish_entry()
    if not entries:
        raise fail(path, "no release entries found")
    versions = [entry["version"] for entry in entries]
    if len(versions) != len(set(versions)):
        raise fail(path, "release versions must be unique")
    return entries


def validate(root: Path) -> tuple[list[dict], list[dict], dict[str, dict[str, str]]]:
    names = plugin_names(root)
    english = parse(root / "plugins/CHANGELOG.md", "en")
    chinese = parse(root / "plugins/CHANGELOG.zh-CN.md", "zh-CN")
    if [(entry["version"], entry["date"]) for entry in english] != [
        (entry["version"], entry["date"]) for entry in chinese
    ]:
        raise fail(root / "plugins", "English and Simplified Chinese release headings must match")
    for english_entry, chinese_entry in zip(english, chinese):
        if len(english_entry["plugins"]) != len(chinese_entry["plugins"]):
            raise fail(root / "plugins", f"release {english_entry['version']} lists different plugins")
        if len({plugin["name"] for plugin in english_entry["plugins"]}) != len(english_entry["plugins"]):
            raise fail(root / "plugins/CHANGELOG.md", f"release {english_entry['version']} lists a plugin more than once")
        if len({plugin["name"] for plugin in chinese_entry["plugins"]}) != len(chinese_entry["plugins"]):
            raise fail(
                root / "plugins/CHANGELOG.zh-CN.md",
                f"release {english_entry['version']} lists a plugin more than once",
            )
        if [plugin["sections"] for plugin in english_entry["plugins"]] != [
            plugin["sections"] for plugin in chinese_entry["plugins"]
        ]:
            raise fail(root / "plugins", f"release {english_entry['version']} lists different sections")

    latest = english[0]
    expected_latest = {data["en"] for data in names.values() if data["version"] == latest["version"]}
    listed_latest = {plugin["name"] for plugin in latest["plugins"]}
    if listed_latest != expected_latest:
        raise fail(
            root / "plugins/CHANGELOG.md",
            f"latest release {latest['version']} must list current plugins {sorted(expected_latest)}",
        )
    expected_chinese = {data["zh-CN"] for data in names.values() if data["version"] == latest["version"]}
    listed_chinese = {plugin["name"] for plugin in chinese[0]["plugins"]}
    if listed_chinese != expected_chinese:
        raise fail(
            root / "plugins/CHANGELOG.zh-CN.md",
            f"latest release {latest['version']} must list current plugins {sorted(expected_chinese)}",
        )
    chinese_by_english = {data["en"]: data["zh-CN"] for data in names.values()}
    for english_plugin, chinese_plugin in zip(english[0]["plugins"], chinese[0]["plugins"]):
        if chinese_by_english.get(english_plugin["name"]) != chinese_plugin["name"]:
            raise fail(root / "plugins", f"latest release {latest['version']} lists different plugin names")
    return english, chinese, names


def render(root: Path) -> str:
    validate(root)

    # Re-render from the source lines to retain the authored copy while selecting the
    # latest entry; the parser above remains the authority for the structure.
    def latest_block(path: Path, language: str) -> list[str]:
        lines = path.read_text(encoding="utf-8").splitlines()
        start = next(index for index, line in enumerate(lines) if line.startswith("## "))
        end = next((index for index in range(start + 1, len(lines)) if lines[index].startswith("## ")), len(lines))
        return [f"#{line}" if line.startswith("#") else line for line in lines[start:end]]

    return "\n".join(
        [
            START_MARKER,
            "## Plugin Changelog",
            "",
            *latest_block(root / "plugins/CHANGELOG.md", "en"),
            "",
            "---",
            "",
            "## 插件更新日志",
            "",
            *latest_block(root / "plugins/CHANGELOG.zh-CN.md", "zh-CN"),
            END_MARKER,
            "",
        ]
    )


def merge(existing: str, notes: str) -> str:
    start = existing.find(START_MARKER)
    end = existing.find(END_MARKER)
    if (start >= 0) != (end >= 0) or end < start:
        raise ValueError("existing release notes contain an incomplete plugin changelog marker")
    if start >= 0:
        end += len(END_MARKER)
        merged = existing[:start].rstrip() + "\n\n" + notes.strip() + existing[end:]
    else:
        merged = existing.rstrip() + "\n\n---\n\n" + notes.strip() + "\n"
    return merged


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--output", type=Path, help="write the latest plugin release notes")
    parser.add_argument("--merge-into", type=Path, help="merge into an existing release body")
    args = parser.parse_args()
    if args.merge_into and not args.output:
        parser.error("--merge-into requires --output")
    try:
        notes = render(args.root.resolve())
        if args.merge_into:
            notes = merge(args.merge_into.read_text(encoding="utf-8"), notes)
        if args.output:
            args.output.write_text(notes, encoding="utf-8")
        else:
            print(notes, end="")
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
