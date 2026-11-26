#!/usr/bin/env python3
"""Exercise the live PC/laptop multiplayer settings through the actual UI.

This is a manual, machine-specific regression tool, not a CI test. Start with
both settings windows focused on the multiplayer page, PC showing the laptop,
and laptop showing no device. The two devices must already be in the same room
and publishing. The script clicks the actual peer controls in both directions,
checks the saved settings and restores the requested laptop-to-PC arrangement.
The user must verify which machine actually controls the cat after restarting.

It never writes configuration files and aborts if the observed window geometry
or the device label next to a target button does not match. Screenshots contain
only BongoCat settings or its overlay and are kept in a private temporary dir.
Requires SSH alias laptop, Hyprland, grim, tesseract (eng), Pillow, ydotool,
ydotoold, tmux on the laptop, and permission to use /dev/uinput on both hosts.
"""

from __future__ import annotations

import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from PIL import Image

CONFIG = "~/.local/share/com.ayangweb.bongo-cat/development/config.json"
GEOMETRY = {"pc": ([768, 26], [768, 838]), "laptop": ([727, 38], [701, 850])}
BUTTONS = {
    "pc-remove": (1485, 744),
    "pc-add": (1460, 744),
    "laptop-add": (1378, 773),
    "laptop-remove": (1378, 773),
}


def command(host, args, *, environment=None, data=False, timeout=10):
    if host == "laptop":
        env = [f"{key}={value}" for key, value in (environment or {}).items()]
        remote = shlex.join(["env", *env, *map(str, args)])
        args = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "laptop", remote]
        environment = None
    result = subprocess.run(
        args,
        env=None if environment is None else {**os.environ, **environment},
        capture_output=True,
        timeout=timeout,
        check=True,
    )
    return result.stdout if data else result.stdout.decode().strip()


