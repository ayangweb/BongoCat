//! The anonymous diagnostics export.
//!
//! One command turns the current settings snapshot plus the application, Cubism
//! Core and update counters into a single private `diagnostics.json` document,
//! and the [`preview_bundle`] module packages those same bytes with a fixed set
//! of application log codes so one archive can be shared for support.
//!
//! The document is counts and stable codes only. Nothing here reads a
//! configuration value, a model title, a path or a log message body, because the
//! export leaves the machine: the privacy rule is enforced by what this module
//! chooses to serialize, and the preview bundle re-serializes log records from
//! the same closed code catalog for the same reason.

mod preview_bundle;

use crate::{ApplicationLogDiagnostics, CoreLogDiagnostics};
use bongocat_storage::{create_private_dir_all, write_private_atomic};
use bongocat_ui_protocol::{
    DIAGNOSTICS_EXPORT_FORMAT_VERSION, RuntimeHealth, SettingsDiagnosticsExportStatus,
    SettingsError, SettingsErrorCode, SettingsInputDiagnostics, SettingsInputServiceStatus,
    SettingsModelAvailability, SettingsModelOrigin, SettingsRuntimeErrorCode, SettingsSnapshot,
};
use bongocat_update::UpdateDiagnostics;
use preview_bundle::write_preview_bundle;
use serde::Serialize;
use std::path::Path;

/// Write the diagnostics document and its preview bundle into `path`'s directory.
///
/// The document is committed first and the bundle second, so a bundle failure
/// leaves a complete document behind rather than a half-written pair. Neither
/// write is allowed to damage the previous export: both replace their target
/// atomically and both refuse a destination that is not a regular file.
pub(super) fn export_diagnostics_file(
    path: &Path,
    snapshot: &SettingsSnapshot,
    application_logs: ApplicationLogDiagnostics,
    core_logs: Option<CoreLogDiagnostics>,
    update: Option<UpdateDiagnostics>,
) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
    let document = diagnostics_document(snapshot, application_logs, core_logs, update);
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_private_dir_all(parent)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    write_private_atomic(path, &bytes)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    let preview_bundle = write_preview_bundle(parent, &bytes)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    Ok(SettingsDiagnosticsExportStatus {
        format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
        bytes_written: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        preview_bundle_format_version: preview_bundle.format_version,
        preview_bundle_bytes_written: preview_bundle.bytes_written,
        preview_bundle_entry_count: preview_bundle.entry_count,
        preview_bundle_skipped_source_files: preview_bundle.skipped_source_files,
    })
}

/// The stable code the report and the application log share for a runtime state.
///
/// The settings snapshot observer writes the same spelling into its
/// "input status changed" record, so the two never disagree about what a state
/// is called.
pub(crate) const fn runtime_health_code(health: RuntimeHealth) -> &'static str {
    match health {
        RuntimeHealth::Starting => "starting",
        RuntimeHealth::Ready => "ready",
        RuntimeHealth::Degraded => "degraded",
        RuntimeHealth::Stopped => "stopped",
    }
}

/// The stable code the report and the application log share for an input service
/// state. See [`runtime_health_code`] for why both callers share one spelling.
pub(crate) const fn input_service_status_code(status: SettingsInputServiceStatus) -> &'static str {
    match status {
        SettingsInputServiceStatus::NotStarted => "not_started",
        SettingsInputServiceStatus::Running => "running",
        SettingsInputServiceStatus::PermissionDenied => "permission_denied",
        SettingsInputServiceStatus::BackendUnavailable => "backend_unavailable",
        SettingsInputServiceStatus::Failed => "failed",
        SettingsInputServiceStatus::Stopped => "stopped",
    }
}

