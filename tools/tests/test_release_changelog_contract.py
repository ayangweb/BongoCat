"""Pin how the release pipeline turns the changelogs into the published release notes.

`CHANGELOG.md` and `CHANGELOG.zh-CN.md` are the authored record of what changed, so the
release notes are read out of them rather than generated from the commits between two
tags. The composed document is then used twice: it is the GitHub release body and it is
the `notes` field of the shared `latest.json` the update window renders. Because both
consumers read one file, the release page and the client cannot show different text.

The extraction itself belongs to `crates/bongocat-packaging`, which covers the Markdown
walking in its own unit tests. What cannot be checked from inside that crate is the
wiring — that the workflow composes the notes at all, that the two consumers read the
same file, and that the bilingual changelogs stay in step — so it is pinned here.
"""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
PACKAGER = ROOT / "crates" / "bongocat-packaging" / "src" / "main.rs"
CHANGELOG = ROOT / "CHANGELOG.md"
CHANGELOG_ZH = ROOT / "CHANGELOG.zh-CN.md"


def read(path):
    return path.read_text(encoding="utf-8")


def rust_string_constant(source, name):
    match = re.search(rf'const {re.escape(name)}: &str = "([^"]*)";', source)
    if match is None:
        raise AssertionError(f"{name} is not declared as a string constant")
    return match.group(1)


def level_two_heading(line):
    """The text of a second-level ATX heading, or None.

    A mirror of the packaging tool's rule: `###` opens a section inside an entry, and
    `##x` is not a heading at all because ATX requires a space or the end of the line.
    """
    rest = line.lstrip(" ")
    if not rest.startswith("##"):
        return None
    rest = rest[2:]
    if rest.startswith("#"):
        return None
    if rest and rest[0] not in " \t":
        return None
    return rest.strip()


def fence_marker(line):
    """The marker a line opens or closes a fenced code block with, or None."""
    stripped = line.lstrip(" ")
    if len(line) - len(stripped) > 3:
        return None
    for marker in ("```", "~~~"):
        if stripped.startswith(marker):
            return marker[0]
    return None


def changelog_entries(path):
    """Each documented version and its body, in the order the file lists them.

    A `##` heading opens an entry and the next one closes it; a heading inside a fenced
    code block is an example rather than an entry. This is the same walk the packaging
    tool performs, so a changelog that satisfies these tests is one it can read.
    """
    lines = read(path).splitlines()

    openings = []
    fence = None
    for index, line in enumerate(lines):
        marker = fence_marker(line)
        if marker is not None:
            if fence is None:
                fence = marker
            elif fence == marker:
                fence = None
            continue
        if fence is None and level_two_heading(line) is not None:
            openings.append(index)

    entries = []
    for position, index in enumerate(openings):
        end = openings[position + 1] if position + 1 < len(openings) else len(lines)
        heading = level_two_heading(lines[index])
        # Keep a Changelog spells an entry `## [<version>] - <date>`; the version is the
        # heading's own first token either way.
        entries.append((heading.split()[0].strip("[]"), "\n".join(lines[index + 1 : end]).strip()))
    return entries


class ReleaseNotesContractTests(unittest.TestCase):
    def test_the_notes_come_from_the_changelogs_and_not_from_github(self):
        workflow = read(WORKFLOW)

        self.assertIn(
            "just release-notes release-notes.md",
            workflow,
            "the release must compose its notes from the changelogs",
        )
        self.assertNotIn(
            "releases/generate-notes",
            workflow,
            "GitHub's generated notes summarise the commits between two tags, which is "
            "not the changelog this project publishes; the release page and the in-app "
            "update window have to show the authored entry",
        )

    def test_the_release_body_and_the_manifest_read_one_notes_file(self):
        workflow = read(WORKFLOW)

        composed = re.search(r"just release-notes (\S+)", workflow)
        self.assertIsNotNone(composed, "the workflow must compose the release notes")
        notes = composed.group(1)

        merged = re.search(r"just release-manifest (\S+) (\S+)", workflow)
        self.assertIsNotNone(merged, "the workflow must merge the manifest fragments")
        self.assertEqual(
            merged.group(2),
            notes,
            "the shared manifest must announce the notes the release was composed from",
        )

        body = re.search(r"--notes-file (\S+)", workflow)
        self.assertIsNotNone(body, "the release body must come from a file")
        self.assertEqual(
            body.group(1),
            notes,
            "the release page and the update window must render the same document",
        )

    def test_the_notes_are_composed_before_they_are_consumed(self):
        workflow = read(WORKFLOW)

        composed = workflow.index("just release-notes")
        self.assertLess(
            composed,
            workflow.index("just release-manifest"),
            "the merge reads the notes file, so it has to exist first",
        )
        self.assertLess(
            composed,
            workflow.index("actions/download-artifact"),
            "a tag whose changelog entry is missing is a mistake in the tag, and it has "
            "to fail before the release artifacts are downloaded",
        )

    def test_the_packaging_tool_reads_the_changelogs_the_repository_has(self):
        source = read(PACKAGER)

        for constant, expected in (
            ("RELEASE_CHANGELOG_NAME", "CHANGELOG.md"),
            ("RELEASE_CHANGELOG_ZH_NAME", "CHANGELOG.zh-CN.md"),
        ):
            with self.subTest(constant=constant):
                declared = rust_string_constant(source, constant)
                self.assertEqual(
                    declared,
                    expected,
                    "the release reads these files, so the tool has to name them",
                )
                self.assertTrue(
                    (ROOT / declared).is_file(),
                    f"{constant} names {declared}, which does not exist",
                )

    def test_the_bilingual_changelogs_document_the_same_releases(self):
        english = changelog_entries(CHANGELOG)
        chinese = changelog_entries(CHANGELOG_ZH)

        self.assertTrue(english, "the changelog must document at least one release")
        versions = [version for version, _ in english]
        self.assertEqual(
            len(versions),
            len(set(versions)),
            f"a version must be documented once: {versions}",
        )
        self.assertEqual(
            versions,
            [version for version, _ in chinese],
            "a release whose entry was written in one language only would publish notes "
            "that describe it once in a language the reader may not have",
        )

    def test_every_documented_release_has_notes(self):
        for path in (CHANGELOG, CHANGELOG_ZH):
            for version, body in changelog_entries(path):
                with self.subTest(changelog=path.name, version=version):
                    self.assertTrue(
                        body,
                        f"{path.name} documents {version} with an empty body, and the "
                        "release reads that body as its notes",
                    )


if __name__ == "__main__":
    unittest.main()
