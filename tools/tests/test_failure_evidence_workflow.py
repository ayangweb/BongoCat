import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "verify.yml"


class FailureEvidenceWorkflowTests(unittest.TestCase):
    def test_every_job_collects_redacted_failure_evidence(self):
        """Each job owns its own evidence, and the count follows the job list.

        A hard-coded total went stale the moment a job was split to run beside
        another one, and a stale total cannot tell a job that lost its evidence
        from one that never had any.
        """
        source = WORKFLOW.read_text(encoding="utf-8")
        jobs_block = source.split("\njobs:\n", 1)[1]
        jobs = re.findall(r"^  ([a-z][a-z0-9-]*):$", jobs_block, re.MULTILINE)
        self.assertGreater(len(jobs), 0)
        collector_steps = source.count("Collect redacted failure evidence")
        upload_steps = source.count("Upload redacted failure evidence")
        self.assertEqual(collector_steps, len(jobs))
        self.assertEqual(collector_steps, upload_steps)
        self.assertEqual(source.count("retention-days: 7"), upload_steps)
        self.assertNotIn("path: ${{ runner.temp }}/*.log", source)
        self.assertNotIn("path: ${{ runner.temp }}/**/*.log", source)
        self.assertEqual(
            source.count("collect-failure-evidence.py --input \"$RUNNER_TEMP\""),
            collector_steps,
        )
        for name in jobs:
            with self.subTest(job=name):
                body = source.split(f"  {name}:\n", 1)[1]
                following = re.search(r"^  [a-z][a-z0-9-]*:$", body, re.MULTILINE)
                job = body[: following.start()] if following else body
                self.assertIn("Collect redacted failure evidence", job)
                self.assertIn("Upload redacted failure evidence", job)

    def test_storage_smoke_exports_preview_on_both_supported_platforms(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        self.assertEqual(source.count("--diagnostics-export-smoke"), 2)
        self.assertIn("Smoke macOS diagnostics preview export", source)
        self.assertIn("Smoke Windows diagnostics preview export", source)
        self.assertEqual(
            source.count("diagnostics export completed with a private preview bundle"),
            2,
        )

    def test_macos_spike_evidence_collection_runs_after_all_smokes(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        macos_job = source[source.index("  macos-spikes:") : source.index("  windows-input-spike:")]
        self.assertGreater(
            macos_job.rfind("Collect redacted failure evidence"),
            macos_job.rfind("Smoke independent AppKit and Metal overlay"),
        )

    def test_windows_gpui_evidence_collection_runs_after_overlay_smokes(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        windows_job = source[source.index("  windows-gpui-spikes:") :]
        self.assertGreater(
            windows_job.rfind("Collect redacted failure evidence"),
            windows_job.rfind("Smoke independent Win32 and D3D11 overlay"),
        )


if __name__ == "__main__":
    unittest.main()
