"""Regression tests for the local plugin checkout orchestration."""

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

    def test_failed_clone_does_not_leave_a_half_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            checkout = Path(directory) / "BongoCat-Plugins"
            checkout.mkdir()
            (checkout / "partial").write_text("partial")
            with mock.patch.object(DEV, "DEV_ROOT", Path(directory)), mock.patch.object(
                DEV, "DEFAULT_CHECKOUT", checkout
            ), mock.patch.object(DEV.subprocess, "run", side_effect=__import__("subprocess").CalledProcessError(1, "git")):
                with self.assertRaises(DEV.DevPluginError):
                    DEV.checkout_pinned_revision("0" * 40, clone=True)
            self.assertFalse(checkout.exists())


if __name__ == "__main__":
    unittest.main()
