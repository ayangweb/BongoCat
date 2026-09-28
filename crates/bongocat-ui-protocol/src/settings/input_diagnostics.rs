//! What the input service can see, and whether it is allowed to.
//!
//! A gamepad, a pointer and a keyboard all report differently: some need
//! permission the platform has to grant, some are only available while a window
//! is focused, and some are simply not there. The window shows these to explain
//! why an input does nothing, so the codes are stable.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsInputDiagnostics {
    pub input_capability: SettingsInputCapability,
    pub service_status: SettingsInputServiceStatus,
    pub service_error_code: Option<&'static str>,
    pub service_start_attempts: u64,
    pub pressed_key_count: usize,
    pub pressed_mouse_button_count: usize,
    pub pressed_gamepad_button_count: usize,
    pub connected_gamepad_count: usize,
    pub platform_gamepad_backend_failures: u64,
    pub platform_gamepad_connection_rejections: u64,
    pub platform_gamepad_button_edges: u64,
    pub platform_gamepad_axis_samples: u64,
    pub platform_gamepad_axis_publish_rejections: u64,
    pub platform_gamepad_event_discards: u64,
    pub captured_down: u64,
    pub captured_up: u64,
    pub reconciled_release: u64,
    pub released_by_reset: u64,
    pub duplicate_down: u64,
    pub unmatched_release: u64,
    pub invalid_source: u64,
    pub reset_count: u64,
    pub sequence_gap_count: u64,
    pub missing_sequence_count: u64,
    pub duplicate_sequence_count: u64,
    pub out_of_order_sequence_count: u64,
    pub non_monotonic_time_count: u64,
    pub gamepad_connections: u64,
    pub gamepad_disconnections: u64,
    pub stale_gamepad_events: u64,
    pub released_by_disconnect: u64,
    pub transport_enqueued: u64,
    pub transport_queue_full: u64,
    pub transport_recovered_after_overflow: u64,
    pub transport_runtime_stopped: u64,
}

/// The platform capability global input needs, and whether this process has it.
///
/// Each platform gates input behind exactly one thing, and the two are not the
/// same kind of thing: macOS asks the user for the Input Monitoring TCC grant,
/// while Windows needs an elevated token to keep receiving Raw Input while a
/// higher-integrity window is in the foreground (ADR-0032). A single
/// granted/denied/unsupported enum can only name the macOS case — `unsupported`
/// on Windows says "this platform has no such concept" when the truth is "this
/// platform has a different one, and right now the process does not have it".
/// Naming the capability and saying whether it is present is true on both
/// platforms and actionable on both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsInputCapability {
    /// The platform's own stable name for it: `input_monitoring` on macOS,
    /// `administrator` on Windows. Anonymous by construction — a capability name
    /// is a property of the platform, never of the machine or the user.
    pub name: &'static str,
    /// Whether this process currently has the capability.
    pub available: bool,
}

/// The capability name a snapshot carries before the service has observed one.
///
/// A snapshot the settings service fills always carries the platform's real
/// capability name. This exists because [`SettingsInputDiagnostics`] derives
/// `Default` for its fixtures, and an unobserved capability is reported as
/// unavailable rather than assumed to be present.
const UNOBSERVED_INPUT_CAPABILITY: &str = "unknown";

impl SettingsInputCapability {
    /// A capability no platform has answered for yet.
    ///
    /// Named rather than left to `Default` so the settings snapshot clock can
    /// seed one inside a `const fn`, and so a reader of that constructor sees
    /// "not observed" instead of "default".
    pub const fn unobserved() -> Self {
        Self {
            name: UNOBSERVED_INPUT_CAPABILITY,
            available: false,
        }
    }
}

impl Default for SettingsInputCapability {
    fn default() -> Self {
        Self::unobserved()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsInputServiceStatus {
    #[default]
    NotStarted,
    Running,
    PermissionDenied,
    BackendUnavailable,
    Failed,
    Stopped,
}

impl SettingsInputServiceStatus {
    pub const ALL: [Self; 6] = [
        Self::NotStarted,
        Self::Running,
        Self::PermissionDenied,
        Self::BackendUnavailable,
        Self::Failed,
        Self::Stopped,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Running => "running",
            Self::PermissionDenied => "permission_denied",
            Self::BackendUnavailable => "backend_unavailable",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsDiagnosticsExportStatus {
    pub format_version: u32,
    pub bytes_written: u64,
    pub preview_bundle_format_version: u32,
    pub preview_bundle_bytes_written: u64,
    pub preview_bundle_entry_count: u32,
    pub preview_bundle_skipped_source_files: u64,
}
