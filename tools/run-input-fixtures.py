#!/usr/bin/env python3
"""Run shared input fixtures through a deterministic protocol model."""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INPUT_DIR = ROOT / "shared" / "fixtures" / "input-sequences"
EXPECTED_DIR = ROOT / "shared" / "fixtures" / "expected-state"


def load(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"{path.relative_to(ROOT)}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"{path.relative_to(ROOT)}: top-level value must be an object")
    return value


def button_key(button: str) -> str:
    return "Gamepad" + "".join(part.capitalize() for part in button.split("_"))


def parameter_names(sequence: dict) -> list[str]:
    names: set[str] = set()
    for key, side in sequence["context"]["keySides"].items():
        if key.startswith("Gamepad"):
            names.add(f"{key}Down")
        elif side == "left":
            names.add("CatParamLeftHandDown")
        else:
            names.add("CatParamRightHandDown")
    for event in sequence["events"]:
        if event["type"] in {"mouse_down", "mouse_up"}:
            names.add(f"ParamMouse{event['button'].capitalize()}Down")
    return sorted(names)


def apply_event(state: dict, event: dict) -> None:
    event_type = event["type"]
    at_ms = event["atMs"]
    if event_type == "key_down":
        # A repeat is a duplicate edge rather than a new press, so the control
        # keeps the time it was first seen down. That time is what picks the
        # overlay when one hand has more than one control held.
        state["pressed_keys"].setdefault(event["key"], at_ms)
    elif event_type == "key_up":
        state["pressed_keys"].pop(event["key"], None)
    elif event_type == "mouse_down":
        state["pressed_mouse_buttons"].add(event["button"])
    elif event_type == "mouse_up":
        state["pressed_mouse_buttons"].discard(event["button"])
    elif event_type == "cursor_moved":
        state["cursor_position"] = dict(event["position"])
    elif event_type == "gamepad_button":
        key = (event["deviceId"], event["button"])
        if event["value"] >= 0.5:
            state["gamepad_buttons"].setdefault(key, at_ms)
        else:
            state["gamepad_buttons"].pop(key, None)
    elif event_type == "gamepad_axis":
        pass
    elif event_type == "device_connected":
        state["connected_devices"].add(event["deviceId"])
    elif event_type == "device_disconnected":
        state["connected_devices"].discard(event["deviceId"])
        for key in [key for key in state["gamepad_buttons"] if key[0] == event["deviceId"]]:
            state["gamepad_buttons"].pop(key, None)
    elif event_type == "reset":
        state["pressed_keys"].clear()
        state["pressed_mouse_buttons"].clear()
        state["gamepad_buttons"].clear()
        state["cursor_position"] = None
        state["last_reset_reason"] = event["reason"]
    elif event_type == "model_switch":
        state["selected_model_id"] = event["modelId"]
        state["active_motion"] = None
        state["motion_priority"] = None
        state["active_expression"] = None
    elif event_type == "motion_start":
        priority = {"idle": 0, "normal": 1, "force": 2}[event["priority"]]
        if state["motion_priority"] is None or priority >= state["motion_priority"]:
            state["active_motion"] = event["motionId"]
            state["motion_priority"] = priority
    elif event_type == "motion_stop":
        if state["active_motion"] == event["motionId"]:
            state["active_motion"] = None
            state["motion_priority"] = None
    elif event_type == "expression_set":
        state["active_expression"] = event["expressionId"]
    elif event_type == "audio_trigger":
        state["audio_trigger_count"] += 1
    else:
        raise ValueError(f"unknown event type: {event_type}")


def hand_state(state: dict, context: dict, side: str) -> bool:
    mapped_keys = {key for key, mapped_side in context["keySides"].items() if mapped_side == side}
    if mapped_keys.intersection(state["pressed_keys"]):
        return True
    return any(
        context["keySides"].get(button_key(button)) == side
        for (_device_id, button) in state["gamepad_buttons"]
    )


def key_overlays(state: dict, context: dict) -> list[dict]:
    """The overlays the model is asked to draw, one per hand.

    Each hand draws the control it most recently saw pressed. This mirrors the
    runtime's key-press projection, which keeps one press per hand and drops a
    control the model has no hand for. A mouse button never reaches it: it drives
    a parameter, not an overlay. Reporting the overlay here is what lets a fixture
    pin that a gamepad button is drawable at all (ADR-0070).

    The whole ``(pressed_at, key)`` pair orders the candidates, so the answer is
    a total order and every reference model agrees on it. No fixture relies on a
    tie: a hand only ever draws one overlay, so a tie would make the drawn
    artwork arbitrary.
    """
    presses = [
        (at_ms, key) for key, at_ms in state["pressed_keys"].items()
    ] + [
        (at_ms, button_key(button))
        for (_device_id, button), at_ms in state["gamepad_buttons"].items()
    ]
    overlays = []
    for side in ("left", "right"):
        candidates = [
            (at_ms, key) for at_ms, key in presses if context["keySides"].get(key) == side
        ]
        if candidates:
            overlays.append({"key": max(candidates)[1], "side": side})
    return overlays


def parameter_value(name: str, state: dict, context: dict) -> float:
    if name == "CatParamLeftHandDown":
        return float(hand_state(state, context, "left"))
    if name == "CatParamRightHandDown":
        return float(hand_state(state, context, "right"))
    if name.startswith("ParamMouse") and name.endswith("Down"):
        button = name[len("ParamMouse") : -len("Down")].lower()
        return float(button in state["pressed_mouse_buttons"])
    if name.startswith("Gamepad") and name.endswith("Down"):
        button_name = name[len("Gamepad") : -len("Down")]
        button = button_name[0].lower() + button_name[1:]
        return float(
            any(
                device_button == button
                for (_device, device_button) in state["gamepad_buttons"]
            )
        )
    raise ValueError(f"unsupported expected parameter: {name}")


def run_fixture(input_path: Path) -> None:
    sequence = load(input_path)
    expected = load(EXPECTED_DIR / f"{input_path.stem}.json")
    context = sequence["context"]
    tracked_parameters = parameter_names(sequence)
    state = {
        "pressed_keys": {},
        "pressed_mouse_buttons": set(),
        "connected_devices": set(),
        "gamepad_buttons": {},
        "cursor_position": None,
        "last_reset_reason": None,
        "selected_model_id": None,
        "active_motion": None,
        "motion_priority": None,
        "active_expression": None,
        "audio_trigger_count": 0,
    }
    event_index = 0
    for checkpoint in expected["checkpoints"]:
        while event_index < len(sequence["events"]) and sequence["events"][event_index]["atMs"] <= checkpoint["atMs"]:
            apply_event(state, sequence["events"][event_index])
            event_index += 1
        expected_input = checkpoint["input"]
        actual_input = {
            "pressedKeys": sorted(state["pressed_keys"]),
            "pressedMouseButtons": sorted(state["pressed_mouse_buttons"]),
            "connectedDevices": sorted(state["connected_devices"]),
            "lastResetReason": state["last_reset_reason"],
        }
        if state["cursor_position"] is not None:
            actual_input["cursorPosition"] = state["cursor_position"]
        actual_model = {
            "leftHandDown": hand_state(state, context, "left"),
            "rightHandDown": hand_state(state, context, "right"),
            "activeKeyOverlays": key_overlays(state, context),
            "parameters": {name: parameter_value(name, state, context) for name in tracked_parameters},
            "activeMotion": state["active_motion"],
            "activeExpression": state["active_expression"],
        }
        if "selectedModelId" in checkpoint["model"]:
            actual_model["selectedModelId"] = state["selected_model_id"]
        # A checkpoint that declares no overlay list is declaring an empty one, so
        # both spellings have to compare alike.
        expected_model = dict(checkpoint["model"])
        expected_model.setdefault("activeKeyOverlays", [])
        actual = {"input": actual_input, "model": actual_model}
        wanted = {"input": expected_input, "model": expected_model}
        if actual != wanted:
            raise ValueError(
                f"{input_path.relative_to(ROOT)} checkpoint {checkpoint['atMs']}ms mismatch\n"
                f"expected={json.dumps(wanted, sort_keys=True)}\nactual={json.dumps(actual, sort_keys=True)}"
            )


def main() -> int:
    count = 0
    for path in sorted(INPUT_DIR.glob("*.json")):
        if path.name == "schema.json":
            continue
        run_fixture(path)
        print(f"ok {path.stem}")
        count += 1
    print(f"ran {count} input fixture(s)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
