use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformInputDiagnostics {
    pub captured_edges: u64,
    pub queued_edges: u64,
    pub consumed_edges: u64,
    pub unmapped_keys: u64,
    pub unsupported_buttons: u64,
    pub callback_panics: u64,
    pub capture_queue_overflows: u64,
    pub capture_queue_discarded: u64,
    pub runtime_queue_overflows: u64,
    pub recovery_resets: u64,
    pub reconciliation_runs: u64,
    pub reconciled_releases: u64,
    pub decode_errors: u64,
    pub tap_restarts: u64,
    pub rejected_after_stop: u64,
    pub cursor_captured: u64,
    pub cursor_coalesced: u64,
    pub cursor_consumed: u64,
    pub cursor_display_lookup_failures: u64,
    pub cursor_publish_rejections: u64,
    pub cursor_rejected_after_stop: u64,
    pub gamepad_polls: u64,
    pub gamepad_backend_unavailable: u64,
    pub gamepad_query_errors: u64,
    pub gamepad_connections: u64,
    pub gamepad_disconnections: u64,
    pub gamepad_button_edges: u64,
    pub gamepad_axis_samples: u64,
    pub gamepad_axis_publish_rejections: u64,
    pub gamepad_unsupported_profiles: u64,
    pub gamepad_rejected_after_stop: u64,
    pub gamepad_invalid_values: u64,
    pub gamepad_background_monitoring_enabled: bool,
    pub gamepad_background_monitoring_restored: bool,
    pub clean_shutdown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformInputDiagnosticsPublishError {
    RuntimeStopped,
}

impl std::fmt::Display for PlatformInputDiagnosticsPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("runtime stopped before platform input diagnostics were published")
    }
}

impl std::error::Error for PlatformInputDiagnosticsPublishError {}

#[derive(Default)]
struct PlatformInputDiagnosticsState {
    value: PlatformInputDiagnostics,
    stopped: bool,
}

#[derive(Clone, Default)]
pub struct PlatformInputDiagnosticsProducer {
    state: Arc<Mutex<PlatformInputDiagnosticsState>>,
}

impl PlatformInputDiagnosticsProducer {
    pub fn publish(
        &self,
        diagnostics: PlatformInputDiagnostics,
    ) -> Result<(), PlatformInputDiagnosticsPublishError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return Err(PlatformInputDiagnosticsPublishError::RuntimeStopped);
        }
        state.value = diagnostics;
        Ok(())
    }

    pub fn diagnostics(&self) -> PlatformInputDiagnostics {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .value
    }

    pub(crate) fn stop(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped = true;
    }
}
