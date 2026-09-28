"""The documentation-only classification the verify pipeline filters on."""

import importlib.util
import os
import unittest
import unittest.mock
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "changed-paths.py"


def load():
    spec = importlib.util.spec_from_file_location("changed_paths", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DocumentationOnlyTests(unittest.TestCase):
    def setUp(self):
        self.changed_paths = load()

    def test_documents_alone_are_documentation(self):
        for path in (
            "README.md",
            "README.zh-CN.md",
            "AGENTS.md",
            "CHANGELOG.md",
            "CHANGELOG.zh-CN.md",
            "docs/adr/0074-verification-pipeline-cache-and-job-split.md",
            "docs/benchmark/data/macos-overlay-frame-timing-90e0aa7.csv",
            ".github/ISSUE_TEMPLATE/01-bug-report.yml",
            ".github/ISSUE_TEMPLATE/config.yml",
            "LICENSE",
            ".gitignore",
        ):
            with self.subTest(path=path):
                self.assertTrue(self.changed_paths.is_documentation(path))

    def test_a_change_set_of_only_documents_is_docs_only(self):
        self.assertTrue(
            self.changed_paths.docs_only(
                ["README.md", ".github/ISSUE_TEMPLATE/02-bug-report.zh-CN.yml"]
            )
        )

    def test_one_code_path_is_enough_to_run_everything(self):
        for code_path in (
            "crates/bongocat-app/src/main.rs",
            "crates/bongocat-app/Cargo.toml",
            "Cargo.lock",
            "Cargo.toml",
            "rust-toolchain.toml",
            "deny.toml",
            "justfile",
            "Justfile",
            "macos/Info.plist",
            "windows/bongocat-app.rc",
            "shared/fixtures/manifest.json",
            "crates/bongocat-i18n/locales/en-US/app.ftl",
            "spikes/gpui-settings/Cargo.toml",
            "tools/validate-locales.py",
            "resources/icons/logo-macos.icns",
            "vendor/cubism/Core.h",
        ):
            with self.subTest(path=code_path):
                self.assertFalse(self.changed_paths.is_documentation(code_path))
                self.assertFalse(self.changed_paths.docs_only([code_path]))
                self.assertFalse(
                    self.changed_paths.docs_only(["README.md", code_path])
                )

    def test_the_gate_configuration_is_never_treated_as_a_document(self):
        """A change to the pipeline has to be run by the pipeline it alters.

        `.github/workflows/` is where the jobs being filtered live, and
        `.github/dependabot.yml` is asserted on by a contract test, so neither may
        reach the documentation allowlist.
        """
        for path in (
            ".github/workflows/verify.yml",
            ".github/workflows/release.yml",
            ".github/dependabot.yml",
        ):
            with self.subTest(path=path):
                self.assertFalse(self.changed_paths.is_documentation(path))

    def test_an_unreadable_change_set_runs_the_whole_pipeline(self):
        """The failure direction is the expensive one, on purpose.

        An empty set is what a failed or shallow diff looks like, and treating it
        as documentation would turn a broken checkout into a green run.
        """
        self.assertFalse(self.changed_paths.docs_only([]))

    def test_the_diff_range_follows_the_event(self):
        diff_range = self.changed_paths.diff_range
        with unittest.mock.patch.dict(os.environ, {"GITHUB_BASE_REF": "master"}):
            self.assertEqual(
                diff_range("pull_request", None), "origin/master...HEAD"
            )
        self.assertEqual(diff_range("push", None), "HEAD^..HEAD")
        self.assertEqual(diff_range("push", "origin/master"), "origin/master...HEAD")

    def test_a_pull_request_without_its_base_ref_is_an_error(self):
        """Refusing is what keeps the caller from diffing against the wrong thing."""
        with unittest.mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(RuntimeError):
                self.changed_paths.diff_range("pull_request", None)


if __name__ == "__main__":
    unittest.main()
