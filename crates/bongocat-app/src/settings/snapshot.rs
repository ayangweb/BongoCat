//! The snapshot the settings window polls, and the clock that says when it
//! changed.
//!
//! Building a snapshot scans the model catalog, so the worker also answers a much
//! cheaper question: `ReadSnapshotRevision` compares the inputs a snapshot is
//! derived from and advances the revision only when one of them moved. That is why
//! the observations live on the clock instead of being recomputed per poll.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

use super::model_projection::*;
use super::projection::*;

/// How long an input-monitoring permission answer stays usable.
///
/// The system answers this query through a TCC round trip on its own dispatch queue,
/// which costs milliseconds and dominated the settings snapshot profile: every snapshot
/// used to pay it, once per settings command, once per settings refresh and once per
/// system-menu poll. The value only decides what the diagnostics page displays, and the
/// input service re-checks the permission itself before it creates or restarts an event
/// tap, so a bounded staleness here changes nothing that matters.
pub(super) const INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// The system input-monitoring permission, re-read at most once per
/// [`INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL`].
#[derive(Default)]
pub(super) struct InputMonitoringPermissionCache {
    checked_at: Option<Instant>,
    value: SettingsInputMonitoringPermission,
}

impl InputMonitoringPermissionCache {
    pub(super) fn resolve(
        &mut self,
        now: Instant,
        probe: impl FnOnce() -> SettingsInputMonitoringPermission,
    ) -> SettingsInputMonitoringPermission {
        let expired = self.checked_at.is_none_or(|checked_at| {
            now.saturating_duration_since(checked_at)
                >= INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL
        });
        if expired {
            self.value = probe();
            self.checked_at = Some(now);
        }
        self.value
    }
}

pub(super) struct SettingsSnapshotClock {
    pub(super) revision: u64,
    observed_config_revision: Option<u64>,
    observed_runtime_diagnostics: Option<SettingsRuntimeDiagnostics>,
    observed_input_diagnostics: Option<SettingsInputDiagnostics>,
    observed_startup_item: Option<SettingsStartupItemStatus>,
    observed_overlay_visible: Option<bool>,
    diagnostics_export: Option<SettingsDiagnosticsExportStatus>,
    input_monitoring_permission: InputMonitoringPermissionCache,
}

impl SettingsSnapshotClock {
    pub(super) const fn new(config_revision: Option<u64>) -> Self {
        Self {
            revision: 0,
            observed_config_revision: config_revision,
            observed_runtime_diagnostics: None,
            observed_input_diagnostics: None,
            observed_startup_item: None,
            observed_overlay_visible: None,
            diagnostics_export: None,
            input_monitoring_permission: InputMonitoringPermissionCache {
                checked_at: None,
                value: SettingsInputMonitoringPermission::Unsupported,
            },
        }
    }

    pub(super) fn input_monitoring_permission(&mut self) -> SettingsInputMonitoringPermission {
        self.input_monitoring_permission
            .resolve(Instant::now(), system_input_monitoring_permission)
    }

    pub(super) fn observe_config(&mut self, config_revision: Option<u64>) {
        if config_revision != self.observed_config_revision {
            self.mark_changed();
            self.observed_config_revision = config_revision;
        }
    }

    pub(super) fn mark_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub(super) fn mark_catalog_changed(&mut self) {
        self.mark_changed();
    }

    pub(super) fn observe_runtime_diagnostics(
        &mut self,
        diagnostics: SettingsRuntimeDiagnostics,
    ) -> Option<SettingsRuntimeDiagnostics> {
        self.observed_runtime_diagnostics.replace(diagnostics)
    }

    pub(super) fn observe_input_diagnostics(
        &mut self,
        diagnostics: SettingsInputDiagnostics,
    ) -> Option<SettingsInputDiagnostics> {
        let previous = self.observed_input_diagnostics.replace(diagnostics);
        if previous.is_some_and(|previous| previous != diagnostics) {
            self.mark_changed();
        }
        previous
    }

    pub(super) fn observe_startup_item(
        &mut self,
        status: SettingsStartupItemStatus,
    ) -> Option<SettingsStartupItemStatus> {
        let previous = self.observed_startup_item.replace(status);
        if previous.is_some_and(|previous| previous != status) {
            self.mark_changed();
        }
        previous
    }

    pub(super) fn observe_overlay_visible(&mut self, visible: bool) {
        if self
            .observed_overlay_visible
            .is_some_and(|previous| previous != visible)
        {
            self.mark_changed();
        }
        self.observed_overlay_visible = Some(visible);
    }

    pub(super) fn observe_diagnostics_export(&mut self, status: SettingsDiagnosticsExportStatus) {
        if self.diagnostics_export != Some(status) {
            self.mark_changed();
            self.diagnostics_export = Some(status);
        }
    }

