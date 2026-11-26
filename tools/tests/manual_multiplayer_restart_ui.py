#!/usr/bin/env python3
"""Exercise two live BongoCat settings windows on Wayland.

This machine-specific manual test uses mouse input through the real UI. It does
not write configuration files or generate keyboard input. The user must verify
the resulting animation on both screens. Run --inspect before --run whenever
the desktop layout changes; the script refuses ambiguous UI matches.
"""
from __future__ import annotations

import argparse
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

REMOTE = "hyperbola@100.126.86.111"
CONFIG = "~/.local/share/com.ayangweb.bongo-cat/development/config.json"


def command(host, args, *, env=None, raw=False, check=True, timeout=15):
    if host == "laptop":
        args = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", REMOTE,
                shlex.join(["env", *(f"{k}={v}" for k, v in (env or {}).items()), *map(str, args)])]
        env = None
    result = subprocess.run(args, env=None if env is None else {**os.environ, **env},
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=check, timeout=timeout)
    return result.stdout if raw else result.stdout.decode().strip()


def desktop_env(host):
    lines = command(host, ["systemctl", "--user", "show-environment"]).splitlines()
    env = dict(line.split("=", 1) for line in lines if "=" in line)
    return {key: env[key] for key in ("XDG_RUNTIME_DIR", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE") if key in env}


def hypr(host, *args):
    return command(host, ["hyprctl", *args], env=desktop_env(host))


def config(host):
    code = ("import json,os; c=json.load(open(os.path.expanduser(" + repr(CONFIG) + "))); "
            "n=c['network']; print(json.dumps({'id':n['device']['id'],'display':n['client']['display'],"
            "'publish':n['client']['publish'],'client':n['client']['enabled'],"
            "'relay':n['server']['enabled'],'port':n['server']['port']}))")
    return json.loads(command(host, ["python3", "-c", code]))


def window(host):
    matches = [w for w in json.loads(hypr(host, "-j", "clients")) if w.get("title") == "BongoCat 设置"]
    if len(matches) > 1:
        owner = int(command(host, ["tmux", "display-message", "-p", "-t", "bongocat-runtime-nov26", "#{pane_pid}"]))
        matches = [w for w in matches if w.get("pid") == owner]
    if len(matches) != 1:
        raise RuntimeError(f"{host}: expected one settings window, found {len(matches)}")
    return matches[0]


def focus(host):
    w = window(host)
    hypr(host, "eval", 'hl.dispatch(hl.dsp.dpms({ action = "on" }))')
    hypr(host, "eval", 'hl.dispatch(hl.dsp.focus({ window = "address:' + w["address"] + '" }))')
    time.sleep(0.4)
    current = json.loads(hypr(host, "-j", "activewindow"))
    if current.get("address") != w["address"]:
        raise RuntimeError(f"{host}: could not focus the settings window")
    return current


def screenshot(host, w):
    x, y = w["at"]
    width, height = w["size"]
    data = command(host, ["grim", "-g", f"{x},{y} {width}x{height}", "-t", "png", "-"],
                   env=desktop_env(host), raw=True)
    return Image.open(io.BytesIO(data)).convert("RGB")


def words(image):
    data = io.BytesIO()
    image.save(data, format="PNG")
    result = subprocess.run(["tesseract", "stdin", "stdout", "-l", "chi_sim+eng", "--psm", "11", "tsv"],
                            input=data.getvalue(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            check=True, timeout=20)
    grouped = {}
    for line in result.stdout.decode().splitlines()[1:]:
        parts = line.split("\t")
        if len(parts) == 12 and parts[11].strip():
            key = tuple(parts[i] for i in (1, 2, 3, 4))
            grouped.setdefault(key, []).append((parts[11].strip(), *(int(parts[i]) for i in (6, 7, 8, 9))))
    found = []
    for row in grouped.values():
        left = min(w[1] for w in row)
        top = min(w[2] for w in row)
        right = max(w[1] + w[3] for w in row)
        bottom = max(w[2] + w[4] for w in row)
        found.append(("".join(w[0] for w in row), left, top, right - left, bottom - top))
    return found


class Mouse:
    def __init__(self, directory):
        self.socket = str(directory / "pc.sock")
        self.local = None
        self.remote = None
        self.session = f"bongocat-ui-test-{os.getpid()}"

    def __enter__(self):
        self.local = subprocess.Popen(["ydotoold", "--socket-path", self.socket],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.remote = command("laptop", ["mktemp", "-d", "/tmp/bongocat-restart-ui.XXXXXX"])
        binaries = [shutil.which("ydotool"), shutil.which("ydotoold")]
        if not all(binaries):
            raise RuntimeError("ydotool and ydotoold are required")
        subprocess.run(["scp", "-q", *binaries, f"{REMOTE}:{self.remote}/"], check=True, timeout=20)
        daemon = shlex.join([self.remote + "/ydotoold", "--socket-path", self.remote + "/socket"])
        command("laptop", ["tmux", "new-session", "-d", "-s", self.session, daemon])
        for _ in range(40):
            try:
                command("laptop", ["test", "-S", self.remote + "/socket"])
                remote_ready = True
            except subprocess.CalledProcessError:
                remote_ready = False
            if Path(self.socket).exists() and remote_ready:
                break
            time.sleep(0.1)
        if not Path(self.socket).exists() or not remote_ready:
            raise RuntimeError("virtual mouse daemon did not start")
        return self

    def __exit__(self, *unused):
        if self.local:
            self.local.terminate()
            self.local.wait(timeout=5)
        if self.remote and self.remote.startswith("/tmp/bongocat-restart-ui."):
            command("laptop", ["tmux", "kill-session", "-t", self.session], check=False)
            command("laptop", ["rm", "-rf", self.remote], check=False)

    def send(self, host, *args):
        binary = "ydotool" if host == "pc" else self.remote + "/ydotool"
        socket = self.socket if host == "pc" else self.remote + "/socket"
        return command(host, [binary, *args], env={"YDOTOOL_SOCKET": socket})

    def click(self, host, x, y, address):
        for _ in range(10):
            cx, cy = map(int, hypr(host, "cursorpos").split(","))
            self.send(host, "mousemove", "--", str(round(x - cx)), str(round(y - cy)))
            cx, cy = map(int, hypr(host, "cursorpos").split(","))
            if abs(cx - x) <= 3 and abs(cy - y) <= 3:
                break
        else:
            raise RuntimeError(f"{host}: pointer at {(cx, cy)} did not reach {(round(x), round(y))}")
        if json.loads(hypr(host, "-j", "activewindow")).get("address") != address:
            raise RuntimeError(f"{host}: focused window changed before click")
        self.send(host, "click", "0xC0")


def open_network(host, mouse):
    w = focus(host)
    image = screenshot(host, w)
    sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    matches = [item for item in words(image) if item[0].endswith("联机") and item[1] < 210 * sx]
    if len(matches) != 1:
        raise RuntimeError(f"{host}: cannot identify the multiplayer navigation uniquely: {matches}")
    _, x, y, width, height = matches[0]
    mouse.click(host, w["at"][0] + (x + width / 2) / sx,
                w["at"][1] + (y + height / 2) / sy, w["address"])
    time.sleep(0.7)


def click_label(host, mouse, label, *, right_edge=False, attempts=3):
    w = focus(host)
    image = screenshot(host, w)
    sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    matches = [item for item in words(image) if label in item[0] and item[1] > 220 * sx]
    if label == "重启":
        matches = [item for item in matches if item[0] == label and item[1] > image.width * 0.7]
        if not matches:
            text = words(image)
            if not any("联机服务在BongoCat" in item[0] and "重启" in item[0] for item in text):
                raise RuntimeError(f"{host}: multiplayer restart notice is not visible")
            pixels = image.load()
            red = [(x, y) for y in range(int(image.height * 0.145), int(image.height * 0.205))
                   for x in range(int(image.width * 0.88), int(image.width * 0.99))
                   if (lambda c: c[0] > 100 and c[0] > c[1] * 1.5 and c[0] > c[2] * 1.2)(pixels[x, y])]
            if red:
                left, right = min(p[0] for p in red), max(p[0] for p in red)
                top, bottom = min(p[1] for p in red), max(p[1] for p in red)
                if len(red) >= 25 and right - left < image.width * 0.08 and bottom - top < image.height * 0.06:
                    matches = [(label, left, top, right - left, bottom - top)]
    if len(matches) != 1:
        if not matches and attempts > 1:
            time.sleep(0.5)
            return click_label(host, mouse, label, right_edge=right_edge, attempts=attempts - 1)
        raise RuntimeError(f"{host}: cannot identify {label!r} uniquely: {matches}")
    _, x, y, width, height = matches[0]
    target_x = image.width - 50 * sx if right_edge else x + width / 2
    mouse.click(host, w["at"][0] + target_x / sx,
                w["at"][1] + (y + height / 2) / sy, w["address"])


def wait_config(host, expected):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        actual = config(host)["display"]
        if set(actual) == set(expected):
            return
        time.sleep(0.2)
    raise RuntimeError(f"{host}: saved display {actual!r}, expected {expected!r}")


def enable_self(host, mouse):
    before = config(host)
    if "self" not in before["display"]:
        click_label(host, mouse, "跟随自己的输入", right_edge=True)
        wait_config(host, [*before["display"], "self"])
    print(f"PASS {host}: own input enabled through settings", flush=True)


def enable_peer(host, peer, mouse):
    before = config(host)
    if peer in before["display"]:
        print(f"PASS {host}: peer already enabled", flush=True)
        return
    w = focus(host)
    image = screenshot(host, w)
    rows = words(image)
    sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    normalize = lambda text: re.sub(r"[^a-z0-9]", "", text.lower()).replace("1", "l")
    labels = [row for row in rows if normalize(peer) in normalize(row[0]) and row[1] > 220 * sx]
    buttons = [row for row in rows if row[0] == "展示此设备" and row[1] > image.width * 0.7]
    matches = [(label, button) for label in labels for button in buttons
               if abs((label[2] + label[4] / 2) - (button[2] + button[4] / 2)) < 35]
    if len(matches) != 1:
        raise RuntimeError(f"{host}: peer and its display button were not uniquely identified")
    _, (_, x, y, width, height) = matches[0]
    mouse.click(host, w["at"][0] + (x + width / 2) / sx,
                w["at"][1] + (y + height / 2) / sy, w["address"])
    wait_config(host, [*before["display"], peer])
    print(f"PASS {host}: peer enabled through settings", flush=True)


def remove_peer(host, peer, mouse):
    before = config(host)
    if peer not in before["display"]:
        return
    w = focus(host)
    image = screenshot(host, w)
    rows = words(image)
    sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    group = [row for row in rows if "状态与设备选择" in row[0] and row[1] < 210 * sx]
    if len(group) == 1:
        _, x, y, width, height = group[0]
        mouse.click(host, w["at"][0] + (x + width / 2) / sx,
                    w["at"][1] + (y + height / 2) / sy, w["address"])
        time.sleep(0.7)
        w = focus(host)
        image = screenshot(host, w)
        rows = words(image)
        sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    normalize = lambda text: re.sub(r"[^a-z0-9]", "", text.lower()).replace("1", "l")
    labels = [row for row in rows if normalize(peer) in normalize(row[0]) and row[1] > 220 * sx]
    buttons = [row for row in rows if row[0] == "移除" and row[1] > image.width * 0.7]
    matches = [(label, button) for label in labels for button in buttons
               if abs((label[2] + label[4] / 2) - (button[2] + button[4] / 2)) < 35]
    if len(matches) != 1:
        raise RuntimeError(f"{host}: displayed peer and its remove button were not uniquely identified")
    _, (_, x, y, width, height) = matches[0]
    mouse.click(host, w["at"][0] + (x + width / 2) / sx,
                w["at"][1] + (y + height / 2) / sy, w["address"])
    wait_config(host, [device for device in before["display"] if device != peer])
    print(f"PASS {host}: main cat follows its own input only", flush=True)


def connections(host, port, listening=False):
    args = ["ss", "-ltnpe" if listening else "-tnpe"]
    result = command(host, args)
    return [line for line in result.splitlines() if re.search(rf":{port}\b", line)]


def socket_inodes(lines):
    return {int(value) for line in lines for value in re.findall(r"ino:(\d+)", line)}


def core_count(host, pid):
    output = command(host, ["coredumpctl", "--no-pager", "--no-legend", "list", str(pid)], check=False)
    return sum(1 for line in output.splitlines() if re.search(rf"\b{pid}\b", line))


def restart(host, mouse):
    w = focus(host)
    pid = w["pid"]
    laptop = config("laptop")
    port = laptop["port"]
    server = laptop["relay"] and host == "laptop"
    before = socket_inodes([line for line in connections(host, port, listening=server) if f"pid={pid}," in line])
    cores_before = core_count(host, pid)
    if not before:
        raise RuntimeError(f"{host}: no expected relay socket before restart")
    image = screenshot(host, w)
    sx, sy = image.width / w["size"][0], image.height / w["size"][1]
    notices = [row for row in words(image) if row[0].startswith("联机服务在") and row[1] < 210 * sx]
    if len(notices) == 1:
        _, x, y, width, height = notices[0]
        mouse.click(host, w["at"][0] + (x + width / 2) / sx,
                    w["at"][1] + (y + height / 2) / sy, w["address"])
        time.sleep(0.5)
    click_label(host, mouse, "重启")
    deadline = time.monotonic() + 35
    while time.monotonic() < deadline:
        try:
            next_window = window(host)
            after = socket_inodes([line for line in connections(host, port, listening=server) if f"pid={pid}," in line])
        except (RuntimeError, subprocess.SubprocessError):
            time.sleep(0.3)
            continue
        if next_window["pid"] == pid and after and before.isdisjoint(after):
            if core_count(host, pid) != cores_before:
                raise RuntimeError(f"{host}: a new core dump was recorded during restart")
            print(f"PASS {host}: UI restart kept process alive and replaced its socket", flush=True)
            return
        time.sleep(0.3)
    raise RuntimeError(f"{host}: UI restart did not recreate the window and socket within 35 seconds")


def verify_room():
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        pc, laptop = config("pc"), config("laptop")
        try:
            pc_pid, laptop_pid = window("pc")["pid"], window("laptop")["pid"]
            pc_conn = [line for line in connections("pc", laptop["port"]) if f"pid={pc_pid}," in line]
            laptop_conn = [line for line in connections("laptop", laptop["port"])
                           if f"pid={laptop_pid}," in line and "127.0.0.1:" in line]
            laptop_listener = [line for line in connections("laptop", laptop["port"], listening=True)
                               if f"pid={laptop_pid}," in line]
        except (RuntimeError, subprocess.SubprocessError):
            time.sleep(0.5)
            continue
        if pc_conn and laptop_conn and laptop_listener:
            break
        time.sleep(0.5)
    else:
        raise RuntimeError("two machines did not reconnect to the laptop relay")
    for host, own, other in (("pc", pc, laptop), ("laptop", laptop, pc)):
        if not own["client"] or not own["publish"] or not {"self", other["id"]}.issubset(own["display"]):
            raise RuntimeError(f"{host}: final settings do not include both publishers")
    print("PASS both clients configured for local and remote input; relay and PC connection are active", flush=True)
    print("MANUAL: user must observe both screens while operating each machine", flush=True)


def run(mouse):
    pc, laptop = config("pc"), config("laptop")
    if not all((pc["client"], pc["publish"], laptop["client"], laptop["publish"], laptop["relay"])):
        raise RuntimeError("both clients/publishers and the laptop relay must already be enabled")
    for host, other in (("pc", laptop["id"]), ("laptop", pc["id"])):
        open_network(host, mouse)
        enable_self(host, mouse)
        enable_peer(host, other, mouse)
        restart(host, mouse)
    verify_room()


def inspect(host):
    w = focus(host)
    image = screenshot(host, w)
    matches = [(word, x, y, width, height) for word, x, y, width, height in words(image)
               if any(s in word for s in ("联机", "重启", "本机", "设备", "self", "f53af1ea", "7d50a6ef"))]
    print(json.dumps({"host": host, "window": {k: w[k] for k in ("address", "pid", "at", "size")},
                      "image_size": image.size, "matches": matches}, ensure_ascii=False), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--inspect", choices=["pc", "laptop"])
    parser.add_argument("--open-network", action="store_true")
    parser.add_argument("--prepare", choices=["pc", "laptop"])
    parser.add_argument("--restart", choices=["pc", "laptop"])
    parser.add_argument("--peer", choices=["pc", "laptop"])
    parser.add_argument("--local-only", choices=["pc", "laptop"])
    parser.add_argument("--run", action="store_true")
    args = parser.parse_args()
    if args.inspect:
        if args.open_network:
            with tempfile.TemporaryDirectory(prefix="bongocat-ui-mouse-") as directory:
                with Mouse(Path(directory)) as mouse:
                    open_network(args.inspect, mouse)
        inspect(args.inspect)
    elif args.run:
        with tempfile.TemporaryDirectory(prefix="bongocat-ui-mouse-") as directory:
            with Mouse(Path(directory)) as mouse:
                run(mouse)
    elif args.prepare or args.restart or args.peer or args.local_only:
        host = args.prepare or args.restart or args.peer or args.local_only
        with tempfile.TemporaryDirectory(prefix="bongocat-ui-mouse-") as directory:
            with Mouse(Path(directory)) as mouse:
                if args.prepare:
                    enable_self(host, mouse)
                elif args.peer:
                    enable_peer(host, config("laptop" if host == "pc" else "pc")["id"], mouse)
                elif args.local_only:
                    open_network(host, mouse)
                    enable_self(host, mouse)
                    remove_peer(host, config("laptop" if host == "pc" else "pc")["id"], mouse)
                    restart(host, mouse)
                else:
                    restart(host, mouse)
    else:
        parser.error("choose --inspect, --prepare, --restart, or --run")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
