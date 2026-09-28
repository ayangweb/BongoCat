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
CONVENTIONS = ROOT / "docs" / "changelog-conventions.md"

# The section vocabulary both changelogs draw their headings from, keyed by emoji.
#
# The release publishes one document composed of both languages, so a section has to
# be nameable in the two of them. Fixing the pair per emoji is what keeps that
# possible: the emoji is the section's identity, the two labels are its fixed wording,
# and a new section is added here rather than invented in one language and translated
# by hand in the other. The order is the conventional one, so a release that has
# nothing to say under a heading simply leaves it out.
#
# This table is the authority for the wording. `docs/changelog-conventions.md` explains
# why the rule exists and points back here, so the rule is enforced in one place
# instead of being maintained twice.
SECTION_VOCABULARY = (
    ("⚠", "Upgrade Notice", "升级说明"),
    ("✨", "Features", "新功能"),
    ("🐛", "Bug Fixes", "问题修复"),
    ("⚡", "Performance", "性能优化"),
    ("🔐", "Security", "安全"),
    ("⬆", "Dependencies", "依赖更新"),
    ("🗑", "Removals", "移除"),
    ("🎨", "Interface", "界面"),
    ("🌍", "Localization", "本地化"),
    ("⚙", "Configuration", "配置"),
    ("🧪", "Testing", "测试"),
    ("♻", "Refactoring", "重构"),
    ("🔧", "Maintenance", "维护"),
    ("📝", "Documentation", "文档"),
    ("⏪", "Reverts", "回退"),
    ("💻", "Support Changes", "支持范围变化"),
)

VARIATION_SELECTOR_16 = "️"

# `docs/localization-copy-conventions.md` keeps the emoji out of this document's
# scope, so the vocabulary is defined and enforced here.


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


def level_three_headings(path):
    """The `###` section headings of a changelog, as `(emoji, label)` pairs.

    Only sections inside a documented release are collected, so the vocabulary is not
    applied to prose that merely looks like a heading.
    """
    lines = read(path).splitlines()
    openings = []
    fence = None
    for index, line in enumerate(lines):
        marker = fence_marker(line)
        if marker is not None:
            fence = None if fence == marker else (marker if fence is None else fence)
            continue
        if fence is None and level_two_heading(line) is not None:
            openings.append(index)

    sections = []
    in_entry = False
    for index, line in enumerate(lines):
        if index in openings:
            in_entry = True
            continue
        if not in_entry:
            continue
        stripped = line.lstrip(" ")
        if not stripped.startswith("### ") and stripped.rstrip() != "###":
            continue
        text = stripped[3:].strip()
        emoji, _, label = text.partition(" ")
        sections.append((emoji, label.strip()))
    return sections


def without_variation_selector(text):
    """`text` with U+FE0F removed.

    Several of the vocabulary's emoji are written with a text-presentation selector
    and several are not, and which one a given editor produces is not something a
    changelog author should have to care about: the selector changes how the glyph is
    drawn, not which section it names. Comparing without it keeps the contract from
    failing on a spelling the reader cannot see.
    """
    return text.replace(VARIATION_SELECTOR_16, "")


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

    def test_the_changelogs_do_not_carry_the_generated_download_block(self):
        """The wrapper heading and the download block are generated, not authored.

        `bongocat-packaging` composes them from the artifact names, the download URL
        shape and the version it already holds, and wraps each language's entry in them
        before appending the block. Pasted into a changelog they would be published
        twice per release, and the second copy would be the one nothing keeps correct.

        The tap is in the list because it is the one link in the block that is not
        derived from this repository at all: it is a separate repository whose cask
        names this project's release assets, so a copy written into a changelog would
        be a third place to keep in step with the other two.

        The heading checks do not collide with the `# Changelog` and `# 更新日志`
        titles the two changelog files legitimately open with.
        """
        generated = (
            "## Changelog",
            "## 更新日志",
            "## Downloads",
            "## 下载地址",
            "## More models",
            "## 更多模型",
            "## Sponsors",
            "## 赞助商",
            "Homebrew-BongoCat",
        )

        for path in (CHANGELOG, CHANGELOG_ZH):
            text = read(path)
            for fragment in generated:
                with self.subTest(changelog=path.name, fragment=fragment):
                    self.assertNotIn(
                        fragment,
                        text,
                        f"{path.name} carries '{fragment}', which the release generates; "
                        "authoring it here would publish it twice and leave the stale "
                        "copy as the one a reader cannot fix",
                    )

        # The generated block's own wiring: the tool has to hold both languages' copy,
        # the sponsor list, the gallery and the tap, or the section silently loses part
        # of its content.
        source = read(PACKAGER)
        for constant in (
            "APPENDIX_ENGLISH",
            "APPENDIX_CHINESE",
            "RELEASE_SPONSORS",
            "MODELS_GALLERY_URL",
            "HOMEBREW_TAP_URL",
        ):
            with self.subTest(constant=constant):
                self.assertRegex(
                    source,
                    rf"(const|static) {constant}\b",
                    f"the generated block needs {constant}",
                )