fn diagnostics_document(
    snapshot: &SettingsSnapshot,
    application_logs: ApplicationLogDiagnostics,
    core_logs: Option<CoreLogDiagnostics>,
    update: Option<UpdateDiagnostics>,
) -> DiagnosticsExportDocument {
    let input = snapshot.input_diagnostics;
    let runtime = snapshot.runtime_diagnostics;
    let mut invalid_diagnostic_codes = Vec::new();
    let mut ready_preset = 0_u64;
    let mut ready_installed = 0_u64;
    let mut invalid_preset = 0_u64;
    let mut invalid_installed = 0_u64;
    for entry in &snapshot.model_catalog.entries {
        let is_preset = entry.origin == SettingsModelOrigin::BuiltIn;
        match &entry.availability {
            SettingsModelAvailability::Ready { .. } => {
                if is_preset {
                    ready_preset = ready_preset.saturating_add(1);
                } else {
                    ready_installed = ready_installed.saturating_add(1);
                }
            }
            SettingsModelAvailability::Invalid { diagnostic } => {
                if is_preset {
                    invalid_preset = invalid_preset.saturating_add(1);
                } else {
                    invalid_installed = invalid_installed.saturating_add(1);
                }
                if let Some(existing) = invalid_diagnostic_codes
                    .iter_mut()
                    .find(|entry: &&mut DiagnosticsCodeCount| entry.code == diagnostic.as_str())
                {
                    existing.count = existing.count.saturating_add(1);
                } else {
                    invalid_diagnostic_codes.push(DiagnosticsCodeCount {
                        code: diagnostic.as_str(),
                        count: 1,
                    });
                }
            }
        }
    }
    invalid_diagnostic_codes.sort_unstable_by(|left, right| left.code.cmp(right.code));
    let core_retained_files = core_logs.map_or(0, |logs| logs.retained_files);
    let core_retained_bytes = core_logs.map_or(0, |logs| logs.retained_bytes);
    DiagnosticsExportDocument {
        format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
        settings_revision: snapshot.revision,
        config_revision: snapshot.config_revision,
        runtime_health: runtime_health_code(snapshot.runtime_health),
        runtime: DiagnosticsRuntime {
            render_error_code: runtime.render_error.map(SettingsRuntimeErrorCode::as_str),
            last_command_failure_code: runtime
                .last_command_failure
                .map(|failure| failure.code.as_str()),
            last_command_failure_sequence: runtime
                .last_command_failure
                .map(|failure| failure.sequence),
            command_enqueued: runtime.command_transport.enqueued,
            command_queue_full: runtime.command_transport.queue_full,
            command_runtime_stopped: runtime.command_transport.runtime_stopped,
            command_sequence_gap_count: runtime.command_transport.sequence_gap_count,
            command_missing_sequence_count: runtime.command_transport.missing_sequence_count,
            command_duplicate_sequence_count: runtime.command_transport.duplicate_sequence_count,
            command_out_of_order_sequence_count: runtime
                .command_transport
                .out_of_order_sequence_count,
            work_budget_exceeded: runtime.work_budget_exceeded,
            last_over_budget_ms: runtime.last_over_budget_ms,
            shutdown_timed_out: runtime.shutdown_timed_out,
            shutdown_worker_panicked: runtime.shutdown_worker_panicked,
        },
        input: diagnostics_input(input),
        configuration: diagnostics_configuration(),
        models: DiagnosticsModels {
            catalog_available: snapshot.model_catalog.error.is_none(),
            ready_preset,
            ready_installed,
            invalid_preset,
            invalid_installed,
            invalid_diagnostic_codes,
            active_model_origin: snapshot
                .active_model
                .as_ref()
                .map(|model| match model.origin {
                    SettingsModelOrigin::BuiltIn => "preset",
                    SettingsModelOrigin::Imported => "installed",
                }),
        },
        application_logs: DiagnosticsApplicationLogs {
            written: application_logs.written,
            dropped: application_logs.dropped,
            rotated: application_logs.rotated,
            pruned: application_logs.pruned,
            bytes: application_logs.bytes,
            retained_files: application_logs.retained_files,
            events: DiagnosticsApplicationLogEvents {
                started: application_logs.events.started,
                previous_run_unclean: application_logs.events.previous_run_unclean,
                shutdown_started: application_logs.events.shutdown_started,
                shutdown_completed: application_logs.events.shutdown_completed,
                shutdown_failed: application_logs.events.shutdown_failed,
                panicked: application_logs.events.panicked,
                runtime_unavailable: application_logs.events.runtime_unavailable,
                diagnostics_export_failed: application_logs.events.diagnostics_export_failed,
                model_selection_fallback: application_logs.events.model_selection_fallback,
            },
        },
        core_logs: core_logs.map(|core_logs| DiagnosticsCoreLogs {
            written: core_logs.written,
            dropped: core_logs.dropped,
            rotated: core_logs.rotated,
            pruned: core_logs.pruned,
            bytes: core_logs.bytes,
            retained_files: core_logs.retained_files,
            retained_bytes: core_logs.retained_bytes,
        }),
        update: update.map(|update| {
            let update = update.sanitized();
            DiagnosticsUpdate {
                last_error_code: update.last_error_code,
                checks_started: update.checks_started,
                checks_succeeded: update.checks_succeeded,
                checks_failed: update.checks_failed,
                downloads_started: update.downloads_started,
                downloads_succeeded: update.downloads_succeeded,
                downloads_failed: update.downloads_failed,
                installs_started: update.installs_started,
                installs_succeeded: update.installs_succeeded,
                installs_failed: update.installs_failed,
            }
        }),
        log_retention: DiagnosticsLogRetention {
            retained_files: application_logs
                .retained_files
                .saturating_add(core_retained_files),
            retained_bytes: application_logs.bytes.saturating_add(core_retained_bytes),
        },
    }
}