    pub(super) fn coalesce_changes_since(&mut self, revision: u64) {
        if self.revision != revision {
            self.revision = revision.saturating_add(1);
        }
    }
}

pub(super) fn snapshot(
    application: &Application,
    clock: &mut SettingsSnapshotClock,
    catalog_changed: bool,
    startup_item: SettingsStartupItemStatus,
) -> SettingsSnapshot {
    let (runtime, input_diagnostics) =
        observe_snapshot_state(application, clock, startup_item, catalog_changed);
    SettingsSnapshot {
        revision: clock.revision,
        config_revision: application.config_revision(),
        build_info: SettingsBuildInfo {
            product_version: PRODUCT_VERSION.to_owned(),
            environment: match BUILD_ENVIRONMENT {
                BuildEnvironment::Development => SettingsBuildEnvironment::Development,
                BuildEnvironment::Production => SettingsBuildEnvironment::Production,
            },
        },
        runtime_health: if input_service_is_degraded(input_diagnostics.service_status) {
            RuntimeHealth::Degraded
        } else {
            match runtime.state {
                RuntimeState::Starting => RuntimeHealth::Starting,
                RuntimeState::Ready => RuntimeHealth::Ready,
                RuntimeState::Degraded | RuntimeState::Stopping => RuntimeHealth::Degraded,
                RuntimeState::Stopped => RuntimeHealth::Stopped,
            }
        },
        runtime_diagnostics: settings_runtime_diagnostics(&runtime),
        appearance_theme: settings_theme(application.config().appearance.theme),
        language: settings_language(application.config().appearance.language),
        resolved_language: settings_language(application.effective_language()),
        status_icon_visible: application.config().system.show_status_icon,
        taskbar_icon_visible: application.config().system.show_taskbar_icon,
        check_for_updates_automatically: application.config().updates.check_automatically,
        check_for_updates_interval_hours: application.config().updates.check_interval_hours,
        overlay_visible: runtime.overlay_visible,
        overlay: SettingsOverlay {
            click_through: runtime.overlay_settings.click_through,
            always_on_top: runtime.overlay_settings.always_on_top,
            scale_percent: runtime.overlay_settings.scale_percent,
            opacity_percent: runtime.overlay_settings.opacity_percent,
            corner_radius_percent: runtime.overlay_settings.corner_radius_percent,
            hide_on_pointer_hover: runtime.overlay_settings.hide_on_pointer_hover,
            hide_on_pointer_hover_delay_seconds: runtime
                .overlay_settings
                .hide_on_pointer_hover_delay_seconds,
            keep_inside_screen: runtime.overlay_settings.keep_inside_screen,
        },
        motion_audio_enabled: runtime.motion_audio_enabled,
        command_shortcuts_enabled: application.config().shortcuts.commands_enabled,
        behavior_shortcuts_enabled: application.config().shortcuts.model_behaviors_enabled,
        maximum_fps: runtime.maximum_fps,
        release_fallback_timeout_ms: runtime.release_fallback_timeout_ms,
        random_behavior: SettingsRandomBehavior {
            enabled: runtime.random_behavior_settings.enabled,
            interval_seconds: runtime.random_behavior_settings.interval_seconds,
        },
        model_settings: SettingsModelSettings {
            mirror: runtime.model_settings.mirror,
            mirror_pointer_tracking: runtime.model_settings.mirror_pointer_tracking,
            ignore_keyboard: runtime.model_settings.ignore_keyboard,
            ignore_gamepad: runtime.model_settings.ignore_gamepad,
            ignore_pointer: runtime.model_settings.ignore_pointer,
        },
        gamepad_axis_settings: SettingsGamepadAxisSettings {
            stick_dead_zone_percent: (runtime.gamepad_axis_settings.stick_dead_zone * 100.0)
                .round()
                .clamp(0.0, 99.0) as u8,
            trigger_dead_zone_percent: (runtime.gamepad_axis_settings.trigger_dead_zone * 100.0)
                .round()
                .clamp(0.0, 99.0) as u8,
        },
        gamepad_auto_switch: settings_gamepad_auto_switch(
            &application.config().model.gamepad_auto_switch,
        ),
        logging: settings_logging_from_config(&application.config().logging),
        shortcuts: settings_shortcuts(application.config()),
        startup_item,
        diagnostics_export: clock.diagnostics_export,
        input_diagnostics,
        active_model: runtime
            .active_model
            .and_then(|model| {
                application
                    .active_model_origin()
                    .map(|origin| SettingsModelKey {
                        id: model.id.as_str().to_owned(),
                        origin: settings_origin_from_model(origin),
                    })
            })
            .or_else(|| configured_model_key(application)),
        model_catalog: settings_model_catalog(application),
    }
}

