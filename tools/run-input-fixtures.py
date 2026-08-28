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


def apply_event(state: dict, event: dict) -> None:
    event_type = event["type"]
    if event_type == "key_down":
        state["pressed_keys"].add(event["key"])
    elif event_type == "key_up":
        state["pressed_keys"].discard(event["key"])
    elif event_type == "mouse_down":
        state["pressed_mouse_buttons"].add(event["button"])
    elif event_type == "mouse_up":
        state["pressed_mouse_buttons"].discard(event["button"])
    elif event_type == "cursor_moved":
        state["cursor_position"] = dict(event["position"])
    elif event_type == "gamepad_button":
        key = (event["deviceId"], event["button"])
        if event["value"] >= 0.5:
            state["gamepad_buttons"][key] = True
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
    else:
        raise ValueError(f"unknown event type: {event_type}")


def hand_state(state: dict, context: dict, side: str) -> bool:
    mapped_keys = {key for key, mapped_side in context["keySides"].items() if mapped_side == side}
    if mapped_keys.intersection(state["pressed_keys"]):
        return True
    return any(
        context["keySides"].get(button_key(button)) == side
        for (_device_id, button), pressed in state["gamepad_buttons"].items()
        if pressed
    )


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
        return float(any(device_button == button and pressed for (_device, device_button), pressed in state["gamepad_buttons"].items()))
    raise ValueError(f"unsupported expected parameter: {name}")


def run_fixture(input_path: Path) -> None:
    sequence = load(input_path)
    expected = load(EXPECTED_DIR / f"{input_path.stem}.json")
    context = sequence["context"]
    state = {
        "pressed_keys": set(),
        "pressed_mouse_buttons": set(),
        "connected_devices": set(),
        "gamepad_buttons": {},
        "cursor_position": None,
        "last_reset_reason": None,
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
            "parameters": {name: parameter_value(name, state, context) for name in checkpoint["model"]["parameters"]},
            "activeMotion": None,
            "activeExpression": None,
        }
        actual = {"input": actual_input, "model": actual_model}
        wanted = {"input": expected_input, "model": checkpoint["model"]}
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
