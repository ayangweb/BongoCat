#!/usr/bin/env python3
"""Validate Phase 0 fixture shape and cross-file invariants."""

from __future__ import annotations

import json
import shutil
import sys
import tempfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
INPUT_DIR = ROOT / "shared" / "fixtures" / "input-sequences"
EXPECTED_DIR = ROOT / "shared" / "fixtures" / "expected-state"
LEGACY_CONFIG_DIR = ROOT / "shared" / "config" / "legacy-pinia"
LEGACY_STORE_NAMES = ("app", "general", "cat", "model", "shortcut")
MODEL_FIXTURE_DIR = ROOT / "shared" / "fixtures" / "model-fixtures"
MODEL_CASE_DIR = MODEL_FIXTURE_DIR / "cases"
MODEL_DIAGNOSTICS = {
    "model_entry_ambiguous",
    "model_entry_missing",
    "model_json_invalid",
    "model_moc_missing",
    "model_reference_escapes_root",
    "model_texture_dimension_exceeded",
    "model_texture_invalid_png",
    "model_texture_missing",
}


def fail(path: Path, message: str) -> None:
    raise ValueError(f"{path.relative_to(ROOT)}: {message}")


def load(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(path, f"invalid JSON: {exc}")
    if not isinstance(value, dict):
        fail(path, "top-level value must be an object")
    return value


def validate_input(path: Path, value: dict) -> tuple[str, list[dict]]:
    required = {"schemaVersion", "id", "description", "context", "events"}
    missing = required - value.keys()
    if missing:
        fail(path, f"missing fields: {sorted(missing)}")
    if value["schemaVersion"] != 1 or not isinstance(value["id"], str):
        fail(path, "schemaVersion must be 1 and id must be a string")
    context = value["context"]
    if not isinstance(context, dict) or context.get("modelMode") not in {"standard", "keyboard", "gamepad"}:
        fail(path, "context.modelMode is invalid")
    if not isinstance(context.get("keySides"), dict):
        fail(path, "context.keySides must be an object")
    events = value["events"]
    if not isinstance(events, list):
        fail(path, "events must be an array")
    last_at = -1
    for index, event in enumerate(events):
        if not isinstance(event, dict) or not isinstance(event.get("atMs"), int) or event["atMs"] < 0:
            fail(path, f"events[{index}].atMs must be a non-negative integer")
        if event["atMs"] < last_at:
            fail(path, f"events must be ordered by atMs (index {index})")
        last_at = event["atMs"]
        event_type = event.get("type")
        if event_type in {"key_down", "key_up"} and not isinstance(event.get("key"), str):
            fail(path, f"events[{index}].key must be a string")
        if event_type == "key_down" and not isinstance(event.get("repeat"), bool):
            fail(path, f"events[{index}].repeat must be boolean")
        if event_type == "reset" and event.get("reason") not in {
            "session_lock", "sleep", "device_removed", "service_restart",
            "queue_overflow", "permission_changed", "test",
        }:
            fail(path, f"events[{index}].reason is invalid")
    return value["id"], events


def validate_expected(path: Path, value: dict, input_id: str, input_events: list[dict]) -> None:
    if value.get("schemaVersion") != 1 or value.get("sequenceId") != input_id:
        fail(path, "schemaVersion or sequenceId does not match input fixture")
    checkpoints = value.get("checkpoints")
    if not isinstance(checkpoints, list):
        fail(path, "checkpoints must be an array")
    event_times = {event["atMs"] for event in input_events}
    previous = -1
    for index, checkpoint in enumerate(checkpoints):
        if not isinstance(checkpoint, dict) or not isinstance(checkpoint.get("atMs"), int):
            fail(path, f"checkpoints[{index}] must have integer atMs")
        at_ms = checkpoint["atMs"]
        if at_ms < previous or at_ms not in event_times:
            fail(path, f"checkpoints[{index}].atMs is not an ordered input event time")
        previous = at_ms
        if not isinstance(checkpoint.get("input"), dict) or not isinstance(checkpoint.get("model"), dict):
            fail(path, f"checkpoints[{index}] must include input and model objects")


def walk_strings(value: object):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from walk_strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from walk_strings(item)


def validate_synthetic_paths(path: Path, value: dict) -> None:
    for text in walk_strings(value):
        is_unix_absolute = text.startswith("/")
        is_windows_absolute = len(text) >= 3 and text[0].isalpha() and text[1] == ":" and text[2] in "\\/"
        if is_unix_absolute or is_windows_absolute:
            fail(path, "synthetic fixture must not contain an absolute user path")


def validate_legacy_config_fixtures() -> int:
    manifest_path = LEGACY_CONFIG_DIR / "manifest.json"
    manifest = load(manifest_path)
    if manifest.get("schemaVersion") != 1 or not isinstance(manifest.get("cases"), list):
        fail(manifest_path, "schemaVersion must be 1 and cases must be an array")

    case_ids: set[str] = set()
    listed_directories: set[str] = set()
    for index, case in enumerate(manifest["cases"]):
        if not isinstance(case, dict):
            fail(manifest_path, f"cases[{index}] must be an object")
        case_id = case.get("id")
        directory = case.get("directory")
        expected = case.get("expected")
        if not isinstance(case_id, str) or case_id in case_ids:
            fail(manifest_path, f"cases[{index}].id must be a unique string")
        if not isinstance(directory, str) or Path(directory).name != directory:
            fail(manifest_path, f"cases[{index}].directory must be one path component")
        if expected not in {"inspectable", "inspectable-with-stale-fields", "invalid"}:
            fail(manifest_path, f"cases[{index}].expected is invalid")
        if case.get("preserveSource") is not True:
            fail(manifest_path, f"cases[{index}] must require source preservation")

        case_dir = LEGACY_CONFIG_DIR / directory
        if not case_dir.is_dir():
            fail(manifest_path, f"case directory does not exist: {directory}")
        case_ids.add(case_id)
        listed_directories.add(directory)

        for store_name in LEGACY_STORE_NAMES:
            store_path = case_dir / f"{store_name}.json"
            if expected == "invalid" and store_name == "cat":
                invalid_path = case_dir / "cat.json.invalid"
                if not invalid_path.is_file() or store_path.exists():
                    fail(case_dir, "invalid case must contain only cat.json.invalid for cat store")
                try:
                    json.loads(invalid_path.read_text(encoding="utf-8"))
                except json.JSONDecodeError:
                    continue
                fail(invalid_path, "damaged fixture must not parse as JSON")

            if not store_path.is_file():
                fail(case_dir, f"missing store: {store_name}.json")
            validate_synthetic_paths(store_path, load(store_path))

        print(f"ok legacy-config {case_id}")

    actual_case_dirs = {
        path.name for path in LEGACY_CONFIG_DIR.iterdir() if path.is_dir()
    }
    if unlisted := actual_case_dirs - listed_directories:
        fail(manifest_path, f"unlisted legacy config directories: {sorted(unlisted)}")
    return len(case_ids)


def safe_package_reference(reference: str) -> tuple[str, ...] | None:
    normalized = reference.replace("\\", "/")
    path = PurePosixPath(normalized)
    if (
        not reference
        or normalized.startswith("/")
        or (len(normalized) >= 2 and normalized[0].isalpha() and normalized[1] == ":")
        or ".." in path.parts
    ):
        return None
    return path.parts


def materialize_model_case(case: dict, source_dir: Path, destination: Path) -> None:
    shutil.copytree(source_dir, destination)

    entry_source = case.get("entrySource")
    materialized_entry = case.get("materializedEntry")
    if entry_source is not None or materialized_entry is not None:
        if not isinstance(entry_source, str) or not isinstance(materialized_entry, str):
            fail(source_dir, "entrySource and materializedEntry must be strings")
        source_parts = safe_package_reference(entry_source)
        target_parts = safe_package_reference(materialized_entry)
        if source_parts is None or target_parts is None:
            fail(source_dir, "materialized entry paths must stay inside the package")
        source = destination.joinpath(*source_parts)
        target = destination.joinpath(*target_parts)
        if not source.is_file() or target.exists():
            fail(source_dir, "materialized entry source must exist and target must not")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source.read_bytes())

    materializations = case.get("materialize", [])
    if not isinstance(materializations, list):
        fail(source_dir, "materialize must be an array")
    for index, item in enumerate(materializations):
        if not isinstance(item, dict) or item.get("encoding") != "hex":
            fail(source_dir, f"materialize[{index}] must use hex encoding")
        source_parts = safe_package_reference(item.get("source", ""))
        target_parts = safe_package_reference(item.get("target", ""))
        if source_parts is None or target_parts is None:
            fail(source_dir, f"materialize[{index}] paths must stay inside the package")
        source = destination.joinpath(*source_parts)
        target = destination.joinpath(*target_parts)
        if not source.is_file() or target.exists():
            fail(source_dir, f"materialize[{index}] source must exist and target must not")
        try:
            content = bytes.fromhex(source.read_text(encoding="ascii").strip())
        except (OSError, UnicodeError, ValueError) as exc:
            fail(source_dir, f"materialize[{index}] is not valid ASCII hex: {exc}")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)