fn diagnostics_input(input: SettingsInputDiagnostics) -> DiagnosticsInput {
    DiagnosticsInput {
        service_status: input_service_status_code(input.service_status),
        service_error_code: input.service_error_code,
        service_start_attempts: input.service_start_attempts,
        pressed_key_count: input.pressed_key_count,
        pressed_mouse_button_count: input.pressed_mouse_button_count,
        pressed_gamepad_button_count: input.pressed_gamepad_button_count,
        connected_gamepad_count: input.connected_gamepad_count,
        platform_gamepad_backend_failures: input.platform_gamepad_backend_failures,
        platform_gamepad_connection_rejections: input.platform_gamepad_connection_rejections,
        platform_gamepad_button_edges: input.platform_gamepad_button_edges,
        platform_gamepad_axis_samples: input.platform_gamepad_axis_samples,
        platform_gamepad_axis_publish_rejections: input.platform_gamepad_axis_publish_rejections,
        platform_gamepad_event_discards: input.platform_gamepad_event_discards,
        captured_down: input.captured_down,
        captured_up: input.captured_up,
        reconciled_release: input.reconciled_release,
        fallback_release: input.fallback_release,
        released_by_reset: input.released_by_reset,
        duplicate_down: input.duplicate_down,
        unmatched_release: input.unmatched_release,
        invalid_source: input.invalid_source,
        reset_count: input.reset_count,
        sequence_gap_count: input.sequence_gap_count,
        missing_sequence_count: input.missing_sequence_count,
        duplicate_sequence_count: input.duplicate_sequence_count,
        out_of_order_sequence_count: input.out_of_order_sequence_count,
        non_monotonic_time_count: input.non_monotonic_time_count,
        gamepad_connections: input.gamepad_connections,
        gamepad_disconnections: input.gamepad_disconnections,
        stale_gamepad_events: input.stale_gamepad_events,
        released_by_disconnect: input.released_by_disconnect,
        transport_enqueued: input.transport_enqueued,
        transport_queue_full: input.transport_queue_full,
        transport_recovered_after_overflow: input.transport_recovered_after_overflow,
        transport_runtime_stopped: input.transport_runtime_stopped,
    }
}

/// Whether the configuration itself could be read.
///
/// A failed load has no typed error code of its own yet, so a report that
/// reached this point always answers `ready`; the counts that matter for a
/// corrupt configuration arrive as the runtime's own degraded state instead.
const fn diagnostics_configuration() -> DiagnosticsConfiguration {
    DiagnosticsConfiguration { status: "ready" }
}

#[derive(Serialize)]
struct DiagnosticsExportDocument {
    format_version: u32,
    settings_revision: u64,
    config_revision: Option<u64>,
    runtime_health: &'static str,
    runtime: DiagnosticsRuntime,
    input: DiagnosticsInput,
    configuration: DiagnosticsConfiguration,
    models: DiagnosticsModels,
    application_logs: DiagnosticsApplicationLogs,
    core_logs: Option<DiagnosticsCoreLogs>,
    update: Option<DiagnosticsUpdate>,
    log_retention: DiagnosticsLogRetention,
}

#[derive(Serialize)]
struct DiagnosticsRuntime {
    render_error_code: Option<&'static str>,
    last_command_failure_code: Option<&'static str>,
    last_command_failure_sequence: Option<u64>,
    command_enqueued: u64,
    command_queue_full: u64,
    command_runtime_stopped: u64,
    command_sequence_gap_count: u64,
    command_missing_sequence_count: u64,
    command_duplicate_sequence_count: u64,
    command_out_of_order_sequence_count: u64,
    work_budget_exceeded: u64,
    last_over_budget_ms: u64,
    shutdown_timed_out: u64,
    shutdown_worker_panicked: u64,
}