def desktop_env(host):
    entries = command(host, ["systemctl", "--user", "show-environment"])
    values = dict(line.split("=", 1) for line in entries.splitlines() if "=" in line)
    names = ("XDG_RUNTIME_DIR", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE")
    return {name: values[name] for name in names if name in values}


def hypr(host, *args):
    return command(host, ["hyprctl", *args], environment=desktop_env(host))


def settings_window(host):
    window = json.loads(hypr(host, "-j", "activewindow"))
    geometry = (window.get("at"), window.get("size"))
    if window.get("title") != "BongoCat 设置" or geometry != GEOMETRY[host]:
        raise RuntimeError(f"{host}: settings window is not focused or its geometry changed")
    return window


def screenshot(host, rect):
    x, y, width, height = rect
    data = command(
        host,
        ["grim", "-g", f"{x},{y} {width}x{height}", "-t", "png", "-"],
        environment=desktop_env(host),
        data=True,
    )
    return Image.open(io.BytesIO(data)).convert("RGB")


def ocr_tokens(image):
    png = io.BytesIO()
    image.save(png, format="PNG")
    output = subprocess.run(
        ["tesseract", "stdin", "stdout", "-l", "eng", "tsv"],
        input=png.getvalue(),
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=True,
        timeout=15,
    ).stdout.decode()
    for line in output.splitlines()[1:]:
        fields = line.split("\t")
        if len(fields) == 12 and fields[11]:
            yield fields[11], int(fields[6]), int(fields[7]), int(fields[8]), int(fields[9])


def normalized(value):
    return re.sub(r"[^a-z0-9]", "", value.lower()).replace("1", "l")


def verify_peer_row(host, peer_id, target_y):
    window = settings_window(host)
    x, y = window["at"]
    width, height = window["size"]
    image = screenshot(host, (x, y, width, height))
    scale = image.width / width
    expected = normalized(peer_id)
    for text, left, top, _width, token_height in ocr_tokens(image):
        token_y = y + (top + token_height / 2) / scale
        if (
            expected in normalized(text)
            and left / scale > width * 0.30
            and abs(token_y - target_y) <= 18
        ):
            return
    raise RuntimeError(f"{host}: peer ID was not recognized next to the expected UI control")


def network(host):
    code = (
        "import json,os; n=json.load(open(os.path.expanduser("
        + repr(CONFIG)
        + ")))[\"network\"]; print(json.dumps({\"id\":n[\"device\"][\"id\"],"
        "\"display\":n[\"client\"][\"display\"],\"publish\":n[\"client\"][\"publish\"],"
        "\"client\":n[\"client\"][\"enabled\"],\"relay\":n[\"server\"][\"enabled\"]}))"
    )
    return json.loads(command(host, ["python3", "-c", code]))


def wait_saved(host, expected):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        actual = network(host)["display"]
        if actual == expected:
            return
        time.sleep(0.2)
    raise RuntimeError(f"{host}: saved display {actual!r}, expected {expected!r}")


class VirtualInput:
    def __init__(self, directory):
        self.local_socket = str(directory / "pc.sock")
        self.local_process = None
        self.remote_dir = None
        self.remote_session = f"bongocat-ui-regression-{os.getpid()}"

    def start(self):
        self.local_process = subprocess.Popen(
            ["ydotoold", "--socket-path", self.local_socket],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.remote_dir = command("laptop", ["mktemp", "-d", "/tmp/bongocat-role-ui.XXXXXX"])
        client = shutil.which("ydotool")
        daemon = shutil.which("ydotoold")
        if not client or not daemon:
            raise RuntimeError("ydotool and ydotoold are required on the PC")
        subprocess.run(["scp", "-q", client, daemon, f"laptop:{self.remote_dir}/"], check=True)
        remote_daemon = shlex.join(
            [f"{self.remote_dir}/ydotoold", "--socket-path", f"{self.remote_dir}/socket"]
        )
        command("laptop", ["tmux", "new-session", "-d", "-s", self.remote_session, remote_daemon])
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline:
            local_ready = Path(self.local_socket).exists()
            try:
                command("laptop", ["test", "-S", f"{self.remote_dir}/socket"])
                remote_ready = True
            except subprocess.CalledProcessError:
                remote_ready = False
            if local_ready and remote_ready:
                return
            time.sleep(0.2)
        raise RuntimeError("virtual input daemon did not become ready")

    def send(self, host, *args):
        executable = "ydotool" if host == "pc" else f"{self.remote_dir}/ydotool"
        socket = self.local_socket if host == "pc" else f"{self.remote_dir}/socket"
        return command(host, [executable, *args], environment={"YDOTOOL_SOCKET": socket})

    def click(self, host, label, peer_id):
        target_x, target_y = BUTTONS[label]
        verify_peer_row(host, peer_id, target_y)
        window = settings_window(host)
        cursor_x, cursor_y = map(int, hypr(host, "cursorpos").split(","))
        for _ in range(10):
            self.send(host, "mousemove", "--", str(target_x - cursor_x), str(target_y - cursor_y))
            cursor_x, cursor_y = map(int, hypr(host, "cursorpos").split(","))
            if abs(cursor_x - target_x) <= 3 and abs(cursor_y - target_y) <= 3:
                break
        else:
            raise RuntimeError(f"{host}: pointer failed to reach the verified button")
        if settings_window(host).get("address") != window.get("address"):
            raise RuntimeError(f"{host}: focused window changed before the click")
        self.send(host, "click", "0xC0")

    def close(self):
        if self.local_process is not None:
            self.local_process.terminate()
            self.local_process.wait(timeout=5)
        if self.remote_dir is not None:
            try:
                command("laptop", ["tmux", "kill-session", "-t", self.remote_session])
            except subprocess.CalledProcessError:
                pass
            command(
                "laptop",
                ["python3", "-c", "import shutil,sys; shutil.rmtree(sys.argv[1])", self.remote_dir],
            )


def run_test(virtual):
    pc, laptop = network("pc"), network("laptop")
    if pc["display"] != [laptop["id"]] or laptop["display"]:
        raise RuntimeError("start with PC displaying laptop and laptop displaying no device")
    if not all((pc["client"], pc["publish"], laptop["client"], laptop["publish"], laptop["relay"])):
        raise RuntimeError("both clients and publishers, and laptop relay, must be enabled")
    for host in ("pc", "laptop"):
        settings_window(host)

    steps = (
        ("pc", "pc-remove", laptop["id"], []),
        ("laptop", "laptop-add", pc["id"], [pc["id"]]),
        ("pc", "pc-add", laptop["id"], [laptop["id"]]),
        ("laptop", "laptop-remove", pc["id"], []),
    )
    for host, label, peer_id, expected in steps:
        virtual.click(host, label, peer_id)
        wait_saved(host, expected)
        print(f"PASS {label}: UI change saved", flush=True)

    print("NOTE: settings saved; restart and have the user verify the input source", flush=True)


def main():
    with tempfile.TemporaryDirectory(prefix="bongocat-role-ui-") as directory:
        virtual = VirtualInput(Path(directory))
        try:
            virtual.start()
            run_test(virtual)
        finally:
            virtual.close()


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"FAIL: {error}; inspect both UI settings before retrying", file=sys.stderr)
        sys.exit(1)