def parse_model_entry(path: Path, fixture_path: Path) -> dict | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    if not isinstance(value, dict):
        fail(fixture_path, "materialized model3 top-level value must be an object")
    return value


def inspect_model_case(package_dir: Path, fixture_path: Path, maximum_dimension: int) -> list[str]:
    entries = sorted(package_dir.glob("*.model3.json"))
    if len(entries) == 0:
        return ["model_entry_missing"]
    if len(entries) > 1:
        return ["model_entry_ambiguous"]

    model = parse_model_entry(entries[0], fixture_path)
    if model is None:
        return ["model_json_invalid"]
    references = model.get("FileReferences")
    if not isinstance(references, dict):
        fail(fixture_path, "model3 FileReferences must be an object")

    diagnostics: list[str] = []
    moc = references.get("Moc")
    if not isinstance(moc, str):
        fail(fixture_path, "model3 FileReferences.Moc must be a string")
    moc_parts = safe_package_reference(moc)
    if moc_parts is None:
        diagnostics.append("model_reference_escapes_root")
    elif not package_dir.joinpath(*moc_parts).is_file():
        diagnostics.append("model_moc_missing")

    textures = references.get("Textures")
    if not isinstance(textures, list) or not all(isinstance(item, str) for item in textures):
        fail(fixture_path, "model3 FileReferences.Textures must be a string array")
    for texture in textures:
        texture_parts = safe_package_reference(texture)
        if texture_parts is None:
            diagnostics.append("model_reference_escapes_root")
            continue
        texture_path = package_dir.joinpath(*texture_parts)
        if not texture_path.is_file():
            diagnostics.append("model_texture_missing")
            continue
        header = texture_path.read_bytes()[:24]
        if len(header) < 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
            diagnostics.append("model_texture_invalid_png")
            continue
        width = int.from_bytes(header[16:20], "big")
        height = int.from_bytes(header[20:24], "big")
        if width > maximum_dimension or height > maximum_dimension:
            diagnostics.append("model_texture_dimension_exceeded")
    return sorted(set(diagnostics))


