import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
NATIVE_WORKFLOW = WORKFLOW_DIRECTORY / "native-rewrite-phase0.yml"


class NativeReleaseTargetMatrixTests(unittest.TestCase):
    def test_no_active_workflow_targets_unsupported_windows_x86(self):
        for workflow in sorted(WORKFLOW_DIRECTORY.glob("*.y*ml")):
            with self.subTest(workflow=workflow.name):
                source = workflow.read_text(encoding="utf-8")
                self.assertNotIn("i686-pc-windows-msvc", source)

    def test_active_native_ci_covers_supported_windows_architectures(self):
        source = NATIVE_WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("x86_64-pc-windows-msvc", source)
        self.assertIn("aarch64-pc-windows-msvc", source)


if __name__ == "__main__":
    unittest.main()
