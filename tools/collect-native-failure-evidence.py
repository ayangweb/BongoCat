#!/usr/bin/env python3
"""Collect bounded, redacted evidence from a failed Native CI job."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path


TEXT_SUFFIXES = {".log", ".txt", ".json"}
IMAGE_SUFFIXES = {".png", ".jpg", ".jpeg"}
IMAGE_NAME = re.compile(r"(?:screenshot|renderer|validation|overlay|gpui)", re.I)
SAFE_NAME = re.compile(r"[^A-Za-z0-9._-]+")
SAFE_LINE = re.compile(
    r"(?i)^(?:\s*(?:bongocat|frame|renderer|overlay|gpui|runtime|error|status|test|tests|clean_shutdown|failures|recoveries|scale_factor|revision|resize|draw|present|shutdown|stopped|started|warning|failed|passed|key_sequence|clipboard|scan_code|pressed_keys?)\b)"
)
ABSOLUTE_PATH = re.compile(r"(?:[A-Za-z]:[\\/]|/Users/|/home/|/Users/)[^\s\"']+")
SENSITIVE_FIELD = re.compile(
    r"(?im)(\b(?:key(?:[_ -]?sequence)?|scan[_ -]?code|clipboard|input[_ -]?sequence|pressed[_ -]?keys?)\b\s*[:=]\s*)[^\r\n,}]+"
)

MAX_FILE_BYTES = 256 * 1024
MAX_TOTAL_BYTES = 2 * 1024 * 1024
MAX_FILES = 100


def safe_name(name: str) -> str:
    cleaned = SAFE_NAME.sub("_", name).strip("._")
    return cleaned[:96] or "evidence"


def redact(value: str) -> str:
    redacted_lines = []
    for line in value.splitlines(keepends=True):
        if not SAFE_LINE.search(line):
            newline = "\n" if line.endswith("\n") else ""
            redacted_lines.append("<redacted-line>" + newline)
            continue
        line = ABSOLUTE_PATH.sub("<path>", line)
        redacted_lines.append(SENSITIVE_FIELD.sub(r"\1<redacted>", line))
    return "".join(redacted_lines)


def is_candidate(path: Path) -> bool:
    if not path.is_file() or path.is_symlink():
        return False
    if path.suffix.lower() in {".log", ".txt"}:
        return True
    if path.suffix.lower() == ".json":
        return bool(IMAGE_NAME.search(path.name))
    return path.suffix.lower() in IMAGE_SUFFIXES and bool(IMAGE_NAME.search(path.name))


def collect(input_root: Path, output_root: Path, platform: str) -> dict[str, object]:
    input_root = input_root.resolve()
    output_root = output_root.resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    entries: list[dict[str, object]] = []
    total_bytes = 0

    for source in sorted(input_root.rglob("*")):
        if source == output_root or output_root in source.parents:
            continue
        if len(entries) >= MAX_FILES or total_bytes >= MAX_TOTAL_BYTES:
            break
        if not is_candidate(source):
            continue
        try:
            relative = source.relative_to(input_root)
            size = source.stat().st_size
        except (OSError, ValueError):
            continue
        if size > MAX_FILE_BYTES:
            continue
        destination_name = safe_name(str(relative).replace("/", "_"))
        destination = output_root / destination_name
        if source.suffix.lower() in TEXT_SUFFIXES:
            try:
                text = redact(source.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
            encoded = text.encode("utf-8")
            if len(encoded) > MAX_FILE_BYTES or total_bytes + len(encoded) > MAX_TOTAL_BYTES:
                continue
            destination.write_bytes(encoded)
            recorded_size = len(encoded)
        else:
            if total_bytes + size > MAX_TOTAL_BYTES:
                continue
            try:
                shutil.copyfile(source, destination)
            except OSError:
                continue
            recorded_size = size
        total_bytes += recorded_size
        entries.append(
            {
                "name": destination.name,
                "kind": "image" if source.suffix.lower() in IMAGE_SUFFIXES else "text",
                "bytes": recorded_size,
                "sha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
            }
        )

    manifest = {
        "schema_version": 1,
        "platform": platform,
        "file_count": len(entries),
        "total_bytes": total_bytes,
        "truncated": len(entries) >= MAX_FILES or total_bytes >= MAX_TOTAL_BYTES,
        "files": entries,
    }
    (output_root / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    args = parser.parse_args()
    collect(args.input, args.output, args.platform)


if __name__ == "__main__":
    main()
