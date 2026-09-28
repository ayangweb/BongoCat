"""The documentation filter in `verify.yml`, and what it is allowed to skip.

A skipped job reports as *skipped*, which branch protection counts as a pass, so
the filter's failure mode is silent: too much skips, and a change nobody
verified merges behind a green run. These tests pin the two halves of that —
which jobs may skip, and that the filter can only ever prove a change
irrelevant.
"""

import importlib.util
import unittest
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "verify.yml"


def load_changed_paths():
    spec = importlib.util.spec_from_file_location(
        "changed_paths", ROOT / "tools" / "changed-paths.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


changed_paths = load_changed_paths()

GATE = "needs.filter-paths.outputs.docs-only"
FILTER_JOB = "filter-paths"

# The two jobs that always run.
#   `filter-paths` produces the answer the others read, so gating it on itself
#     would leave every dependent without an input.
#   `fixtures` runs the contract tests, which read `verify.yml` and `release.yml`
#     themselves, both changelogs, `justfile`, `macos/Info.plist`, `deny.toml`,
#     `.github/dependabot.yml` and `docs/product-runtime.md` — so a run that skips
#     it would skip the check that its own filter is still honest.
ALWAYS_RUN = {FILTER_JOB, "fixtures"}


def jobs():
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))["jobs"]


class DocumentationFilterTests(unittest.TestCase):
    def test_every_job_has_made_a_deliberate_skip_decision(self):
        """No job may be added without deciding whether it can skip.

        A job with no `if` runs on a documentation change; a job filtered on
        something other than this gate is filtering on the wrong input. Either
        one is a decision someone has to make out loud, so the test makes it.
        """
        for name, job in jobs().items():
            with self.subTest(job=name):
                condition = job.get("if")
                if name in ALWAYS_RUN:
                    self.assertIsNone(condition, f"{name} must always run")
                    continue
                self.assertIsNotNone(
                    condition, f"{name} needs an if: so docs-only runs can skip it"
                )
                self.assertIn(GATE, condition)
                self.assertIn("filter-paths", job.get("needs") or [])

    def test_the_filter_job_publishes_the_output_the_others_read(self):
        workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        outputs = workflow["jobs"][FILTER_JOB]["outputs"]
        self.assertIn("docs-only", outputs)
        self.assertIn("steps.classify.outputs.docs-only", outputs["docs-only"])

    def test_the_workflow_always_runs_so_a_skipped_job_still_reports(self):
        """The trap this design exists to avoid.

        A workflow-level `paths:` that matches nothing runs no workflow at all:
        no job reports, and a pull request with required checks waits forever for
        a status nobody sends. Job-level `if:` reports *skipped* instead, which
        satisfies the requirement.
        """
        triggers = WORKFLOW.read_text(encoding="utf-8").split("\njobs:\n", 1)[0]
        for forbidden in ("paths:", "paths-ignore:"):
            self.assertNotIn(
                forbidden, triggers, f"workflow-level {forbidden} silences the pipeline"
            )
        # The parsed trigger, not the file text: the comment recording why the
        # other triggers were dropped has to be able to name them.
        events = set(yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))[True])
        self.assertEqual(
            events,
            {"pull_request"},
            "a `push` or `schedule` trigger was measured and dropped; putting one "
            "back is a deliberate decision",
        )

    def test_the_filter_keeps_its_own_inputs_out_of_the_documentation_set(self):
        """Changing the pipeline must be verified by the pipeline.

        Asserted against the allowlist constants rather than the file text, so
        the comment explaining *why* these paths are excluded can keep naming
        them.
        """
        for forbidden in (".github/workflows", "dependabot", "tools/", "crates/"):
            for entry in changed_paths.DOCUMENTATION_PREFIXES:
                self.assertFalse(
                    forbidden in entry,
                    f"{forbidden} must stay out of DOCUMENTATION_PREFIXES",
                )
            self.assertNotIn(forbidden, changed_paths.DOCUMENTATION_NAMES)

    def test_no_job_is_left_reachable_only_through_a_manual_trigger(self):
        """Every gated job keeps a trigger that reaches it on a real change."""
        triggers = WORKFLOW.read_text(encoding="utf-8").split("\njobs:\n", 1)[0]
        for name, job in jobs().items():
            with self.subTest(job=name):
                self.assertTrue(job.get("runs-on"), f"{name} has no runner")
                self.assertIn("pull_request:", triggers)


if __name__ == "__main__":
    unittest.main()