class ChangelogSectionVocabularyTests(unittest.TestCase):
    """The bilingual section headings both changelogs are allowed to use."""

    def test_the_english_headings_are_from_the_vocabulary(self):
        known = {emoji: english for emoji, english, _ in SECTION_VOCABULARY}

        for emoji, label in level_three_headings(CHANGELOG):
            with self.subTest(heading=label):
                self.assertIn(
                    without_variation_selector(emoji),
                    known,
                    f"'{emoji} {label}' is not a section of this changelog; add it to "
                    "SECTION_VOCABULARY with its Chinese label rather than inventing one",
                )
                self.assertEqual(
                    label,
                    known[without_variation_selector(emoji)],
                    f"'{emoji}' is spelled '{label}'; the vocabulary fixes it as "
                    f"'{known[without_variation_selector(emoji)]}'",
                )

    def test_the_chinese_headings_are_from_the_vocabulary(self):
        known = {emoji: chinese for emoji, _, chinese in SECTION_VOCABULARY}

        for emoji, label in level_three_headings(CHANGELOG_ZH):
            with self.subTest(heading=label):
                self.assertIn(
                    without_variation_selector(emoji),
                    known,
                    f"'{emoji} {label}' is not a section of this changelog; add it to "
                    "SECTION_VOCABULARY with its English label rather than inventing one",
                )
                self.assertEqual(
                    label,
                    known[without_variation_selector(emoji)],
                    f"'{emoji}' is spelled '{label}'; the vocabulary fixes it as "
                    f"'{known[without_variation_selector(emoji)]}'",
                )

    def test_both_changelogs_carry_the_same_sections_in_the_same_order(self):
        # The release joins the two entries into one document, so a section present in
        # only one language would reach a reader in a language they may not read.
        self.assertEqual(
            [without_variation_selector(emoji) for emoji, _ in level_three_headings(CHANGELOG)],
            [without_variation_selector(emoji) for emoji, _ in level_three_headings(CHANGELOG_ZH)],
            "the two changelogs document different sections, so the published notes "
            "would describe one release twice in a language the reader may not have",
        )

    def test_the_vocabulary_does_not_repeat_a_section(self):
        emojis = [without_variation_selector(emoji) for emoji, _, _ in SECTION_VOCABULARY]
        self.assertEqual(
            len(emojis),
            len(set(emojis)),
            f"a section is listed twice: {emojis}",
        )

    def test_the_documented_vocabulary_is_the_enforced_one(self):
        # The conventions document shows the same table so a reader does not have to read
        # this test to learn the wording. That only stays true while the two agree, so
        # the document is checked against the table rather than trusted.
        documented = []
        after_header = False
        for line in read(CONVENTIONS).splitlines():
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) != 3 or not cells[0]:
                continue
            # The `| --- |` rule ends the header, so the rows before it are column
            # labels rather than sections and must not be read as entries.
            if set(cells[0]) <= {"-", ":"} and not after_header:
                after_header = True
                continue
            if not after_header:
                continue
            documented.append(tuple(without_variation_selector(cell) for cell in cells))

        expected = [
            tuple(without_variation_selector(cell) for cell in row)
            for row in SECTION_VOCABULARY
        ]
        self.assertEqual(
            documented,
            expected,
            f"{CONVENTIONS.name} documents a vocabulary the tests do not enforce; the "
            "document and SECTION_VOCABULARY have to describe the same sections",
        )


if __name__ == "__main__":
    unittest.main()
