use crate::{
    Application, ApplicationConfigStatus, ApplicationError, ApplicationLogCode,
    ApplicationLogComponent, ApplicationLogDiagnostics, ApplicationLogEvent, ApplicationLogLevel,
    ApplicationShortcutSignals,
};
use atomic_write_file::AtomicWriteFile;
use bongocat_config::{
    ConfigError, ConfigWriteFailureReason, NativeConfig, OverlayWindowPlacement, ShortcutCommand,
    StateError, WindowPlacement,
};
use bongocat_model::{
    ModelCatalogEntry, ModelDiagnostic, ModelImportProgress, ModelImportStage, ModelOrigin,
    ModelStoreDiagnostic,
};
#[cfg(target_os = "macos")]
use bongocat_platform::{InputPermission, input_monitoring_permission};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
    open_directory, set_startup_item_enabled, startup_item_state,
};
use bongocat_runtime::{
    InputSnapshot, ModelSettings, OverlaySettings, PlatformInputDiagnostics,
    PlatformInputServiceStatus, RuntimeRenderErrorCode, RuntimeState,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::SettingsStartupItemError;
use bongocat_ui::{
    DIAGNOSTICS_EXPORT_FORMAT_VERSION, RuntimeHealth, SettingsApplicationShortcut, SettingsClient,
    SettingsCommand, SettingsConfigRecovery, SettingsConfigurationStatus,
    SettingsDiagnosticsExportStatus, SettingsError, SettingsErrorCode, SettingsGamepadAxisSettings,
    SettingsInputDiagnostics, SettingsInputMonitoringPermission, SettingsInputServiceStatus,
    SettingsLanguage, SettingsModelAvailability, SettingsModelBehaviorBinding,
    SettingsModelCatalog, SettingsModelCatalogError, SettingsModelDiagnostic, SettingsModelEntry,
    SettingsModelImportProgress, SettingsModelImportStage, SettingsModelKey, SettingsModelOrigin,
    SettingsModelSettings, SettingsOverlay, SettingsRuntimeCommandFailure,
    SettingsRuntimeCommandTransportDiagnostics, SettingsRuntimeDiagnostics,
    SettingsRuntimeErrorCode, SettingsServiceEndpoint, SettingsShortcutBinding, SettingsShortcuts,
    SettingsSnapshot, SettingsStartupItemState, SettingsStartupItemStatus,
    SettingsStartupItemUnsupportedReason, SettingsTheme, SettingsWindowPlacement,
    SettingsWindowState,
};
use serde::Serialize;
use std::fs;
use std::{
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver as ShortcutReceiver, RecvTimeoutError},
    },
    thread,
    time::Duration,
};

const SETTINGS_COMMAND_CAPACITY: usize = 16;

pub struct ApplicationSettingsService {
    client: SettingsClient,
    window_state: SettingsWindowState,
    worker: Option<thread::JoinHandle<()>>,
    shortcut_forwarder: Option<thread::JoinHandle<()>>,
    shortcut_forwarder_stop: Arc<AtomicBool>,
}

impl ApplicationSettingsService {
    pub fn start(application: Application) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            Arc::new(UnavailableStatusIcon),
            Arc::new(UnavailableTaskbarIcon),
            None,
            None,
        )
    }

    pub fn start_with_shortcut_receiver(
        application: Application,
        receiver: ShortcutReceiver<bongocat_config::ShortcutCommand>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            Arc::new(UnavailableStatusIcon),
            Arc::new(UnavailableTaskbarIcon),
            Some(receiver),
            None,
        )
    }

    pub fn start_with_shortcut_receiver_and_signals(
        application: Application,
        receiver: ShortcutReceiver<bongocat_config::ShortcutCommand>,
        signals: ApplicationShortcutSignals,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            Arc::new(UnavailableStatusIcon),
            Arc::new(UnavailableTaskbarIcon),
            Some(receiver),
            Some(signals),
        )
    }

    pub fn start_with_product_capabilities(
        application: Application,
        receiver: ShortcutReceiver<bongocat_config::ShortcutCommand>,
        signals: ApplicationShortcutSignals,
        status_icon: Arc<dyn StatusIconCapability>,
        #[cfg(target_os = "windows")] taskbar_icon: Arc<dyn TaskbarIconCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        #[cfg(not(target_os = "windows"))]
        let taskbar_icon = Arc::new(UnavailableTaskbarIcon);
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            status_icon,
            taskbar_icon,
            Some(receiver),
            Some(signals),
        )
    }

    #[cfg(test)]
    fn start_with_startup_item(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            startup_item,
            Arc::new(UnavailableStatusIcon),
            Arc::new(UnavailableTaskbarIcon),
            None,
            None,
        )
    }

    #[cfg(test)]
    fn start_with_status_icon(
        application: Application,
        status_icon: Arc<dyn StatusIconCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            status_icon,
            Arc::new(UnavailableTaskbarIcon),
            None,
            None,
        )
    }

    #[cfg(test)]
    fn start_with_taskbar_icon(
        application: Application,
        taskbar_icon: Arc<dyn TaskbarIconCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            Arc::new(UnavailableStatusIcon),
            taskbar_icon,
            None,
            None,
        )
    }

    fn start_with_startup_item_and_shortcuts(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
        status_icon: Arc<dyn StatusIconCapability>,
        taskbar_icon: Arc<dyn TaskbarIconCapability>,
        shortcut_receiver: Option<ShortcutReceiver<bongocat_config::ShortcutCommand>>,
        shortcut_signals: Option<ApplicationShortcutSignals>,
    ) -> Result<Self, SettingsServiceJoinError> {
        let backup_location = Arc::new(SystemBackupLocation {
            path: application.config_backup_directory().to_owned(),
        });
        let diagnostics_export = Arc::new(SystemDiagnosticsExport {
            path: application.logs_directory().join("diagnostics.json"),
        });
        Self::start_with_capabilities_and_shortcuts(
            application,
            startup_item,
            VisibilityCapabilities {
                status_icon,
                taskbar_icon,
            },
            backup_location,
            diagnostics_export,
            shortcut_receiver,
            shortcut_signals,
        )
    }

    #[cfg(test)]
    fn start_with_capabilities(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
        backup_location: Arc<dyn BackupLocationCapability>,
        diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_capabilities_and_shortcuts(
            application,
            startup_item,
            VisibilityCapabilities {
                status_icon: Arc::new(UnavailableStatusIcon),
                taskbar_icon: Arc::new(UnavailableTaskbarIcon),
            },
            backup_location,
            diagnostics_export,
            None,
            None,
        )
    }

    fn start_with_capabilities_and_shortcuts(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
        visibility: VisibilityCapabilities,
        backup_location: Arc<dyn BackupLocationCapability>,
        diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
        shortcut_receiver: Option<ShortcutReceiver<bongocat_config::ShortcutCommand>>,
        shortcut_signals: Option<ApplicationShortcutSignals>,
    ) -> Result<Self, SettingsServiceJoinError> {
        let (client, endpoint) = SettingsClient::bounded(SETTINGS_COMMAND_CAPACITY);
        let window_state = client.track_window_state(
            application
                .settings_window_placement()
                .and_then(settings_window_placement),
        );
        let worker_window_state = window_state.clone();
        let worker = thread::Builder::new()
            .name("bongocat-settings-service".to_owned())
            .spawn(move || {
                run_service(
                    application,
                    endpoint,
                    startup_item,
                    visibility,
                    backup_location,
                    diagnostics_export,
                    worker_window_state,
                )
            })
            .map_err(SettingsServiceJoinError::Spawn)?;
        let shortcut_forwarder_stop = Arc::new(AtomicBool::new(false));
        let shortcut_forwarder = shortcut_receiver.map(|receiver| {
            let client = client.clone();
            let signals = shortcut_signals;
            let worker_stop = Arc::clone(&shortcut_forwarder_stop);
            thread::Builder::new()
                .name("bongocat-shortcut-forwarder".to_owned())
                .spawn(move || {
                    while !worker_stop.load(Ordering::Acquire) {
                        let command = match receiver.recv_timeout(Duration::from_millis(50)) {
                            Ok(command) => command,
                            Err(RecvTimeoutError::Timeout) => continue,
                            Err(RecvTimeoutError::Disconnected) => break,
                        };
                        if command == bongocat_config::ShortcutCommand::OpenSettings {
                            if let Some(signals) = signals.as_ref() {
                                signals.request_open_settings();
                            }
                            continue;
                        }
                        let Some(command) = settings_shortcut(command) else {
                            continue;
                        };
                        if client.enqueue_application_shortcut(command).is_err() {
                            break;
                        }
                    }
                })
                .expect("shortcut forwarder thread")
        });
        Ok(Self {
            client,
            window_state,
            worker: Some(worker),
            shortcut_forwarder,
            shortcut_forwarder_stop,
        })
    }

    pub fn client(&self) -> SettingsClient {
        self.client.clone()
    }

    pub fn window_state(&self) -> SettingsWindowState {
        self.window_state.clone()
    }

    fn stop_shortcut_forwarder(&mut self) {
        self.shortcut_forwarder_stop.store(true, Ordering::Release);
        if let Some(forwarder) = self.shortcut_forwarder.take() {
            let _ = forwarder.join();
        }
    }

    pub fn join(mut self) -> Result<(), SettingsServiceJoinError> {
        let worker_result = self
            .worker
            .take()
            .expect("settings service worker is present")
            .join()
            .map_err(|_| SettingsServiceJoinError::Panicked);
        self.stop_shortcut_forwarder();
        worker_result
    }
}

trait StartupItemCapability: Send + Sync + 'static {
    fn state(&self) -> SettingsStartupItemStatus;

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError>;
}

pub trait StatusIconCapability: Send + Sync + 'static {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError>;
}

pub trait TaskbarIconCapability: Send + Sync + 'static {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError>;
}

struct VisibilityCapabilities {
    status_icon: Arc<dyn StatusIconCapability>,
    taskbar_icon: Arc<dyn TaskbarIconCapability>,
}

trait BackupLocationCapability: Send + Sync + 'static {
    fn open(&self) -> Result<(), SettingsError>;
}

trait DiagnosticsExportCapability: Send + Sync + 'static {
    fn export(
        &self,
        snapshot: &SettingsSnapshot,
        application_logs: ApplicationLogDiagnostics,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError>;
}

struct SystemStartupItem;

struct UnavailableStatusIcon;

struct UnavailableTaskbarIcon;

struct SystemBackupLocation {
    path: PathBuf,
}

struct SystemDiagnosticsExport {
    path: PathBuf,
}

impl StartupItemCapability for SystemStartupItem {
    fn state(&self) -> SettingsStartupItemStatus {
        system_startup_item_state()
    }

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError> {
        system_set_startup_item_enabled(enabled)
    }
}

impl StatusIconCapability for UnavailableStatusIcon {
    fn set_visible(&self, _visible: bool) -> Result<(), SettingsError> {
        Err(SettingsError::new(
            SettingsErrorCode::StatusIconUpdateFailed,
        ))
    }
}

impl TaskbarIconCapability for UnavailableTaskbarIcon {
    fn set_visible(&self, _visible: bool) -> Result<(), SettingsError> {
        Err(SettingsError::new(
            SettingsErrorCode::TaskbarIconUpdateFailed,
        ))
    }
}

impl BackupLocationCapability for SystemBackupLocation {
    fn open(&self) -> Result<(), SettingsError> {
        system_open_backup_location(&self.path)
    }
}

impl DiagnosticsExportCapability for SystemDiagnosticsExport {
    fn export(
        &self,
        snapshot: &SettingsSnapshot,
        application_logs: ApplicationLogDiagnostics,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
        export_diagnostics_file(&self.path, snapshot, application_logs)
    }
}

impl Drop for ApplicationSettingsService {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.client.shutdown_blocking();
            let _ = worker.join();
        }
        self.stop_shortcut_forwarder();
    }
}

#[derive(Debug)]
pub enum SettingsServiceJoinError {
    Spawn(std::io::Error),
    Panicked,
}

impl fmt::Display for SettingsServiceJoinError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "failed to start settings service: {error}"),
            Self::Panicked => formatter.write_str("settings service panicked"),
        }
    }
}

impl std::error::Error for SettingsServiceJoinError {}

