import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "fetch_release_assets", ROOT / "tools" / "fetch_release_assets.py"
)
fetch_release_assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fetch_release_assets)


class FetchReleaseAssetsTests(unittest.TestCase):
    def test_draft_and_published_release_queries_use_release_view(self):
        for draft in [False, True]:
            with self.subTest(draft=draft), tempfile.TemporaryDirectory() as directory:
                calls = []

                def run(arguments, **kwargs):
                    calls.append(arguments)
                    if arguments[:2] == ["gh", "release"]:
                        return subprocess.CompletedProcess(
                            arguments,
                            0,
                            stdout=json.dumps(
                                {
                                    "databaseId": 9,
                                    "tagName": "v1.0.0",
                                    "isDraft": draft,
                                }
                            ),
                            stderr="",
                        )
                    if arguments[2].endswith("/releases/9"):
                        return subprocess.CompletedProcess(
                            arguments,
                            0,
                            stdout=json.dumps(
                                [{"id": 7, "name": "plugins.json", "size": 2}]
                            ),
                            stderr="",
                        )
                    return subprocess.CompletedProcess(
                        arguments, 0, stdout=b"{}", stderr=b""
                    )

                fetch_release_assets.fetch(
                    "owner/repository",
                    "v1.0.0",
                    Path(directory),
                    draft=draft,
                    run=run,
                )
                self.assertEqual(
                    calls[0],
                    [
                        "gh",
                        "release",
                        "view",
                        "v1.0.0",
                        "--repo",
                        "owner/repository",
                        "--json",
                        "databaseId,tagName,isDraft",
                    ],
                )
                self.assertEqual(
                    calls[1],
                    [
                        "gh",
                        "api",
                        "repos/owner/repository/releases/9",
                        "--jq",
                        ".assets",
                    ],
                )
                self.assertEqual(
                    calls[2][:4],
                    ["gh", "api", "repos/owner/repository/releases/assets/7", "--header"],
                )

    def test_release_state_is_downloaded_with_selected_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)

            def run(arguments, **kwargs):
                if arguments[:2] == ["gh", "release"]:
                    return subprocess.CompletedProcess(
                        arguments,
                        0,
                        stdout=json.dumps(
                            {
                                "databaseId": 9,
                                "tagName": "v1.0.0",
                                "isDraft": False,
                            }
                        ),
                        stderr="",
                    )
                if arguments[2].endswith("/releases/9"):
                    return subprocess.CompletedProcess(
                        arguments,
                        0,
                        stdout=json.dumps(
                            [
                                {"id": 1, "name": "plugins.json", "size": 2},
                                {"id": 2, "name": "release-state.json", "size": 2},
                                {"id": 3, "name": "unrelated.zip", "size": 2},
                            ]
                        ),
                        stderr="",
                    )
                return subprocess.CompletedProcess(arguments, 0, stdout=b"{}", stderr=b"")

            fetch_release_assets.fetch(
                "owner/repository",
                "v1.0.0",
                output,
                names={"plugins.json"},
                run=run,
            )
            self.assertEqual(
                {path.name for path in output.iterdir()},
                {"plugins.json", "release-state.json"},
            )

    def test_wrong_release_kind_is_rejected(self):
        def run(arguments, **kwargs):
            if arguments[:2] == ["gh", "api"]:
                return subprocess.CompletedProcess(arguments, 0, stdout="[]", stderr="")
            return subprocess.CompletedProcess(
                arguments,
                0,
                stdout=json.dumps(
                    {"tagName": "v1.0.0", "isDraft": False, "assets": []}
                ),
                stderr="",
            )

        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "not a draft release"):
                fetch_release_assets.fetch(
                    "owner/repository",
                    "v1.0.0",
                    Path(directory),
                    draft=True,
                    run=run,
                )


if __name__ == "__main__":
    unittest.main()
