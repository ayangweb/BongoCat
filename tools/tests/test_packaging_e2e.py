import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BIN = ROOT / "target" / "debug" / "bongocat-plugin-packaging"
HOST_TRIPLES = {
    ("darwin", "arm64"): "aarch64-apple-darwin",
    ("darwin", "x86_64"): "x86_64-apple-darwin",
    ("win32", "amd64"): "x86_64-pc-windows-msvc",
    ("win32", "x86_64"): "x86_64-pc-windows-msvc",
}
PLATFORM_KEYS = {
    "aarch64-apple-darwin": "macos-aarch64",
    "x86_64-apple-darwin": "macos-x86_64",
    "x86_64-pc-windows-msvc": "windows-x86_64",
}


def host_triple():
    machine = os.uname().machine if hasattr(os, "uname") else os.environ.get("PROCESSOR_ARCHITECTURE")
    machine = (machine or "").lower()
    if machine == "amd64":
        machine = "x86_64"
    return HOST_TRIPLES.get((sys.platform, machine))


HOST_TRIPLE = host_triple()
HOST_PLATFORM = PLATFORM_KEYS[HOST_TRIPLE] if HOST_TRIPLE else None


def stage_packages(root: Path, platform: str) -> Path:
    source = ROOT / "target" / "plugins" / f"test-{HOST_PLATFORM}"
    output = root / platform
    output.mkdir(parents=True)
    for package in source.glob("*.zip"):
        name = package.name.replace("macos-aarch64", platform)
        if platform == "windows-x86_64":
            import zipfile

            with zipfile.ZipFile(package) as archive:
                manifest = archive.read("plugin.json")
            metadata = json.loads(manifest)["metadata"]
            with zipfile.ZipFile(package) as source_archive:
                with zipfile.ZipFile(output / name, "w") as archive:
                    for info in source_archive.infolist():
                        data = source_archive.read(info.filename)
                        name_in_zip = (
                            f"{metadata['entry']}.exe"
                            if info.filename == metadata["entry"]
                            else info.filename
                        )
                        archive.writestr(name_in_zip, data)
        else:
            shutil.copyfile(package, output / name)
    return output


