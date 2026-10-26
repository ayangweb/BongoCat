"""Regression tests for the local plugin build orchestration."""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "dev_plugins.py"
SPEC = importlib.util.spec_from_file_location("dev_plugins", SCRIPT)
DEV = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEV)


class DevPluginsTests(unittest.TestCase):
    def test_host_triples_are_derived_from_os_and_arch(self):
        self.assertEqual(DEV.host_triple_for("darwin", "arm64"), "aarch64-apple-darwin")
        self.assertEqual(DEV.host_triple_for("darwin", "x86_64"), "x86_64-apple-darwin")
        self.assertEqual(
            DEV.host_triple_for("win32", "AMD64"), "x86_64-pc-windows-msvc"
        )
        with self.assertRaises(DEV.DevPluginError):
            DEV.host_triple_for("linux", "x86_64")

    def test_pruning_removes_only_unreferenced_development_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output / "kept.zip").write_bytes(b"zip")
            (output / "kept.png").write_bytes(b"png")
            (output / "stale.zip").write_bytes(b"old")
            (output / "stale.png").write_bytes(b"old")
            (output / "unrelated.txt").write_text("keep")
            catalog = output / "catalog.json"
            catalog.write_text(
                json.dumps(
                    {
                        "plugins": [
                            {
                                "downloads": {"macos-aarch64": "kept.zip"},
                                "icon_url": str(output / "kept.png"),
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            DEV.prune_catalog_output(output, catalog)
            self.assertTrue((output / "kept.zip").is_file())
            self.assertTrue((output / "kept.png").is_file())
            self.assertTrue((output / "unrelated.txt").is_file())
            self.assertFalse((output / "stale.zip").exists())
            self.assertFalse((output / "stale.png").exists())

    def test_build_failure_is_reported_without_running_the_command(self):
        with mock.patch.object(
            DEV.subprocess,
            "run",
            side_effect=__import__("subprocess").CalledProcessError(1, "cargo"),
        ), mock.patch.object(DEV, "host_triple", return_value="aarch64-apple-darwin"), mock.patch.object(
            DEV, "CATALOG_DIRECTORY", Path(tempfile.mkdtemp())
        ):
            with self.assertRaises(DEV.DevPluginError):
                DEV.prepare_catalog()

    def test_catalog_build_uses_the_repository_packager(self):
        commands = []

        def run(command, **kwargs):
            commands.append((command, kwargs))
            catalog = DEV.CATALOG_DIRECTORY / "catalog.json"
            catalog.write_text('{"plugins": []}', encoding="utf-8")

        with tempfile.TemporaryDirectory() as directory, mock.patch.object(
            DEV, "CATALOG_DIRECTORY", Path(directory)
        ), mock.patch.object(
            DEV, "host_triple", return_value="aarch64-apple-darwin"
        ), mock.patch.object(DEV.subprocess, "run", side_effect=run):
            triple, catalog = DEV.prepare_catalog()
        self.assertEqual(triple, "aarch64-apple-darwin")
        command = commands[0][0]
        self.assertIn("bongocat-plugin-packaging", command)
        self.assertEqual(command[-1], str(Path(directory)))


if __name__ == "__main__":
    unittest.main()
