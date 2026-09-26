//! What the input service can see, and whether it is allowed to.
//!
//! A gamepad, a pointer and a keyboard all report differently: some need
//! permission the platform has to grant, some are only available while a window
//! is focused, and some are simply not there. The window shows these to explain
//! why an input does nothing, so the codes are stable.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsInputDiagnostics {
    pub input_monitoring_permission: SettingsInputMonitoringPermission,
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
    pub fallback_release: u64,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsInputMonitoringPermission {
    #[default]
    Unsupported,
    Denied,
    Granted,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsDiagnosticsExportStatus {
    pub format_version: u32,
    pub bytes_written: u64,
    pub preview_bundle_format_version: u32,
    pub preview_bundle_bytes_written: u64,
    pub preview_bundle_entry_count: u32,
    pub preview_bundle_skipped_source_files: u64,
}
