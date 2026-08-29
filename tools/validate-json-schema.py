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
CONFIG_DIR = ROOT / "shared" / "config" / "fixtures"
MODEL_FIXTURE_DIR = ROOT / "shared" / "fixtures" / "model-fixtures"


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


def validate_file(path: Path, validator: Draft202012Validator) -> None:
    try:
        validator.validate(load(path))
    except ValidationError as exc:
        location = ".".join(str(item) for item in exc.absolute_path)
        suffix = f" at {location}" if location else ""
        raise RuntimeError(f"{path.relative_to(ROOT)}{suffix}: {exc.message}") from exc
    print(f"ok json-schema {path.relative_to(ROOT)}")


def validate_config_fixtures(validator: Draft202012Validator) -> int:
    manifest_path = CONFIG_DIR / "manifest.json"
    manifest = load(manifest_path)
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 1:
        raise RuntimeError(f"{manifest_path.relative_to(ROOT)}: schemaVersion must be 1")
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        raise RuntimeError(f"{manifest_path.relative_to(ROOT)}: cases must be a non-empty array")

    listed_files: set[str] = set()
    listed_ids: set[str] = set()
    for case in cases:
        if not isinstance(case, dict):
            raise RuntimeError(f"{manifest_path.relative_to(ROOT)}: each case must be an object")
        case_id = case.get("id")
        file_name = case.get("file")
        expected = case.get("expected")
        if (
            not isinstance(case_id, str)
            or not case_id
            or case_id in listed_ids
            or not isinstance(file_name, str)
            or Path(file_name).name != file_name
            or file_name in listed_files
            or expected not in {"accept", "reject"}
        ):
            raise RuntimeError(f"{manifest_path.relative_to(ROOT)}: invalid or duplicate case")
        path = CONFIG_DIR / file_name
        if not path.is_file():
            raise RuntimeError(f"{manifest_path.relative_to(ROOT)}: missing fixture {file_name}")
        listed_ids.add(case_id)
        listed_files.add(file_name)
        errors = list(validator.iter_errors(load(path)))
        accepted = not errors
        if accepted != (expected == "accept"):
            detail = "fixture unexpectedly accepted" if accepted else errors[0].message
            raise RuntimeError(f"{path.relative_to(ROOT)}: expected {expected}, got {detail}")
        print(f"ok json-schema config {case_id} ({expected})")

    actual_files = {
        path.name for path in CONFIG_DIR.glob("*.json") if path.name != "manifest.json"
    }
    if actual_files != listed_files:
        raise RuntimeError(
            f"{manifest_path.relative_to(ROOT)}: fixture list mismatch; "
            f"unlisted={sorted(actual_files - listed_files)}, "
            f"missing={sorted(listed_files - actual_files)}"
        )
    return len(cases)


def main() -> int:
    input_count = validate_directory(
        INPUT_DIR, validate_schema(INPUT_DIR / "schema.json")
    )
    expected_count = validate_directory(
        EXPECTED_DIR, validate_schema(EXPECTED_DIR / "schema.json")
    )
    config_count = validate_config_fixtures(
        validate_schema(ROOT / "shared" / "config" / "config.schema.json")
    )
    validate_file(
        MODEL_FIXTURE_DIR / "legacy-core-baseline.json",
        validate_schema(MODEL_FIXTURE_DIR / "legacy-core-baseline.schema.json"),
    )
    print(
        f"validated {input_count} input, {expected_count} expected, and "
        f"{config_count} config fixture(s), plus the legacy Core baseline "
        "with Draft 2020-12"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
