import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "record-native-provenance.py"


class NativeProvenanceTests(unittest.TestCase):
    def test_writes_path_free_reproducible_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "provenance.json"
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--workspace",
                    str(ROOT / "native"),
                    "--output",
                    str(output),
                    "--target",
                    "test-target",
                    "--profile",
                    "release",
                    "--features",
                    "default",
                    "--environment",
                    "development",
                ],
                cwd=ROOT,
                check=True,
            )
            metadata = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(metadata["schema_version"], 1)
        self.assertRegex(metadata["source_commit"], r"^[0-9a-f]{40}$")
        self.assertRegex(metadata["cargo_lock_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(metadata["target"], "test-target")
        self.assertEqual(metadata["profile"], "release")
        self.assertEqual(metadata["features"], "default")
        self.assertEqual(metadata["build_environment"], "development")
        self.assertIn("rustc", metadata["rust_toolchain"])
        self.assertIn("commit_hash", metadata["rust_toolchain"])
        self.assertNotIn(str(ROOT), json.dumps(metadata))


if __name__ == "__main__":
    unittest.main()
