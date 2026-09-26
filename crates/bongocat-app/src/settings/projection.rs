//! Projecting application state onto the settings protocol.
//!
//! The settings window only knows the protocol's own vocabulary, so every value it
//! displays is built here from the configuration, the runtime snapshot and the
//! input service. Keeping the projection in one place is what lets a page never
//! name a configuration or runtime type.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

pub(super) const fn settings_theme(theme: bongocat_config::Theme) -> SettingsTheme {
    match theme {
        bongocat_config::Theme::System => SettingsTheme::System,
        bongocat_config::Theme::Light => SettingsTheme::Light,
        bongocat_config::Theme::Dark => SettingsTheme::Dark,
    }
}

pub(super) const fn config_theme(theme: SettingsTheme) -> bongocat_config::Theme {
    match theme {
        SettingsTheme::System => bongocat_config::Theme::System,
        SettingsTheme::Light => bongocat_config::Theme::Light,
        SettingsTheme::Dark => bongocat_config::Theme::Dark,
    }
}

pub(super) const fn settings_language(language: bongocat_config::Language) -> SettingsLanguage {
    match language {
        bongocat_config::Language::System => SettingsLanguage::System,
        bongocat_config::Language::ChineseSimplified => SettingsLanguage::ChineseSimplified,
        bongocat_config::Language::EnglishUnitedStates => SettingsLanguage::EnglishUnitedStates,
    }
}

pub(super) const fn config_language(language: SettingsLanguage) -> bongocat_config::Language {
    match language {
        SettingsLanguage::System => bongocat_config::Language::System,
        SettingsLanguage::ChineseSimplified => bongocat_config::Language::ChineseSimplified,
        SettingsLanguage::EnglishUnitedStates => bongocat_config::Language::EnglishUnitedStates,
    }
}

pub(super) fn settings_shortcuts(config: &NativeConfig) -> SettingsShortcuts {
    SettingsShortcuts {
        commands: config
            .shortcuts
            .command_bindings
            .iter()
            .map(|binding| SettingsShortcutBinding {
                command: binding.command.clone(),
                shortcut: binding.shortcut.clone(),
            })
            .collect(),
        model_behaviors: config
            .shortcuts
            .model_behavior_bindings
            .iter()
            .map(|binding| SettingsModelBehaviorBinding {
                model: SettingsModelKey {
                    id: binding.model.id.clone(),
                    origin: settings_origin_from_config(binding.model.source),
                },
                behavior_id: binding.behavior_id.clone(),
                shortcut: binding.shortcut.clone(),
            })
            .collect(),
    }
}

pub(super) fn settings_shortcut(command: ShortcutCommand) -> Option<SettingsApplicationShortcut> {
    Some(match command {
        ShortcutCommand::ToggleOverlay => SettingsApplicationShortcut::ToggleOverlay,
        ShortcutCommand::ToggleMirror => SettingsApplicationShortcut::ToggleMirror,
        ShortcutCommand::ToggleIgnoreMouseInput => {
            SettingsApplicationShortcut::ToggleIgnoreMouseInput
        }
        ShortcutCommand::ToggleIgnoreKeyboardInput => {
            SettingsApplicationShortcut::ToggleIgnoreKeyboardInput
        }
        ShortcutCommand::ToggleIgnoreGamepadInput => {
            SettingsApplicationShortcut::ToggleIgnoreGamepadInput
        }
        ShortcutCommand::ToggleClickThrough => SettingsApplicationShortcut::ToggleClickThrough,
        ShortcutCommand::ToggleAlwaysOnTop => SettingsApplicationShortcut::ToggleAlwaysOnTop,
        ShortcutCommand::OpenSettings => SettingsApplicationShortcut::OpenSettings,
    })
}