def validate_model_fixtures() -> int:
    manifest_path = MODEL_FIXTURE_DIR / "cases.json"
    manifest = load(manifest_path)
    limits = manifest.get("limits")
    cases = manifest.get("cases")
    if manifest.get("schemaVersion") != 1 or not isinstance(limits, dict) or not isinstance(cases, list):
        fail(manifest_path, "schemaVersion must be 1; limits and cases must be objects/arrays")
    maximum_dimension = limits.get("maximumTextureDimension")
    if not isinstance(maximum_dimension, int) or maximum_dimension <= 0:
        fail(manifest_path, "limits.maximumTextureDimension must be a positive integer")

    case_ids: set[str] = set()
    listed_directories: set[str] = set()
    allowed_stages = {"package_discovery", "json_parse", "reference_resolution", "texture_header"}
    for index, case in enumerate(cases):
        if not isinstance(case, dict):
            fail(manifest_path, f"cases[{index}] must be an object")
        case_id = case.get("id")
        directory = case.get("directory")
        expected = case.get("expected")
        diagnostics = case.get("expectedDiagnostics")
        if not isinstance(case_id, str) or case_id in case_ids:
            fail(manifest_path, f"cases[{index}].id must be a unique string")
        if not isinstance(directory, str) or Path(directory).name != directory:
            fail(manifest_path, f"cases[{index}].directory must be one path component")
        if case.get("stage") not in allowed_stages or expected not in {"accept", "reject"}:
            fail(manifest_path, f"cases[{index}] has an invalid stage or expected value")
        if (
            not isinstance(diagnostics, list)
            or not all(isinstance(code, str) and code in MODEL_DIAGNOSTICS for code in diagnostics)
            or len(diagnostics) != len(set(diagnostics))
        ):
            fail(manifest_path, f"cases[{index}].expectedDiagnostics is invalid")
        if (expected == "accept") != (len(diagnostics) == 0):
            fail(manifest_path, f"cases[{index}] accept/reject must match diagnostics")

        source_dir = MODEL_CASE_DIR / directory
        if not source_dir.is_dir():
            fail(manifest_path, f"model case directory does not exist: {directory}")
        if case_id == "non-ascii-path":
            names = [directory, *(path.name for path in source_dir.iterdir())]
            if not any(any(ord(char) > 127 for char in name) for name in names) or not any(" " in name for name in names):
                fail(source_dir, "non-ascii path case must include non-ASCII and space characters")

        with tempfile.TemporaryDirectory(prefix="bongocat-model-fixture-") as temp_dir:
            package_dir = Path(temp_dir) / "package"
            materialize_model_case(case, source_dir, package_dir)
            actual = inspect_model_case(package_dir, source_dir, maximum_dimension)
        if actual != sorted(diagnostics):
            fail(source_dir, f"expected diagnostics {sorted(diagnostics)}, got {actual}")

        case_ids.add(case_id)
        listed_directories.add(directory)
        print(f"ok model-package {case_id}")

    actual_directories = {path.name for path in MODEL_CASE_DIR.iterdir() if path.is_dir()}
    if unlisted := actual_directories - listed_directories:
        fail(manifest_path, f"unlisted model case directories: {sorted(unlisted)}")
    return len(case_ids)


