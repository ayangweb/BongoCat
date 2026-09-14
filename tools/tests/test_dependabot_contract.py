"""Keep the Dependabot directory list in step with the workspaces that exist.

`.github/dependabot.yml` is the only place that decides which Cargo workspaces
receive dependency updates, and nothing in the compiler or in CI notices when it
drifts from the filesystem. It already did: `214d579 refactor: promote native
workspace to root` moved the product workspace to the repository root and deleted
`native/Cargo.toml`, but left `/native` in the Dependabot configuration. As a
result the root workspace — every `crates/*` member — was never scanned, while
Dependabot was still pointed at a directory that no longer exists.

These tests fail loudly on that class of drift instead of silently dropping
workspaces out of dependency maintenance.
"""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CONFIGURATION = ROOT / ".github" / "dependabot.yml"
ROOT_ENTRY = "/"


def read(path):
    return path.read_text(encoding="utf-8")


def configured_directories():
    """The `directories` list of the single Cargo update entry."""
    lines = read(CONFIGURATION).splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "directories:":
            continue
        entries = []
        for following in lines[index + 1:]:
            stripped = following.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if not stripped.startswith("- "):
                break
            entries.append(stripped[2:].strip())
        return entries
    raise AssertionError("dependabot.yml has no `directories` key")


def workspace_directories():
    """Directories holding an independent Cargo workspace, in repository form."""
    manifests = [ROOT / "Cargo.toml", *sorted(ROOT.glob("*/*/Cargo.toml"))]
    found = []
    for manifest in manifests:
        if not manifest.is_file():
            continue
        if not re.search(r"^\[workspace\]", read(manifest), re.MULTILINE):
            continue
        relative = manifest.parent.relative_to(ROOT)
        found.append(ROOT_ENTRY if relative == Path(".") else f"/{relative.as_posix()}")
    return sorted(set(found))


class DependabotContractTests(unittest.TestCase):
    def test_the_configuration_has_one_cargo_update_entry(self):
        source = read(CONFIGURATION)
        self.assertIn("version: 2", source)
        self.assertEqual(source.count("package-ecosystem: cargo"), 1)
        self.assertIn("target-branch: next", source)

    def test_every_configured_directory_exists_and_holds_a_manifest(self):
        entries = configured_directories()
        self.assertTrue(entries, "the Cargo update entry must list directories")

        for entry in entries:
            with self.subTest(directory=entry):
                directory = ROOT if entry == ROOT_ENTRY else ROOT / entry.lstrip("/")
                self.assertTrue(
                    directory.is_dir(),
                    f"{entry} no longer exists, so Dependabot would never find a manifest there",
                )
                self.assertTrue(
                    (directory / "Cargo.toml").is_file(),
                    f"{entry} has no Cargo.toml, so Dependabot would never find a manifest there",
                )

    def test_every_independent_workspace_is_monitored(self):
        configured = set(configured_directories())
        discovered = set(workspace_directories())

        self.assertIn(ROOT_ENTRY, discovered, "the repository root must hold a workspace")
        self.assertEqual(
            sorted(discovered - configured),
            [],
            "these Cargo workspaces exist but would never receive dependency updates",
        )

    def test_entries_use_repository_relative_paths(self):
        for entry in configured_directories():
            with self.subTest(directory=entry):
                self.assertTrue(
                    entry.startswith("/"),
                    "Dependabot directory entries must be absolute repository paths",
                )
                self.assertEqual(entry.rstrip("/") or ROOT_ENTRY, entry)


if __name__ == "__main__":
    unittest.main()
