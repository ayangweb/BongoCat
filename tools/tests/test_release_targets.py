import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
CI_WORKFLOW = WORKFLOW_DIRECTORY / "verify.yml"
RELEASE_WORKFLOW = WORKFLOW_DIRECTORY / "release.yml"
NIGHTLY_WORKFLOW = WORKFLOW_DIRECTORY / "nightly.yml"
DENY = ROOT / "deny.toml"
DEPENDENCY_POLICY = ROOT / "tools" / "check-dependencies.sh"

# The three shipped target/arch combinations. Windows ARM64 is not one of them:
# Cubism Native R5 has no desktop ARM64 Core, and Windows runs the x64 build
# under emulation. See docs/adr/0033-build-packaging-and-release-toolchain.md.
SHIPPED_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
)


class ReleaseTargetTests(unittest.TestCase):
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

    def test_active_ci_still_builds_windows_x64(self):
        source = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("x86_64-pc-windows-msvc", source)

    def test_nightly_workflow_covers_every_shipped_target(self):
        """A nightly that silently skipped a target would look like a green run.

        The release matrix and the nightly matrix are two lists written by hand in
        two files, so a target added to one and not the other produces a nightly
        that simply does not publish the platform a maintainer wanted to try.
        """
        source = NIGHTLY_WORKFLOW.read_text(encoding="utf-8")

        for target in SHIPPED_TARGETS:
            with self.subTest(target=target):
                self.assertIn(target, source)

    def test_nightly_publishes_nothing_the_updater_can_install(self):
        """The updater reads `latest.json` from the latest release, so a nightly
        that created one would move every installed copy onto an untagged build.
        The signature is the other half: an unsigned payload is refused outright,
        and signing a nightly would only hide that refusal behind a download link
        that appears to work.

        The assertions match how a secret reaches a build — the `secrets.`
        context — rather than the name of the variable, so a comment explaining
        why the key is withheld does not read as a leak.
        """
        source = NIGHTLY_WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("gh release", source)
        self.assertNotIn("--merge-manifests", source)
        self.assertNotIn("secrets.", source)
        # The artifacts are workflow artifacts, which expire, rather than a
        # release anyone can install from.
        self.assertIn("actions/upload-artifact@v4", source)
        self.assertIn("retention-days: 7", source)

    def test_dependency_policy_audits_exactly_the_shipped_targets(self):
        """The offline audit walks a target list of its own, outside `deny.toml`.

        It is the only place that enumerates release targets in shell, so nothing
        else would notice a target surviving there after the shipped matrix
        changes — an audit for a target the product does not build is dead work
        that hides which targets are actually covered.
        """
        source = DEPENDENCY_POLICY.read_text(encoding="utf-8")
        self.assertIn(
            "--package bongocat-app",
            source,
            "the release dependency audit must inspect the shipped application, not packaging tools",
        )
        policy = sorted(
            set(
                re.findall(
                    r"([a-z0-9_]+-[a-z0-9_-]+)",
                    re.search(
                        r"for target in(.*?); do",
                        source,
                        re.DOTALL,
                    ).group(1),
                )
            )
        )
        self.assertEqual(
            policy,
            sorted(SHIPPED_TARGETS),
            "the dependency policy must audit exactly the shipped release targets",
        )

        declared = sorted(
            set(
                re.findall(
                    r'"([a-z0-9_]+-[a-z0-9_-]+)"',
                    re.search(
                        r"\[graph\](.*?)\n\[",
                        DENY.read_text(encoding="utf-8"),
                        re.DOTALL,
                    ).group(1),
                )
            )
        )
        self.assertEqual(
            policy,
            declared,
            "the audited targets and deny.toml's dependency-policy targets must agree",
        )


if __name__ == "__main__":
    unittest.main()
