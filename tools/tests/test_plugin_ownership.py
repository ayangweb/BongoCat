"""Plugins own optional business capabilities, not application crates."""

import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class PluginOwnershipTests(unittest.TestCase):
    def test_every_plugin_only_depends_on_the_shared_sdk(self):
        manifests = sorted((ROOT / "plugins").glob("*/Cargo.toml"))
        self.assertTrue(manifests)
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
        ))
        packages = {Path(package["manifest_path"]): package for package in metadata["packages"]}
        for path in manifests:
            with self.subTest(plugin=path.parent.name):
                for dependency in packages[path]["dependencies"]:
                    name = dependency["name"]
                    if name.startswith("bongocat-"):
                        self.assertEqual(name, "bongocat-plugin-sdk")
                self.assertTrue((path.parent / "plugin.json").is_file())

    def test_plugin_source_files_do_not_exceed_one_thousand_lines(self):
        for path in (ROOT / "plugins").rglob("*"):
            if path.suffix in (".rs", ".py"):
                with self.subTest(path=path.relative_to(ROOT)):
                    self.assertLessEqual(len(path.read_text().splitlines()), 1000)

    def test_input_source_adapter_is_not_exposed_by_the_application(self):
        platform = ROOT / "crates" / "bongocat-platform" / "src"
        self.assertFalse((platform / "input_source.rs").exists())
        self.assertFalse((platform / "input_source").exists())
        self.assertNotIn("InputSourceMonitor", (platform / "lib.rs").read_text())


if __name__ == "__main__":
    unittest.main()