pub(super) fn apply_application_shortcut(
    application: &mut Application,
    command: SettingsApplicationShortcut,
) -> Result<(), ApplicationError> {
    match command {
        SettingsApplicationShortcut::OpenSettings => return Ok(()),
        SettingsApplicationShortcut::ToggleOverlay => {
            let visible = application.runtime_client().snapshot().overlay_visible;
            application.set_overlay_visible(!visible)?;
        }
        SettingsApplicationShortcut::ToggleMirror => {
            let settings = application.runtime_client().snapshot().model_settings;
            application.set_model_settings(ModelSettings {
                mirror: !settings.mirror,
                ..settings
            })?;
        }
        SettingsApplicationShortcut::ToggleIgnoreMouseInput => {
            let settings = application.runtime_client().snapshot().model_settings;
            application.set_model_settings(ModelSettings {
                ignore_pointer: !settings.ignore_pointer,
                ..settings
            })?;
        }
        SettingsApplicationShortcut::ToggleIgnoreKeyboardInput => {
            let settings = application.runtime_client().snapshot().model_settings;
            application.set_model_settings(ModelSettings {
                ignore_keyboard: !settings.ignore_keyboard,
                ..settings
            })?;
        }
        SettingsApplicationShortcut::ToggleIgnoreGamepadInput => {
            let settings = application.runtime_client().snapshot().model_settings;
            application.set_model_settings(ModelSettings {
                ignore_gamepad: !settings.ignore_gamepad,
                ..settings
            })?;
        }
        SettingsApplicationShortcut::ToggleClickThrough => {
            let current = application.runtime_client().snapshot().overlay_settings;
            application.set_overlay_settings(OverlaySettings {
                click_through: !current.click_through,
                ..current
            })?;
        }
        SettingsApplicationShortcut::ToggleAlwaysOnTop => {
            let current = application.runtime_client().snapshot().overlay_settings;
            application.set_overlay_settings(OverlaySettings {
                always_on_top: !current.always_on_top,
                ..current
            })?;
        }
    }
    Ok(())
}

pub(super) const fn settings_runtime_error_code(
    code: RuntimeRenderErrorCode,
) -> SettingsRuntimeErrorCode {
    match code {
        RuntimeRenderErrorCode::ModelLoadFailed => SettingsRuntimeErrorCode::ModelLoadFailed,
        RuntimeRenderErrorCode::ModelEvaluationFailed => {
            SettingsRuntimeErrorCode::ModelEvaluationFailed
        }
        RuntimeRenderErrorCode::MotionLoadFailed => SettingsRuntimeErrorCode::MotionLoadFailed,
        RuntimeRenderErrorCode::ExpressionLoadFailed => {
            SettingsRuntimeErrorCode::ExpressionLoadFailed
        }
        RuntimeRenderErrorCode::GpuPreparationFailed => {
            SettingsRuntimeErrorCode::GpuPreparationFailed
        }
        RuntimeRenderErrorCode::TransportClosed => SettingsRuntimeErrorCode::TransportClosed,
        RuntimeRenderErrorCode::OverlaySettingsInvalid => {
            SettingsRuntimeErrorCode::OverlaySettingsInvalid
        }
        RuntimeRenderErrorCode::MaximumFpsInvalid => SettingsRuntimeErrorCode::MaximumFpsInvalid,
        RuntimeRenderErrorCode::ReleaseFallbackTimeoutInvalid => {
            SettingsRuntimeErrorCode::ReleaseFallbackTimeoutInvalid
        }
        RuntimeRenderErrorCode::RandomBehaviorSettingsInvalid => {
            SettingsRuntimeErrorCode::RandomBehaviorSettingsInvalid
        }
    }
}

pub(super) fn settings_runtime_diagnostics(
    runtime: &bongocat_runtime::RuntimeSnapshot,
) -> SettingsRuntimeDiagnostics {
    SettingsRuntimeDiagnostics {
        render_error: runtime.render_error.map(settings_runtime_error_code),
        last_command_failure: runtime.last_command_failure.map(|failure| {
            SettingsRuntimeCommandFailure {
                sequence: failure.sequence,
                code: settings_runtime_error_code(failure.code),
            }
        }),
        command_transport: SettingsRuntimeCommandTransportDiagnostics {
            enqueued: runtime.command_transport.enqueued,
            queue_full: runtime.command_transport.queue_full,
            runtime_stopped: runtime.command_transport.runtime_stopped,
            sequence_gap_count: runtime.command_transport.sequence_gap_count,
            missing_sequence_count: runtime.command_transport.missing_sequence_count,
            duplicate_sequence_count: runtime.command_transport.duplicate_sequence_count,
            out_of_order_sequence_count: runtime.command_transport.out_of_order_sequence_count,
        },
        work_budget_exceeded: runtime.work.budget_exceeded,
        last_over_budget_ms: runtime.work.last_over_budget_ms,
        shutdown_timed_out: runtime.shutdown.timed_out,
        shutdown_worker_panicked: runtime.shutdown.worker_panicked,
    }
}

