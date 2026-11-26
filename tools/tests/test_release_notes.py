import tempfile
import unittest
from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "release_notes", ROOT / "tools" / "release_notes.py"
)
notes = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notes)


class ReleaseNotesTests(unittest.TestCase):
    def test_current_manifests_have_bilingual_entries(self):
        names = notes.validate(ROOT)
        self.assertEqual(
            set(names),
            {
                "com.bongocat.input-source",
                "com.bongocat.key-sounds",
                "com.bongocat.pomodoro",
            },
        )

    def test_render_uses_changelog_entries_english_then_chinese(self):
        rendered = notes.render(ROOT, "v1.0.0")
        self.assertTrue(rendered.startswith("## Changelog\n\n### Input Source\n"))
        self.assertIn(
            "#### ✨ Features\n\n"
            "- Added the Input Source plugin, showing the current input source in real time.",
            rendered,
        )
        self.assertIn("\n\n---\n\n## 更新日志\n\n### 输入法状态\n", rendered)
        self.assertIn(
            "#### ✨ 新功能\n\n"
            "- 新增输入法状态插件，实时显示当前使用的输入法。",
            rendered,
        )
        self.assertNotIn("Published independently", rendered)
        self.assertNotIn("`com.bongocat.input-source`", rendered)
        self.assertNotIn("latest.json", rendered)
        self.assertEqual(rendered, notes.render(ROOT, "v1.0.0"))

    def test_missing_release_version_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "no release notes for 9.9.9"):
            notes.render(ROOT, "v9.9.9")

    def test_plugin_changelogs_live_beside_plugin_sources(self):
        self.assertTrue((ROOT / "plugins" / "CHANGELOG.md").is_file())
        self.assertTrue((ROOT / "plugins" / "CHANGELOG.zh-CN.md").is_file())
        self.assertTrue((ROOT / "CHANGELOG.md").is_file())
        self.assertTrue((ROOT / "CHANGELOG.zh-CN.md").is_file())

    def test_unknown_plugin_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "plugins").mkdir()
            with self.assertRaisesRegex(ValueError, "no plugin manifests"):
                notes.validate(root)


if __name__ == "__main__":
    unittest.main()