@unittest.skipUnless(HOST_TRIPLE, "packaging e2e tests require macOS or Windows")
class PackagingEndToEndTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(
            [
                "cargo",
                "build",
                "--locked",
                "-p",
                "bongocat-plugin-packaging",
            ],
            cwd=ROOT,
            check=True,
        )
        subprocess.run(
            [
                "cargo",
                "run",
                "--locked",
                "-p",
                "bongocat-plugin-packaging",
                "--",
                "build",
                "--root",
                str(ROOT),
                "--target",
                HOST_TRIPLE,
                "--output",
                str(ROOT / "target" / "plugins" / f"test-{HOST_PLATFORM}"),
            ],
            cwd=ROOT,
            check=True,
        )

    def test_catalog_sign_verify_and_immutability(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            inputs.mkdir()
            private_key = root / "plugin-release.key"
            public_key = root / "plugin-release.pub"
            subprocess.run(
                [
                    str(BIN),
                    "keygen",
                    "--private",
                    str(private_key),
                    "--public",
                    str(public_key),
                ],
                check=True,
            )
            for platform in ["macos-aarch64", "macos-x86_64", "windows-x86_64"]:
                stage_packages(inputs, platform)
            output = root / "output"
            subprocess.run(
                [
                    str(BIN),
                    "catalog",
                    "assemble",
                    "--root",
                    str(ROOT),
                    "--tag",
                    "plugins-v1.0.0",
                    "--previous",
                    str(root / "missing"),
                    "--output",
                    str(output),
                    "--platform",
                    "macos-aarch64",
                    str(inputs / "macos-aarch64"),
                    "--platform",
                    "macos-x86_64",
                    str(inputs / "macos-x86_64"),
                    "--platform",
                    "windows-x86_64",
                    str(inputs / "windows-x86_64"),
                ],
                check=True,
            )
            catalog = output / "plugins.json"
            environment = {
                **dict(__import__("os").environ),
                "PLUGIN_SIGNING_PRIVATE_KEY": private_key.read_text().strip(),
            }
            subprocess.run(
                [
                    str(BIN),
                    "catalog",
                    "sign",
                    "--catalog",
                    str(output / "catalog.json"),
                    "--output",
                    str(catalog),
                    "--public-key",
                    str(public_key),
                ],
                check=True,
                env=environment,
            )
            subprocess.run(
                [
                    str(BIN),
                    "verify",
                    "--catalog",
                    str(catalog),
                    "--state",
                    str(output),
                    "--public-key",
                    str(public_key),
                ],
                check=True,
            )
            state = json.loads((output / "release-state.json").read_text())
            self.assertEqual(len(state["plugins"]), 3)
            self.assertEqual(state["retired"], [])
            self.assertEqual(len(state["plugins"][0]["packages"]), 3)
            catalog = json.loads(catalog.read_text())["catalog"]
            self.assertEqual(len(catalog["plugins"]), 9)
            assets = {
                value
                for entry in catalog["plugins"]
                for value in [*entry["downloads"].values(), entry["icon_url"]]
            }
            self.assertIn("input-source-1.0.0-windows-x86_64.zip", assets)
            self.assertIn("key-sounds-1.0.0-macos-aarch64.zip", assets)
            self.assertIn("pomodoro-1.0.0.png", assets)
            self.assertFalse(any(value.startswith("com.bongocat.") for value in assets))

            # Same source and version must reuse the exact historical package
            # bytes even when a runner produces different ZIP metadata.
            second = root / "second"
            subprocess.run(
                [
                    str(BIN),
                    "catalog",
                    "assemble",
                    "--root",
                    str(ROOT),
                    "--tag",
                    "plugins-v1.0.1",
                    "--previous",
                    str(output),
                    "--output",
                    str(second),
                    "--platform",
                    "macos-aarch64",
                    str(inputs / "macos-aarch64"),
                    "--platform",
                    "macos-x86_64",
                    str(inputs / "macos-x86_64"),
                    "--platform",
                    "windows-x86_64",
                    str(inputs / "windows-x86_64"),
                ],
                check=True,
            )
            for package in output.glob("*.zip"):
                copied = second / package.name
                self.assertEqual(
                    hashlib.sha256(package.read_bytes()).digest(),
                    hashlib.sha256(copied.read_bytes()).digest(),
                )
            state2 = json.loads((second / "release-state.json").read_text())
            for plugin in state2["plugins"]:
                self.assertTrue(plugin["source_sha256"])

    def test_removed_plugin_versions_cannot_be_reintroduced(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            inputs.mkdir()
            for platform in ["macos-aarch64", "macos-x86_64", "windows-x86_64"]:
                stage_packages(inputs, platform)
            output = root / "output"

            def assemble(tag, previous):
                subprocess.run(
                    [
                        str(BIN),
                        "catalog",
                        "assemble",
                        "--root",
                        str(ROOT),
                        "--tag",
                        tag,
                        "--previous",
                        str(previous),
                        "--output",
                        str(output),
                        "--platform",
                        "macos-aarch64",
                        str(inputs / "macos-aarch64"),
                        "--platform",
                        "macos-x86_64",
                        str(inputs / "macos-x86_64"),
                        "--platform",
                        "windows-x86_64",
                        str(inputs / "windows-x86_64"),
                    ],
                    check=True,
                )

            assemble("plugins-v1.0.0", root / "missing")
            initial = json.loads((output / "release-state.json").read_text())
            retired = initial["plugins"].pop(0)
            initial["retired"] = [retired]
            (output / "release-state.json").write_text(json.dumps(initial))
            manifest = ROOT / "plugins/input-source/plugin.json"
            original_manifest = manifest.read_bytes()
            metadata = json.loads(original_manifest)
            metadata["metadata"]["version"] = "0.9.0"
            try:
                manifest.write_text(json.dumps(metadata))
                result = subprocess.run(
                    [
                        str(BIN),
                        "catalog",
                        "assemble",
                        "--root",
                        str(ROOT),
                        "--tag",
                        "plugins-v1.0.1",
                        "--previous",
                        str(output),
                        "--output",
                        str(root / "second"),
                        "--platform",
                        "macos-aarch64",
                        str(inputs / "macos-aarch64"),
                        "--platform",
                        "macos-x86_64",
                        str(inputs / "macos-x86_64"),
                        "--platform",
                        "windows-x86_64",
                        str(inputs / "windows-x86_64"),
                    ],
                    check=False,
                    capture_output=True,
                    text=True,
                )
            finally:
                manifest.write_bytes(original_manifest)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("version regressed", result.stderr)

    def test_manifest_change_requires_a_version_bump(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            inputs.mkdir()
            for platform in ["macos-aarch64", "macos-x86_64", "windows-x86_64"]:
                stage_packages(inputs, platform)
            output = root / "output"
            arguments = [
                str(BIN),
                "catalog",
                "assemble",
                "--root",
                str(ROOT),
                "--tag",
                "plugins-v1.0.0",
                "--previous",
                str(root / "missing"),
                "--output",
                str(output),
                "--platform",
                "macos-aarch64",
                str(inputs / "macos-aarch64"),
                "--platform",
                "macos-x86_64",
                str(inputs / "macos-x86_64"),
                "--platform",
                "windows-x86_64",
                str(inputs / "windows-x86_64"),
            ]
            subprocess.run(arguments, check=True)
            manifest = ROOT / "plugins/pomodoro/plugin.json"
            original = manifest.read_bytes()
            try:
                data = json.loads(original)
                data["metadata"]["description"] += " Updated."
                manifest.write_text(json.dumps(data))
                for platform in ["macos-aarch64", "macos-x86_64"]:
                    source = inputs / platform
                    for package in source.glob("pomodoro-*.zip"):
                        import zipfile

                        with zipfile.ZipFile(package) as archive:
                            entries = {
                                info.filename: archive.read(info.filename)
                                for info in archive.infolist()
                            }
                        with zipfile.ZipFile(package, "w") as archive:
                            for name, contents in entries.items():
                                if name == "plugin.json":
                                    contents = json.dumps(
                                        {"metadata": data["metadata"]}
                                    ).encode()
                                info = zipfile.ZipInfo(name)
                                info.external_attr = (
                                    (0o100755 if name == "bongocat-pomodoro" else 0o100644)
                                    << 16
                                )
                                archive.writestr(info, contents)
                arguments[arguments.index("plugins-v1.0.0")] = "plugins-v1.0.1"
                arguments[arguments.index(str(output))] = str(root / "second")
                generated = subprocess.run(
                    [
                        str(BIN),
                        "catalog",
                        "assemble",
                        "--root",
                        str(ROOT),
                        "--tag",
                        "plugins-v1.0.0",
                        "--previous",
                        str(root / "missing"),
                        "--output",
                        str(root / "changed"),
                        "--platform",
                        "macos-aarch64",
                        str(inputs / "macos-aarch64"),
                        "--platform",
                        "macos-x86_64",
                        str(inputs / "macos-x86_64"),
                        "--platform",
                        "windows-x86_64",
                        str(inputs / "windows-x86_64"),
                    ],
                    check=False,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(generated.returncode, 0, generated.stderr)
                baseline = json.loads((output / "release-state.json").read_text())
                changed = json.loads((root / "changed/release-state.json").read_text())
                baseline_plugin = next(
                    plugin
                    for plugin in baseline["plugins"]
                    if plugin["id"] == "com.bongocat.pomodoro"
                )
                changed_plugin = next(
                    plugin
                    for plugin in changed["plugins"]
                    if plugin["id"] == "com.bongocat.pomodoro"
                )
                self.assertNotEqual(
                    baseline_plugin["source_sha256"], changed_plugin["source_sha256"]
                )
                previous_dir = root / "previous"
                previous_dir.mkdir()
                shutil.copyfile(output / "release-state.json", previous_dir / "release-state.json")
                for package in output.glob("*.zip"):
                    shutil.copyfile(package, previous_dir / package.name)
                for package in output.glob("*.png"):
                    shutil.copyfile(package, previous_dir / package.name)
                arguments = [
                    str(BIN),
                    "catalog",
                    "assemble",
                    "--root",
                    str(ROOT),
                    "--tag",
                    "plugins-v1.0.1",
                    "--previous",
                    str(previous_dir),
                    "--output",
                    str(root / "second"),
                    "--platform",
                    "macos-aarch64",
                    str(inputs / "macos-aarch64"),
                    "--platform",
                    "macos-x86_64",
                    str(inputs / "macos-x86_64"),
                    "--platform",
                    "windows-x86_64",
                    str(inputs / "windows-x86_64"),
                ]
                state = json.loads((previous_dir / "release-state.json").read_text())
                state_plugin = next(
                    plugin
                    for plugin in state["plugins"]
                    if plugin["id"] == "com.bongocat.pomodoro"
                )
                self.assertEqual(state_plugin["source_sha256"], baseline_plugin["source_sha256"])
                result = subprocess.run(
                    arguments, check=False, capture_output=True, text=True
                )
            finally:
                manifest.write_bytes(original)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("changed without a version bump", result.stderr)

class ReleaseWorkflowTests(unittest.TestCase):
    def test_workflow_is_plugin_only(self):
        workflow = (ROOT / ".github/workflows/plugin-release.yml").read_text()
        for forbidden in [
            "secrets.SIGNING_PRIVATE_KEY",
            "secrets.RELEASE_SIGNING_KEY",
            "workflow_run",
        ]:
            self.assertNotIn(forbidden, workflow)
        self.assertNotIn("workflow_dispatch", workflow)
        self.assertIn(
            'git merge-base --is-ancestor "$tag" refs/remotes/origin/master', workflow
        )
        self.assertIn("PLUGIN_SIGNING_PRIVATE_KEY", workflow)
        self.assertIn("plugin-assets/plugins.json", workflow)
        self.assertIn("release-state.json", workflow)
        self.assertIn("--latest=false", workflow)
        self.assertIn("plugin-catalog", workflow)
        self.assertNotIn("--repository ayangweb/", workflow)


if __name__ == "__main__":
    unittest.main()