pub(super) fn settings_input_diagnostics(
    input: &InputSnapshot,
    platform: PlatformInputDiagnostics,
    input_monitoring_permission: SettingsInputMonitoringPermission,
) -> SettingsInputDiagnostics {
    SettingsInputDiagnostics {
        input_monitoring_permission,
        service_status: match platform.service_status {
            PlatformInputServiceStatus::NotStarted => SettingsInputServiceStatus::NotStarted,
            PlatformInputServiceStatus::Running => SettingsInputServiceStatus::Running,
            PlatformInputServiceStatus::PermissionDenied => {
                SettingsInputServiceStatus::PermissionDenied
            }
            PlatformInputServiceStatus::BackendUnavailable => {
                SettingsInputServiceStatus::BackendUnavailable
            }
            PlatformInputServiceStatus::Failed => SettingsInputServiceStatus::Failed,
            PlatformInputServiceStatus::Stopped => SettingsInputServiceStatus::Stopped,
        },
        service_error_code: platform
            .service_error_code
            .filter(|code| bongocat_input::is_stable_platform_input_error_code(code)),
        service_start_attempts: platform.service_start_attempts,
        pressed_key_count: input.pressed_key_count,
        pressed_mouse_button_count: input.pressed_mouse_button_count,
        pressed_gamepad_button_count: input.pressed_gamepad_button_count,
        connected_gamepad_count: input.connected_gamepad_count,
        platform_gamepad_backend_failures: platform.gamepad_backend_failures,
        platform_gamepad_connection_rejections: platform.gamepad_connection_rejections,
        platform_gamepad_button_edges: platform.gamepad_button_edges,
        platform_gamepad_axis_samples: platform.gamepad_axis_samples,
        platform_gamepad_axis_publish_rejections: platform.gamepad_axis_publish_rejections,
        platform_gamepad_event_discards: platform.gamepad_event_discards,
        captured_down: input.diagnostics.captured_down,
        captured_up: input.diagnostics.captured_up,
        reconciled_release: input.diagnostics.reconciled_release,
        fallback_release: input.diagnostics.fallback_release,
        released_by_reset: input.diagnostics.released_by_reset,
        duplicate_down: input.diagnostics.duplicate_down,
        unmatched_release: input.diagnostics.unmatched_release,
        invalid_source: input.diagnostics.invalid_source,
        reset_count: input.diagnostics.reset_count,
        sequence_gap_count: input.diagnostics.sequence_gap_count,
        missing_sequence_count: input.diagnostics.missing_sequence_count,
        duplicate_sequence_count: input.diagnostics.duplicate_sequence_count,
        out_of_order_sequence_count: input.diagnostics.out_of_order_sequence_count,
        non_monotonic_time_count: input.diagnostics.non_monotonic_time_count,
        gamepad_connections: input.diagnostics.gamepad_connections,
        gamepad_disconnections: input.diagnostics.gamepad_disconnections,
        stale_gamepad_events: input.diagnostics.stale_gamepad_events,
        released_by_disconnect: input.diagnostics.released_by_disconnect,
        transport_enqueued: input.transport.enqueued,
        transport_queue_full: input.transport.queue_full,
        transport_recovered_after_overflow: input.transport.recovered_after_overflow,
        transport_runtime_stopped: input.transport.runtime_stopped,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn system_input_monitoring_permission() -> SettingsInputMonitoringPermission {
    match input_monitoring_permission() {
        InputPermission::Denied => SettingsInputMonitoringPermission::Denied,
        InputPermission::Granted => SettingsInputMonitoringPermission::Granted,
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) const fn system_input_monitoring_permission() -> SettingsInputMonitoringPermission {
    SettingsInputMonitoringPermission::Unsupported
}

pub(super) const fn startup_item_status_code(status: SettingsStartupItemStatus) -> &'static str {
    match status {
        SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled) => "disabled",
        SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled) => "enabled",
        SettingsStartupItemStatus::State(SettingsStartupItemState::Stale) => "stale",
        SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval) => {
            "requires_approval"
        }
        SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound) => "not_found",
        SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
            SettingsStartupItemUnsupportedReason::Platform,
        )) => "unsupported_platform",
        SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
            SettingsStartupItemUnsupportedReason::OperatingSystem,
        )) => "unsupported_operating_system",
        SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
            SettingsStartupItemUnsupportedReason::BuildEnvironment,
        )) => "unsupported_build_environment",
        SettingsStartupItemStatus::ReadError(
            SettingsStartupItemError::CurrentExecutableUnavailable,
        ) => "current_executable_unavailable",
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::InvalidExecutablePath) => {
            "invalid_executable_path"
        }
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::BackendUnavailable) => {
            "backend_unavailable"
        }
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::StateReadFailed) => {
            "state_read_failed"
        }
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::EnableFailed) => {
            "enable_failed"
        }
        SettingsStartupItemStatus::ReadError(SettingsStartupItemError::DisableFailed) => {
            "disable_failed"
        }
    }
}