#[derive(Serialize)]
struct DiagnosticsInput {
    service_status: &'static str,
    service_error_code: Option<&'static str>,
    service_start_attempts: u64,
    pressed_key_count: usize,
    pressed_mouse_button_count: usize,
    pressed_gamepad_button_count: usize,
    connected_gamepad_count: usize,
    platform_gamepad_backend_failures: u64,
    platform_gamepad_connection_rejections: u64,
    platform_gamepad_button_edges: u64,
    platform_gamepad_axis_samples: u64,
    platform_gamepad_axis_publish_rejections: u64,
    platform_gamepad_event_discards: u64,
    captured_down: u64,
    captured_up: u64,
    reconciled_release: u64,
    fallback_release: u64,
    released_by_reset: u64,
    duplicate_down: u64,
    unmatched_release: u64,
    invalid_source: u64,
    reset_count: u64,
    sequence_gap_count: u64,
    missing_sequence_count: u64,
    duplicate_sequence_count: u64,
    out_of_order_sequence_count: u64,
    non_monotonic_time_count: u64,
    gamepad_connections: u64,
    gamepad_disconnections: u64,
    stale_gamepad_events: u64,
    released_by_disconnect: u64,
    transport_enqueued: u64,
    transport_queue_full: u64,
    transport_recovered_after_overflow: u64,
    transport_runtime_stopped: u64,
}

#[derive(Serialize)]
struct DiagnosticsConfiguration {
    status: &'static str,
}

#[derive(Serialize)]
struct DiagnosticsModels {
    catalog_available: bool,
    ready_preset: u64,
    ready_installed: u64,
    invalid_preset: u64,
    invalid_installed: u64,
    invalid_diagnostic_codes: Vec<DiagnosticsCodeCount>,
    active_model_origin: Option<&'static str>,
}

#[derive(Serialize)]
struct DiagnosticsCodeCount {
    code: &'static str,
    count: u64,
}

#[derive(Serialize)]
struct DiagnosticsApplicationLogs {
    written: u64,
    dropped: u64,
    rotated: u64,
    pruned: u64,
    bytes: u64,
    retained_files: u64,
    events: DiagnosticsApplicationLogEvents,
}

#[derive(Serialize)]
struct DiagnosticsApplicationLogEvents {
    started: u64,
    previous_run_unclean: u64,
    shutdown_started: u64,
    shutdown_completed: u64,
    shutdown_failed: u64,
    panicked: u64,
    runtime_unavailable: u64,
    diagnostics_export_failed: u64,
    model_selection_fallback: u64,
}

#[derive(Serialize)]
struct DiagnosticsCoreLogs {
    written: u64,
    dropped: u64,
    rotated: u64,
    pruned: u64,
    bytes: u64,
    retained_files: u64,
    retained_bytes: u64,
}

#[derive(Serialize)]
struct DiagnosticsUpdate {
    last_error_code: Option<&'static str>,
    checks_started: u64,
    checks_succeeded: u64,
    checks_failed: u64,
    downloads_started: u64,
    downloads_succeeded: u64,
    downloads_failed: u64,
    installs_started: u64,
    installs_succeeded: u64,
    installs_failed: u64,
}

