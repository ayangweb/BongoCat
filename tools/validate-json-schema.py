#!/usr/bin/env python3
"""Validate shared fixture schemas and every fixture against Draft 2020-12."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError, ValidationError


ROOT = Path(__file__).resolve().parents[1]
INPUT_DIR = ROOT / "shared" / "fixtures" / "input-sequences"
EXPECTED_DIR = ROOT / "shared" / "fixtures" / "expected-state"


def load(path: Path) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"{path.relative_to(ROOT)}: invalid JSON: {exc}") from exc


def validate_schema(schema_path: Path) -> Draft202012Validator:
    schema = load(schema_path)
    try:
        Draft202012Validator.check_schema(schema)
    except SchemaError as exc:
        raise RuntimeError(f"{schema_path.relative_to(ROOT)}: invalid Draft 2020-12 schema: {exc.message}") from exc
    return Draft202012Validator(schema)


def validate_directory(directory: Path, validator: Draft202012Validator) -> int:
    count = 0
    for path in sorted(directory.glob("*.json")):
        if path.name == "schema.json":
            continue
        try:
            validator.validate(load(path))
        except ValidationError as exc:
            location = ".".join(str(item) for item in exc.absolute_path)
            suffix = f" at {location}" if location else ""
            raise RuntimeError(f"{path.relative_to(ROOT)}{suffix}: {exc.message}") from exc
        print(f"ok json-schema {path.relative_to(ROOT)}")
        count += 1
    return count


def main() -> int:
    input_count = validate_directory(
        INPUT_DIR, validate_schema(INPUT_DIR / "schema.json")
    )
    expected_count = validate_directory(
        EXPECTED_DIR, validate_schema(EXPECTED_DIR / "schema.json")
    )
    print(f"validated {input_count} input and {expected_count} expected fixture(s) with Draft 2020-12")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