pub(super) const fn input_service_is_degraded(status: SettingsInputServiceStatus) -> bool {
    matches!(
        status,
        SettingsInputServiceStatus::PermissionDenied
            | SettingsInputServiceStatus::BackendUnavailable
            | SettingsInputServiceStatus::Failed
    )
}

/// Whether this build can offer login startup at all.
///
/// Login startup registers the running executable with the operating system, so
/// the registration outlives the process that made it and points at whatever
/// executable was current when it was written. A development build's executable
/// is a build output rather than an installed application, so the capability
/// belongs to a released product: a Development build reports the build
/// environment as the reason and never asks the platform (ADR-0051).
///
/// The test is written as "is this Production" rather than "is this
/// Development" so an added environment defaults to unavailable instead of
/// silently gaining a registration.
pub(super) const fn startup_item_available() -> bool {
    matches!(BUILD_ENVIRONMENT, BuildEnvironment::Production)
}

/// The state and the mutation answer a Development build reports.
///
/// Both directions report the same value so a client that reads and then writes
/// never observes the capability changing underneath it.
pub(super) const fn startup_item_build_environment_state() -> SettingsStartupItemState {
    SettingsStartupItemState::Unsupported(SettingsStartupItemUnsupportedReason::BuildEnvironment)
}

pub(super) fn system_startup_item_state() -> SettingsStartupItemStatus {
    if !startup_item_available() {
        return SettingsStartupItemStatus::State(startup_item_build_environment_state());
    }
    startup_item_state(startup_item_environment())
        .map(settings_startup_item_state)
        .map(SettingsStartupItemStatus::State)
        .unwrap_or_else(|error| {
            SettingsStartupItemStatus::ReadError(settings_startup_item_error(error))
        })
}

pub(super) fn system_set_startup_item_enabled(
    enabled: bool,
) -> Result<SettingsStartupItemState, SettingsError> {
    if !startup_item_available() {
        // A no-op that reports the capability instead of an error: the switch
        // that would send this command renders disabled, and a command that
        // still arrives (a stale window, a scripted client) must not raise a
        // failure the user has no way to act on.
        return Ok(startup_item_build_environment_state());
    }
    set_startup_item_enabled(startup_item_environment(), enabled)
        .map(settings_startup_item_state)
        .map_err(|_| SettingsError::new(SettingsErrorCode::StartupItemUpdateFailed))
}

pub(super) const fn startup_item_environment() -> StartupItemEnvironment {
    match crate::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => StartupItemEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => StartupItemEnvironment::Production,
    }
}

pub(super) const fn settings_startup_item_state(
    state: StartupItemState,
) -> SettingsStartupItemState {
    match state {
        StartupItemState::Unsupported(reason) => {
            SettingsStartupItemState::Unsupported(match reason {
                StartupItemUnsupportedReason::Platform => {
                    SettingsStartupItemUnsupportedReason::Platform
                }
                StartupItemUnsupportedReason::OperatingSystem => {
                    SettingsStartupItemUnsupportedReason::OperatingSystem
                }
                StartupItemUnsupportedReason::BuildEnvironment => {
                    SettingsStartupItemUnsupportedReason::BuildEnvironment
                }
            })
        }
        StartupItemState::Disabled => SettingsStartupItemState::Disabled,
        StartupItemState::Enabled => SettingsStartupItemState::Enabled,
        StartupItemState::Stale => SettingsStartupItemState::Stale,
        StartupItemState::RequiresApproval => SettingsStartupItemState::RequiresApproval,
        StartupItemState::NotFound => SettingsStartupItemState::NotFound,
    }
}

pub(super) const fn settings_startup_item_error(
    error: StartupItemError,
) -> SettingsStartupItemError {
    match error {
        StartupItemError::CurrentExecutableUnavailable => {
            SettingsStartupItemError::CurrentExecutableUnavailable
        }
        StartupItemError::InvalidExecutablePath => SettingsStartupItemError::InvalidExecutablePath,
        StartupItemError::BackendUnavailable => SettingsStartupItemError::BackendUnavailable,
        StartupItemError::StateReadFailed => SettingsStartupItemError::StateReadFailed,
        StartupItemError::EnableFailed => SettingsStartupItemError::EnableFailed,
        StartupItemError::DisableFailed => SettingsStartupItemError::DisableFailed,
    }
}
