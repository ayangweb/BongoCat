import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "collect-native-failure-evidence.py"


class NativeFailureEvidenceTests(unittest.TestCase):
    def test_collects_only_bounded_redacted_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "runner-temp"
            output = Path(directory) / "evidence"
            source.mkdir()
            (source / "smoke.stdout.log").write_text(
                "renderer validation passed path=/Users/alice/private\n"
                "key_sequence=Ctrl+Alt+A clipboard=secret error=relative/user/model\n",
                encoding="utf-8",
            )
            (source / "arbitrary.json").write_text('{"private":"content"}', encoding="utf-8")
            (source / "ignored.bin").write_bytes(b"private")
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--input",
                    str(source),
                    "--output",
                    str(output),
                    "--platform",
                    "macOS",
                ],
                check=True,
            )
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["file_count"], 1)
            evidence = (output / "smoke.stdout.log").read_text(encoding="utf-8")
            self.assertNotIn("/Users/alice", evidence)
            self.assertNotIn("Ctrl+Alt+A", evidence)
            self.assertNotIn("secret", evidence)
            self.assertNotIn("relative/user/model", evidence)
            self.assertIn("<redacted>", evidence)
            self.assertNotIn(str(source), json.dumps(manifest))

    def test_skips_symlinks_and_non_validation_images(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "runner-temp"
            output = Path(directory) / "evidence"
            source.mkdir()
            (source / "renderer-screenshot.png").write_bytes(b"png")
            (source / "photo.png").write_bytes(b"private")
            (source / "linked.log").symlink_to(source / "renderer-screenshot.png")
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--input",
                    str(source),
                    "--output",
                    str(output),
                    "--platform",
                    "Windows",
                ],
                check=True,
            )
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["file_count"], 1)
            self.assertEqual(manifest["files"][0]["kind"], "image")

    def test_unknown_log_lines_are_replaced_with_a_placeholder(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "runner-temp"
            output = Path(directory) / "evidence"
            source.mkdir()
            (source / "smoke.log").write_text(
                "user model contents: private text\nstatus=failed\n", encoding="utf-8"
            )
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--input",
                    str(source),
                    "--output",
                    str(output),
                    "--platform",
                    "Linux",
                ],
                check=True,
            )
            evidence = (output / "smoke.log").read_text(encoding="utf-8")
            self.assertNotIn("private text", evidence)
            self.assertIn("<redacted-line>", evidence)
            self.assertIn("status=failed", evidence)

    def test_output_directory_inside_input_is_not_collected(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "runner-temp"
            output = source / "bongocat-failure-evidence"
            source.mkdir()
            (source / "smoke.log").write_text("status=failed\n", encoding="utf-8")
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--input",
                    str(source),
                    "--output",
                    str(output),
                    "--platform",
                    "Linux",
                ],
                check=True,
            )
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["file_count"], 1)
            self.assertEqual(manifest["files"][0]["name"], "smoke.log")


if __name__ == "__main__":
    unittest.main()
