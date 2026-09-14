import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
NATIVE_WORKFLOW = WORKFLOW_DIRECTORY / "native-rewrite-phase0.yml"
RELEASE_WORKFLOW = WORKFLOW_DIRECTORY / "release.yml"

# The three shipped target/arch combinations. Windows ARM64 is not one of them:
# Cubism Native R5 has no desktop ARM64 Core, and Windows runs the x64 build
# under emulation. See docs/adr/0033-build-packaging-and-release-toolchain.md.
SHIPPED_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
)


class NativeReleaseTargetMatrixTests(unittest.TestCase):
    def test_no_active_workflow_targets_unsupported_windows_architectures(self):
        for workflow in sorted(WORKFLOW_DIRECTORY.glob("*.y*ml")):
            with self.subTest(workflow=workflow.name):
                source = workflow.read_text(encoding="utf-8")
                self.assertNotIn("i686-pc-windows-msvc", source)
                self.assertNotIn("aarch64-pc-windows-msvc", source)

    def test_release_workflow_covers_every_shipped_target(self):
        source = RELEASE_WORKFLOW.read_text(encoding="utf-8")

        for target in SHIPPED_TARGETS:
            with self.subTest(target=target):
                self.assertIn(target, source)

    def test_active_native_ci_still_builds_windows_x64(self):
        source = NATIVE_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("x86_64-pc-windows-msvc", source)


if __name__ == "__main__":
    unittest.main()