/// Bring the snapshot clock up to date and report what it observed.
///
/// Split out of [`snapshot`] so the revision can be polled without building the snapshot:
/// everything here is derived from state the application already holds in memory, while
/// the snapshot's own construction — the model catalog scan above all — is only worth
/// paying for when a caller renders it. Everything that moves the revision happens here,
/// which is what keeps a probed revision equal to the one the next snapshot reports.
pub(super) fn observe_snapshot_state(
    application: &Application,
    clock: &mut SettingsSnapshotClock,
    startup_item: SettingsStartupItemStatus,
    catalog_changed: bool,
) -> (RuntimeSnapshot, SettingsInputDiagnostics) {
    let revision_before = clock.revision;
    let runtime = application.runtime_client().snapshot();
    clock.observe_overlay_visible(runtime.overlay_visible);
    let input_diagnostics = settings_input_diagnostics(
        &runtime.input,
        runtime.platform_input,
        clock.input_monitoring_permission(),
    );
    clock.observe_config(application.config_revision());
    let runtime_diagnostics = settings_runtime_diagnostics(&runtime);
    if let Some(previous) = clock.observe_runtime_diagnostics(runtime_diagnostics) {
        match (previous.render_error, runtime_diagnostics.render_error) {
            (None, Some(error)) => application.record_log(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                    .with_context(ApplicationLogContext::Service("renderer"))
                    .with_context(ApplicationLogContext::Reason(error.as_str())),
            ),
            (Some(_), None) => application.record_log(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceRecovered)
                    .with_context(ApplicationLogContext::Service("renderer"))
                    .with_context(ApplicationLogContext::Reason("render_recovered")),
            ),
            (Some(previous), Some(current)) if previous != current => application.record_log(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                    .with_context(ApplicationLogContext::Service("renderer"))
                    .with_context(ApplicationLogContext::Reason(current.as_str())),
            ),
            _ => {}
        }
        if runtime_diagnostics.command_transport.queue_full > previous.command_transport.queue_full
        {
            application.record_log_once(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                    .with_context(ApplicationLogContext::Service("runtime_command_transport"))
                    .with_context(ApplicationLogContext::Reason("queue_full")),
            );
        }
    }
    if let Some(previous) = clock.observe_input_diagnostics(input_diagnostics) {
        if previous.service_status != input_diagnostics.service_status {
            application.record_log(
                ApplicationLogEvent::new(ApplicationLogCode::InputStatusChanged).with_context(
                    ApplicationLogContext::State(input_service_status_code(
                        input_diagnostics.service_status,
                    )),
                ),
            );
        }
        if previous.input_monitoring_permission != input_diagnostics.input_monitoring_permission {
            match input_diagnostics.input_monitoring_permission {
                SettingsInputMonitoringPermission::Denied => application.record_log(
                    ApplicationLogEvent::new(ApplicationLogCode::InputPermissionUnavailable)
                        .with_context(ApplicationLogContext::Reason("permission_denied")),
                ),
                SettingsInputMonitoringPermission::Granted => application.record_log(
                    ApplicationLogEvent::new(ApplicationLogCode::InputStatusChanged)
                        .with_context(ApplicationLogContext::State("input_monitoring_granted")),
                ),
                SettingsInputMonitoringPermission::Unsupported => application.record_log(
                    ApplicationLogEvent::new(ApplicationLogCode::InputStatusChanged)
                        .with_context(ApplicationLogContext::State("input_monitoring_unsupported")),
                ),
            }
        }
        if input_diagnostics.transport_queue_full > previous.transport_queue_full {
            application.record_log_once(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                    .with_context(ApplicationLogContext::Service("input"))
                    .with_context(ApplicationLogContext::Reason("transport_queue_full")),
            );
        }
        if input_diagnostics.transport_recovered_after_overflow
            > previous.transport_recovered_after_overflow
        {
            application.record_log(
                ApplicationLogEvent::new(ApplicationLogCode::ServiceRecovered)
                    .with_context(ApplicationLogContext::Service("input"))
                    .with_context(ApplicationLogContext::Reason(
                        "transport_overflow_recovered",
                    )),
            );
        }
    }
    if let Some(previous) = clock.observe_startup_item(startup_item)
        && previous != startup_item
    {
        let event = match startup_item {
            SettingsStartupItemStatus::ReadError(_) => {
                ApplicationLogEvent::new(ApplicationLogCode::ServiceDegraded)
                    .with_context(ApplicationLogContext::Service("startup_item"))
            }
            SettingsStartupItemStatus::State(_) => {
                ApplicationLogEvent::new(ApplicationLogCode::ServiceRecovered)
                    .with_context(ApplicationLogContext::Service("startup_item"))
            }
        };
        application.record_log(event.with_context(ApplicationLogContext::State(
            startup_item_status_code(startup_item),
        )));
    }
    if catalog_changed {
        clock.mark_catalog_changed();
    }
    clock.coalesce_changes_since(revision_before);
    (runtime, input_diagnostics)
}
