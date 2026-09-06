import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RELEASE_WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"


class NativeReleaseTargetMatrixTests(unittest.TestCase):
    def test_windows_matrix_excludes_i686_and_keeps_only_supported_architectures(self):
        source = RELEASE_WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("i686-pc-windows-msvc", source)
        self.assertEqual(source.count("target: x86_64-pc-windows-msvc"), 1)
        self.assertEqual(source.count("target: aarch64-pc-windows-msvc"), 1)


if __name__ == "__main__":
    unittest.main()
