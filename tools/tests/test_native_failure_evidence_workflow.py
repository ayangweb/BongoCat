import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "native-rewrite-phase0.yml"


class NativeFailureEvidenceWorkflowTests(unittest.TestCase):
    def test_failure_artifacts_use_redactor_and_short_retention(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        collector_steps = source.count("Collect redacted Native failure evidence")
        upload_steps = source.count("Upload redacted Native failure evidence")
        self.assertGreaterEqual(collector_steps, 7)
        self.assertEqual(collector_steps, upload_steps)
        self.assertEqual(source.count("retention-days: 7"), upload_steps)
        self.assertNotIn("path: ${{ runner.temp }}/*.log", source)
        self.assertNotIn("path: ${{ runner.temp }}/**/*.log", source)
        self.assertEqual(
            source.count("collect-native-failure-evidence.py --input \"$RUNNER_TEMP\""),
            collector_steps,
        )

    def test_storage_smoke_exports_preview_on_both_native_platforms(self):
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
            macos_job.rfind("Collect redacted Native failure evidence"),
            macos_job.rfind("Smoke independent AppKit and Metal overlay"),
        )

    def test_windows_gpui_evidence_collection_runs_after_overlay_smokes(self):
        source = WORKFLOW.read_text(encoding="utf-8")
        windows_job = source[source.index("  windows-gpui-spikes:") :]
        self.assertGreater(
            windows_job.rfind("Collect redacted Native failure evidence"),
            windows_job.rfind("Smoke independent Win32 and D3D11 overlay"),
        )


if __name__ == "__main__":
    unittest.main()
