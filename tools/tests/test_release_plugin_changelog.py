import importlib.util
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "plugin-release.yml"
SPEC = importlib.util.spec_from_file_location(
    "release_plugin_changelog", ROOT / "tools" / "release-plugin-changelog.py"
)
changelog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(changelog)


class ReleasePluginChangelogTests(unittest.TestCase):
    def test_current_shared_changelog_validates(self):
        english, chinese, _ = changelog.validate(ROOT)
        self.assertEqual(english[0]["version"], "1.0.0")
        self.assertEqual(english[0]["version"], chinese[0]["version"])
        self.assertIn("<!-- BONGOCAT_PLUGIN_CHANGELOG:START -->", changelog.render(ROOT))

    def test_merge_replaces_previous_plugin_block(self):
        notes = "<!-- BONGOCAT_PLUGIN_CHANGELOG:START -->\nnew\n<!-- BONGOCAT_PLUGIN_CHANGELOG:END -->\n"
        merged = changelog.merge("product\n\n" + notes + "\n", notes.replace("new", "updated"))
        self.assertEqual(merged.count("BONGOCAT_PLUGIN_CHANGELOG:START"), 1)
        self.assertIn("updated", merged)
        self.assertNotIn("\nnew\n", merged)

    def test_mismatched_language_headings_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "plugins").mkdir()
            (root / "plugins/one").mkdir()
            (root / "plugins/one/plugin.json").write_text(
                '{"metadata":{"id":"one","name":"One","version":"1.0.0","localizations":{"zh-CN":{"name":"一"}}}}',
                encoding="utf-8",
            )
            (root / "plugins/CHANGELOG.md").write_text(
                "# Plugin Changelog\n\n## 1.0.0\n\n### One\n\n#### ✨ Features\n\n- Added one.\n",
                encoding="utf-8",
            )
            (root / "plugins/CHANGELOG.zh-CN.md").write_text(
                "# 插件更新日志\n\n## 1.0.0\n\n### 错误名称\n\n#### ✨ 新功能\n\n- 新增一个功能。\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "must list current plugins"):
                changelog.validate(root)

    def test_plugin_sections_reuse_the_product_icon_vocabulary(self):
        self.assertIn("#### ✨ Features", (ROOT / "plugins/CHANGELOG.md").read_text(encoding="utf-8"))
        self.assertIn("🐛 Bug Fixes", changelog.SECTIONS["en"])
        self.assertIn("🐛 问题修复", changelog.SECTIONS["zh-CN"])

    def test_independent_workflow_publishes_the_rendered_notes(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("tools/release-plugin-changelog.py", workflow)
        self.assertIn("gh release edit", workflow)
        self.assertIn("--notes-file merged-release-body.md", workflow)


if __name__ == "__main__":
    unittest.main()
