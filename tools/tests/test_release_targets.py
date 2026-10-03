import re
import unittest
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIRECTORY = ROOT / ".github" / "workflows"
CI_WORKFLOW = WORKFLOW_DIRECTORY / "verify.yml"
RELEASE_WORKFLOW = WORKFLOW_DIRECTORY / "release.yml"
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

    def test_every_macos_release_leg_runs_on_a_runner_of_its_own_architecture(self):
        """A macOS leg may not build for an architecture its runner is not.

        The macOS input monitoring panel links a Swift static library whose
        build script compiles for the machine it runs on rather than the target
        being built, and it never treats macOS as a cross build. Both macOS legs
        on one runner therefore cannot both link: the leg that does not match
        the runner's architecture fails in the release job, on undefined
        `_permission_flow_*` symbols, with every other check green (ADR-0078,
        ADR-0033).

        Nothing in the workflow itself notices this, and the failure only shows
        up on a tagged run, so the architecture each runner label stands for is
        pinned here instead. GitHub's macOS images are published per
        architecture; the labels below are the ones the workflow relies on.
        """
        apple_runners = {
            "macos-latest": "arm64",
            "macos-26": "arm64",
            "macos-15": "arm64",
            # Xcode 27 image labels are Apple-silicon-only; Intel Macs never got
            # one because macOS 27 is Apple-silicon-only.
            "xcode-27": "arm64",
            "xcode-27-xlarge": "arm64",
            "macos-26-intel": "x86_64",
            "macos-26-large": "x86_64",
            "macos-15-intel": "x86_64",
            "macos-15-large": "x86_64",
        }
        job = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))["jobs"]["macos"]
        self.assertIn("matrix.runs_on", job["runs-on"], "each macOS leg declares its own runner")

        for leg in job["strategy"]["matrix"]["include"]:
            with self.subTest(leg=leg["label"]):
                runner = leg["runs_on"]
                self.assertIn(
                    runner,
                    apple_runners,
                    f"{runner} is not a documented macOS runner label, so its architecture "
                    "cannot be checked",
                )
                self.assertEqual(
                    apple_runners[runner],
                    "arm64" if leg["triple"].startswith("aarch64") else "x86_64",
                    f"the {leg['label']} leg builds {leg['triple']} on {runner}, whose "
                    "architecture does not match it",
                )

    def test_active_ci_still_builds_windows_x64(self):
        source = CI_WORKFLOW.read_text(encoding="utf-8")
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