def main() -> int:
    legacy_case_count = validate_legacy_config_fixtures()
    model_case_count = validate_model_fixtures()
    input_files = sorted(INPUT_DIR.glob("*.json"))
    if not input_files:
        print("no input fixtures found", file=sys.stderr)
        return 1
    input_ids: set[str] = set()
    for input_path in input_files:
        if input_path.name == "schema.json":
            continue
        fixture_id, events = validate_input(input_path, load(input_path))
        if input_path.stem != fixture_id:
            fail(input_path, "filename must match fixture id")
        input_ids.add(fixture_id)
        expected_path = EXPECTED_DIR / f"{fixture_id}.json"
        if not expected_path.is_file():
            fail(input_path, f"missing expected fixture {expected_path.name}")
        validate_expected(expected_path, load(expected_path), fixture_id, events)
        print(f"ok {fixture_id}")
    expected_ids = {
        path.stem for path in EXPECTED_DIR.glob("*.json") if path.name != "schema.json"
    }
    if orphaned := expected_ids - input_ids:
        fail(EXPECTED_DIR, f"expected fixtures without input sequences: {sorted(orphaned)}")
    print(
        f"validated {len(input_ids)} input fixture(s), "
        f"{legacy_case_count} legacy config case(s), and {model_case_count} model package case(s)"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
