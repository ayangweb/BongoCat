#!/usr/bin/env python3
"""Validate the legacy locale inventory before Native Rewrite migration."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LOCALE_DIR = ROOT / "src" / "locales"
EXPECTED_LOCALES = ("en-US", "pt-BR", "vi-VN", "zh-CN", "zh-TW")
PLACEHOLDER = re.compile(r"\{([A-Za-z][A-Za-z0-9_]*)\}")


def fail(path: Path, message: str) -> None:
    relative = path.relative_to(ROOT) if path.is_relative_to(ROOT) else path
    raise ValueError(f"{relative}: {message}")


def load_locale(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(path, f"invalid UTF-8 JSON: {exc}")
    if not isinstance(value, dict):
        fail(path, "top-level value must be an object")
    return value


def flatten(value: object, path: str, source: Path) -> dict[str, str]:
    if isinstance(value, dict):
        if not value:
            fail(source, f"{path or '<root>'} must not be empty")
        flattened: dict[str, str] = {}
        for key, child in value.items():
            if not isinstance(key, str) or not key.strip():
                fail(source, f"{path or '<root>'} contains a blank key")
            child_path = f"{path}.{key}" if path else key
            for leaf, text in flatten(child, child_path, source).items():
                if leaf in flattened:
                    fail(source, f"duplicate flattened key {leaf}")
                flattened[leaf] = text
        return flattened
    if not isinstance(value, str) or not value.strip():
        fail(source, f"{path} must be a non-empty string")
    return {path: value}


def placeholder_names(value: str) -> tuple[str, ...]:
    return tuple(sorted(PLACEHOLDER.findall(value)))


def main() -> int:
    expected_files = {f"{locale}.json" for locale in EXPECTED_LOCALES}
    actual_files = {path.name for path in LOCALE_DIR.glob("*.json")}
    if actual_files != expected_files:
        missing = sorted(expected_files - actual_files)
        extra = sorted(actual_files - expected_files)
        raise ValueError(f"src/locales: locale file mismatch; missing={missing}, extra={extra}")

    flattened = {
        locale: flatten(load_locale(LOCALE_DIR / f"{locale}.json"), "", LOCALE_DIR / f"{locale}.json")
        for locale in EXPECTED_LOCALES
    }
    reference = flattened[EXPECTED_LOCALES[0]]
    for locale in EXPECTED_LOCALES[1:]:
        current = flattened[locale]
        missing = sorted(set(reference) - set(current))
        extra = sorted(set(current) - set(reference))
        if missing or extra:
            raise ValueError(f"{locale}: key mismatch; missing={missing}, extra={extra}")
        for key, reference_text in reference.items():
            current_text = current[key]
            if placeholder_names(reference_text) != placeholder_names(current_text):
                raise ValueError(
                    f"{locale}: placeholder mismatch for {key}; "
                    f"expected={placeholder_names(reference_text)}, "
                    f"actual={placeholder_names(current_text)}"
                )

    print(f"validated {len(EXPECTED_LOCALES)} locale(s), {len(reference)} key(s) each")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
