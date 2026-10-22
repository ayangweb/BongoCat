import json
import tempfile
import unittest
from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "release_plugin_history", ROOT / "tools" / "release-plugin-history.py"
)
history = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(history)


class FakeResponse:
    def __init__(self, value):
        self.stdout = json.dumps(value).encode()


class ReleasePluginHistoryTests(unittest.TestCase):
    def test_fetches_only_catalogs_and_rejects_existing_tag(self):
        calls = []
        releases = [
            {
                "tag_name": "v2.3.1",
                "draft": False,
                "prerelease": False,
                "assets": [{"name": "plugins.json", "id": 7, "size": 2}],
            },
            {"tag_name": "v2.3.0", "draft": True, "prerelease": False, "assets": []},
        ]

        def run(args, **kwargs):
            calls.append(args)
            if args[2].endswith("/releases"):
                return FakeResponse([releases])
            return type("Binary", (), {"stdout": b"{}"})()

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "history"
            history.fetch_history("ayangweb/BongoCat", "v2.4.0", output, run)
            self.assertEqual((output / "7.json").read_bytes(), b"{}")
            self.assertEqual(len(calls), 2)
            self.assertIn("releases/assets/7", calls[1][2])

        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                history.fetch_history("ayangweb/BongoCat", "v2.3.1", Path(directory) / "history", run)

        with tempfile.TemporaryDirectory() as directory:
            history.fetch_history(
                "ayangweb/BongoCat",
                "v2.3.1",
                Path(directory) / "history",
                run,
                allow_existing_tag=True,
            )

    def test_does_not_create_history_on_api_failure(self):
        def fail(*args, **kwargs):
            raise RuntimeError("API unavailable")

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "history"
            with self.assertRaises(RuntimeError):
                history.fetch_history("ayangweb/BongoCat", "v2.4.0", output, fail)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
