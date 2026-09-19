import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
NATIVE_WORKFLOW = WORKFLOW_DIRECTORY / "native-rewrite-phase0.yml"
RELEASE_WORKFLOW = WORKFLOW_DIRECTORY / "release.yml"
DENY = ROOT / "deny.toml"
DEPENDENCY_POLICY = ROOT / "tools" / "check-native-dependencies.sh"

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
