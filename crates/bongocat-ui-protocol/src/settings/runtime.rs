//! What the runtime is doing, and how a command to it failed.
//!
//! The runtime is a separate process-shaped service behind a channel, so a
//! command to it can fail in ways a command to the settings service cannot: the
//! transport can drop, the runtime can refuse, and neither is a settings error
//! the user can fix. Those are reported apart so the window can say which.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeHealth {
    Starting,
    Ready,
    Degraded,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRuntimeErrorCode {
    ModelLoadFailed,
    ModelEvaluationFailed,
    MotionLoadFailed,
    ExpressionLoadFailed,
    GpuPreparationFailed,
    TransportClosed,
    OverlaySettingsInvalid,
    MaximumFpsInvalid,
    ReleaseFallbackTimeoutInvalid,
    RandomBehaviorSettingsInvalid,
}

impl SettingsRuntimeErrorCode {
    pub const ALL: [Self; 10] = [
        Self::ModelLoadFailed,
        Self::ModelEvaluationFailed,
        Self::MotionLoadFailed,
        Self::ExpressionLoadFailed,
        Self::GpuPreparationFailed,
        Self::TransportClosed,
        Self::OverlaySettingsInvalid,
        Self::MaximumFpsInvalid,
        Self::ReleaseFallbackTimeoutInvalid,
        Self::RandomBehaviorSettingsInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLoadFailed => "model_load_failed",
            Self::ModelEvaluationFailed => "model_evaluation_failed",
            Self::MotionLoadFailed => "motion_load_failed",
            Self::ExpressionLoadFailed => "expression_load_failed",
            Self::GpuPreparationFailed => "gpu_preparation_failed",
            Self::TransportClosed => "transport_closed",
            Self::OverlaySettingsInvalid => "overlay_settings_invalid",
            Self::MaximumFpsInvalid => "maximum_fps_invalid",
            Self::ReleaseFallbackTimeoutInvalid => "release_fallback_timeout_invalid",
            Self::RandomBehaviorSettingsInvalid => "random_behavior_settings_invalid",
        }
    }
}

impl fmt::Display for SettingsRuntimeErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsRuntimeCommandFailure {
    pub sequence: u64,
    pub code: SettingsRuntimeErrorCode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsRuntimeCommandTransportDiagnostics {
    pub enqueued: u64,
    pub queue_full: u64,
    pub runtime_stopped: u64,
    pub sequence_gap_count: u64,
    pub missing_sequence_count: u64,
    pub duplicate_sequence_count: u64,
    pub out_of_order_sequence_count: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsRuntimeDiagnostics {
    pub render_error: Option<SettingsRuntimeErrorCode>,
    pub last_command_failure: Option<SettingsRuntimeCommandFailure>,
    pub command_transport: SettingsRuntimeCommandTransportDiagnostics,
    pub work_budget_exceeded: u64,
    pub last_over_budget_ms: u64,
    pub shutdown_timed_out: u64,
    pub shutdown_worker_panicked: u64,
}