fn run_service(
    mut application: Application,
    endpoint: SettingsServiceEndpoint,
    startup_item: Arc<dyn StartupItemCapability>,
    visibility: VisibilityCapabilities,
    backup_location: Arc<dyn BackupLocationCapability>,
    diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
    window_state: SettingsWindowState,
) {
    let mut clock = SettingsSnapshotClock::new(
        application.runtime_client().snapshot().revision,
        application.config_revision(),
    );
    loop {
        let Ok(command) = endpoint.recv_blocking() else {
            let _ = persist_window_state(&mut application, &window_state);
            let _ = application.shutdown();
            break;
        };
        match command {
            SettingsCommand::SettingsWindowPlacementChanged => {
                let _ = persist_window_state(&mut application, &window_state);
            }
            SettingsCommand::OverlayWindowPlacementChanged {
                x,
                y,
                width,
                height,
            } => {
                if let Ok(placement) = OverlayWindowPlacement::new(x, y, width, height) {
                    let _ = application.persist_overlay_window_placement(placement);
                }
            }
            SettingsCommand::TriggerApplicationShortcut { command } => {
                if let Err(error) = apply_application_shortcut(&mut application, command) {
                    application.record_log(ApplicationLogEvent {
                        component: ApplicationLogComponent::Settings,
                        level: ApplicationLogLevel::Error,
                        code: ApplicationLogCode::RuntimeUnavailable,
                    });
                    let _ = error;
                }
            }
            SettingsCommand::ReadSnapshot { reply } => {
                let _ = reply.respond(Ok(snapshot(
                    &application,
                    &mut clock,
                    false,
                    startup_item.state(),
                )));
            }
            SettingsCommand::SetOverlayVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_overlay_visible(visible)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetAppearanceTheme {
                expected_config_revision,
                theme,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_appearance_theme(config_theme(theme))
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetLanguage {
                expected_config_revision,
                language,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            return Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated));
                        }
                        application
                            .set_language(config_language(language))
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStatusIconVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            return Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated));
                        }
                        let previous = application.config().application.show_status_icon;
                        if previous == visible {
                            return Ok(());
                        }
                        visibility.status_icon.set_visible(visible)?;
                        if let Err(error) = application.set_status_icon_visible(visible) {
                            if visibility.status_icon.set_visible(previous).is_err() {
                                return Err(SettingsError::new(
                                    SettingsErrorCode::StatusIconUpdateFailed,
                                ));
                            }
                            return Err(map_application_error(error));
                        }
                        Ok(())
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetTaskbarIconVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            return Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated));
                        }
                        let previous = application.config().application.show_taskbar_icon;
                        if previous == visible {
                            return Ok(());
                        }
                        visibility.taskbar_icon.set_visible(visible)?;
                        if let Err(error) = application.set_taskbar_icon_visible(visible) {
                            if visibility.taskbar_icon.set_visible(previous).is_err() {
                                return Err(SettingsError::new(
                                    SettingsErrorCode::TaskbarIconUpdateFailed,
                                ));
                            }
                            return Err(map_application_error(error));
                        }
                        Ok(())
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCheckForUpdatesAutomatically {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_check_for_updates_automatically(enabled)
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetOverlaySettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = OverlaySettings {
                    click_through: settings.click_through,
                    always_on_top: settings.always_on_top,
                    scale_percent: settings.scale_percent,
                    opacity_percent: settings.opacity_percent,
                    keep_inside_work_area: settings.keep_inside_work_area,
                };
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_overlay_settings(runtime_settings)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMotionAudioEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_motion_audio_enabled(enabled)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetBehaviorShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_behavior_shortcuts_enabled(enabled)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMaximumFps {
                expected_config_revision,
                maximum_fps,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else if !bongocat_runtime::maximum_fps_is_valid(maximum_fps) {
                            Err(SettingsError::new(SettingsErrorCode::InvalidMaximumFps))
                        } else {
                            application
                                .set_maximum_fps(maximum_fps)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetReleaseFallbackTimeout {
                expected_config_revision,
                timeout_ms,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else if !bongocat_runtime::release_fallback_timeout_is_valid(timeout_ms) {
                            Err(SettingsError::new(
                                SettingsErrorCode::InvalidReleaseFallbackTimeout,
                            ))
                        } else {
                            application
                                .set_release_fallback_timeout(timeout_ms)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = ModelSettings {
                    mirror: settings.mirror,
                    mirror_pointer_tracking: settings.mirror_pointer_tracking,
                    ignore_pointer: settings.ignore_pointer,
                };
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_model_settings(runtime_settings)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetGamepadAxisSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let valid = settings.stick_dead_zone_percent < 100
                    && settings.trigger_dead_zone_percent < 100;
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else if !valid {
                            Err(SettingsError::new(
                                SettingsErrorCode::InvalidGamepadAxisSettings,
                            ))
                        } else {
                            let runtime_settings = bongocat_runtime::GamepadAxisSettings::new(
                                f32::from(settings.stick_dead_zone_percent) / 100.0,
                                f32::from(settings.trigger_dead_zone_percent) / 100.0,
                            )
                            .ok_or_else(|| {
                                SettingsError::new(SettingsErrorCode::InvalidGamepadAxisSettings)
                            })?;
                            application
                                .set_gamepad_axis_settings(runtime_settings)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetShortcuts {
                expected_config_revision,
                shortcuts,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .set_shortcuts(shortcuts)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::RestoreDefaultShortcuts {
                expected_config_revision,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .restore_default_shortcuts()
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStartupItemEnabled { enabled, reply } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| startup_item.set_enabled(enabled))
                    .map(|state| {
                        snapshot(
                            &application,
                            &mut clock,
                            false,
                            SettingsStartupItemStatus::State(state),
                        )
                    });
                let _ = reply.respond(result);
            }
            SettingsCommand::SelectModel {
                expected_config_revision,
                model,
                reply,
            } => {
                let result = require_operational(&application)
                    .map_err(map_application_error)
                    .and_then(|()| {
                        if application.config_revision() != Some(expected_config_revision) {
                            Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
                        } else {
                            application
                                .select_model(model_origin(model.origin), model.id)
                                .map(|_| ())
                                .map_err(map_application_error)
                        }
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ImportModel {
                request,
                operation,
                reply,
            } => {
                let progress = operation.clone();
                let cancellation = operation.clone();
                let result = require_operational(&application)
                    .and_then(|()| {
                        application
                            .import_model_with_observer(
                                request.id,
                                request.source_root,
                                move |update| {
                                    let _ =
                                        progress.report_progress(settings_import_progress(update));
                                },
                                move || cancellation.is_cancelled(),
                            )
                            .map(|_| ())
                    })
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::DeleteModel { model, reply } => {
                let result = require_operational(&application)
                    .and_then(|()| application.delete_model(model_origin(model.origin), model.id))
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_delete_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::RestoreDefaultConfiguration { reply } => {
                let result = application
                    .restore_default_configuration()
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()))
                    .map_err(map_configuration_recovery_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenConfigBackupLocation { reply } => {
                let result = backup_location
                    .open()
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ExportDiagnostics { reply } => {
                let result = {
                    let current = snapshot(&application, &mut clock, false, startup_item.state());
                    diagnostics_export
                        .export(&current, application.application_log_diagnostics())
                        .map(|status| {
                            clock.observe_diagnostics_export(status);
                            snapshot(&application, &mut clock, false, startup_item.state())
                        })
                };
                if result.is_err() {
                    application.record_log(ApplicationLogEvent {
                        component: ApplicationLogComponent::Settings,
                        level: ApplicationLogLevel::Error,
                        code: ApplicationLogCode::DiagnosticsExportFailed,
                    });
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::Shutdown { reply } => {
                let before_shutdown =
                    snapshot(&application, &mut clock, false, startup_item.state());
                let state_result = persist_window_state(&mut application, &window_state);
                let shutdown_result = application.shutdown();
                let result = match (state_result, shutdown_result) {
                    (Ok(()), Ok(stopped)) => {
                        clock.observe_runtime(stopped.revision);
                        Ok(SettingsSnapshot {
                            revision: clock.revision,
                            runtime_health: RuntimeHealth::Stopped,
                            ..before_shutdown
                        })
                    }
                    (Err(_), Ok(_)) => {
                        Err(SettingsError::new(SettingsErrorCode::StatePersistFailed))
                    }
                    (_, Err(_)) => Err(SettingsError::new(SettingsErrorCode::ShutdownFailed)),
                };
                let _ = reply.respond(result);
                break;
            }
        }
    }
}

fn settings_window_placement(placement: WindowPlacement) -> Option<SettingsWindowPlacement> {
    SettingsWindowPlacement::new(
        placement.x,
        placement.y,
        placement.width,
        placement.height,
        placement.maximized,
    )
}

fn persist_window_state(
    application: &mut Application,
    window_state: &SettingsWindowState,
) -> Result<(), ApplicationError> {
    let placement = window_state
        .placement()
        .map(|placement| {
            WindowPlacement::new(
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                placement.maximized,
            )
        })
        .transpose()?;
    match application.persist_settings_window_placement(placement) {
        Err(ApplicationError::State(StateError::UnsupportedSchema(_))) => Ok(()),
        result => result,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_open_backup_location(path: &std::path::Path) -> Result<(), SettingsError> {
    open_directory(path)
        .map_err(|_| SettingsError::new(SettingsErrorCode::BackupLocationOpenFailed))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_open_backup_location(_path: &std::path::Path) -> Result<(), SettingsError> {
    Err(SettingsError::new(
        SettingsErrorCode::BackupLocationOpenFailed,
    ))
}

fn require_operational(application: &Application) -> Result<(), ApplicationError> {
    application
        .is_operational()
        .then_some(())
        .ok_or(ApplicationError::ConfigurationRecoveryRequired)
}

const fn settings_import_progress(progress: ModelImportProgress) -> SettingsModelImportProgress {
    SettingsModelImportProgress {
        stage: match progress.stage {
            ModelImportStage::Preparing => SettingsModelImportStage::Preparing,
            ModelImportStage::Copying => SettingsModelImportStage::Copying,
            ModelImportStage::Validating => SettingsModelImportStage::Validating,
            ModelImportStage::Committing => SettingsModelImportStage::Committing,
        },
        files_copied: progress.files_copied,
        bytes_copied: progress.bytes_copied,
    }
}

struct SettingsSnapshotClock {
    revision: u64,
    observed_runtime_revision: u64,
    observed_config_revision: Option<u64>,
    observed_input_diagnostics: Option<SettingsInputDiagnostics>,
    observed_startup_item: Option<SettingsStartupItemStatus>,
    diagnostics_export: Option<SettingsDiagnosticsExportStatus>,
}

impl SettingsSnapshotClock {
    const fn new(runtime_revision: u64, config_revision: Option<u64>) -> Self {
        Self {
            revision: runtime_revision,
            observed_runtime_revision: runtime_revision,
            observed_config_revision: config_revision,
            observed_input_diagnostics: None,
            observed_startup_item: None,
            diagnostics_export: None,
        }
    }

    fn observe_runtime(&mut self, runtime_revision: u64) {
        if runtime_revision != self.observed_runtime_revision {
            self.revision = self.revision.saturating_add(1);
            self.observed_runtime_revision = runtime_revision;
        }
    }

    fn observe_config(&mut self, config_revision: Option<u64>) {
        if config_revision != self.observed_config_revision {
            self.revision = self.revision.saturating_add(1);
            self.observed_config_revision = config_revision;
        }
    }

    fn mark_catalog_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn observe_input_diagnostics(&mut self, diagnostics: SettingsInputDiagnostics) {
        match self.observed_input_diagnostics.replace(diagnostics) {
            Some(previous) if previous != diagnostics => {
                self.revision = self.revision.saturating_add(1);
            }
            Some(_) | None => {}
        }
    }

    fn observe_startup_item(&mut self, status: SettingsStartupItemStatus) {
        match self.observed_startup_item.replace(status) {
            Some(previous) if previous != status => {
                self.revision = self.revision.saturating_add(1);
            }
            Some(_) | None => {}
        }
    }

    fn observe_diagnostics_export(&mut self, status: SettingsDiagnosticsExportStatus) {
        if self.diagnostics_export != Some(status) {
            self.revision = self.revision.saturating_add(1);
            self.diagnostics_export = Some(status);
        }
    }

    fn coalesce_changes_since(&mut self, revision: u64) {
        if self.revision != revision {
            self.revision = revision.saturating_add(1);
        }
    }
}

fn snapshot(
    application: &Application,
    clock: &mut SettingsSnapshotClock,
    catalog_changed: bool,
    startup_item: SettingsStartupItemStatus,
) -> SettingsSnapshot {
    let revision_before = clock.revision;
    let runtime = application.runtime_client().snapshot();
    let input_diagnostics = settings_input_diagnostics(&runtime.input, runtime.platform_input);
    clock.observe_runtime(runtime.revision);
    clock.observe_config(application.config_revision());
    clock.observe_input_diagnostics(input_diagnostics);
    clock.observe_startup_item(startup_item);
    if catalog_changed {
        clock.mark_catalog_changed();
    }
    clock.coalesce_changes_since(revision_before);
    SettingsSnapshot {
        revision: clock.revision,
        config_revision: application.config_revision(),
        runtime_health: if input_service_is_degraded(input_diagnostics.service_status) {
            RuntimeHealth::Degraded
        } else if application.is_operational() {
            match runtime.state {
                RuntimeState::Starting => RuntimeHealth::Starting,
                RuntimeState::Ready => RuntimeHealth::Ready,
                RuntimeState::Degraded | RuntimeState::Stopping => RuntimeHealth::Degraded,
                RuntimeState::Stopped => RuntimeHealth::Stopped,
            }
        } else {
            RuntimeHealth::Degraded
        },
        runtime_diagnostics: settings_runtime_diagnostics(&runtime),
        appearance_theme: settings_theme(application.config().appearance.theme),
        language: settings_language(application.config().appearance.language),
        resolved_language: settings_language(application.effective_language()),
        status_icon_visible: application.config().application.show_status_icon,
        taskbar_icon_visible: application.config().application.show_taskbar_icon,
        check_for_updates_automatically: application
            .config()
            .application
            .check_for_updates_automatically,
        overlay_visible: runtime.overlay_visible,
        overlay: SettingsOverlay {
            click_through: runtime.overlay_settings.click_through,
            always_on_top: runtime.overlay_settings.always_on_top,
            scale_percent: runtime.overlay_settings.scale_percent,
            opacity_percent: runtime.overlay_settings.opacity_percent,
            keep_inside_work_area: runtime.overlay_settings.keep_inside_work_area,
        },
        motion_audio_enabled: runtime.motion_audio_enabled,
        behavior_shortcuts_enabled: application.config().model.enable_behavior_shortcuts,
        maximum_fps: runtime.maximum_fps,
        release_fallback_timeout_ms: runtime.release_fallback_timeout_ms,
        model_settings: SettingsModelSettings {
            mirror: runtime.model_settings.mirror,
            mirror_pointer_tracking: runtime.model_settings.mirror_pointer_tracking,
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
        shortcuts: settings_shortcuts(application.config()),
        startup_item,
        configuration_status: match application.config_status() {
            ApplicationConfigStatus::Ready => SettingsConfigurationStatus::Ready,
            ApplicationConfigStatus::RecoveryRequired { checked_backups } => {
                SettingsConfigurationStatus::RecoveryRequired {
                    checked_backups: u32::try_from(checked_backups).unwrap_or(u32::MAX),
                }
            }
            ApplicationConfigStatus::DefaultsRestoredRestartRequired => {
                SettingsConfigurationStatus::DefaultsRestoredRestartRequired
            }
        },
        config_recovery: application
            .config_recovery()
            .map(|recovery| SettingsConfigRecovery {
                source_schema_version: recovery.source_schema_version(),
                skipped_newer_backups: recovery.skipped_newer_backups(),
            }),
        diagnostics_export: clock.diagnostics_export,
        input_diagnostics,
        active_model: runtime
            .active_model
            .and_then(|model| {
                application
                    .active_model_origin()
                    .map(|origin| SettingsModelKey {
                        id: model.id.as_str().to_owned(),
                        origin: settings_model_origin(origin),
                    })
            })
            .or_else(|| configured_model_key(application)),
        model_catalog: settings_model_catalog(application),
    }
}

const fn settings_theme(theme: bongocat_config::Theme) -> SettingsTheme {
    match theme {
        bongocat_config::Theme::System => SettingsTheme::System,
        bongocat_config::Theme::Light => SettingsTheme::Light,
        bongocat_config::Theme::Dark => SettingsTheme::Dark,
    }
}

const fn config_theme(theme: SettingsTheme) -> bongocat_config::Theme {
    match theme {
        SettingsTheme::System => bongocat_config::Theme::System,
        SettingsTheme::Light => bongocat_config::Theme::Light,
        SettingsTheme::Dark => bongocat_config::Theme::Dark,
    }
}

const fn settings_language(language: bongocat_config::Language) -> SettingsLanguage {
    match language {
        bongocat_config::Language::System => SettingsLanguage::System,
        bongocat_config::Language::ChineseSimplified => SettingsLanguage::ChineseSimplified,
        bongocat_config::Language::EnglishUnitedStates => SettingsLanguage::EnglishUnitedStates,
    }
}

const fn config_language(language: SettingsLanguage) -> bongocat_config::Language {
    match language {
        SettingsLanguage::System => bongocat_config::Language::System,
        SettingsLanguage::ChineseSimplified => bongocat_config::Language::ChineseSimplified,
        SettingsLanguage::EnglishUnitedStates => bongocat_config::Language::EnglishUnitedStates,
    }
}

fn settings_shortcuts(config: &NativeConfig) -> SettingsShortcuts {
    SettingsShortcuts {
        commands: config
            .shortcuts
            .commands
            .iter()
            .map(|binding| SettingsShortcutBinding {
                command: binding.command.clone(),
                shortcut: binding.shortcut.clone(),
            })
            .collect(),
        model_behaviors: config
            .shortcuts
            .model_behaviors
            .iter()
            .map(|binding| SettingsModelBehaviorBinding {
                model_id: binding.model_id.clone(),
                behavior_id: binding.behavior_id.clone(),
                shortcut: binding.shortcut.clone(),
            })
            .collect(),
    }
}

fn settings_shortcut(command: ShortcutCommand) -> Option<SettingsApplicationShortcut> {
    Some(match command {
        ShortcutCommand::ToggleOverlay => SettingsApplicationShortcut::ToggleOverlay,
        ShortcutCommand::ToggleMirror => SettingsApplicationShortcut::ToggleMirror,
        ShortcutCommand::ToggleClickThrough => SettingsApplicationShortcut::ToggleClickThrough,
        ShortcutCommand::ToggleAlwaysOnTop => SettingsApplicationShortcut::ToggleAlwaysOnTop,
        ShortcutCommand::OpenSettings => SettingsApplicationShortcut::OpenSettings,
    })
}

fn apply_application_shortcut(
    application: &mut Application,
    command: SettingsApplicationShortcut,
) -> Result<(), ApplicationError> {
    match command {
        SettingsApplicationShortcut::OpenSettings => return Ok(()),
        SettingsApplicationShortcut::ToggleOverlay => {
            application.set_overlay_visible(!application.config().overlay.visible)?;
        }
        SettingsApplicationShortcut::ToggleMirror => {
            let settings = application.runtime_client().snapshot().model_settings;
            application.set_model_settings(ModelSettings {
                mirror: !settings.mirror,
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

const fn settings_runtime_error_code(code: RuntimeRenderErrorCode) -> SettingsRuntimeErrorCode {
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
        RuntimeRenderErrorCode::PlatformUnsupported => {
            SettingsRuntimeErrorCode::PlatformUnsupported
        }
        RuntimeRenderErrorCode::TransportClosed => SettingsRuntimeErrorCode::TransportClosed,
        RuntimeRenderErrorCode::OverlaySettingsInvalid => {
            SettingsRuntimeErrorCode::OverlaySettingsInvalid
        }
        RuntimeRenderErrorCode::MaximumFpsInvalid => SettingsRuntimeErrorCode::MaximumFpsInvalid,
        RuntimeRenderErrorCode::ReleaseFallbackTimeoutInvalid => {
            SettingsRuntimeErrorCode::ReleaseFallbackTimeoutInvalid
        }
    }
}

fn settings_runtime_diagnostics(
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
    }
}

fn settings_input_diagnostics(
    input: &InputSnapshot,
    platform: PlatformInputDiagnostics,
) -> SettingsInputDiagnostics {
    SettingsInputDiagnostics {
        input_monitoring_permission: system_input_monitoring_permission(),
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
        service_start_attempts: platform.service_start_attempts,
        pressed_key_count: input.pressed_key_count,
        pressed_mouse_button_count: input.pressed_mouse_button_count,
        pressed_gamepad_button_count: input.pressed_gamepad_button_count,
        connected_gamepad_count: input.connected_gamepad_count,
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
fn system_input_monitoring_permission() -> SettingsInputMonitoringPermission {
    match input_monitoring_permission() {
        InputPermission::Denied => SettingsInputMonitoringPermission::Denied,
        InputPermission::Granted => SettingsInputMonitoringPermission::Granted,
    }
}

#[cfg(not(target_os = "macos"))]
const fn system_input_monitoring_permission() -> SettingsInputMonitoringPermission {
    SettingsInputMonitoringPermission::Unsupported
}

const fn input_service_is_degraded(status: SettingsInputServiceStatus) -> bool {
    matches!(
        status,
        SettingsInputServiceStatus::PermissionDenied
            | SettingsInputServiceStatus::BackendUnavailable
            | SettingsInputServiceStatus::Failed
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_startup_item_state() -> SettingsStartupItemStatus {
    startup_item_state(startup_item_environment())
        .map(settings_startup_item_state)
        .map(SettingsStartupItemStatus::State)
        .unwrap_or_else(|error| {
            SettingsStartupItemStatus::ReadError(settings_startup_item_error(error))
        })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn system_startup_item_state() -> SettingsStartupItemStatus {
    SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
        SettingsStartupItemUnsupportedReason::Platform,
    ))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_set_startup_item_enabled(
    enabled: bool,
) -> Result<SettingsStartupItemState, SettingsError> {
    set_startup_item_enabled(startup_item_environment(), enabled)
        .map(settings_startup_item_state)
        .map_err(|_| SettingsError::new(SettingsErrorCode::StartupItemUpdateFailed))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_set_startup_item_enabled(
    _enabled: bool,
) -> Result<SettingsStartupItemState, SettingsError> {
    Err(SettingsError::new(
        SettingsErrorCode::StartupItemUpdateFailed,
    ))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn startup_item_environment() -> StartupItemEnvironment {
    match crate::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => StartupItemEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => StartupItemEnvironment::Production,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn settings_startup_item_state(state: StartupItemState) -> SettingsStartupItemState {
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn settings_startup_item_error(error: StartupItemError) -> SettingsStartupItemError {
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

fn settings_model_catalog(application: &Application) -> SettingsModelCatalog {
    match application.model_catalog() {
        Ok(entries) => SettingsModelCatalog {
            entries: entries.into_iter().map(settings_model_entry).collect(),
            error: None,
        },
        Err(_) => SettingsModelCatalog {
            entries: Vec::new(),
            error: Some(SettingsModelCatalogError::Unavailable),
        },
    }
}

fn configured_model_key(application: &Application) -> Option<SettingsModelKey> {
    let id = application.config().model.selected_model_id.clone()?;
    let origin = application.config().model.selected_model_origin?;
    Some(SettingsModelKey {
        id,
        origin: match origin {
            bongocat_config::SelectedModelOrigin::Preset => SettingsModelOrigin::Preset,
            bongocat_config::SelectedModelOrigin::Installed => SettingsModelOrigin::Installed,
        },
    })
}

const fn settings_model_origin(origin: ModelOrigin) -> SettingsModelOrigin {
    match origin {
        ModelOrigin::Preset => SettingsModelOrigin::Preset,
        ModelOrigin::Installed => SettingsModelOrigin::Installed,
    }
}

const fn model_origin(origin: SettingsModelOrigin) -> ModelOrigin {
    match origin {
        SettingsModelOrigin::Preset => ModelOrigin::Preset,
        SettingsModelOrigin::Installed => ModelOrigin::Installed,
    }
}

fn settings_model_entry(entry: ModelCatalogEntry) -> SettingsModelEntry {
    let id = entry.id().as_str().to_owned();
    let origin = match entry.origin() {
        ModelOrigin::Preset => SettingsModelOrigin::Preset,
        ModelOrigin::Installed => SettingsModelOrigin::Installed,
    };
    let availability = match entry {
        ModelCatalogEntry::Ready { snapshot, .. } => SettingsModelAvailability::Ready {
            texture_count: snapshot.texture_count,
            expression_count: snapshot.expression_count,
            motion_count: snapshot.motion_count,
        },
        ModelCatalogEntry::Invalid { code, .. } => SettingsModelAvailability::Invalid {
            diagnostic: settings_model_diagnostic(code),
        },
    };
    SettingsModelEntry {
        id,
        origin,
        availability,
    }
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
}

#[derive(Serialize)]
struct DiagnosticsInput {
    service_status: &'static str,
    service_start_attempts: u64,
    pressed_key_count: usize,
    pressed_mouse_button_count: usize,
    pressed_gamepad_button_count: usize,
    connected_gamepad_count: usize,
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
    checked_backups: Option<u32>,
    recovery_source_schema_version: Option<u32>,
    recovery_skipped_newer_backups: Option<u32>,
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
}

fn export_diagnostics_file(
    path: &std::path::Path,
    snapshot: &SettingsSnapshot,
    application_logs: ApplicationLogDiagnostics,
) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
    let document = diagnostics_document(snapshot, application_logs);
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    set_private_directory(parent)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut options = AtomicWriteFile::options();
        options.preserve_mode(false).mode(0o600);
        options
    };
    #[cfg(not(unix))]
    let options = AtomicWriteFile::options();
    let mut file = options
        .open(path)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    std::io::Write::write_all(&mut file, &bytes)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    file.commit()
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    set_private_path(path)
        .map_err(|_| SettingsError::new(SettingsErrorCode::DiagnosticsExportFailed))?;
    Ok(SettingsDiagnosticsExportStatus {
        format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
        bytes_written: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    })
}

fn set_private_directory(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn set_private_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn diagnostics_document(
    snapshot: &SettingsSnapshot,
    application_logs: ApplicationLogDiagnostics,
) -> DiagnosticsExportDocument {
    let input = snapshot.input_diagnostics;
    let runtime = snapshot.runtime_diagnostics;
    let mut invalid_diagnostic_codes = Vec::new();
    let mut ready_preset = 0_u64;
    let mut ready_installed = 0_u64;
    let mut invalid_preset = 0_u64;
    let mut invalid_installed = 0_u64;
    for entry in &snapshot.model_catalog.entries {
        let is_preset = entry.origin == SettingsModelOrigin::Preset;
        match entry.availability {
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
        },
        input: diagnostics_input(input),
        configuration: diagnostics_configuration(snapshot),
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
                    SettingsModelOrigin::Preset => "preset",
                    SettingsModelOrigin::Installed => "installed",
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
            },
        },
    }
}

const fn runtime_health_code(health: RuntimeHealth) -> &'static str {
    match health {
        RuntimeHealth::Starting => "starting",
        RuntimeHealth::Ready => "ready",
        RuntimeHealth::Degraded => "degraded",
        RuntimeHealth::Stopped => "stopped",
    }
}

const fn input_service_status_code(status: SettingsInputServiceStatus) -> &'static str {
    match status {
        SettingsInputServiceStatus::NotStarted => "not_started",
        SettingsInputServiceStatus::Running => "running",
        SettingsInputServiceStatus::PermissionDenied => "permission_denied",
        SettingsInputServiceStatus::BackendUnavailable => "backend_unavailable",
        SettingsInputServiceStatus::Failed => "failed",
        SettingsInputServiceStatus::Stopped => "stopped",
    }
}

const fn diagnostics_input(input: SettingsInputDiagnostics) -> DiagnosticsInput {
    DiagnosticsInput {
        service_status: input_service_status_code(input.service_status),
        service_start_attempts: input.service_start_attempts,
        pressed_key_count: input.pressed_key_count,
        pressed_mouse_button_count: input.pressed_mouse_button_count,
        pressed_gamepad_button_count: input.pressed_gamepad_button_count,
        connected_gamepad_count: input.connected_gamepad_count,
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

fn diagnostics_configuration(snapshot: &SettingsSnapshot) -> DiagnosticsConfiguration {
    let (status, checked_backups) = match snapshot.configuration_status {
        SettingsConfigurationStatus::Ready => ("ready", None),
        SettingsConfigurationStatus::RecoveryRequired { checked_backups } => {
            ("recovery_required", Some(checked_backups))
        }
        SettingsConfigurationStatus::DefaultsRestoredRestartRequired => {
            ("defaults_restored_restart_required", None)
        }
    };
    DiagnosticsConfiguration {
        status,
        checked_backups,
        recovery_source_schema_version: snapshot
            .config_recovery
            .map(|recovery| recovery.source_schema_version),
        recovery_skipped_newer_backups: snapshot
            .config_recovery
            .map(|recovery| recovery.skipped_newer_backups),
    }
}

const fn settings_model_diagnostic(diagnostic: ModelDiagnostic) -> SettingsModelDiagnostic {
    match diagnostic {
        ModelDiagnostic::InvalidModelId => SettingsModelDiagnostic::InvalidModelId,
        ModelDiagnostic::ModelEntryAmbiguous => SettingsModelDiagnostic::ModelEntryAmbiguous,
        ModelDiagnostic::ModelEntryMissing => SettingsModelDiagnostic::ModelEntryMissing,
        ModelDiagnostic::ModelFileCountExceeded => SettingsModelDiagnostic::ModelFileCountExceeded,
        ModelDiagnostic::ModelFileTooLarge => SettingsModelDiagnostic::ModelFileTooLarge,
        ModelDiagnostic::ModelIoError => SettingsModelDiagnostic::ModelIoError,
        ModelDiagnostic::ModelJsonInvalid => SettingsModelDiagnostic::ModelJsonInvalid,
        ModelDiagnostic::ModelJsonTooLarge => SettingsModelDiagnostic::ModelJsonTooLarge,
        ModelDiagnostic::ModelMocMissing => SettingsModelDiagnostic::ModelMocMissing,
        ModelDiagnostic::ModelPackageDepthExceeded => {
            SettingsModelDiagnostic::ModelPackageDepthExceeded
        }
        ModelDiagnostic::ModelPackageSizeExceeded => {
            SettingsModelDiagnostic::ModelPackageSizeExceeded
        }
        ModelDiagnostic::ModelReferenceEscapesRoot => {
            SettingsModelDiagnostic::ModelReferenceEscapesRoot
        }
        ModelDiagnostic::ModelReferenceInvalid => SettingsModelDiagnostic::ModelReferenceInvalid,
        ModelDiagnostic::ModelReferenceSymlinkEscape => {
            SettingsModelDiagnostic::ModelReferenceSymlinkEscape
        }
        ModelDiagnostic::ModelResourceInvalid => SettingsModelDiagnostic::ModelResourceInvalid,
        ModelDiagnostic::ModelResourceMissing => SettingsModelDiagnostic::ModelResourceMissing,
        ModelDiagnostic::ModelResourceNotFile => SettingsModelDiagnostic::ModelResourceNotFile,
        ModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
            SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported
        }
        ModelDiagnostic::ModelTextureDimensionExceeded => {
            SettingsModelDiagnostic::ModelTextureDimensionExceeded
        }
        ModelDiagnostic::ModelTextureInvalidPng => SettingsModelDiagnostic::ModelTextureInvalidPng,
        ModelDiagnostic::ModelTextureMissing => SettingsModelDiagnostic::ModelTextureMissing,
        ModelDiagnostic::ModelUnsupportedVersion => {
            SettingsModelDiagnostic::ModelUnsupportedVersion
        }
    }
}

fn map_application_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::PlatformStorage(_) => SettingsErrorCode::ConfigPersistFailed,
        ApplicationError::Config(error) | ApplicationError::ConfigRollback(error) => {
            settings_config_error_code(&error).unwrap_or(SettingsErrorCode::ConfigPersistFailed)
        }
        ApplicationError::State(_) => SettingsErrorCode::StatePersistFailed,
        ApplicationError::ConfigurationRecoveryRequired => {
            SettingsErrorCode::ConfigurationRecoveryRequired
        }
        ApplicationError::Model(_) | ApplicationError::ModelStore(_) => {
            SettingsErrorCode::ModelUnavailable
        }
        ApplicationError::Shutdown(_) | ApplicationError::MotionAudioShutdown(_) => {
            SettingsErrorCode::ShutdownFailed
        }
        ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish
        | ApplicationError::RuntimeDidNotPrepareModel => SettingsErrorCode::ModelSwitchFailed,
        _ => SettingsErrorCode::RuntimeUnavailable,
    };
    SettingsError::new(code)
}

fn map_configuration_recovery_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Config(error) => settings_config_error_code(&error)
            .unwrap_or(SettingsErrorCode::ConfigurationRecoveryFailed),
        _ => SettingsErrorCode::ConfigurationRecoveryFailed,
    };
    SettingsError::new(code)
}

fn settings_config_error_code(error: &ConfigError) -> Option<SettingsErrorCode> {
    if matches!(error, ConfigError::InvalidValue(field) if field.starts_with("shortcuts.")) {
        return Some(SettingsErrorCode::InvalidShortcutBindings);
    }
    match error.write_failure_reason()? {
        ConfigWriteFailureReason::PermissionDenied => {
            Some(SettingsErrorCode::ConfigPermissionDenied)
        }
        ConfigWriteFailureReason::StorageFull => Some(SettingsErrorCode::ConfigStorageFull),
        ConfigWriteFailureReason::TargetOccupied => Some(SettingsErrorCode::ConfigTargetOccupied),
    }
}

fn map_model_import_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) => {
            if error.code == ModelDiagnostic::InvalidModelId {
                SettingsErrorCode::InvalidModelId
            } else {
                SettingsErrorCode::ModelImportInvalidPackage
            }
        }
        ApplicationError::ModelStore(error) => map_model_store_import_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

const fn map_model_store_import_diagnostic(diagnostic: ModelStoreDiagnostic) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::AlreadyExists => SettingsErrorCode::ModelAlreadyInstalled,
        ModelStoreDiagnostic::Cancelled => SettingsErrorCode::ModelImportCancelled,
        ModelStoreDiagnostic::InvalidPackage => SettingsErrorCode::ModelImportInvalidPackage,
        ModelStoreDiagnostic::SourceContainsStore => SettingsErrorCode::ModelImportSourceInvalid,
        ModelStoreDiagnostic::SourceChanged => SettingsErrorCode::ModelImportSourceChanged,
        ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported => {
            SettingsErrorCode::ModelImportSourceUnsupported
        }
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::NotFound
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelImportFailed,
    }
}

fn map_model_delete_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::PresetModelDeletion(_) => SettingsErrorCode::PresetModelCannotBeDeleted,
        ApplicationError::SelectedModelDeletion(_) => {
            SettingsErrorCode::SelectedModelCannotBeDeleted
        }
        ApplicationError::ModelStore(error) => map_model_store_delete_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

const fn map_model_store_delete_diagnostic(diagnostic: ModelStoreDiagnostic) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::NotFound => SettingsErrorCode::ModelNotInstalled,
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::AlreadyExists
        | ModelStoreDiagnostic::Cancelled
        | ModelStoreDiagnostic::InvalidPackage
        | ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::SourceContainsStore
        | ModelStoreDiagnostic::SourceChanged
        | ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelDeleteFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApplicationLogEventCounts;
    use bongocat_config::{ConfigStore, OverlayWindowPlacement, StateStore, StorageLayout};
    use bongocat_runtime::{InputDiagnostics, InputTransportDiagnostics};
    use bongocat_ui::{SettingsModelImportRequest, SettingsStartupItemError};
    use std::{
        io,
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };
    use tempfile::tempdir;

    struct TestStartupItem {
        status: Mutex<SettingsStartupItemStatus>,
        fail_updates: AtomicBool,
    }

    impl TestStartupItem {
        fn new(status: SettingsStartupItemStatus) -> Self {
            Self {
                status: Mutex::new(status),
                fail_updates: AtomicBool::new(false),
            }
        }

        fn replace(&self, status: SettingsStartupItemStatus) {
            *self
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
        }
    }

    impl StartupItemCapability for TestStartupItem {
        fn state(&self) -> SettingsStartupItemStatus {
            *self
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError> {
            if self.fail_updates.load(Ordering::Acquire) {
                return Err(SettingsError::new(
                    SettingsErrorCode::StartupItemUpdateFailed,
                ));
            }
            let state = if enabled {
                SettingsStartupItemState::Enabled
            } else {
                SettingsStartupItemState::Disabled
            };
            self.replace(SettingsStartupItemStatus::State(state));
            Ok(state)
        }
    }

    struct TestBackupLocation {
        invocations: AtomicUsize,
        fail: AtomicBool,
    }

    struct TestStatusIcon {
        visible: Mutex<bool>,
        updates: Mutex<Vec<bool>>,
        fail_updates: AtomicBool,
    }

    struct TestTaskbarIcon {
        visible: Mutex<bool>,
        updates: Mutex<Vec<bool>>,
        fail_updates: AtomicBool,
    }

    impl TestStatusIcon {
        fn new(visible: bool) -> Self {
            Self {
                visible: Mutex::new(visible),
                updates: Mutex::new(Vec::new()),
                fail_updates: AtomicBool::new(false),
            }
        }

        fn visible(&self) -> bool {
            *self
                .visible
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn updates(&self) -> Vec<bool> {
            self.updates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl TestTaskbarIcon {
        fn new(visible: bool) -> Self {
            Self {
                visible: Mutex::new(visible),
                updates: Mutex::new(Vec::new()),
                fail_updates: AtomicBool::new(false),
            }
        }

        fn visible(&self) -> bool {
            *self
                .visible
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn updates(&self) -> Vec<bool> {
            self.updates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl StatusIconCapability for TestStatusIcon {
        fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
            self.updates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(visible);
            if self.fail_updates.load(Ordering::Acquire) {
                return Err(SettingsError::new(
                    SettingsErrorCode::StatusIconUpdateFailed,
                ));
            }
            *self
                .visible
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = visible;
            Ok(())
        }
    }

    impl TaskbarIconCapability for TestTaskbarIcon {
        fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
            self.updates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(visible);
            if self.fail_updates.load(Ordering::Acquire) {
                return Err(SettingsError::new(
                    SettingsErrorCode::TaskbarIconUpdateFailed,
                ));
            }
            *self
                .visible
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = visible;
            Ok(())
        }
    }

    impl TestBackupLocation {
        fn new() -> Self {
            Self {
                invocations: AtomicUsize::new(0),
                fail: AtomicBool::new(false),
            }
        }
    }

    impl BackupLocationCapability for TestBackupLocation {
        fn open(&self) -> Result<(), SettingsError> {
            self.invocations.fetch_add(1, Ordering::AcqRel);
            if self.fail.load(Ordering::Acquire) {
                Err(SettingsError::new(
                    SettingsErrorCode::BackupLocationOpenFailed,
                ))
            } else {
                Ok(())
            }
        }
    }

    struct TestDiagnosticsExport;

    impl DiagnosticsExportCapability for TestDiagnosticsExport {
        fn export(
            &self,
            _snapshot: &SettingsSnapshot,
            _application_logs: ApplicationLogDiagnostics,
        ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
            Ok(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 1,
            })
        }
    }

    #[test]
    fn diagnostics_export_is_atomic_aggregated_and_path_free() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("diagnostics directory");
        let path = directory.path().join("logs").join("diagnostics.json");
        let snapshot = SettingsSnapshot {
            revision: 42,
            config_revision: Some(7),
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
            },
            appearance_theme: SettingsTheme::System,
            language: SettingsLanguage::System,
            resolved_language: SettingsLanguage::EnglishUnitedStates,
            status_icon_visible: true,
            taskbar_icon_visible: true,
            check_for_updates_automatically: true,
            overlay_visible: true,
            overlay: SettingsOverlay::default(),
            motion_audio_enabled: true,
            behavior_shortcuts_enabled: true,
            maximum_fps: 60,
            release_fallback_timeout_ms: 500,
            model_settings: bongocat_ui::SettingsModelSettings::default(),
            gamepad_axis_settings: bongocat_ui::SettingsGamepadAxisSettings::default(),
            shortcuts: SettingsShortcuts::default(),
            startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            configuration_status: SettingsConfigurationStatus::Ready,
            config_recovery: None,
            diagnostics_export: None,
            input_diagnostics: SettingsInputDiagnostics {
                captured_down: 3,
                transport_queue_full: 2,
                ..SettingsInputDiagnostics::default()
            },
            active_model: Some(SettingsModelKey {
                id: "private-model-name".to_owned(),
                origin: SettingsModelOrigin::Installed,
            }),
            model_catalog: SettingsModelCatalog {
                entries: vec![
                    SettingsModelEntry {
                        id: "private-model-name".to_owned(),
                        origin: SettingsModelOrigin::Installed,
                        availability: SettingsModelAvailability::Ready {
                            texture_count: 1,
                            expression_count: 0,
                            motion_count: 0,
                        },
                    },
                    SettingsModelEntry {
                        id: "broken-private-model".to_owned(),
                        origin: SettingsModelOrigin::Installed,
                        availability: SettingsModelAvailability::Invalid {
                            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
                        },
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
        )
        .expect("export diagnostics");
        let bytes = std::fs::read(&path).expect("read exported diagnostics");
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path)
                .expect("diagnostics metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(status.format_version, DIAGNOSTICS_EXPORT_FORMAT_VERSION);
        assert_eq!(status.bytes_written, bytes.len() as u64);
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(document["format_version"], 1);
        assert_eq!(document["settings_revision"], 42);
        assert_eq!(
            document["runtime"]["render_error_code"],
            "gpu_preparation_failed"
        );
        assert_eq!(document["runtime"]["command_enqueued"], 11);
        assert_eq!(document["runtime"]["command_queue_full"], 2);
        assert_eq!(document["runtime"]["command_sequence_gap_count"], 3);
        assert_eq!(document["runtime"]["command_missing_sequence_count"], 5);
        assert_eq!(document["input"]["transport_queue_full"], 2);
        assert_eq!(document["models"]["ready_installed"], 1);
        assert_eq!(document["application_logs"]["written"], 3);
        assert_eq!(document["application_logs"]["dropped"], 1);
        assert_eq!(document["application_logs"]["events"]["started"], 1);
        assert_eq!(document["application_logs"]["events"]["panicked"], 2);
        assert_eq!(document["models"]["invalid_installed"], 1);
        assert_eq!(
            document["models"]["invalid_diagnostic_codes"][0]["code"],
            "model_json_invalid"
        );
        let text = String::from_utf8(bytes).expect("UTF-8 export");
        assert!(!text.contains("private-model-name"));
        assert!(!text.contains(directory.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn config_write_failures_map_to_stable_settings_codes() {
        for (error, expected) in [
            (
                ConfigError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
                SettingsErrorCode::ConfigPermissionDenied,
            ),
            (
                ConfigError::Io(io::Error::from(io::ErrorKind::StorageFull)),
                SettingsErrorCode::ConfigStorageFull,
            ),
            (
                ConfigError::WriteTargetOccupied,
                SettingsErrorCode::ConfigTargetOccupied,
            ),
        ] {
            assert_eq!(settings_config_error_code(&error), Some(expected));
            assert_eq!(
                map_application_error(ApplicationError::Config(error)).code(),
                expected
            );
        }
        assert_eq!(
            map_configuration_recovery_error(ApplicationError::Config(ConfigError::Io(
                io::Error::from(io::ErrorKind::StorageFull)
            )))
            .code(),
            SettingsErrorCode::ConfigStorageFull
        );
    }

    #[test]
    fn input_diagnostics_projection_is_complete_and_advances_its_own_revision() {
        let input = InputSnapshot {
            pressed_key_count: 1,
            pressed_mouse_button_count: 2,
            pressed_gamepad_button_count: 20,
            connected_gamepad_count: 21,
            diagnostics: InputDiagnostics {
                captured_down: 3,
                captured_up: 4,
                reconciled_release: 5,
                fallback_release: 26,
                released_by_reset: 6,
                duplicate_down: 7,
                unmatched_release: 8,
                invalid_source: 9,
                reset_count: 10,
                sequence_gap_count: 11,
                missing_sequence_count: 12,
                duplicate_sequence_count: 13,
                out_of_order_sequence_count: 14,
                non_monotonic_time_count: 15,
                gamepad_connections: 22,
                gamepad_disconnections: 23,
                stale_gamepad_events: 24,
                released_by_disconnect: 25,
            },
            transport: InputTransportDiagnostics {
                enqueued: 16,
                queue_full: 17,
                recovered_after_overflow: 18,
                runtime_stopped: 19,
            },
            ..InputSnapshot::default()
        };
        let projected = settings_input_diagnostics(
            &input,
            PlatformInputDiagnostics {
                service_status: PlatformInputServiceStatus::PermissionDenied,
                service_start_attempts: 1,
                ..PlatformInputDiagnostics::default()
            },
        );
        assert_eq!(
            projected.service_status,
            SettingsInputServiceStatus::PermissionDenied
        );
        assert_eq!(projected.service_start_attempts, 1);
        assert_eq!(projected.pressed_key_count, 1);
        assert_eq!(projected.pressed_mouse_button_count, 2);
        assert_eq!(projected.pressed_gamepad_button_count, 20);
        assert_eq!(projected.connected_gamepad_count, 21);
        assert_eq!(projected.captured_down, 3);
        assert_eq!(projected.captured_up, 4);
        assert_eq!(projected.reconciled_release, 5);
        assert_eq!(projected.fallback_release, 26);
        assert_eq!(projected.released_by_reset, 6);
        assert_eq!(projected.duplicate_down, 7);
        assert_eq!(projected.unmatched_release, 8);
        assert_eq!(projected.invalid_source, 9);
        assert_eq!(projected.reset_count, 10);
        assert_eq!(projected.sequence_gap_count, 11);
        assert_eq!(projected.missing_sequence_count, 12);
        assert_eq!(projected.duplicate_sequence_count, 13);
        assert_eq!(projected.out_of_order_sequence_count, 14);
        assert_eq!(projected.non_monotonic_time_count, 15);
        assert_eq!(projected.gamepad_connections, 22);
        assert_eq!(projected.gamepad_disconnections, 23);
        assert_eq!(projected.stale_gamepad_events, 24);
        assert_eq!(projected.released_by_disconnect, 25);
        assert_eq!(projected.transport_enqueued, 16);
        assert_eq!(projected.transport_queue_full, 17);
        assert_eq!(projected.transport_recovered_after_overflow, 18);
        assert_eq!(projected.transport_runtime_stopped, 19);

        let mut clock = SettingsSnapshotClock::new(40, Some(7));
        clock.observe_input_diagnostics(projected);
        assert_eq!(clock.revision, 40);
        let changed = SettingsInputDiagnostics {
            transport_queue_full: 20,
            ..projected
        };
        clock.observe_input_diagnostics(changed);
        assert_eq!(clock.revision, 41);
        clock.observe_input_diagnostics(changed);
        assert_eq!(clock.revision, 41);
    }

    #[test]
    fn snapshot_clock_coalesces_changes_observed_in_one_snapshot() {
        let diagnostics = SettingsInputDiagnostics::default();
        let startup = SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled);
        let mut clock = SettingsSnapshotClock::new(40, Some(7));
        clock.observe_runtime(41);
        clock.observe_config(Some(8));
        clock.observe_input_diagnostics(diagnostics);
        clock.observe_startup_item(startup);
        clock.mark_catalog_changed();
        clock.observe_diagnostics_export(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 1,
        });
        clock.coalesce_changes_since(40);
        assert_eq!(clock.revision, 41);

        clock.coalesce_changes_since(41);
        assert_eq!(clock.revision, 41);
    }

    #[test]
    fn input_start_failures_degrade_health_without_treating_stop_as_failure() {
        for status in [
            SettingsInputServiceStatus::PermissionDenied,
            SettingsInputServiceStatus::BackendUnavailable,
            SettingsInputServiceStatus::Failed,
        ] {
            assert!(input_service_is_degraded(status));
        }
        for status in [
            SettingsInputServiceStatus::NotStarted,
            SettingsInputServiceStatus::Running,
            SettingsInputServiceStatus::Stopped,
        ] {
            assert!(!input_service_is_degraded(status));
        }
    }

    #[test]
    fn service_projects_anonymous_configuration_recovery_across_refresh_and_shutdown() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.visible = false;
        store.commit(&config).expect("hidden config commit");
        config.overlay.visible = true;
        store.commit(&config).expect("visible config commit");
        std::fs::write(&layout.config, b"corrupt-current").expect("corrupt current config");

        let application = Application::start_with_layout(layout).expect("recovered application");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let expected = Some(SettingsConfigRecovery {
            source_schema_version: bongocat_config::SCHEMA_VERSION,
            skipped_newer_backups: 0,
        });

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(initial.config_recovery, expected);
        let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
        assert_eq!(refreshed.config_recovery, expected);
        assert_eq!(refreshed.revision, initial.revision);
        let stopped = client.shutdown_blocking().expect("service shutdown");
        assert_eq!(stopped.config_recovery, expected);
        service.join().expect("service join");
    }

    #[test]
    fn service_restricts_commands_until_invalid_configuration_is_explicitly_replaced() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let invalid = b"invalid-current-without-backup";
        std::fs::write(&layout.config, invalid).expect("invalid current config");

        let application = Application::start_with_layout(layout.clone()).expect("safe mode start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("safe mode snapshot");
        assert_eq!(
            initial.configuration_status,
            SettingsConfigurationStatus::RecoveryRequired { checked_backups: 0 }
        );
        assert_eq!(initial.runtime_health, RuntimeHealth::Degraded);
        assert_eq!(
            client
                .set_overlay_visible_blocking(0, true)
                .expect_err("business command rejected")
                .code(),
            SettingsErrorCode::ConfigurationRecoveryRequired
        );
        assert_eq!(
            std::fs::read(&layout.config).expect("preserved invalid"),
            invalid
        );

        let recovered = client
            .restore_default_configuration_blocking()
            .expect("restore defaults command");
        assert_eq!(
            recovered.configuration_status,
            SettingsConfigurationStatus::DefaultsRestoredRestartRequired
        );
        assert_eq!(recovered.runtime_health, RuntimeHealth::Degraded);
        assert_eq!(
            client
                .restore_default_configuration_blocking()
                .expect_err("second restore rejected")
                .code(),
            SettingsErrorCode::ConfigurationRecoveryFailed
        );
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");

        let restarted = store.load_or_default().expect("restart config");
        assert_eq!(restarted.config, bongocat_config::NativeConfig::default());
        assert!(
            std::fs::read_dir(&layout.backups)
                .expect("backup directory")
                .any(|entry| entry
                    .expect("backup entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config-corrupt-"))
        );
    }

    #[test]
    fn service_opens_anonymous_backup_location_without_advancing_revision() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )));
        let backup_location = Arc::new(TestBackupLocation::new());
        let service = ApplicationSettingsService::start_with_capabilities(
            application,
            startup_item,
            backup_location.clone(),
            Arc::new(TestDiagnosticsExport),
        )
        .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let opened = client
            .open_config_backup_location_blocking()
            .expect("open backup location");
        assert_eq!(opened, initial);
        assert_eq!(backup_location.invocations.load(Ordering::Acquire), 1);

        backup_location.fail.store(true, Ordering::Release);
        let error = client
            .open_config_backup_location_blocking()
            .expect_err("backup location failure");
        assert_eq!(error.code(), SettingsErrorCode::BackupLocationOpenFailed);
        assert_eq!(
            error.to_string(),
            "configuration backup folder could not be opened"
        );
        assert!(
            !error
                .to_string()
                .contains(base.path().to_string_lossy().as_ref())
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, initial);
        assert_eq!(backup_location.invocations.load(Ordering::Acquire), 2);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_advances_settings_revision_once_for_one_control_change() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let updated = client
            .set_overlay_visible_blocking(
                initial.config_revision.expect("config revision"),
                !initial.overlay_visible,
            )
            .expect("toggle overlay visibility");
        assert_eq!(updated.revision, initial.revision.saturating_add(1));
        assert_ne!(updated.config_revision, initial.config_revision);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_persists_and_projects_the_selected_appearance_theme() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(initial.appearance_theme, SettingsTheme::System);
        let updated = client
            .set_appearance_theme_blocking(
                initial.config_revision.expect("config revision"),
                SettingsTheme::Dark,
            )
            .expect("select dark theme");
        assert_eq!(updated.appearance_theme, SettingsTheme::Dark);
        assert_eq!(updated.revision, initial.revision.saturating_add(1));
        assert_ne!(updated.config_revision, initial.config_revision);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert_eq!(
            restarted.config().appearance.theme,
            bongocat_config::Theme::Dark
        );
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_persists_and_projects_the_selected_language() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(initial.language, SettingsLanguage::System);
        assert_eq!(
            initial.resolved_language,
            SettingsLanguage::EnglishUnitedStates
        );
        let updated = client
            .set_language_blocking(
                initial.config_revision.expect("config revision"),
                SettingsLanguage::ChineseSimplified,
            )
            .expect("select simplified Chinese");
        assert_eq!(updated.language, SettingsLanguage::ChineseSimplified);
        assert_eq!(
            updated.resolved_language,
            SettingsLanguage::ChineseSimplified
        );
        assert_eq!(updated.revision, initial.revision.saturating_add(1));
        assert_ne!(updated.config_revision, initial.config_revision);

        let stale = client
            .set_language_blocking(
                initial.config_revision.expect("config revision"),
                SettingsLanguage::EnglishUnitedStates,
            )
            .expect_err("reject stale language update");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert_eq!(
            restarted.config().appearance.language,
            bongocat_config::Language::ChineseSimplified
        );
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_applies_and_persists_status_icon_visibility_transactionally() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let status_icon = Arc::new(TestStatusIcon::new(true));
        let service =
            ApplicationSettingsService::start_with_status_icon(application, status_icon.clone())
                .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.status_icon_visible);
        let hidden = client
            .set_status_icon_visible_blocking(
                initial.config_revision.expect("config revision"),
                false,
            )
            .expect("hide status icon");
        assert!(!hidden.status_icon_visible);
        assert!(!status_icon.visible());
        assert_eq!(status_icon.updates(), vec![false]);

        let stale = client
            .set_status_icon_visible_blocking(
                initial.config_revision.expect("config revision"),
                true,
            )
            .expect_err("stale status icon update");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(status_icon.updates(), vec![false]);

        status_icon.fail_updates.store(true, Ordering::Release);
        let failed = client
            .set_status_icon_visible_blocking(
                hidden.config_revision.expect("hidden config revision"),
                true,
            )
            .expect_err("platform update failure");
        assert_eq!(failed.code(), SettingsErrorCode::StatusIconUpdateFailed);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, hidden);
        assert!(!status_icon.visible());

        status_icon.fail_updates.store(false, Ordering::Release);
        let occupied = layout.config.with_extension("json.tmp");
        std::fs::create_dir(&occupied).expect("occupied temp target");
        let persist_failed = client
            .set_status_icon_visible_blocking(
                hidden.config_revision.expect("hidden config revision"),
                true,
            )
            .expect_err("config persist failure");
        assert_eq!(
            persist_failed.code(),
            SettingsErrorCode::ConfigTargetOccupied
        );
        assert!(!status_icon.visible());
        assert_eq!(status_icon.updates(), vec![false, true, true, false]);
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect("rolled back snapshot"),
            hidden
        );
        std::fs::remove_dir(occupied).expect("remove occupied temp target");

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(!restarted.config().application.show_status_icon);
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_applies_and_persists_taskbar_icon_visibility_transactionally() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let taskbar_icon = Arc::new(TestTaskbarIcon::new(true));
        let service =
            ApplicationSettingsService::start_with_taskbar_icon(application, taskbar_icon.clone())
                .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.taskbar_icon_visible);
        let hidden = client
            .set_taskbar_icon_visible_blocking(
                initial.config_revision.expect("config revision"),
                false,
            )
            .expect("hide taskbar icon");
        assert!(!hidden.taskbar_icon_visible);
        assert!(!taskbar_icon.visible());
        assert_eq!(taskbar_icon.updates(), vec![false]);

        let stale = client
            .set_taskbar_icon_visible_blocking(
                initial.config_revision.expect("config revision"),
                true,
            )
            .expect_err("stale taskbar icon update");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(taskbar_icon.updates(), vec![false]);

        taskbar_icon.fail_updates.store(true, Ordering::Release);
        let failed = client
            .set_taskbar_icon_visible_blocking(
                hidden.config_revision.expect("hidden config revision"),
                true,
            )
            .expect_err("platform update failure");
        assert_eq!(failed.code(), SettingsErrorCode::TaskbarIconUpdateFailed);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, hidden);
        assert!(!taskbar_icon.visible());

        taskbar_icon.fail_updates.store(false, Ordering::Release);
        let occupied = layout.config.with_extension("json.tmp");
        std::fs::create_dir(&occupied).expect("occupied temp target");
        let persist_failed = client
            .set_taskbar_icon_visible_blocking(
                hidden.config_revision.expect("hidden config revision"),
                true,
            )
            .expect_err("config persist failure");
        assert_eq!(
            persist_failed.code(),
            SettingsErrorCode::ConfigTargetOccupied
        );
        assert!(!taskbar_icon.visible());
        assert_eq!(taskbar_icon.updates(), vec![false, true, true, false]);
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect("rolled back snapshot"),
            hidden
        );
        std::fs::remove_dir(occupied).expect("remove occupied temp target");

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(!restarted.config().application.show_taskbar_icon);
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_persists_automatic_update_check_preference_and_rejects_stale_revision() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.check_for_updates_automatically);
        let disabled = client
            .set_check_for_updates_automatically_blocking(
                initial.config_revision.expect("config revision"),
                false,
            )
            .expect("disable automatic update checks");
        assert!(!disabled.check_for_updates_automatically);
        assert_ne!(disabled.config_revision, initial.config_revision);

        let stale = client
            .set_check_for_updates_automatically_blocking(
                initial.config_revision.expect("config revision"),
                true,
            )
            .expect_err("reject stale automatic update preference");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(
            client.read_snapshot_blocking().expect("unchanged snapshot"),
            disabled
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(
            !restarted
                .config()
                .application
                .check_for_updates_automatically
        );
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_exports_diagnostics_and_reports_the_result_in_a_new_snapshot() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )));
        let backup_location = Arc::new(TestBackupLocation::new());
        let service = ApplicationSettingsService::start_with_capabilities(
            application,
            startup_item,
            backup_location,
            Arc::new(TestDiagnosticsExport),
        )
        .expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.diagnostics_export.is_none());
        let exported = client
            .export_diagnostics_blocking()
            .expect("export diagnostics");
        assert!(exported.revision > initial.revision);
        assert_eq!(
            exported.diagnostics_export,
            Some(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 1,
            })
        );
        let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
        assert_eq!(refreshed, exported);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_can_open_backup_location_while_configuration_recovery_is_required() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        ConfigStore::new(layout.clone()).expect("config store");
        std::fs::write(&layout.config, b"invalid-current-without-backup")
            .expect("invalid current config");
        let application = Application::start_with_layout(layout).expect("safe mode start");
        let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )));
        let backup_location = Arc::new(TestBackupLocation::new());
        let service = ApplicationSettingsService::start_with_capabilities(
            application,
            startup_item,
            backup_location.clone(),
            Arc::new(TestDiagnosticsExport),
        )
        .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("safe mode snapshot");
        assert!(matches!(
            initial.configuration_status,
            SettingsConfigurationStatus::RecoveryRequired { .. }
        ));
        let opened = client
            .open_config_backup_location_blocking()
            .expect("open backup location in safe mode");
        assert_eq!(opened, initial);
        assert_eq!(backup_location.invocations.load(Ordering::Acquire), 1);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    fn model_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .join("shared/fixtures/model-fixtures/cases/非 ASCII 模型")
    }

    fn shortcut_fixture() -> SettingsShortcuts {
        shortcut_fixture_with(
            "toggle_overlay",
            "Control+Alt+B",
            "motion:TapBody:0",
            "Control+Alt+M",
        )
    }

    fn shortcut_fixture_with(
        command: &str,
        command_shortcut: &str,
        behavior_id: &str,
        behavior_shortcut: &str,
    ) -> SettingsShortcuts {
        SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: command.to_owned(),
                shortcut: command_shortcut.to_owned(),
            }],
            model_behaviors: vec![SettingsModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: behavior_id.to_owned(),
                shortcut: behavior_shortcut.to_owned(),
            }],
        }
    }

    #[test]
    fn shortcut_config_errors_map_to_a_stable_settings_code() {
        for field in [
            "shortcuts.commands",
            "shortcuts.command",
            "shortcuts.behavior",
            "shortcuts.binding",
            "shortcuts.conflict",
        ] {
            assert_eq!(
                settings_config_error_code(&ConfigError::InvalidValue(field)),
                Some(SettingsErrorCode::InvalidShortcutBindings),
                "field {field}"
            );
        }
        assert_eq!(
            settings_config_error_code(&ConfigError::InvalidValue("appearance.language")),
            None
        );
    }

    #[test]
    fn service_persists_shortcuts_and_restores_them_after_restart() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let expected = shortcut_fixture();
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let updated = client
            .set_shortcuts_blocking(
                initial.config_revision.expect("config revision"),
                expected.clone(),
            )
            .expect("persist shortcuts");
        assert_eq!(updated.shortcuts, expected);
        assert!(updated.revision > initial.revision);
        assert_ne!(updated.config_revision, initial.config_revision);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("toggle_overlay"));
        assert!(persisted.contains("Control+Alt+B"));
        assert!(persisted.contains("motion:TapBody:0"));
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");

        let restarted = Application::start_with_layout(layout).expect("application restart");
        let restarted_service =
            ApplicationSettingsService::start(restarted).expect("service restart");
        let restored = restarted_service
            .client()
            .read_snapshot_blocking()
            .expect("restored snapshot");
        assert_eq!(restored.shortcuts, expected);
        restarted_service
            .client()
            .shutdown_blocking()
            .expect("restarted service shutdown");
        restarted_service.join().expect("restarted service join");
    }

    #[test]
    fn service_executes_application_shortcuts_from_the_platform_handoff() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("start application");
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let service =
            ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
                .expect("start settings service");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        sender
            .send(ShortcutCommand::ToggleOverlay)
            .expect("queue application shortcut");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let updated = loop {
            let snapshot = client.read_snapshot_blocking().expect("updated snapshot");
            if !snapshot.overlay_visible || std::time::Instant::now() >= deadline {
                break snapshot;
            }
            std::thread::yield_now();
        };
        assert!(!updated.overlay_visible);
        assert_ne!(updated.config_revision, initial.config_revision);
        drop(sender);
        client.shutdown_blocking().expect("shutdown service");
        service.join().expect("join service");
    }

    #[test]
    fn service_routes_open_settings_to_the_gpui_signal_without_touching_ui() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("start application");
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let signals = ApplicationShortcutSignals::default();
        let service = ApplicationSettingsService::start_with_shortcut_receiver_and_signals(
            application,
            receiver,
            signals.clone(),
        )
        .expect("start settings service");
        sender
            .send(ShortcutCommand::OpenSettings)
            .expect("queue open settings");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut observed = false;
        while !observed && std::time::Instant::now() < deadline {
            observed = signals.take_open_settings_request();
            std::thread::yield_now();
        }
        assert!(observed);
        drop(sender);
        service
            .client()
            .shutdown_blocking()
            .expect("shutdown service");
        service.join().expect("join service");
    }

    #[test]
    fn dropping_service_joins_shortcut_forwarder_while_sender_is_alive() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("start application");
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let service =
            ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
                .expect("start settings service");

        drop(service);

        assert!(
            sender.send(ShortcutCommand::OpenSettings).is_err(),
            "shortcut receiver must be dropped before the service drop returns"
        );
    }

    #[test]
    fn service_canonicalizes_shortcuts_before_persisting_them() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let submitted = SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: " toggle_overlay ".to_owned(),
                shortcut: " shift + ctrl + b ".to_owned(),
            }],
            model_behaviors: vec![SettingsModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: " expression: happy ".to_owned(),
                shortcut: "cmd+option+p".to_owned(),
            }],
        };
        let expected = shortcut_fixture_with(
            "toggle_overlay",
            "Control+Shift+B",
            "expression:happy",
            "Alt+Meta+P",
        );
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let updated = client
            .set_shortcuts_blocking(initial.config_revision.expect("config revision"), submitted)
            .expect("canonicalize shortcuts");
        assert_eq!(updated.shortcuts, expected);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(!persisted.contains(" shift + ctrl + b "));
        assert!(persisted.contains("Control+Shift+B"));
        assert!(persisted.contains("expression:happy"));
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_persists_behavior_shortcut_state_and_rejects_stale_updates() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.behavior_shortcuts_enabled);
        let initial_revision = initial.config_revision.expect("config revision");

        let disabled = client
            .set_behavior_shortcuts_enabled_blocking(initial_revision, false)
            .expect("disable behavior shortcuts");
        assert!(!disabled.behavior_shortcuts_enabled);
        assert!(
            std::fs::read_to_string(&layout.config)
                .expect("persisted config")
                .contains("\"enable_behavior_shortcuts\": false")
        );

        let error = client
            .set_behavior_shortcuts_enabled_blocking(initial_revision, true)
            .expect_err("stale behavior shortcut update");
        assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
        assert!(
            !client
                .read_snapshot_blocking()
                .expect("unchanged snapshot")
                .behavior_shortcuts_enabled
        );
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");

        let restarted = Application::start_with_layout(layout).expect("application restart");
        let restarted_service =
            ApplicationSettingsService::start(restarted).expect("restarted service");
        let restarted_client = restarted_service.client();
        assert!(
            !restarted_client
                .read_snapshot_blocking()
                .expect("restarted snapshot")
                .behavior_shortcuts_enabled
        );
        restarted_client
            .shutdown_blocking()
            .expect("restarted service shutdown");
        restarted_service.join().expect("restarted service join");
    }

    #[test]
    fn service_rejects_invalid_shortcuts_without_mutating_config() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let original_config = std::fs::read(&layout.config).expect("initial config");
        let cases = [
            SettingsShortcuts {
                commands: vec![SettingsShortcutBinding {
                    command: "unknown".to_owned(),
                    shortcut: "Control+Alt+B".to_owned(),
                }],
                ..SettingsShortcuts::default()
            },
            SettingsShortcuts {
                commands: vec![SettingsShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+".to_owned(),
                }],
                ..SettingsShortcuts::default()
            },
            SettingsShortcuts {
                model_behaviors: vec![SettingsModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "physics:0".to_owned(),
                    shortcut: "Control+Alt+M".to_owned(),
                }],
                ..SettingsShortcuts::default()
            },
        ];
        for shortcuts in cases {
            let error = client
                .set_shortcuts_blocking(
                    initial.config_revision.expect("config revision"),
                    shortcuts,
                )
                .expect_err("invalid shortcut binding");
            assert_eq!(error.code(), SettingsErrorCode::InvalidShortcutBindings);
            assert_eq!(
                std::fs::read(&layout.config).expect("config remains readable"),
                original_config
            );
        }
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, initial);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_rejects_stale_shortcuts_without_mutating_config_or_snapshot() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let committed = client
            .set_shortcuts_blocking(
                initial.config_revision.expect("config revision"),
                shortcut_fixture(),
            )
            .expect("first shortcut update");
        let committed_config = std::fs::read(&layout.config).expect("committed config");
        let stale = SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: "toggle_mirror".to_owned(),
                shortcut: "Control+Alt+X".to_owned(),
            }],
            ..SettingsShortcuts::default()
        };
        let error = client
            .set_shortcuts_blocking(initial.config_revision.expect("config revision"), stale)
            .expect_err("stale shortcut update");
        assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, committed);
        assert_eq!(
            std::fs::read(&layout.config).expect("preserved config"),
            committed_config
        );
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_restores_default_shortcuts_with_revision_check() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let configured = client
            .set_shortcuts_blocking(
                initial.config_revision.expect("config revision"),
                shortcut_fixture(),
            )
            .expect("configure shortcuts");
        let restored = client
            .restore_default_shortcuts_blocking(
                configured.config_revision.expect("config revision"),
            )
            .expect("restore defaults");
        assert_eq!(restored.shortcuts, SettingsShortcuts::default());
        assert!(restored.revision > configured.revision);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("\"commands\": []"));
        assert!(persisted.contains("\"model_behaviors\": []"));

        let stale = client
            .restore_default_shortcuts_blocking(
                configured.config_revision.expect("stale config revision"),
            )
            .expect_err("stale restore");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged, restored);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_rejects_shortcut_restore_while_configuration_recovery_is_required() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        ConfigStore::new(layout.clone()).expect("config store");
        std::fs::write(&layout.config, b"invalid-current-without-backup")
            .expect("invalid current config");
        let application = Application::start_with_layout(layout).expect("safe mode start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let error = client
            .restore_default_shortcuts_blocking(0)
            .expect_err("restore shortcuts in recovery");
        assert_eq!(
            error.code(),
            SettingsErrorCode::ConfigurationRecoveryRequired
        );
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_orders_updates_persists_them_and_stops_runtime() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config_revision = initial.config_revision.expect("config revision");
        assert_eq!(initial.model_catalog.entries.len(), 3);
        assert!(initial.model_catalog.error.is_none());
        assert!(initial.model_catalog.entries.iter().all(|entry| {
            entry.origin == SettingsModelOrigin::Preset
                && matches!(entry.availability, SettingsModelAvailability::Ready { .. })
        }));
        let selected = client
            .select_model_blocking(
                initial_config_revision,
                SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::Preset,
                },
            )
            .expect("select preset model");
        let selected_config_revision = selected.config_revision.expect("config revision");
        assert_eq!(
            selected.active_model,
            Some(SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
        );
        let overlay_settings = SettingsOverlay {
            click_through: true,
            always_on_top: false,
            scale_percent: 125,
            opacity_percent: 80,
            keep_inside_work_area: false,
        };
        let configured = client
            .set_overlay_settings_blocking(selected_config_revision, overlay_settings)
            .expect("update overlay settings");
        assert_eq!(
            configured.overlay, overlay_settings,
            "settings snapshot must acknowledge the committed overlay settings"
        );
        let hidden = client
            .set_overlay_visible_blocking(
                configured.config_revision.expect("config revision"),
                false,
            )
            .expect("hide overlay");
        let muted = client
            .set_motion_audio_enabled_blocking(
                hidden.config_revision.expect("config revision"),
                false,
            )
            .expect("disable motion audio");
        let model_settings = bongocat_ui::SettingsModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_pointer: true,
        };
        let configured_model = client
            .set_model_settings_blocking(
                muted.config_revision.expect("config revision"),
                model_settings,
            )
            .expect("update model settings");
        assert_eq!(configured_model.model_settings, model_settings);
        let configured_frame_rate = client
            .set_maximum_fps_blocking(
                configured_model.config_revision.expect("config revision"),
                120,
            )
            .expect("update maximum FPS");
        assert_eq!(configured_frame_rate.maximum_fps, 120);
        let configured_fallback = client
            .set_release_fallback_timeout_blocking(
                configured_frame_rate
                    .config_revision
                    .expect("config revision"),
                1_500,
            )
            .expect("update release fallback timeout");
        assert_eq!(configured_fallback.release_fallback_timeout_ms, 1_500);
        let gamepad_settings = bongocat_ui::SettingsGamepadAxisSettings {
            stick_dead_zone_percent: 20,
            trigger_dead_zone_percent: 10,
        };
        let configured_gamepad = client
            .set_gamepad_axis_settings_blocking(
                configured_fallback
                    .config_revision
                    .expect("config revision"),
                gamepad_settings,
            )
            .expect("update gamepad settings");
        assert_eq!(configured_gamepad.gamepad_axis_settings, gamepad_settings);
        assert!(hidden.revision > initial.revision);
        assert!(muted.revision > hidden.revision);
        assert!(!muted.overlay_visible);
        assert!(!muted.motion_audio_enabled);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
        assert!(persisted.contains("\"play_motion_audio\": false"));
        assert!(persisted.contains("\"selected_model_id\": \"keyboard\""));
        assert!(persisted.contains("\"selected_model_origin\": \"preset\""));
        assert!(persisted.contains("\"click_through\": true"));
        assert!(persisted.contains("\"opacity_percent\": 80"));
        assert!(persisted.contains("\"keep_inside_work_area\": false"));
        assert!(persisted.contains("\"mirror\": true"));
        assert!(persisted.contains("\"mirror_pointer_tracking\": true"));
        assert!(persisted.contains("\"ignore_pointer\": true"));
        assert!(persisted.contains("\"gamepad_stick_dead_zone\": 0.2"));
        assert!(persisted.contains("\"gamepad_trigger_dead_zone\": 0.1"));
        assert!(persisted.contains("\"maximum_fps\": 120"));
        assert!(persisted.contains("\"release_fallback_timeout_ms\": 1500"));

        let stopped = client.shutdown_blocking().expect("service shutdown");
        assert_eq!(stopped.runtime_health, RuntimeHealth::Stopped);
        service.join().expect("service join");

        let restarted = Application::start_with_layout(layout).expect("application restart");
        assert_eq!(
            restarted
                .runtime_client()
                .snapshot()
                .release_fallback_timeout_ms,
            1_500
        );
        assert!(
            !restarted
                .runtime_client()
                .snapshot()
                .overlay_settings
                .keep_inside_work_area
        );
        restarted
            .shutdown()
            .expect("restarted application shutdown");
    }

    #[test]
    fn service_rejects_stale_overlay_settings_without_mutating_runtime_or_config() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config_revision = initial.config_revision.expect("config revision");
        let original_settings = SettingsOverlay {
            click_through: false,
            always_on_top: false,
            scale_percent: 125,
            opacity_percent: 80,
            keep_inside_work_area: false,
        };
        let committed = client
            .set_overlay_settings_blocking(initial_config_revision, original_settings)
            .expect("first overlay update");
        let committed_config = std::fs::read(&config_path).expect("committed config");

        let stale_settings = SettingsOverlay {
            click_through: true,
            always_on_top: true,
            scale_percent: 400,
            opacity_percent: 10,
            keep_inside_work_area: true,
        };
        let error = client
            .set_overlay_settings_blocking(initial_config_revision, stale_settings)
            .expect_err("stale overlay update");
        assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(
            error.to_string(),
            "settings changed in the background; review the latest values and retry"
        );
        assert!(!error.to_string().contains('/') && !error.to_string().contains('\\'));

        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, committed.revision);
        assert_eq!(unchanged.overlay, original_settings);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved config"),
            committed_config
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_rejects_stale_direct_settings_without_mutating_runtime_or_config() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config_revision = initial.config_revision.expect("config revision");
        let initial_active_model = initial.active_model.clone();
        let initial_config = std::fs::read(&config_path).expect("initial config");
        let invalid_frame_rate_error = client
            .set_maximum_fps_blocking(initial_config_revision, 241)
            .expect_err("invalid maximum FPS update");
        assert_eq!(
            invalid_frame_rate_error.code(),
            SettingsErrorCode::InvalidMaximumFps
        );
        let after_invalid_frame_rate = client
            .read_snapshot_blocking()
            .expect("snapshot after invalid maximum FPS");
        assert_eq!(after_invalid_frame_rate.revision, initial.revision);
        assert_eq!(after_invalid_frame_rate.maximum_fps, initial.maximum_fps);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved initial config"),
            initial_config
        );
        let invalid_fallback_error = client
            .set_release_fallback_timeout_blocking(initial_config_revision, 60_001)
            .expect_err("invalid release fallback timeout update");
        assert_eq!(
            invalid_fallback_error.code(),
            SettingsErrorCode::InvalidReleaseFallbackTimeout
        );
        let after_invalid_fallback = client
            .read_snapshot_blocking()
            .expect("snapshot after invalid release fallback timeout");
        assert_eq!(after_invalid_fallback.revision, initial.revision);
        assert_eq!(
            after_invalid_fallback.release_fallback_timeout_ms,
            initial.release_fallback_timeout_ms
        );
        assert_eq!(
            std::fs::read(&config_path).expect("preserved initial config"),
            initial_config
        );
        let hidden = client
            .set_overlay_visible_blocking(initial_config_revision, false)
            .expect("hide overlay");
        let hidden_config = std::fs::read(&config_path).expect("hidden config");

        let stale_theme_error = client
            .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
            .expect_err("stale appearance theme update");
        assert_eq!(
            stale_theme_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );

        let stale_model_error = client
            .set_model_settings_blocking(
                initial_config_revision,
                bongocat_ui::SettingsModelSettings {
                    mirror: true,
                    mirror_pointer_tracking: true,
                    ignore_pointer: true,
                },
            )
            .expect_err("stale model settings update");
        assert_eq!(
            stale_model_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let after_stale_model_settings = client
            .read_snapshot_blocking()
            .expect("snapshot after stale model settings");
        assert_eq!(after_stale_model_settings.revision, hidden.revision);
        assert_eq!(
            after_stale_model_settings.model_settings,
            bongocat_ui::SettingsModelSettings::default()
        );
        assert_eq!(
            std::fs::read(&config_path).expect("preserved hidden config"),
            hidden_config
        );

        let stale_gamepad_error = client
            .set_gamepad_axis_settings_blocking(
                initial_config_revision,
                bongocat_ui::SettingsGamepadAxisSettings {
                    stick_dead_zone_percent: 20,
                    trigger_dead_zone_percent: 10,
                },
            )
            .expect_err("stale gamepad settings update");
        assert_eq!(
            stale_gamepad_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );

        let stale_frame_rate_error = client
            .set_maximum_fps_blocking(initial_config_revision, 120)
            .expect_err("stale maximum FPS update");
        assert_eq!(
            stale_frame_rate_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let stale_fallback_error = client
            .set_release_fallback_timeout_blocking(initial_config_revision, 1_500)
            .expect_err("stale release fallback timeout update");
        assert_eq!(
            stale_fallback_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );

        let stale_model_error = client
            .select_model_blocking(
                initial_config_revision,
                SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::Preset,
                },
            )
            .expect_err("stale model selection");
        assert_eq!(
            stale_model_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let after_stale_model = client
            .read_snapshot_blocking()
            .expect("snapshot after stale model");
        assert_eq!(after_stale_model.revision, hidden.revision);
        assert_eq!(after_stale_model.active_model, initial_active_model);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved hidden config"),
            hidden_config
        );

        let stale_audio_error = client
            .set_motion_audio_enabled_blocking(initial_config_revision, false)
            .expect_err("stale motion audio update");
        assert_eq!(
            stale_audio_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let after_stale_audio = client
            .read_snapshot_blocking()
            .expect("snapshot after stale audio");
        assert_eq!(after_stale_audio.revision, hidden.revision);
        assert!(!after_stale_audio.overlay_visible);
        assert!(after_stale_audio.motion_audio_enabled);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved hidden config"),
            hidden_config
        );

        let muted = client
            .set_motion_audio_enabled_blocking(
                hidden.config_revision.expect("config revision"),
                false,
            )
            .expect("disable motion audio");
        let muted_config = std::fs::read(&config_path).expect("muted config");
        let stale_visibility_error = client
            .set_overlay_visible_blocking(hidden.config_revision.expect("config revision"), true)
            .expect_err("stale overlay visibility update");
        assert_eq!(
            stale_visibility_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, muted.revision);
        assert!(!unchanged.overlay_visible);
        assert!(!unchanged.motion_audio_enabled);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved muted config"),
            muted_config
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_reports_an_occupied_config_target_without_changing_snapshot_or_current() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let occupied = config_path.with_extension("json.tmp");
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let original = std::fs::read(&config_path).expect("initial config");
        std::fs::create_dir(&occupied).expect("occupied temp target");

        let initial_config_revision = initial.config_revision.expect("config revision");
        let error = client
            .set_overlay_visible_blocking(initial_config_revision, !initial.overlay_visible)
            .expect_err("occupied target error");
        assert_eq!(error.code(), SettingsErrorCode::ConfigTargetOccupied);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, initial.revision);
        assert_eq!(unchanged.overlay_visible, initial.overlay_visible);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved config"),
            original
        );
        assert!(occupied.is_dir());

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_observes_and_updates_startup_item_without_touching_config() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )));
        let service =
            ApplicationSettingsService::start_with_startup_item(application, startup_item.clone())
                .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config = std::fs::read(&config_path).expect("initial config");
        assert_eq!(
            initial.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)
        );

        startup_item.replace(SettingsStartupItemStatus::ReadError(
            SettingsStartupItemError::StateReadFailed,
        ));
        let read_failed = client
            .read_snapshot_blocking()
            .expect("read failure remains a snapshot");
        assert!(read_failed.revision > initial.revision);
        assert_eq!(
            read_failed.startup_item,
            SettingsStartupItemStatus::ReadError(SettingsStartupItemError::StateReadFailed)
        );

        startup_item.replace(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Stale,
        ));
        let externally_changed = client.read_snapshot_blocking().expect("external change");
        assert!(externally_changed.revision > read_failed.revision);
        assert_eq!(
            externally_changed.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)
        );

        let enabled = client
            .set_startup_item_enabled_blocking(true)
            .expect("enable startup item");
        assert!(enabled.revision > externally_changed.revision);
        assert_eq!(
            enabled.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)
        );
        assert_eq!(
            std::fs::read(&config_path).expect("unchanged config"),
            initial_config
        );

        startup_item.fail_updates.store(true, Ordering::Release);
        let failed = client
            .set_startup_item_enabled_blocking(false)
            .expect_err("failed startup update");
        assert_eq!(failed.code(), SettingsErrorCode::StartupItemUpdateFailed);
        let unchanged = client.read_snapshot_blocking().expect("unchanged state");
        assert_eq!(unchanged.revision, enabled.revision);
        assert_eq!(unchanged.startup_item, enabled.startup_item);
        assert_eq!(
            std::fs::read(&config_path).expect("config after failure"),
            initial_config
        );

        let stopped = client.shutdown_blocking().expect("service shutdown");
        assert_eq!(stopped.startup_item, unchanged.startup_item);
        service.join().expect("service join");
    }

    #[test]
    fn client_reports_closed_service_without_exposing_application_errors() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect_err("closed service")
                .code(),
            SettingsErrorCode::ServiceUnavailable
        );
    }

    #[test]
    fn service_imports_a_model_without_selecting_it_and_refreshes_the_catalog() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");

        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "custom-model".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import model");
        assert!(imported.revision > initial.revision);
        assert_eq!(
            imported.active_model, None,
            "import must not implicitly activate the model"
        );
        assert!(imported.model_catalog.entries.iter().any(|entry| {
            entry.id == "custom-model"
                && entry.origin == SettingsModelOrigin::Installed
                && matches!(entry.availability, SettingsModelAvailability::Ready { .. })
        }));
        assert!(models_root.join("custom-model/猫.model3.json").is_file());

        let duplicate = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "custom-model".to_owned(),
                source_root: model_fixture(),
            })
            .expect_err("duplicate import");
        assert_eq!(duplicate.code(), SettingsErrorCode::ModelAlreadyInstalled);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, imported.revision);
        assert_eq!(unchanged.model_catalog, imported.model_catalog);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_observes_import_cancellation_without_committing_or_revising_catalog() {
        let source = tempdir().expect("model source");
        std::fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        std::fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        std::fs::File::create(source.path().join("payload.bin"))
            .and_then(|file| file.set_len(16 * 1024 * 1024))
            .expect("large payload");

        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");

        let operation = client
            .start_model_import_blocking(SettingsModelImportRequest {
                id: "cancelled-model".to_owned(),
                source_root: source.path().to_owned(),
            })
            .expect("start import");
        let operation_id = operation.operation_id();
        assert!(operation.cancel());
        let final_result = operation.final_result_blocking();
        assert_eq!(final_result.operation_id, operation_id);
        assert_eq!(
            final_result.result.expect_err("cancelled import").code(),
            SettingsErrorCode::ModelImportCancelled
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, initial.revision);
        assert!(!models_root.join("cancelled-model").exists());

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn every_model_store_import_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::AlreadyExists,
                SettingsErrorCode::ModelAlreadyInstalled,
            ),
            (
                ModelStoreDiagnostic::Cancelled,
                SettingsErrorCode::ModelImportCancelled,
            ),
            (
                ModelStoreDiagnostic::InvalidPackage,
                SettingsErrorCode::ModelImportInvalidPackage,
            ),
            (
                ModelStoreDiagnostic::SourceContainsStore,
                SettingsErrorCode::ModelImportSourceInvalid,
            ),
            (
                ModelStoreDiagnostic::SourceChanged,
                SettingsErrorCode::ModelImportSourceChanged,
            ),
            (
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                SettingsErrorCode::ModelImportSourceUnsupported,
            ),
            (
                ModelStoreDiagnostic::SourceEntryUnsupported,
                SettingsErrorCode::ModelImportSourceUnsupported,
            ),
            (
                ModelStoreDiagnostic::StoreBusy,
                SettingsErrorCode::ModelStoreBusy,
            ),
            (
                ModelStoreDiagnostic::IoError,
                SettingsErrorCode::ModelImportFailed,
            ),
            (
                ModelStoreDiagnostic::NotFound,
                SettingsErrorCode::ModelImportFailed,
            ),
            (
                ModelStoreDiagnostic::StoreEntryUnsupported,
                SettingsErrorCode::ModelImportFailed,
            ),
        ];

        for (diagnostic, expected) in cases {
            assert_eq!(map_model_store_import_diagnostic(diagnostic), expected);
        }
    }

    #[test]
    fn every_model_store_delete_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::NotFound,
                SettingsErrorCode::ModelNotInstalled,
            ),
            (
                ModelStoreDiagnostic::StoreBusy,
                SettingsErrorCode::ModelStoreBusy,
            ),
            (
                ModelStoreDiagnostic::AlreadyExists,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::Cancelled,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::InvalidPackage,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::IoError,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceContainsStore,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceChanged,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceEntryUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::StoreEntryUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
        ];

        for (diagnostic, expected) in cases {
            assert_eq!(map_model_store_delete_diagnostic(diagnostic), expected);
        }
    }

    #[test]
    fn service_deletes_only_unselected_installed_source_identity() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "standard".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import installed duplicate");
        let selected = client
            .select_model_blocking(
                imported.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::Preset,
                },
            )
            .expect("select preset duplicate");
        let deleted = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect("delete installed duplicate");
        assert!(selected.revision > imported.revision);
        assert!(deleted.revision > selected.revision);
        assert_eq!(
            deleted.active_model,
            Some(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
        );
        assert!(!deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Installed
        }));
        assert!(deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Preset
        }));

        let preset_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
            .expect_err("preset deletion");
        assert_eq!(
            preset_error.code(),
            SettingsErrorCode::PresetModelCannotBeDeleted
        );
        let missing_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "missing".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("missing installed model");
        assert_eq!(missing_error.code(), SettingsErrorCode::ModelNotInstalled);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_rejects_deleting_the_selected_installed_model() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "selected".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import model");
        let selected = client
            .select_model_blocking(
                imported.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "selected".to_owned(),
                    origin: SettingsModelOrigin::Installed,
                },
            )
            .expect("select installed model");

        let error = client
            .delete_model_blocking(SettingsModelKey {
                id: "selected".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("selected deletion");
        assert_eq!(
            error.code(),
            SettingsErrorCode::SelectedModelCannotBeDeleted
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, selected.revision);
        assert!(unchanged.model_catalog.entries.iter().any(|entry| {
            entry.id == "selected" && entry.origin == SettingsModelOrigin::Installed
        }));

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_maps_invalid_model_inputs_to_stable_errors() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let invalid_id = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "../escape".to_owned(),
                source_root: model_fixture(),
            })
            .expect_err("invalid model id");
        assert_eq!(invalid_id.code(), SettingsErrorCode::InvalidModelId);

        let invalid_delete_id = client
            .delete_model_blocking(SettingsModelKey {
                id: "../escape".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("invalid delete model id");
        assert_eq!(invalid_delete_id.code(), SettingsErrorCode::InvalidModelId);

        let invalid_package_source = tempdir().expect("invalid package");
        std::fs::write(
            invalid_package_source.path().join("not-a-model.txt"),
            b"invalid",
        )
        .expect("invalid model marker");
        let invalid_package = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "invalid-package".to_owned(),
                source_root: invalid_package_source.path().to_owned(),
            })
            .expect_err("invalid package");
        assert_eq!(
            invalid_package.code(),
            SettingsErrorCode::ModelImportInvalidPackage
        );
        assert!(
            !invalid_package
                .to_string()
                .contains(&invalid_package_source.path().display().to_string())
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn settings_window_layout_flushes_on_shutdown_and_restores_after_restart() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let expected = SettingsWindowPlacement::new(-240, 96, 960, 720, true)
            .expect("valid settings window placement");
        service.window_state().update(expected);

        service
            .client()
            .shutdown_blocking()
            .expect("service shutdown");
        service.join().expect("service join");

        let restarted =
            Application::start_with_layout(layout.clone()).expect("application restart");
        assert_eq!(
            restarted.settings_window_placement(),
            Some(
                WindowPlacement::new(-240, 96, 960, 720, true).expect("valid persisted placement")
            )
        );
        let config =
            std::fs::read_to_string(&layout.config).expect("configuration remains readable");
        assert!(!config.contains("settings_window"));
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn settings_window_layout_is_saved_while_running_and_survives_product_updates() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let expected = SettingsWindowPlacement::new(-360, 144, 1040, 760, false)
            .expect("valid settings window placement");
        let window_state = service.window_state();
        let revision = window_state.update(expected).expect("changed placement");
        assert!(window_state.request_persist_if_current(revision));
        client
            .update_overlay_window_placement(-640, 220, 420, 560)
            .expect("publish overlay placement");
        let initial = client
            .read_snapshot_blocking()
            .expect("wait for queued window placement writes");

        let state = StateStore::new(layout.clone()).load_or_default().state;
        assert_eq!(
            state.settings_window,
            Some(
                WindowPlacement::new(-360, 144, 1040, 760, false)
                    .expect("valid persisted placement")
            )
        );
        assert_eq!(
            state.overlay_window,
            Some(
                OverlayWindowPlacement::new(-640, 220, 420, 560).expect("valid overlay placement")
            )
        );
        let selected = client
            .select_model_blocking(
                initial.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::Preset,
                },
            )
            .expect("select model");
        client
            .set_overlay_visible_blocking(selected.config_revision.expect("config revision"), false)
            .expect("update configuration");

        let persisted_state = StateStore::new(layout).load_or_default().state;
        assert_eq!(
            persisted_state.settings_window,
            Some(
                WindowPlacement::new(-360, 144, 1040, 760, false)
                    .expect("valid persisted placement")
            )
        );
        assert_eq!(
            persisted_state.overlay_window,
            Some(
                OverlayWindowPlacement::new(-640, 220, 420, 560).expect("valid overlay placement")
            )
        );
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn corrupt_state_never_blocks_configuration_or_runtime_startup() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let loaded = store.load_or_default().expect("default config");
        let config_before = std::fs::read(&layout.config).expect("config bytes");
        std::fs::write(&layout.state, b"corrupt-state").expect("corrupt state fixture");

        let application = Application::start_with_layout(layout.clone())
            .expect("corrupt state must not block application startup");
        assert!(application.is_operational());
        assert_eq!(application.settings_window_placement(), None);
        assert_eq!(application.config(), &loaded.config);
        assert_eq!(
            std::fs::read(&layout.config).expect("config preserved"),
            config_before
        );
        assert_eq!(
            std::fs::read(&layout.state).expect("state preserved until flush"),
            b"corrupt-state"
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn future_state_is_preserved_without_failing_service_shutdown() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        store.load_or_default().expect("default config");
        let future = br#"{"schema_version":2,"settings_window":null,"overlay_window":null}"#;
        std::fs::write(&layout.state, future).expect("future state");

        let application = Application::start_with_layout(layout.clone())
            .expect("future state must not block startup");
        let service = ApplicationSettingsService::start(application).expect("service start");
        service
            .client()
            .shutdown_blocking()
            .expect("future state must not fail shutdown");
        service.join().expect("service join");
        assert_eq!(
            std::fs::read(&layout.state).expect("future state preserved"),
            future
        );
    }

    #[test]
    fn state_write_failure_is_reported_after_runtime_still_stops() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        service.window_state().update(
            SettingsWindowPlacement::new(20, 40, 800, 600, false)
                .expect("valid settings window placement"),
        );
        let state_lock = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(layout.locks.join("state.writer.lock"))
            .expect("state writer lock");
        state_lock.lock().expect("hold state writer lock");

        let error = service
            .client()
            .shutdown_blocking()
            .expect_err("state lock must report persistence failure");
        assert_eq!(error.code(), SettingsErrorCode::StatePersistFailed);
        service
            .join()
            .expect("runtime shutdown and service join still complete");
        state_lock.unlock().expect("release state writer lock");
    }

    #[test]
    fn dropping_the_service_performs_a_fallback_shutdown_and_join() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        drop(service);
    }
}