#[derive(Serialize)]
struct DiagnosticsLogRetention {
    retained_files: u64,
    retained_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApplicationLogEventCounts, PRODUCT_VERSION};
    use bongocat_ui_protocol::SettingsGamepadAutoSwitch;
    use bongocat_ui_protocol::{
        SettingsBuildEnvironment, SettingsBuildInfo, SettingsGamepadAxisSettings,
        SettingsInputMonitoringPermission, SettingsLanguage, SettingsLogging,
        SettingsModelBehavior, SettingsModelCatalog, SettingsModelDiagnostic, SettingsModelEntry,
        SettingsModelKey, SettingsModelMode, SettingsModelSettings, SettingsOverlay,
        SettingsRandomBehavior, SettingsRuntimeCommandFailure,
        SettingsRuntimeCommandTransportDiagnostics, SettingsRuntimeDiagnostics, SettingsShortcuts,
        SettingsStartupItemState, SettingsStartupItemStatus, SettingsTheme,
    };
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn diagnostics_export_is_atomic_aggregated_and_path_free() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("diagnostics directory");
        let path = directory.path().join("logs").join("diagnostics.json");
        let snapshot = SettingsSnapshot {
            revision: 42,
            config_revision: Some(7),
            build_info: SettingsBuildInfo {
                product_version: PRODUCT_VERSION.to_owned(),
                environment: SettingsBuildEnvironment::Development,
            },
            runtime_health: RuntimeHealth::Degraded,
            runtime_diagnostics: SettingsRuntimeDiagnostics {
                render_error: Some(SettingsRuntimeErrorCode::GpuPreparationFailed),
                last_command_failure: Some(SettingsRuntimeCommandFailure {
                    sequence: 9,
                    code: SettingsRuntimeErrorCode::TransportClosed,
                }),
                command_transport: SettingsRuntimeCommandTransportDiagnostics {
                    enqueued: 11,
                    queue_full: 2,
                    runtime_stopped: 1,
                    sequence_gap_count: 3,
                    missing_sequence_count: 5,
                    duplicate_sequence_count: 4,
                    out_of_order_sequence_count: 6,
                },
                work_budget_exceeded: 12,
                last_over_budget_ms: 34,
                shutdown_timed_out: 2,
                shutdown_worker_panicked: 1,
            },
            appearance_theme: SettingsTheme::System,
            language: SettingsLanguage::System,
            resolved_language: SettingsLanguage::EnglishUnitedStates,
            status_icon_visible: true,
            taskbar_icon_visible: true,
            check_for_updates_automatically: false,
            check_for_updates_interval_hours: 24,
            overlay_visible: true,
            overlay: SettingsOverlay::default(),
            motion_audio_enabled: true,
            command_shortcuts_enabled: true,
            behavior_shortcuts_enabled: true,
            maximum_fps: 60,
            release_fallback_timeout_ms: 500,
            random_behavior: SettingsRandomBehavior::default(),
            model_settings: SettingsModelSettings::default(),
            gamepad_axis_settings: SettingsGamepadAxisSettings::default(),
            gamepad_auto_switch: SettingsGamepadAutoSwitch::default(),
            logging: SettingsLogging::default(),
            shortcuts: SettingsShortcuts::default(),
            startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            diagnostics_export: None,
            input_diagnostics: SettingsInputDiagnostics {
                service_status: SettingsInputServiceStatus::PermissionDenied,
                service_error_code: Some("platform_input_permission_denied"),
                captured_down: 3,
                captured_up: 4,
                reconciled_release: 5,
                released_by_reset: 6,
                duplicate_down: 7,
                transport_queue_full: 8,
                transport_recovered_after_overflow: 9,
                input_monitoring_permission: SettingsInputMonitoringPermission::Unsupported,
                ..SettingsInputDiagnostics::default()
            },
            active_model: Some(SettingsModelKey {
                id: "private-model-name".to_owned(),
                origin: SettingsModelOrigin::Imported,
            }),
            model_catalog: SettingsModelCatalog {
                entries: vec![
                    SettingsModelEntry {
                        id: "private-model-name".to_owned(),
                        title: "我的猫".to_owned(),
                        input_mode: Some(SettingsModelMode::Keyboard),
                        origin: SettingsModelOrigin::Imported,
                        availability: SettingsModelAvailability::Ready {
                            behaviors: Vec::<SettingsModelBehavior>::new(),
                        },
                        // The exported document is counts and codes only, so
                        // these page-facing paths must not reach it.
                        directory: Some(PathBuf::from("/private/secret/model")),
                        cover: Some(PathBuf::from("/private/secret/model/resources/cover.png")),
                    },
                    SettingsModelEntry {
                        id: "broken-private-model".to_owned(),
                        title: "broken-private-model".to_owned(),
                        input_mode: None,
                        origin: SettingsModelOrigin::Imported,
                        availability: SettingsModelAvailability::Invalid {
                            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
                        },
                        directory: None,
                        cover: None,
                    },
                ],
                error: None,
            },
        };

        let status = export_diagnostics_file(
            &path,
            &snapshot,
            ApplicationLogDiagnostics {
                written: 3,
                dropped: 1,
                rotated: 2,
                pruned: 4,
                bytes: 128,
                retained_files: 2,
                events: ApplicationLogEventCounts {
                    started: 1,
                    panicked: 2,
                    ..ApplicationLogEventCounts::default()
                },
            },
            Some(CoreLogDiagnostics {
                written: 5,
                dropped: 6,
                rotated: 7,
                pruned: 8,
                bytes: 256,
                retained_files: 3,
                retained_bytes: 384,
            }),
            Some(UpdateDiagnostics {
                last_error_code: Some("update_download_transport_failed"),
                checks_started: 3,
                checks_succeeded: 2,
                checks_failed: 1,
                downloads_started: 2,
                downloads_succeeded: 1,
                downloads_failed: 1,
                installs_started: 1,
                installs_succeeded: 0,
                installs_failed: 1,
            }),
        )
        .expect("export diagnostics");
        let bytes = std::fs::read(&path).expect("read exported diagnostics");
        let preview_path = directory
            .path()
            .join("logs")
            .join("diagnostics-preview.zip");
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&path)
                .expect("diagnostics metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(status.format_version, DIAGNOSTICS_EXPORT_FORMAT_VERSION);
        assert_eq!(status.bytes_written, bytes.len() as u64);
        assert!(preview_path.is_file());
        assert_eq!(status.preview_bundle_format_version, 1);
        assert_eq!(
            status.preview_bundle_bytes_written,
            std::fs::metadata(&preview_path)
                .expect("preview metadata")
                .len()
        );
        assert_eq!(status.preview_bundle_entry_count, 3);
        assert_eq!(status.preview_bundle_skipped_source_files, 0);
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(document["format_version"], 1);
        assert_eq!(document["settings_revision"], 42);
        assert_eq!(document["configuration"]["status"], "ready");
        assert_eq!(
            document["runtime"]["render_error_code"],
            "gpu_preparation_failed"
        );
        assert_eq!(document["runtime"]["command_enqueued"], 11);
        assert_eq!(document["runtime"]["command_queue_full"], 2);
        assert_eq!(document["runtime"]["command_sequence_gap_count"], 3);
        assert_eq!(
            document["update"]["last_error_code"],
            "update_download_transport_failed"
        );
        assert_eq!(document["update"]["downloads_failed"], 1);
        assert_eq!(document["runtime"]["work_budget_exceeded"], 12);
        assert_eq!(document["runtime"]["last_over_budget_ms"], 34);
        assert_eq!(document["runtime"]["shutdown_timed_out"], 2);
        assert_eq!(document["runtime"]["shutdown_worker_panicked"], 1);
        assert_eq!(document["runtime"]["command_missing_sequence_count"], 5);
        assert_eq!(document["input"]["captured_down"], 3);
        assert_eq!(document["input"]["service_status"], "permission_denied");
        assert_eq!(
            document["input"]["service_error_code"],
            "platform_input_permission_denied"
        );
        assert_eq!(document["input"]["captured_up"], 4);
        assert_eq!(document["input"]["reconciled_release"], 5);
        assert_eq!(document["input"]["released_by_reset"], 6);
        assert_eq!(document["input"]["duplicate_down"], 7);
        assert_eq!(document["input"]["transport_queue_full"], 8);
        assert_eq!(document["input"]["transport_recovered_after_overflow"], 9);
        assert_eq!(document["models"]["ready_installed"], 1);
        assert_eq!(document["application_logs"]["written"], 3);
        assert_eq!(document["application_logs"]["dropped"], 1);
        assert_eq!(document["application_logs"]["events"]["started"], 1);
        assert_eq!(document["application_logs"]["events"]["panicked"], 2);
        assert_eq!(document["core_logs"]["written"], 5);
        assert_eq!(document["core_logs"]["dropped"], 6);
        assert_eq!(document["core_logs"]["retained_files"], 3);
        assert_eq!(document["core_logs"]["retained_bytes"], 384);
        assert_eq!(document["log_retention"]["retained_files"], 5);
        assert_eq!(document["log_retention"]["retained_bytes"], 512);
        assert_eq!(document["models"]["invalid_installed"], 1);
        assert_eq!(
            document["models"]["invalid_diagnostic_codes"][0]["code"],
            "model_json_invalid"
        );
        let text = String::from_utf8(bytes).expect("UTF-8 export");
        assert!(!text.contains("private-model-name"));
        assert!(!text.contains(directory.path().to_string_lossy().as_ref()));

        let filtered_path = directory.path().join("logs").join("filtered.json");
        export_diagnostics_file(
            &filtered_path,
            &snapshot,
            ApplicationLogDiagnostics::default(),
            None,
            Some(UpdateDiagnostics {
                last_error_code: Some("private_update_detail"),
                checks_started: 4,
                ..UpdateDiagnostics::default()
            }),
        )
        .expect("export filtered diagnostics");
        let filtered: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&filtered_path).expect("read filtered diagnostics"),
        )
        .expect("valid filtered JSON");
        assert!(filtered["update"]["last_error_code"].is_null());
        assert_eq!(filtered["update"]["checks_started"], 4);
    }
}
