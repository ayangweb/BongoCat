#!/usr/bin/env python3
"""Validate Phase 0 fixture shape and input/expected cross-file invariants."""

from __future__ import annotations

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
INPUT_DIR = ROOT / "shared" / "fixtures" / "input-sequences"
EXPECTED_DIR = ROOT / "shared" / "fixtures" / "expected-state"


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


def main() -> int:
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
    print(f"validated {len(input_ids)} fixture(s)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
