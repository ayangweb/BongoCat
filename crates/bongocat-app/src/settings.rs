use crate::app_log::ApplicationLogContext;
use crate::diagnostics_export::{export_diagnostics_file, input_service_status_code};
use crate::model_identity::{
    model_origin_from_settings, settings_key_from_config, settings_origin_from_config,
    settings_origin_from_model,
};
use crate::{
    Application, ApplicationError, ApplicationLogCode, ApplicationLogDiagnostics,
    ApplicationLogEvent, ApplicationMainThreadSignals, BUILD_ENVIRONMENT, CoreLogDiagnostics,
    PRODUCT_VERSION, settings_logging_from_config,
};
use bongocat_config::{
    BuildEnvironment, ConfigError, ConfigWriteFailureReason, GamepadAutoSwitchConfig,
    ModelInputMode, NativeConfig, OverlayWindowPlacement, ShortcutCommand, WindowPlacement,
    WindowStateError,
};
use bongocat_input::{PlatformInputDiagnostics, PlatformInputServiceStatus};
use bongocat_model::{
    CommittedModel, ModelBehaviorSnapshot, ModelCatalogEntry, ModelDiagnostic, ModelOrigin,
};
use bongocat_model_store::{
    ModelImportProgress, ModelImportStage, ModelStoreDiagnostic, MverInputMode,
};
#[cfg(target_os = "macos")]
use bongocat_platform::{InputPermission, input_monitoring_permission};
use bongocat_platform::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
    open_directory, set_startup_item_enabled, startup_item_state,
};
use bongocat_runtime::{
    InputSnapshot, ModelSettings, OverlaySettings, RandomBehaviorSettings, RuntimeRenderErrorCode,
    RuntimeSnapshot, RuntimeState,
};
use bongocat_ui_protocol::{
    AutomaticUpdateSettings, RuntimeHealth, SettingsApplicationShortcut, SettingsBuildEnvironment,
    SettingsBuildInfo, SettingsClient, SettingsCommand, SettingsDiagnosticsExportStatus,
    SettingsError, SettingsErrorCode, SettingsGamepadAutoSwitch, SettingsGamepadAxisSettings,
    SettingsInputDiagnostics, SettingsInputMonitoringPermission, SettingsInputServiceStatus,
    SettingsLanguage, SettingsModelAvailability, SettingsModelBehavior,
    SettingsModelBehaviorBinding, SettingsModelCatalog, SettingsModelCatalogError,
    SettingsModelDiagnostic, SettingsModelEntry, SettingsModelImportProgress,
    SettingsModelImportStage, SettingsModelKey, SettingsModelMode, SettingsModelSettings,
    SettingsOverlay, SettingsRandomBehavior, SettingsRuntimeCommandFailure,
    SettingsRuntimeCommandTransportDiagnostics, SettingsRuntimeDiagnostics,
    SettingsRuntimeErrorCode, SettingsServiceEndpoint, SettingsShortcutBinding, SettingsShortcuts,
    SettingsSnapshot, SettingsStartupItemError, SettingsStartupItemState,
    SettingsStartupItemStatus, SettingsStartupItemUnsupportedReason, SettingsTheme,
    SettingsWindowPlacement, SettingsWindowState,
};
use bongocat_update::UpdateDiagnostics;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver as ShortcutReceiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
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
        signals: ApplicationMainThreadSignals,
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
        signals: ApplicationMainThreadSignals,
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
        signals: Option<ApplicationMainThreadSignals>,
    ) -> Result<Self, SettingsServiceJoinError> {
        let backup_location = Arc::new(SystemBackupLocation {
            path: application.config_backup_directory().to_owned(),
        });
        let diagnostics_export = Arc::new(SystemDiagnosticsExport {
            path: application.logs_directory().join("diagnostics.json"),
        });
        let log_location = Arc::new(SystemLogLocation {
            path: application.logs_directory().to_owned(),
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
            Arc::new(SystemModelLocation),
            log_location,
            shortcut_receiver,
            signals,
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
            Arc::new(UnavailableModelLocation),
            Arc::new(UnavailableLogLocation),
            None,
            None,
        )
    }

    /// Like `start_with_capabilities`, but with a log-location recorder, for
    /// the one test that asserts "open application logs" without launching a
    /// real file manager.
    #[cfg(test)]
    fn start_with_log_location(
        application: Application,
        log_location: Arc<dyn LogLocationCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_capabilities_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            VisibilityCapabilities {
                status_icon: Arc::new(UnavailableStatusIcon),
                taskbar_icon: Arc::new(UnavailableTaskbarIcon),
            },
            Arc::new(SystemBackupLocation {
                path: PathBuf::new(),
            }),
            Arc::new(SystemDiagnosticsExport {
                path: PathBuf::new(),
            }),
            Arc::new(UnavailableModelLocation),
            log_location,
            None,
            None,
        )
    }

    /// Like `start_with_capabilities`, but with a model-location recorder, for
    /// the one test that asserts "open model folder" without launching a real
    /// file manager.
    #[cfg(test)]
    fn start_with_model_location(
        application: Application,
        model_location: Arc<dyn ModelLocationCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_capabilities_and_shortcuts(
            application,
            Arc::new(SystemStartupItem),
            VisibilityCapabilities {
                status_icon: Arc::new(UnavailableStatusIcon),
                taskbar_icon: Arc::new(UnavailableTaskbarIcon),
            },
            Arc::new(SystemBackupLocation {
                path: PathBuf::new(),
            }),
            Arc::new(SystemDiagnosticsExport {
                path: PathBuf::new(),
            }),
            model_location,
            Arc::new(UnavailableLogLocation),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with_capabilities_and_shortcuts(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
        visibility: VisibilityCapabilities,
        backup_location: Arc<dyn BackupLocationCapability>,
        diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
        model_location: Arc<dyn ModelLocationCapability>,
        log_location: Arc<dyn LogLocationCapability>,
        shortcut_receiver: Option<ShortcutReceiver<bongocat_config::ShortcutCommand>>,
        signals: Option<ApplicationMainThreadSignals>,
    ) -> Result<Self, SettingsServiceJoinError> {
        let (client, endpoint) = SettingsClient::bounded(SETTINGS_COMMAND_CAPACITY);
        let window_state = client.track_window_state(
            application
                .settings_window_placement()
                .and_then(settings_window_placement),
        );
        let worker_window_state = window_state.clone();
        let worker_signals = signals.clone();
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
                    model_location,
                    log_location,
                    worker_window_state,
                    worker_signals,
                )
            })
            .map_err(SettingsServiceJoinError::Spawn)?;
        let shortcut_forwarder_stop = Arc::new(AtomicBool::new(false));
        let shortcut_forwarder = shortcut_receiver.map(|receiver| {
            let client = client.clone();
            let signals = signals.clone();
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

/// Opening a model's own folder in the system file manager.
///
/// The same seam the configuration backup folder uses: a unit test must be able
/// to assert the outcome of "open model folder" without actually launching a
/// window manager, so the system call sits behind a capability.
trait ModelLocationCapability: Send + Sync + 'static {
    fn open(&self, path: &Path) -> Result<(), SettingsError>;
}

trait DiagnosticsExportCapability: Send + Sync + 'static {
    fn export(
        &self,
        snapshot: &SettingsSnapshot,
        application_logs: ApplicationLogDiagnostics,
        core_logs: Option<CoreLogDiagnostics>,
        update: Option<UpdateDiagnostics>,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError>;
}

struct SystemStartupItem;

struct UnavailableStatusIcon;

struct UnavailableTaskbarIcon;

struct SystemBackupLocation {
    path: PathBuf,
}

struct SystemModelLocation;

/// The test seam for a file manager that is not there. Refusing is a real
/// outcome the page reports, and it keeps a test from opening a real window.
#[cfg(test)]
struct UnavailableModelLocation;

/// Opening the application-owned log directory in the system file manager.
///
/// The path is kept in the settings worker so the UI never needs to know a
/// storage root, just as it does for the configuration backup location.
trait LogLocationCapability: Send + Sync + 'static {
    fn open(&self) -> Result<(), SettingsError>;
}

#[cfg(test)]
struct UnavailableLogLocation;

struct SystemLogLocation {
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

impl ModelLocationCapability for SystemModelLocation {
    fn open(&self, path: &Path) -> Result<(), SettingsError> {
        system_open_model_location(path)
    }
}

#[cfg(test)]
impl ModelLocationCapability for UnavailableModelLocation {
    fn open(&self, _path: &Path) -> Result<(), SettingsError> {
        Err(SettingsError::new(
            SettingsErrorCode::ModelLocationOpenFailed,
        ))
    }
}

impl LogLocationCapability for SystemLogLocation {
    fn open(&self) -> Result<(), SettingsError> {
        open_directory(&self.path)
            .map_err(|_| SettingsError::new(SettingsErrorCode::LogLocationOpenFailed))
    }
}

#[cfg(test)]
impl LogLocationCapability for UnavailableLogLocation {
    fn open(&self) -> Result<(), SettingsError> {
        Err(SettingsError::new(SettingsErrorCode::LogLocationOpenFailed))
    }
}

impl DiagnosticsExportCapability for SystemDiagnosticsExport {
    fn export(
        &self,
        snapshot: &SettingsSnapshot,
        application_logs: ApplicationLogDiagnostics,
        core_logs: Option<CoreLogDiagnostics>,
        update: Option<UpdateDiagnostics>,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
        export_diagnostics_file(&self.path, snapshot, application_logs, core_logs, update)
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

#[derive(Debug, thiserror::Error)]
pub enum SettingsServiceJoinError {
    #[error("failed to start settings service: {0}")]
    Spawn(std::io::Error),
    #[error("settings service panicked")]
    Panicked,
}

#[allow(clippy::too_many_arguments)]
fn run_service(
    mut application: Application,
    endpoint: SettingsServiceEndpoint,
    startup_item: Arc<dyn StartupItemCapability>,
    visibility: VisibilityCapabilities,
    backup_location: Arc<dyn BackupLocationCapability>,
    diagnostics_export: Arc<dyn DiagnosticsExportCapability>,
    model_location: Arc<dyn ModelLocationCapability>,
    log_location: Arc<dyn LogLocationCapability>,
    window_state: SettingsWindowState,
    signals: Option<ApplicationMainThreadSignals>,
) {
    let mut clock = SettingsSnapshotClock::new(application.config_revision());
    loop {
        let Ok(command) = endpoint.recv_blocking() else {
            if persist_window_state(&mut application, &window_state).is_err() {
                application.record_log_once(
                    ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                        .with_context(ApplicationLogContext::State("window_state"))
                        .with_context(ApplicationLogContext::Operation("service_shutdown"))
                        .with_context(ApplicationLogContext::Reason("window_state_persist_failed")),
                );
            }
            application.record_log_once(
                ApplicationLogEvent::new(ApplicationLogCode::UiTransportFailed)
                    .with_context(ApplicationLogContext::Operation("settings_endpoint")),
            );
            let _ = application.shutdown();
            break;
        };
        match command {
            SettingsCommand::SettingsWindowPlacementChanged => {
                if persist_window_state(&mut application, &window_state).is_err() {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                            .with_context(ApplicationLogContext::State("window_state"))
                            .with_context(ApplicationLogContext::Operation("persist"))
                            .with_context(ApplicationLogContext::Reason(
                                "window_state_persist_failed",
                            )),
                    );
                } else {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::WindowStatePersisted)
                            .with_context(ApplicationLogContext::Operation("settings")),
                    );
                }
            }
            SettingsCommand::OverlayWindowPlacementChanged {
                x,
                y,
                width,
                height,
            } => match OverlayWindowPlacement::new(x, y, width, height) {
                Ok(placement) => {
                    if application
                        .persist_overlay_window_placement(placement)
                        .is_err()
                    {
                        application.record_log_once(
                            ApplicationLogEvent::new(ApplicationLogCode::StatePersistFailed)
                                .with_context(ApplicationLogContext::State("window_state"))
                                .with_context(ApplicationLogContext::Operation("persist_overlay"))
                                .with_context(ApplicationLogContext::Reason(
                                    "window_state_persist_failed",
                                )),
                        );
                    } else {
                        application.record_log_once(
                            ApplicationLogEvent::new(ApplicationLogCode::WindowStatePersisted)
                                .with_context(ApplicationLogContext::Operation("overlay")),
                        );
                    }
                }
                Err(_) => application.record_log_once(
                    ApplicationLogEvent::new(ApplicationLogCode::ParsingFailed)
                        .with_context(ApplicationLogContext::Operation("overlay_placement"))
                        .with_context(ApplicationLogContext::Reason("invalid_window_placement")),
                ),
            },
            SettingsCommand::TriggerApplicationShortcut { command } => {
                if let Err(error) = apply_application_shortcut(&mut application, command) {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
                            .with_context(ApplicationLogContext::Operation("application_shortcut")),
                    );
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
            // The application polls this from its system menu loop. It answers "did anything
            // change?" and stops there on purpose: building the snapshot also scans the model
            // catalog, so polling the whole thing twenty times a second spends milliseconds of
            // filesystem work per tick on a value the poller only compares for equality.
            SettingsCommand::ReadSnapshotRevision { reply } => {
                let _ =
                    observe_snapshot_state(&application, &mut clock, startup_item.state(), false);
                let _ = reply.respond(clock.revision);
            }
            SettingsCommand::ReadAutomaticUpdateSettings { reply } => {
                let application_config = &application.config().updates;
                let _ = reply.respond(Ok(AutomaticUpdateSettings {
                    enabled: application_config.check_automatically,
                    interval_hours: application_config.check_interval_hours,
                }));
            }
            SettingsCommand::SetOverlayVisible {
                expected_config_revision,
                visible,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_overlay_visible(visible)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                match &result {
                    Ok(_) => application.record_log(
                        ApplicationLogEvent::new(ApplicationLogCode::WindowVisibilityChanged)
                            .with_context(ApplicationLogContext::Result(if visible {
                                "visible"
                            } else {
                                "hidden"
                            })),
                    ),
                    Err(error) => application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::SettingsCommandFailed)
                            .with_context(ApplicationLogContext::Operation("overlay_visibility"))
                            .with_context(ApplicationLogContext::Reason(error.code().as_str())),
                    ),
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::SetAppearanceTheme {
                expected_config_revision,
                theme,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_appearance_theme(config_theme(theme))
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetLanguage {
                expected_config_revision,
                language,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
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
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let previous = application.config().system.show_status_icon;
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
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let previous = application.config().system.show_taskbar_icon;
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
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_check_for_updates_automatically(enabled)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCheckForUpdatesIntervalHours {
                expected_config_revision,
                interval_hours,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_check_for_updates_interval_hours(interval_hours)
                            .map_err(map_application_error)
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
                    corner_radius_percent: settings.corner_radius_percent,
                    hide_on_pointer_hover: settings.hide_on_pointer_hover,
                    hide_on_pointer_hover_delay_seconds: settings
                        .hide_on_pointer_hover_delay_seconds,
                    keep_inside_screen: settings.keep_inside_screen,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_overlay_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMotionAudioEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_motion_audio_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetBehaviorShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_behavior_shortcuts_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetCommandShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_command_shortcuts_enabled(enabled)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMaximumFps {
                expected_config_revision,
                maximum_fps,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_maximum_fps(maximum_fps)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetReleaseFallbackTimeout {
                expected_config_revision,
                timeout_ms,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_release_fallback_timeout(timeout_ms)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetRandomBehaviorSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let runtime_settings = RandomBehaviorSettings {
                    enabled: settings.enabled,
                    interval_seconds: settings.interval_seconds,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_random_behavior_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
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
                    ignore_keyboard: settings.ignore_keyboard,
                    ignore_gamepad: settings.ignore_gamepad,
                    ignore_pointer: settings.ignore_pointer,
                };
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_model_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetGamepadAxisSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        let runtime_settings = bongocat_input::GamepadAxisSettings::new(
                            f32::from(settings.stick_dead_zone_percent.min(99)) / 100.0,
                            f32::from(settings.trigger_dead_zone_percent.min(99)) / 100.0,
                        )
                        .expect("bounded gamepad percentages are below 100");
                        application
                            .set_gamepad_axis_settings(runtime_settings)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetGamepadAutoSwitch {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_gamepad_auto_switch(settings)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::GamepadConnectionChanged => {
                if let Err(error) = application.apply_gamepad_auto_switch() {
                    // The current model stays on screen and the reason is
                    // anonymous, exactly as it is for a selection the user made.
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::ModelActivationFailed)
                            .with_context(ApplicationLogContext::Operation("gamepad_auto_switch"))
                            .with_context(ApplicationLogContext::Reason(error.stable_code())),
                    );
                }
            }
            SettingsCommand::SetLoggingSettings {
                expected_config_revision,
                settings,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_logging_settings(settings)
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                if let Err(error) = &result {
                    application.record_log_once(
                        ApplicationLogEvent::new(ApplicationLogCode::SettingsCommandFailed)
                            .with_context(ApplicationLogContext::Operation("logging_settings"))
                            .with_context(ApplicationLogContext::Reason(error.code().as_str())),
                    );
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::SetShortcuts {
                expected_config_revision,
                shortcuts,
                reply,
            } => {
                let result =
                    check_revision(&application, expected_config_revision).and_then(|()| {
                        application
                            .set_shortcuts(shortcuts)
                            .map(|_| ())
                            .map_err(map_application_error)
                    });
                if result.is_err() {
                    let _ = application.resume_shortcut_capture();
                }
                let result =
                    result.map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SuspendShortcutCapture {
                expected_config_revision,
                shortcuts_without_capture_target,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .suspend_shortcut_capture(shortcuts_without_capture_target)
                            .map_err(map_application_error)
                    })
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ResumeShortcutCapture { reply } => {
                let result = application
                    .resume_shortcut_capture()
                    .map_err(map_application_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStartupItemEnabled { enabled, reply } => {
                let result = startup_item.set_enabled(enabled).map(|state| {
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
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .select_model(model_origin_from_settings(model.origin), model.id)
                            .map(|_| ())
                            .map_err(map_application_error)
                    })
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::PreviewModelBehavior {
                model,
                behavior,
                reply,
            } => {
                let result = preview_model_behavior(&application, &model, behavior)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelTitle {
                expected_config_revision,
                model,
                title,
                reply,
            } => {
                let result = check_revision(&application, expected_config_revision)
                    .and_then(|()| {
                        application
                            .set_model_title(
                                model_origin_from_settings(model.origin),
                                model.id,
                                title,
                            )
                            .map_err(map_model_metadata_error)
                    })
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::SetModelCover {
                model,
                source,
                reply,
            } => {
                let result = application
                    .set_model_cover(model_origin_from_settings(model.origin), model.id, source)
                    .map(|_| ())
                    .map_err(map_model_cover_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::ReplaceModelCover { model, png, reply } => {
                // The capture that produced these bytes already knows the model is
                // installed, so a rejection here means the model disappeared between
                // the import that queued the capture and the write, or that the
                // encoder produced something the package contract refuses.
                let result = application
                    .set_model_cover_bytes(model_origin_from_settings(model.origin), model.id, &png)
                    .map(|_| ())
                    .map_err(map_model_cover_error)
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenModelLocation { model, reply } => {
                let result = match application
                    .model_directory(model_origin_from_settings(model.origin), &model.id)
                {
                    Some(directory) => model_location.open(&directory),
                    None => Err(SettingsError::new(
                        SettingsErrorCode::ModelLocationOpenFailed,
                    )),
                }
                .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::InspectModelSource { source_root, reply } => {
                let result = application
                    .inspect_model_source(source_root)
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::ImportModel {
                request,
                operation,
                reply,
            } => {
                let progress = operation.clone();
                let cancellation = operation.clone();
                let result = application
                    .import_models_with_selected_modes_with_observer(
                        request.title,
                        request.source_root,
                        request
                            .selected_mver_modes
                            .into_iter()
                            .map(model_mver_input_mode)
                            .collect(),
                        move |update| {
                            let _ = progress.report_progress(settings_import_progress(update));
                        },
                        move || cancellation.is_cancelled(),
                    )
                    .map(|installed| {
                        // A freshly imported model gets a cover rendered from the
                        // model itself rather than the one its source shipped,
                        // which for a converted BongoCatMver model is the same
                        // placeholder in every mode. The worker only queues the
                        // work: rendering it needs a native window, and this thread
                        // does not own one.
                        if let Some(signals) = signals.as_ref() {
                            for model in installed {
                                let key = SettingsModelKey {
                                    id: model.id().as_str().to_owned(),
                                    origin: settings_origin_from_model(ModelOrigin::Installed),
                                };
                                signals
                                    .request_model_cover_capture(key, CommittedModel::from(model));
                            }
                        }
                    })
                    .map(|()| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::DeleteModel { model, reply } => {
                let result = application
                    .delete_model(model_origin_from_settings(model.origin), model.id)
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_delete_error);
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
                        .export(
                            &current,
                            application.application_log_diagnostics(),
                            application.core_log_diagnostics(),
                            application.update_diagnostics(),
                        )
                        .map(|status| {
                            clock.observe_diagnostics_export(status);
                            snapshot(&application, &mut clock, false, startup_item.state())
                        })
                };
                if result.is_err() {
                    application.record_log(
                        ApplicationLogEvent::new(ApplicationLogCode::DiagnosticsExportFailed)
                            .with_context(ApplicationLogContext::Operation("export")),
                    );
                }
                let _ = reply.respond(result);
            }
            SettingsCommand::OpenLogsLocation { reply } => {
                let result = log_location
                    .open()
                    .map(|()| snapshot(&application, &mut clock, false, startup_item.state()));
                let _ = reply.respond(result);
            }
            SettingsCommand::Shutdown { reply } => {
                let before_shutdown =
                    snapshot(&application, &mut clock, false, startup_item.state());
                let state_result = persist_window_state(&mut application, &window_state);
                let shutdown_result = application.shutdown();
                let result = match (state_result, shutdown_result) {
                    (Ok(()), Ok(stopped)) => {
                        clock.mark_changed();
                        let mut stopped_snapshot = SettingsSnapshot {
                            revision: clock.revision,
                            runtime_health: RuntimeHealth::Stopped,
                            ..before_shutdown
                        };
                        stopped_snapshot.runtime_diagnostics =
                            settings_runtime_diagnostics(&stopped);
                        stopped_snapshot.input_diagnostics = settings_input_diagnostics(
                            &stopped.input,
                            stopped.platform_input,
                            clock.input_monitoring_permission(),
                        );
                        Ok(stopped_snapshot)
                    }
                    (Err(_), Ok(_)) => Err(SettingsError::new(
                        SettingsErrorCode::WindowStatePersistFailed,
                    )),
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
        Err(ApplicationError::WindowState(WindowStateError::UnsupportedSchema(_))) => Ok(()),
        result => result,
    }
}

fn system_open_backup_location(path: &std::path::Path) -> Result<(), SettingsError> {
    open_directory(path)
        .map_err(|_| SettingsError::new(SettingsErrorCode::BackupLocationOpenFailed))
}

fn system_open_model_location(path: &std::path::Path) -> Result<(), SettingsError> {
    open_directory(path).map_err(|_| SettingsError::new(SettingsErrorCode::ModelLocationOpenFailed))
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

/// How long an input-monitoring permission answer stays usable.
///
/// The system answers this query through a TCC round trip on its own dispatch queue,
/// which costs milliseconds and dominated the settings snapshot profile: every snapshot
/// used to pay it, once per settings command, once per settings refresh and once per
/// system-menu poll. The value only decides what the diagnostics page displays, and the
/// input service re-checks the permission itself before it creates or restarts an event
/// tap, so a bounded staleness here changes nothing that matters.
const INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// The system input-monitoring permission, re-read at most once per
/// [`INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL`].
#[derive(Default)]
struct InputMonitoringPermissionCache {
    checked_at: Option<Instant>,
    value: SettingsInputMonitoringPermission,
}

impl InputMonitoringPermissionCache {
    fn resolve(
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

struct SettingsSnapshotClock {
    revision: u64,
    observed_config_revision: Option<u64>,
    observed_runtime_diagnostics: Option<SettingsRuntimeDiagnostics>,
    observed_input_diagnostics: Option<SettingsInputDiagnostics>,
    observed_startup_item: Option<SettingsStartupItemStatus>,
    observed_overlay_visible: Option<bool>,
    diagnostics_export: Option<SettingsDiagnosticsExportStatus>,
    input_monitoring_permission: InputMonitoringPermissionCache,
}

impl SettingsSnapshotClock {
    const fn new(config_revision: Option<u64>) -> Self {
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

    fn input_monitoring_permission(&mut self) -> SettingsInputMonitoringPermission {
        self.input_monitoring_permission
            .resolve(Instant::now(), system_input_monitoring_permission)
    }

    fn observe_config(&mut self, config_revision: Option<u64>) {
        if config_revision != self.observed_config_revision {
            self.mark_changed();
            self.observed_config_revision = config_revision;
        }
    }

    fn mark_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn mark_catalog_changed(&mut self) {
        self.mark_changed();
    }

    fn observe_runtime_diagnostics(
        &mut self,
        diagnostics: SettingsRuntimeDiagnostics,
    ) -> Option<SettingsRuntimeDiagnostics> {
        self.observed_runtime_diagnostics.replace(diagnostics)
    }

    fn observe_input_diagnostics(
        &mut self,
        diagnostics: SettingsInputDiagnostics,
    ) -> Option<SettingsInputDiagnostics> {
        let previous = self.observed_input_diagnostics.replace(diagnostics);
        if previous.is_some_and(|previous| previous != diagnostics) {
            self.mark_changed();
        }
        previous
    }

    fn observe_startup_item(
        &mut self,
        status: SettingsStartupItemStatus,
    ) -> Option<SettingsStartupItemStatus> {
        let previous = self.observed_startup_item.replace(status);
        if previous.is_some_and(|previous| previous != status) {
            self.mark_changed();
        }
        previous
    }

    fn observe_overlay_visible(&mut self, visible: bool) {
        if self
            .observed_overlay_visible
            .is_some_and(|previous| previous != visible)
        {
            self.mark_changed();
        }
        self.observed_overlay_visible = Some(visible);
    }

    fn observe_diagnostics_export(&mut self, status: SettingsDiagnosticsExportStatus) {
        if self.diagnostics_export != Some(status) {
            self.mark_changed();
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
fn observe_snapshot_state(
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

fn settings_shortcut(command: ShortcutCommand) -> Option<SettingsApplicationShortcut> {
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

fn apply_application_shortcut(
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
        work_budget_exceeded: runtime.work.budget_exceeded,
        last_over_budget_ms: runtime.work.last_over_budget_ms,
        shutdown_timed_out: runtime.shutdown.timed_out,
        shutdown_worker_panicked: runtime.shutdown.worker_panicked,
    }
}

fn settings_input_diagnostics(
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

const fn startup_item_status_code(status: SettingsStartupItemStatus) -> &'static str {
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

const fn input_service_is_degraded(status: SettingsInputServiceStatus) -> bool {
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
const fn startup_item_available() -> bool {
    matches!(BUILD_ENVIRONMENT, BuildEnvironment::Production)
}

/// The state and the mutation answer a Development build reports.
///
/// Both directions report the same value so a client that reads and then writes
/// never observes the capability changing underneath it.
const fn startup_item_build_environment_state() -> SettingsStartupItemState {
    SettingsStartupItemState::Unsupported(SettingsStartupItemUnsupportedReason::BuildEnvironment)
}

fn system_startup_item_state() -> SettingsStartupItemStatus {
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

fn system_set_startup_item_enabled(
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

const fn startup_item_environment() -> StartupItemEnvironment {
    match crate::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => StartupItemEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => StartupItemEnvironment::Production,
    }
}

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
            entries: entries
                .into_iter()
                .map(|entry| settings_model_entry(application, entry))
                .collect(),
            error: None,
        },
        Err(_) => SettingsModelCatalog {
            entries: Vec::new(),
            error: Some(SettingsModelCatalogError::Unavailable),
        },
    }
}

fn configured_model_key(application: &Application) -> Option<SettingsModelKey> {
    let selected = application.config().model.selected_model.as_ref()?;
    Some(SettingsModelKey {
        id: selected.id.clone(),
        origin: settings_origin_from_config(selected.source),
    })
}

/// The configured gamepad-connection model switch.
///
/// A `None` target is the default and stays `None` in the snapshot: it means "the
/// last model activated for this input family", which the product resolves from
/// what happened rather than from configuration. The settings window therefore
/// shows it as its own choice instead of an empty control.
fn settings_gamepad_auto_switch(switch: &GamepadAutoSwitchConfig) -> SettingsGamepadAutoSwitch {
    SettingsGamepadAutoSwitch {
        enabled: switch.enabled,
        connected_model: switch
            .connected_model
            .as_ref()
            .map(settings_key_from_config),
        disconnected_model: switch
            .disconnected_model
            .as_ref()
            .map(settings_key_from_config),
    }
}

fn model_mver_input_mode(mode: bongocat_ui_protocol::SettingsMverMode) -> MverInputMode {
    match mode {
        bongocat_ui_protocol::SettingsMverMode::Standard => MverInputMode::Standard,
        bongocat_ui_protocol::SettingsMverMode::Keyboard => MverInputMode::Keyboard,
        bongocat_ui_protocol::SettingsMverMode::Gamepad => MverInputMode::Gamepad,
    }
}

fn settings_model_mode(mode: ModelInputMode) -> SettingsModelMode {
    match mode {
        ModelInputMode::Standard => SettingsModelMode::Standard,
        ModelInputMode::Keyboard => SettingsModelMode::Keyboard,
        ModelInputMode::Gamepad => SettingsModelMode::Gamepad,
    }
}

fn settings_model_entry(application: &Application, entry: ModelCatalogEntry) -> SettingsModelEntry {
    let id = entry.id().as_str().to_owned();
    let model_origin = entry.origin();
    let origin = settings_origin_from_model(model_origin);
    // The title is user-editable metadata; a model that was never renamed —
    // which is every preset the user has not customised — displays the stable
    // id instead of inventing a name.
    let title = application
        .recorded_model_title(model_origin, &id)
        .map(str::to_owned)
        .unwrap_or_else(|| id.clone());
    let input_mode = application
        .model_input_mode(model_origin, &id)
        .map(settings_model_mode);
    let availability = match entry {
        ModelCatalogEntry::Ready { snapshot, .. } => SettingsModelAvailability::Ready {
            behaviors: snapshot
                .behaviors
                .into_iter()
                .map(settings_model_behavior)
                .collect(),
        },
        ModelCatalogEntry::Invalid { code, .. } => SettingsModelAvailability::Invalid {
            diagnostic: settings_model_diagnostic(code),
        },
    };
    // The directory and the cover are read here rather than in the page: the
    // page only ever displays a path, and a model with no cover at all is
    // reported as `None` instead of a path that does not resolve. The cover a
    // preset ships lives in the bundle, so this is also where the user's
    // replacement gets its say.
    let directory = application.model_directory(model_origin, &id);
    let cover = application.model_cover_path(model_origin, &id);
    SettingsModelEntry {
        id,
        title,
        input_mode,
        origin,
        availability,
        directory,
        cover,
    }
}

fn settings_model_behavior(behavior: ModelBehaviorSnapshot) -> SettingsModelBehavior {
    match behavior {
        ModelBehaviorSnapshot::Motion { group, index } => {
            SettingsModelBehavior::Motion { group, index }
        }
        ModelBehaviorSnapshot::Expression { name } => SettingsModelBehavior::Expression { name },
    }
}

/// Play one behavior of the model the runtime is actually running.
///
/// The request names the model it was rendered for, because a shortcut row
/// belongs to one model's behavior list and the page keeps rows for whichever
/// model is live. The runtime only plays the active model's own motions and
/// expressions, so a request whose model has since been switched away from is
/// answered with [`SettingsErrorCode::ModelBehaviorPreviewUnavailable`] instead
/// of being played against whatever is loaded now. Nothing is persisted, and
/// the failure of a preview never changes the model in use.
fn preview_model_behavior(
    application: &Application,
    model: &SettingsModelKey,
    behavior: SettingsModelBehavior,
) -> Result<(), SettingsError> {
    let runtime = application.runtime_client().snapshot();
    let active_matches = runtime.active_model.is_some_and(|active| {
        active.id.as_str() == model.id
            && application.active_model_origin() == Some(model_origin_from_settings(model.origin))
    });
    if !active_matches {
        return Err(SettingsError::new(
            SettingsErrorCode::ModelBehaviorPreviewUnavailable,
        ));
    }

    let result = match behavior {
        SettingsModelBehavior::Motion { group, index } => application.preview_motion(group, index),
        SettingsModelBehavior::Expression { name } => application.set_expression(name),
    };
    result.map(|_| ()).map_err(map_preview_error)
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

fn check_revision(application: &Application, expected: u64) -> Result<(), SettingsError> {
    if application.config_revision() == Some(expected) {
        Ok(())
    } else {
        Err(SettingsError::new(SettingsErrorCode::SnapshotOutdated))
    }
}

/// The error a failed preview reports.
///
/// A preview is the only path that reaches the runtime's motion and expression
/// commands with an id the page built, so the id errors are preview failures
/// rather than the generic "the setting did not take effect": the page that
/// sent one has to be able to say the behavior itself could not be played.
fn map_preview_error(error: ApplicationError) -> SettingsError {
    match error {
        ApplicationError::MotionId(_)
        | ApplicationError::ExpressionId(_)
        | ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish => {
            SettingsError::new(SettingsErrorCode::ModelBehaviorPreviewFailed)
        }
        other => map_application_error(other),
    }
}

fn map_application_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::PlatformStorage(_) => SettingsErrorCode::ConfigPersistFailed,
        ApplicationError::Config(error) | ApplicationError::ConfigRollback(error) => {
            settings_config_error_code(&error).unwrap_or(SettingsErrorCode::ConfigPersistFailed)
        }
        ApplicationError::WindowState(_) => SettingsErrorCode::WindowStatePersistFailed,
        ApplicationError::Model(_) | ApplicationError::ModelStore(_) => {
            SettingsErrorCode::ModelUnavailable
        }
        ApplicationError::Shutdown(_)
        | ApplicationError::MotionAudioShutdown(_)
        | ApplicationError::ShutdownAggregate(_) => SettingsErrorCode::ShutdownFailed,
        ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish
        | ApplicationError::RuntimeDidNotPrepareModel => SettingsErrorCode::ModelSwitchFailed,
        _ => SettingsErrorCode::RuntimeUnavailable,
    };
    SettingsError::new(code)
}

/// Map a rename failure to its own code.
///
/// The three outcomes a user can act on differently — a name the configuration
/// will not accept, and a model that is no longer on disk — each get their own
/// code instead of collapsing into the generic settings failure.
fn map_model_metadata_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::ModelTitleInvalid => SettingsErrorCode::ModelTitleInvalid,
        ApplicationError::ModelNotFound(_) => SettingsErrorCode::ModelNotFound,
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

fn map_model_cover_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::ModelCoverInvalid => SettingsErrorCode::ModelCoverInvalid,
        ApplicationError::ModelNotFound(_) => SettingsErrorCode::ModelNotFound,
        ApplicationError::ModelStore(error) if error.code == ModelStoreDiagnostic::NotFound => {
            SettingsErrorCode::ModelNotFound
        }
        ApplicationError::ModelStore(_) => SettingsErrorCode::ModelCoverUpdateFailed,
        error => return map_application_error(error),
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
        // A source the store cannot read at all — an entry that is not a regular
        // file or directory, a symbolic link, or a BongoCatMver source it cannot
        // convert — is the same user-facing outcome as any other unsupported
        // source entry: the chosen source cannot be imported as it stands. The
        // distinction between them stays in the diagnostic, which is what the
        // diagnostics bundle and the log carry.
        ModelStoreDiagnostic::SourceConversionFailed
        | ModelStoreDiagnostic::SourceSymlinkUnsupported
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
        ApplicationError::ModelStore(error) => map_model_store_delete_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

const fn map_model_store_delete_diagnostic(diagnostic: ModelStoreDiagnostic) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::NotFound => SettingsErrorCode::ModelNotFound,
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::AlreadyExists
        | ModelStoreDiagnostic::Cancelled
        | ModelStoreDiagnostic::InvalidPackage
        | ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::SourceConversionFailed
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
    use bongocat_config::{
        ConfigStore, OverlayWindowPlacement, StorageLayout, WINDOW_STATE_WRITER_LOCK_FILE_NAME,
        WindowStateStore,
    };
    use bongocat_input::{
        InputDiagnostics, InputEvent, InputTransportDiagnostics, MonotonicMillis,
    };
    use bongocat_runtime::{RuntimeOwner, RuntimeWorkDiagnostics};
    use bongocat_ui_protocol::{
        DIAGNOSTICS_EXPORT_FORMAT_VERSION, SettingsModelImportRequest, SettingsModelOrigin,
        SettingsStartupItemError,
    };
    use std::{
        fs, io,
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

    struct TestModelLocation {
        opened: Mutex<Vec<PathBuf>>,
        fail: AtomicBool,
    }

    impl TestModelLocation {
        fn new() -> Self {
            Self {
                opened: Mutex::new(Vec::new()),
                fail: AtomicBool::new(false),
            }
        }

        fn opened(&self) -> Vec<PathBuf> {
            self.opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl ModelLocationCapability for TestModelLocation {
        fn open(&self, path: &Path) -> Result<(), SettingsError> {
            self.opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(path.to_owned());
            if self.fail.load(Ordering::Acquire) {
                Err(SettingsError::new(
                    SettingsErrorCode::ModelLocationOpenFailed,
                ))
            } else {
                Ok(())
            }
        }
    }

    struct TestLogLocation {
        invocations: AtomicUsize,
        fail: AtomicBool,
    }

    impl TestLogLocation {
        fn new() -> Self {
            Self {
                invocations: AtomicUsize::new(0),
                fail: AtomicBool::new(false),
            }
        }
    }

    impl LogLocationCapability for TestLogLocation {
        fn open(&self) -> Result<(), SettingsError> {
            self.invocations.fetch_add(1, Ordering::AcqRel);
            if self.fail.load(Ordering::Acquire) {
                Err(SettingsError::new(SettingsErrorCode::LogLocationOpenFailed))
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
            _core_logs: Option<CoreLogDiagnostics>,
            _update: Option<UpdateDiagnostics>,
        ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
            Ok(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 1,
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 2,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 0,
            })
        }
    }

    struct FailingDiagnosticsExport {
        calls: AtomicUsize,
    }

    impl DiagnosticsExportCapability for FailingDiagnosticsExport {
        fn export(
            &self,
            _snapshot: &SettingsSnapshot,
            _application_logs: ApplicationLogDiagnostics,
            _core_logs: Option<CoreLogDiagnostics>,
            _update: Option<UpdateDiagnostics>,
        ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
            if self.calls.fetch_add(1, Ordering::AcqRel) == 0 {
                Ok(SettingsDiagnosticsExportStatus {
                    format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                    bytes_written: 11,
                    preview_bundle_format_version: 1,
                    preview_bundle_bytes_written: 22,
                    preview_bundle_entry_count: 3,
                    preview_bundle_skipped_source_files: 0,
                })
            } else {
                Err(SettingsError::new(
                    SettingsErrorCode::DiagnosticsExportFailed,
                ))
            }
        }
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
                service_error_code: Some("platform_input_permission_denied"),
                service_start_attempts: 1,
                gamepad_backend_failures: 26,
                gamepad_connection_rejections: 27,
                gamepad_button_edges: 28,
                gamepad_axis_samples: 29,
                gamepad_axis_publish_rejections: 30,
                gamepad_event_discards: 31,
                ..PlatformInputDiagnostics::default()
            },
            SettingsInputMonitoringPermission::Granted,
        );
        // The permission is handed in rather than queried here, so what the projection
        // reports is exactly what the caller resolved.
        assert_eq!(
            projected.input_monitoring_permission,
            SettingsInputMonitoringPermission::Granted
        );
        assert_eq!(
            projected.service_status,
            SettingsInputServiceStatus::PermissionDenied
        );
        assert_eq!(projected.service_start_attempts, 1);
        assert_eq!(
            projected.service_error_code,
            Some("platform_input_permission_denied")
        );
        assert_eq!(projected.pressed_key_count, 1);
        assert_eq!(projected.pressed_mouse_button_count, 2);
        assert_eq!(projected.pressed_gamepad_button_count, 20);
        assert_eq!(projected.connected_gamepad_count, 21);
        assert_eq!(projected.platform_gamepad_backend_failures, 26);
        assert_eq!(projected.platform_gamepad_connection_rejections, 27);
        assert_eq!(projected.platform_gamepad_button_edges, 28);
        assert_eq!(projected.platform_gamepad_axis_samples, 29);
        assert_eq!(projected.platform_gamepad_axis_publish_rejections, 30);
        assert_eq!(projected.platform_gamepad_event_discards, 31);
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

        let mut clock = SettingsSnapshotClock::new(Some(7));
        let _ = clock.observe_input_diagnostics(projected);
        assert_eq!(clock.revision, 0);
        let changed = SettingsInputDiagnostics {
            transport_queue_full: 20,
            ..projected
        };
        let _ = clock.observe_input_diagnostics(changed);
        assert_eq!(clock.revision, 1);
        let _ = clock.observe_input_diagnostics(changed);
        assert_eq!(clock.revision, 1);
    }

    #[test]
    fn runtime_work_diagnostics_projection_preserves_snapshot_values() {
        let owner = RuntimeOwner::start(false, 4);
        let mut runtime = owner.client().snapshot();
        runtime.work = RuntimeWorkDiagnostics {
            budget_exceeded: 7,
            last_over_budget_ms: 19,
        };

        let projected = settings_runtime_diagnostics(&runtime);
        assert_eq!(projected.work_budget_exceeded, 7);
        assert_eq!(projected.last_over_budget_ms, 19);

        owner
            .shutdown(Duration::from_secs(1))
            .expect("runtime shutdown");
    }

    #[test]
    fn runtime_shutdown_diagnostics_projection_preserves_snapshot_values() {
        let owner = RuntimeOwner::start(false, 4);
        let mut runtime = owner.client().snapshot();
        runtime.shutdown = bongocat_runtime::RuntimeShutdownDiagnostics {
            timed_out: 3,
            worker_panicked: 2,
        };

        let projected = settings_runtime_diagnostics(&runtime);
        assert_eq!(projected.shutdown_timed_out, 3);
        assert_eq!(projected.shutdown_worker_panicked, 2);

        owner
            .shutdown(Duration::from_secs(1))
            .expect("runtime shutdown");
    }

    #[test]
    fn snapshot_clock_coalesces_changes_observed_in_one_snapshot() {
        let diagnostics = SettingsInputDiagnostics::default();
        let startup = SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled);
        let mut clock = SettingsSnapshotClock::new(Some(7));
        clock.observe_config(Some(8));
        let _ = clock.observe_input_diagnostics(diagnostics);
        let _ = clock.observe_startup_item(startup);
        clock.mark_catalog_changed();
        clock.observe_diagnostics_export(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 1,
            preview_bundle_format_version: 1,
            preview_bundle_bytes_written: 2,
            preview_bundle_entry_count: 3,
            preview_bundle_skipped_source_files: 0,
        });
        clock.coalesce_changes_since(0);
        assert_eq!(clock.revision, 1);

        clock.coalesce_changes_since(1);
        assert_eq!(clock.revision, 1);
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
    fn input_service_error_code_is_preserved_without_guessing_from_status() {
        let diagnostics = settings_input_diagnostics(
            &InputSnapshot::default(),
            PlatformInputDiagnostics {
                service_status: PlatformInputServiceStatus::Failed,
                service_error_code: Some("platform_input_tap_create_failed"),
                ..PlatformInputDiagnostics::default()
            },
            SettingsInputMonitoringPermission::Unsupported,
        );
        assert_eq!(
            diagnostics.service_status,
            SettingsInputServiceStatus::Failed
        );
        assert_eq!(
            diagnostics.service_error_code,
            Some("platform_input_tap_create_failed")
        );
    }

    #[test]
    fn input_service_error_code_drops_unregistered_provider_details() {
        let diagnostics = settings_input_diagnostics(
            &InputSnapshot::default(),
            PlatformInputDiagnostics {
                service_status: PlatformInputServiceStatus::Failed,
                service_error_code: Some("platform_input_private_detail"),
                ..PlatformInputDiagnostics::default()
            },
            SettingsInputMonitoringPermission::Unsupported,
        );
        assert_eq!(
            diagnostics.service_status,
            SettingsInputServiceStatus::Failed
        );
        assert_eq!(diagnostics.service_error_code, None);
    }

    /// The probe reports what a full snapshot would, without building one.
    ///
    /// The application polls the revision at 20 Hz for the system menu, so the cheap
    /// answer has to be exact: same value as the snapshot a client would read next, and
    /// moving whenever the configuration does.
    #[test]
    fn the_revision_probe_matches_the_snapshot_it_stands_in_for() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let revision = client
            .read_snapshot_revision_blocking()
            .expect("initial revision");
        let snapshot = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(revision, snapshot.revision);
        assert_eq!(
            client
                .read_snapshot_revision_blocking()
                .expect("stable revision"),
            revision,
            "a probe between two unchanged snapshots must not invent a revision"
        );

        let changed = client
            .set_overlay_visible_blocking(
                snapshot.config_revision.expect("configuration revision"),
                false,
            )
            .expect("overlay visibility");
        assert!(changed.revision > revision);
        assert_eq!(
            client
                .read_snapshot_revision_blocking()
                .expect("changed revision"),
            changed.revision,
            "the probe must report the change the command already published"
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    /// The input-monitoring permission is a system query, not a per-snapshot one.
    ///
    /// Snapshot builds happen for every settings command, for the window's refresh and
    /// for the system-menu poll, and the macOS answer costs milliseconds, so the cache
    /// has to hold it for a while and still pick up a permission the user grants while
    /// the product runs.
    #[test]
    fn the_input_monitoring_permission_is_cached_until_it_goes_stale() {
        let started = Instant::now();
        let answer = std::cell::Cell::new(SettingsInputMonitoringPermission::Denied);
        let probes = std::cell::Cell::new(0_u32);
        let probe = || {
            probes.set(probes.get() + 1);
            answer.get()
        };
        let mut cache = InputMonitoringPermissionCache::default();

        assert_eq!(
            cache.resolve(started, probe),
            SettingsInputMonitoringPermission::Denied
        );
        assert_eq!(probes.get(), 1, "the first read queries the system");

        answer.set(SettingsInputMonitoringPermission::Granted);
        assert_eq!(
            cache.resolve(
                started + INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL / 2,
                probe
            ),
            SettingsInputMonitoringPermission::Denied,
            "a fresh answer is reused instead of re-queried"
        );
        assert_eq!(probes.get(), 1);
        assert_eq!(
            cache.resolve(
                started + INPUT_MONITORING_PERMISSION_REFRESH_INTERVAL,
                probe
            ),
            SettingsInputMonitoringPermission::Granted,
            "a stale answer is re-queried"
        );
        assert_eq!(probes.get(), 2);
    }

    #[test]
    fn service_uses_defaults_when_current_and_backups_are_invalid() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        std::fs::write(&layout.config, b"invalid-current").expect("invalid current config");

        let application = Application::start_with_layout(layout.clone()).expect("default startup");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let snapshot = client.read_snapshot_blocking().expect("snapshot");
        assert_eq!(snapshot.runtime_health, RuntimeHealth::Ready);
        let updated = client
            .set_overlay_visible_blocking(
                snapshot.config_revision.expect("configuration revision"),
                false,
            )
            .expect("business command remains available");
        assert_eq!(updated.config_revision, snapshot.config_revision);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");

        let reloaded = store.load_or_default().expect("reloaded defaults").config;
        assert_eq!(reloaded, bongocat_config::NativeConfig::default());
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
            "The configuration backup folder could not be opened"
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
        assert_eq!(updated.config_revision, initial.config_revision);

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
        assert!(!restarted.config().system.show_status_icon);
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
        assert!(!restarted.config().system.show_taskbar_icon);
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_persists_automatic_update_preferences_and_rejects_stale_revisions() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(!initial.check_for_updates_automatically);
        assert_eq!(initial.check_for_updates_interval_hours, 24);
        let custom_interval = client
            .set_check_for_updates_interval_hours_blocking(
                initial.config_revision.expect("config revision"),
                48,
            )
            .expect("set automatic update interval");
        assert!(!custom_interval.check_for_updates_automatically);
        assert_eq!(custom_interval.check_for_updates_interval_hours, 48);

        let enabled = client
            .set_check_for_updates_automatically_blocking(
                custom_interval
                    .config_revision
                    .expect("custom interval config revision"),
                true,
            )
            .expect("enable automatic update checks");
        assert!(enabled.check_for_updates_automatically);
        assert_eq!(enabled.check_for_updates_interval_hours, 48);

        let disabled = client
            .set_check_for_updates_automatically_blocking(
                enabled.config_revision.expect("enabled config revision"),
                false,
            )
            .expect("disable automatic update checks");
        assert!(!disabled.check_for_updates_automatically);
        assert_eq!(disabled.check_for_updates_interval_hours, 48);

        let stale_enabled = client
            .set_check_for_updates_automatically_blocking(
                initial.config_revision.expect("initial config revision"),
                true,
            )
            .expect_err("reject stale automatic update preference");
        assert_eq!(stale_enabled.code(), SettingsErrorCode::SnapshotOutdated);
        // Reverting the boolean restores the custom-interval content revision, so use
        // the enabled snapshot's revision to exercise a genuinely stale interval command.
        let stale_interval = client
            .set_check_for_updates_interval_hours_blocking(
                enabled.config_revision.expect("enabled config revision"),
                72,
            )
            .expect_err("reject stale automatic update interval");
        assert_eq!(stale_interval.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(
            client.read_snapshot_blocking().expect("unchanged snapshot"),
            disabled
        );

        for invalid_interval in [
            0,
            bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS + 1,
        ] {
            let invalid = client
                .set_check_for_updates_interval_hours_blocking(
                    disabled.config_revision.expect("disabled config revision"),
                    invalid_interval,
                )
                .expect_err("reject invalid automatic update interval");
            assert_eq!(invalid.code(), SettingsErrorCode::ConfigPersistFailed);
        }
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect("invalid interval is unchanged"),
            disabled
        );
        assert_eq!(
            client
                .read_automatic_update_settings_blocking()
                .expect("automatic update schedule"),
            AutomaticUpdateSettings {
                enabled: false,
                interval_hours: 48,
            }
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(!restarted.config().updates.check_automatically);
        assert_eq!(restarted.config().updates.check_interval_hours, 48);
        restarted.shutdown().expect("restart shutdown");
    }

    #[test]
    fn service_persists_logging_settings_and_applies_them_after_commit() {
        use bongocat_ui_protocol::{SettingsLogLevel, SettingsLogging};

        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let controller = application.log_settings_controller();
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(initial.logging, SettingsLogging::default());
        let initial_revision = initial.config_revision.expect("initial config revision");
        let committed = client
            .set_logging_settings_blocking(
                initial_revision,
                SettingsLogging {
                    level: SettingsLogLevel::Trace,
                    retention_days: 30,
                },
            )
            .expect("commit logging settings");
        assert_eq!(committed.logging.level, SettingsLogLevel::Trace);
        assert_eq!(committed.logging.retention_days, 30);
        assert_ne!(committed.config_revision, initial.config_revision);
        assert_eq!(
            controller.settings(),
            bongocat_log::LogSettings {
                level: bongocat_log::LogLevel::Trace,
                retention_days: 30,
            }
        );
        let committed_config = fs::read(&config_path).expect("committed config");

        let stale = client
            .set_logging_settings_blocking(
                initial_revision,
                SettingsLogging {
                    level: SettingsLogLevel::Error,
                    retention_days: 1,
                },
            )
            .expect_err("stale logging settings");
        assert_eq!(stale.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(
            fs::read(&config_path).expect("preserved config"),
            committed_config
        );
        assert_eq!(controller.settings().retention_days, 30);

        let current_revision = committed.config_revision.expect("current config revision");
        for invalid_retention in [0, 31] {
            let invalid = client
                .set_logging_settings_blocking(
                    current_revision,
                    SettingsLogging {
                        level: SettingsLogLevel::Info,
                        retention_days: invalid_retention,
                    },
                )
                .expect_err("invalid logging retention");
            assert_eq!(invalid.code(), SettingsErrorCode::ConfigPersistFailed);
            assert_eq!(
                fs::read(&config_path).expect("preserved config"),
                committed_config
            );
            assert_eq!(controller.settings().retention_days, 30);
        }

        let occupied = config_path.with_extension("json.tmp");
        fs::create_dir(&occupied).expect("occupied config target");
        let persist_failed = client
            .set_logging_settings_blocking(
                current_revision,
                SettingsLogging {
                    level: SettingsLogLevel::Warn,
                    retention_days: 14,
                },
            )
            .expect_err("logging persistence failure");
        assert_eq!(
            persist_failed.code(),
            SettingsErrorCode::ConfigTargetOccupied
        );
        assert_eq!(
            client.read_snapshot_blocking().expect("unchanged snapshot"),
            committed
        );
        assert_eq!(controller.settings().level, bongocat_log::LogLevel::Trace);
        fs::remove_dir(occupied).expect("remove occupied config target");

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        let restarted = Application::start_with_layout(layout).expect("application restart");
        assert_eq!(
            restarted.config().logging.level,
            bongocat_config::LoggingLevel::Trace
        );
        assert_eq!(restarted.config().logging.retention_days, 30);
        assert_eq!(
            restarted.log_settings_controller().settings(),
            bongocat_log::LogSettings {
                level: bongocat_log::LogLevel::Trace,
                retention_days: 30,
            }
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
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 2,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 0,
            })
        );
        let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
        assert_eq!(refreshed, exported);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn failed_diagnostics_retry_preserves_the_last_successful_snapshot_result() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start_with_capabilities(
            application,
            Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
                SettingsStartupItemState::Disabled,
            ))),
            Arc::new(TestBackupLocation::new()),
            Arc::new(FailingDiagnosticsExport {
                calls: AtomicUsize::new(0),
            }),
        )
        .expect("service start");
        let client = service.client();

        let exported = client
            .export_diagnostics_blocking()
            .expect("initial diagnostics export");
        let retry_error = client
            .export_diagnostics_blocking()
            .expect_err("diagnostics retry must expose the provider failure");
        assert_eq!(
            retry_error.code(),
            SettingsErrorCode::DiagnosticsExportFailed
        );
        let refreshed = client.read_snapshot_blocking().expect("refreshed snapshot");
        assert_eq!(refreshed, exported);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    fn model_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root")
            .join("shared/fixtures/model-fixtures/cases/非 ASCII 模型")
    }

    /// Seed the environment model store with a package stored under an exact
    /// id. Imports always generate UUID ids, so a store entry whose id
    /// collides with a preset id can only be produced through this direct
    /// seeding; the merged catalog must still keep both identities.
    fn seed_installed_model(models_root: &std::path::Path, id: &str) {
        let destination = models_root.join(id);
        copy_model_fixture_tree(&model_fixture(), &destination);
    }

    fn copy_model_fixture_tree(source: &std::path::Path, destination: &std::path::Path) {
        std::fs::create_dir_all(destination).expect("seeded model directory");
        for entry in std::fs::read_dir(source).expect("fixture entries") {
            let entry = entry.expect("fixture entry");
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("fixture file type").is_dir() {
                copy_model_fixture_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).expect("seeded package file");
            }
        }
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
                model: SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
                behavior_id: behavior_id.to_owned(),
                shortcut: behavior_shortcut.to_owned(),
            }],
        }
    }

    #[test]
    fn shortcut_config_errors_map_to_a_stable_settings_code() {
        for field in [
            "shortcuts.command_bindings",
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
    fn service_suspends_and_restores_shortcuts_without_persisting_capture_state() {
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
            .expect("configure shortcut");
        let persisted_before_capture = std::fs::read(&layout.config).expect("persisted config");

        let suspended = client
            .suspend_shortcut_capture_blocking(
                configured.config_revision.expect("configured revision"),
                SettingsShortcuts::default(),
            )
            .expect("suspend shortcut capture");
        assert_eq!(suspended.shortcuts, configured.shortcuts);
        assert_eq!(
            std::fs::read(&layout.config).expect("capture must not persist"),
            persisted_before_capture
        );

        let resumed = client
            .resume_shortcut_capture_blocking()
            .expect("resume shortcut capture");
        assert_eq!(resumed.shortcuts, configured.shortcuts);
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
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
        // Probe the cheap revision while the handoff is in flight. Building a
        // full settings snapshot scans the model catalog, which can exceed the
        // test's whole wait under the workspace's parallel load.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let updated = loop {
            let revision = client
                .read_snapshot_revision_blocking()
                .expect("shortcut revision");
            if revision > initial.revision {
                let snapshot = client.read_snapshot_blocking().expect("updated snapshot");
                if !snapshot.overlay_visible {
                    break snapshot;
                }
            }
            if std::time::Instant::now() >= deadline {
                break client.read_snapshot_blocking().expect("updated snapshot");
            }
            std::thread::yield_now();
        };
        assert!(!updated.overlay_visible);
        assert_eq!(updated.config_revision, initial.config_revision);
        drop(sender);
        client.shutdown_blocking().expect("shutdown service");
        service.join().expect("join service");
    }

    #[test]
    fn service_maps_ignore_input_shortcuts_to_model_settings() {
        fn wait_for_snapshot<F>(client: &SettingsClient, mut predicate: F) -> SettingsSnapshot
        where
            F: FnMut(&SettingsSnapshot) -> bool,
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let snapshot = client.read_snapshot_blocking().expect("settings snapshot");
                if predicate(&snapshot) {
                    return snapshot;
                }
                assert!(
                    Instant::now() < deadline,
                    "ignore-input shortcut did not update the settings snapshot"
                );
                std::thread::yield_now();
            }
        }

        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let service =
            ApplicationSettingsService::start_with_shortcut_receiver(application, receiver)
                .expect("start settings service");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let mut revision = initial.config_revision.expect("config revision");

        for (command, field) in [
            (ShortcutCommand::ToggleIgnoreMouseInput, "ignore_pointer"),
            (
                ShortcutCommand::ToggleIgnoreKeyboardInput,
                "ignore_keyboard",
            ),
            (ShortcutCommand::ToggleIgnoreGamepadInput, "ignore_gamepad"),
        ] {
            sender.send(command).expect("queue ignore-input shortcut");
            let updated = wait_for_snapshot(&client, |snapshot| {
                snapshot.config_revision != Some(revision)
                    && match field {
                        "ignore_pointer" => snapshot.model_settings.ignore_pointer,
                        "ignore_keyboard" => snapshot.model_settings.ignore_keyboard,
                        "ignore_gamepad" => snapshot.model_settings.ignore_gamepad,
                        _ => false,
                    }
            });
            revision = updated.config_revision.expect("updated config revision");
        }

        let final_snapshot = client.read_snapshot_blocking().expect("final snapshot");
        assert!(final_snapshot.model_settings.ignore_pointer);
        assert!(final_snapshot.model_settings.ignore_keyboard);
        assert!(final_snapshot.model_settings.ignore_gamepad);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("\"ignore_pointer\": true"));
        assert!(persisted.contains("\"ignore_keyboard\": true"));
        assert!(persisted.contains("\"ignore_gamepad\": true"));

        drop(sender);
        client.shutdown_blocking().expect("shutdown service");
        service.join().expect("join service");
    }

    /// The whole chain a gamepad plug travels: the frame source's notice, the
    /// settings service reading the runtime's own answer, and the model that ends
    /// up on screen.
    #[test]
    fn a_gamepad_connection_notice_switches_the_model_the_settings_service_owns() {
        fn wait_for_snapshot<F>(client: &SettingsClient, mut predicate: F) -> SettingsSnapshot
        where
            F: FnMut(&SettingsSnapshot) -> bool,
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let snapshot = client.read_snapshot_blocking().expect("settings snapshot");
                if predicate(&snapshot) {
                    return snapshot;
                }
                assert!(
                    Instant::now() < deadline,
                    "the gamepad connection never reached the active model"
                );
                std::thread::yield_now();
            }
        }

        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("start application");
        // The producers are handles into the runtime the application owns, so
        // they are taken before the service takes ownership of it.
        let input = application.input_producer();
        let axis = application.gamepad_axis_producer();
        let service =
            ApplicationSettingsService::start(application).expect("start settings service");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(!initial.gamepad_auto_switch.enabled);
        assert_eq!(initial.gamepad_auto_switch.connected_model, None);
        // Startup activates a model before anything else runs, and the "last
        // model used" targets are exactly that history.
        let standard = SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        };
        let started = client
            .select_model_blocking(
                initial.config_revision.expect("config revision"),
                standard.clone(),
            )
            .expect("activate the startup model");
        let gamepad_model = SettingsModelKey {
            id: "gamepad".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        };
        let configured = client
            .set_gamepad_auto_switch_blocking(
                started.config_revision.expect("config revision"),
                SettingsGamepadAutoSwitch {
                    enabled: true,
                    connected_model: Some(gamepad_model.clone()),
                    disconnected_model: None,
                },
            )
            .expect("enable the auto switch");
        assert!(configured.gamepad_auto_switch.enabled);
        assert_eq!(
            configured.gamepad_auto_switch.connected_model,
            Some(gamepad_model.clone())
        );
        let revision = configured.config_revision.expect("config revision");

        // A notice with no gamepad attached is a no-op: the model that matches
        // the current state is already the one on screen.
        client
            .notify_gamepad_connection_changed()
            .expect("queue the notice");
        let untouched = client
            .read_snapshot_blocking()
            .expect("snapshot after notice");
        assert_eq!(untouched.active_model, Some(standard.clone()));

        let connection = axis.connect(0).expect("gamepad connection");
        input
            .publish(InputEvent::GamepadConnected {
                connection,
                at: MonotonicMillis::new(0),
            })
            .expect("connection event");
        // The frame source only notices the transition after the runtime has
        // applied it, so the notice is queued against an observed state.
        wait_for_snapshot(&client, |snapshot| {
            snapshot.input_diagnostics.connected_gamepad_count == 1
        });
        client
            .notify_gamepad_connection_changed()
            .expect("queue the notice");
        let connected = wait_for_snapshot(&client, |snapshot| {
            snapshot.active_model.as_ref() == Some(&gamepad_model)
        });
        assert_ne!(
            connected.config_revision,
            Some(revision),
            "the automatic switch is an ordinary model selection and is persisted"
        );

        input
            .publish(InputEvent::GamepadDisconnected {
                connection,
                at: MonotonicMillis::new(1),
            })
            .expect("disconnection event");
        wait_for_snapshot(&client, |snapshot| {
            snapshot.input_diagnostics.connected_gamepad_count == 0
        });
        client
            .notify_gamepad_connection_changed()
            .expect("queue the notice");
        let disconnected = wait_for_snapshot(&client, |snapshot| {
            snapshot.active_model.as_ref() == Some(&standard)
        });
        assert_eq!(disconnected.input_diagnostics.connected_gamepad_count, 0);
        let persisted = std::fs::read_to_string(&layout.config).expect("persisted config");
        assert!(persisted.contains("\"gamepad_auto_switch\""));
        assert!(persisted.contains("\"id\": \"gamepad\""));

        client.shutdown_blocking().expect("shutdown service");
        service.join().expect("join service");
    }

    /// An import queues one cover capture per installed model for the GPUI thread,
    /// and the bytes that projection of the model produced land on the package
    /// cover the settings catalog reports.
    #[test]
    fn service_queues_a_cover_capture_per_imported_model_and_installs_the_result() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("start application");
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let signals = ApplicationMainThreadSignals::default();
        let service = ApplicationSettingsService::start_with_shortcut_receiver_and_signals(
            application,
            receiver,
            signals.clone(),
        )
        .expect("start settings service");
        let client = service.client();
        client
            .import_model_blocking(SettingsModelImportRequest {
                title: "我的猫".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import model");

        // The worker installs the model and hands it over; it never renders it.
        let queued = signals.take_model_cover_captures();
        assert_eq!(queued.len(), 1);
        let key = queued[0].key().clone();
        assert_eq!(key.origin, SettingsModelOrigin::Imported);
        assert_eq!(queued[0].model().id().as_str(), key.id);
        // Draining is what the GPUI loop does: a second poll has nothing left.
        assert!(signals.take_model_cover_captures().is_empty());

        let canonical_models_root = models_root.canonicalize().expect("canonical models root");
        let expected_cover = canonical_models_root
            .join(&key.id)
            .join("resources/cover.png");
        let captured = b"\x89PNG\r\n\x1a\ncaptured cat";
        let covered = client
            .replace_model_cover_blocking(key.clone(), captured.to_vec())
            .expect("install captured cover");
        let entry = covered
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == key.id && entry.origin == key.origin)
            .expect("covered entry");
        assert_eq!(entry.cover, Some(expected_cover.clone()));
        assert_eq!(
            std::fs::read(&expected_cover).expect("installed cover"),
            captured
        );

        // A capture that produced something other than a PNG is refused under the
        // same contract a user-chosen cover passes, and the installed one stays.
        assert_eq!(
            client
                .replace_model_cover_blocking(key.clone(), b"not an image".to_vec())
                .expect_err("a non-PNG capture is refused")
                .code(),
            SettingsErrorCode::ModelCoverInvalid
        );
        assert_eq!(
            std::fs::read(&expected_cover).expect("unchanged cover"),
            captured
        );

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
        let signals = ApplicationMainThreadSignals::default();
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
                model: SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
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
        assert!(!initial.behavior_shortcuts_enabled);
        let initial_revision = initial.config_revision.expect("config revision");

        let enabled = client
            .set_behavior_shortcuts_enabled_blocking(initial_revision, true)
            .expect("enable behavior shortcuts");
        assert!(enabled.behavior_shortcuts_enabled);
        assert!(
            std::fs::read_to_string(&layout.config)
                .expect("persisted config")
                .contains("\"model_behaviors_enabled\": true")
        );

        let error = client
            .set_behavior_shortcuts_enabled_blocking(initial_revision, false)
            .expect_err("stale behavior shortcut update");
        assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
        assert!(
            client
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
            restarted_client
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
                    model: SettingsModelKey {
                        id: "standard".to_owned(),
                        origin: SettingsModelOrigin::BuiltIn,
                    },
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

    /// The Shortcuts page renders from this snapshot: the model catalog
    /// supplies the rows and the shortcut list supplies the chord shown in each
    /// one. Auto-assignment only counts if it reaches here — before it landed
    /// the list stayed empty and every row rendered blank.
    #[test]
    fn snapshot_carries_the_auto_assigned_behavior_shortcuts() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert!(initial.shortcuts.model_behaviors.is_empty());
        assert!(!initial.behavior_shortcuts_enabled);

        let selected = client
            .select_model_blocking(
                initial.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
            )
            .expect("select standard model");

        // Seven behaviours, one chord each, in the legacy order.
        assert_eq!(selected.shortcuts.model_behaviors.len(), 7);
        let primary = if cfg!(target_os = "macos") {
            "Meta"
        } else {
            "Control"
        };
        for (behavior_id, slot) in [
            ("motion:CAT_motion:0", 1),
            ("motion:CAT_motion_lock:1", 4),
            ("expression:live2d_expression2.exp3.json", 7),
        ] {
            let binding = selected
                .shortcuts
                .model_behaviors
                .iter()
                .find(|binding| binding.behavior_id == behavior_id)
                .unwrap_or_else(|| panic!("{behavior_id} has no default binding"));
            assert_eq!(binding.model.id, "standard");
            assert_eq!(
                binding.shortcut,
                format!("{primary}+{slot}"),
                "{behavior_id}"
            );
        }

        // The rows themselves come from the catalog entry's behaviour list, so
        // a snapshot with bindings but no behaviours would still render empty.
        let entry = selected
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == "standard")
            .expect("standard model entry");
        match &entry.availability {
            SettingsModelAvailability::Ready { behaviors, .. } => {
                assert_eq!(behaviors.len(), 7);
            }
            SettingsModelAvailability::Invalid { .. } => {
                panic!("the bundled standard model must stay valid")
            }
        }
        assert!(selected.revision > initial.revision);
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
            entry.origin == SettingsModelOrigin::BuiltIn
                && matches!(&entry.availability, SettingsModelAvailability::Ready { .. })
        }));
        assert_eq!(
            initial
                .model_catalog
                .entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.input_mode))
                .collect::<Vec<_>>(),
            vec![
                ("standard", Some(SettingsModelMode::Standard)),
                ("keyboard", Some(SettingsModelMode::Keyboard)),
                ("gamepad", Some(SettingsModelMode::Gamepad)),
            ]
        );
        let standard = initial
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == "standard" && entry.origin == SettingsModelOrigin::BuiltIn)
            .expect("standard model entry");
        let SettingsModelAvailability::Ready { behaviors, .. } = &standard.availability else {
            panic!("standard model is ready");
        };
        assert!(behaviors.contains(&SettingsModelBehavior::Motion {
            group: "CAT_motion".to_owned(),
            index: 0,
        }));
        assert!(behaviors.contains(&SettingsModelBehavior::Expression {
            name: "live2d_expression0.exp3.json".to_owned(),
        }));
        // Every preset ships its own folder and cover, so the catalog the page
        // renders has a real image and a real "open folder" target to work with.
        assert!(
            standard
                .directory
                .as_ref()
                .is_some_and(|path| path.is_dir())
        );
        assert!(standard.cover.as_ref().is_some_and(|path| path.is_file()));
        let selected = client
            .select_model_blocking(
                initial_config_revision,
                SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
            )
            .expect("select preset model");
        let selected_config_revision = selected.config_revision.expect("config revision");
        assert_eq!(
            selected.active_model,
            Some(SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            })
        );
        let overlay_settings = SettingsOverlay {
            click_through: true,
            always_on_top: false,
            scale_percent: 125,
            opacity_percent: 80,
            corner_radius_percent: 25,
            hide_on_pointer_hover: true,
            hide_on_pointer_hover_delay_seconds: 1,
            keep_inside_screen: false,
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
        // A fresh v1 configuration is silent, so the update that gets persisted
        // has to enable audio to leave a value the file can prove.
        let audio_enabled = client
            .set_motion_audio_enabled_blocking(
                hidden.config_revision.expect("config revision"),
                true,
            )
            .expect("enable motion audio");
        let model_settings = bongocat_ui_protocol::SettingsModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_keyboard: true,
            ignore_gamepad: true,
            ignore_pointer: true,
        };
        let configured_model = client
            .set_model_settings_blocking(
                audio_enabled.config_revision.expect("config revision"),
                model_settings,
            )
            .expect("update model settings");
        assert_eq!(configured_model.model_settings, model_settings);
        let random_behavior = SettingsRandomBehavior {
            enabled: true,
            interval_seconds: 9,
        };
        let configured_random_behavior = client
            .set_random_behavior_settings_blocking(
                configured_model.config_revision.expect("config revision"),
                random_behavior,
            )
            .expect("update random behavior settings");
        assert_eq!(configured_random_behavior.random_behavior, random_behavior);
        let configured_frame_rate = client
            .set_maximum_fps_blocking(
                configured_random_behavior
                    .config_revision
                    .expect("config revision"),
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
        let gamepad_settings = bongocat_ui_protocol::SettingsGamepadAxisSettings {
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
        assert!(audio_enabled.revision > hidden.revision);
        assert!(!audio_enabled.overlay_visible);
        assert!(audio_enabled.motion_audio_enabled);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(!persisted.contains("\"visible\""));
        assert!(persisted.contains("\"play_motion_audio\": true"));
        assert!(persisted.contains("\"selected_model\": {"));
        assert!(persisted.contains("\"id\": \"keyboard\""));
        assert!(persisted.contains("\"source\": \"built_in\""));
        assert!(persisted.contains("\"click_through\": true"));
        assert!(persisted.contains("\"opacity_percent\": 80"));
        assert!(persisted.contains("\"keep_inside_screen\": false"));
        assert!(persisted.contains("\"mirror\": true"));
        assert!(persisted.contains("\"mirror_pointer_tracking\": true"));
        assert!(persisted.contains("\"ignore_keyboard\": true"));
        assert!(persisted.contains("\"ignore_gamepad\": true"));
        assert!(persisted.contains("\"ignore_pointer\": true"));
        assert!(persisted.contains("\"stick_dead_zone\": 0.2"));
        assert!(persisted.contains("\"trigger_dead_zone\": 0.1"));
        assert!(persisted.contains("\"maximum_fps\": 120"));
        assert!(persisted.contains("\"release_fallback_timeout_ms\": 1500"));
        assert!(persisted.contains("\"random_behavior\": {"));
        assert!(persisted.contains("\"enabled\": true"));
        assert!(persisted.contains("\"interval_seconds\": 9"));

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
        assert_eq!(
            restarted.runtime_client().snapshot().model_settings,
            ModelSettings {
                mirror: true,
                mirror_pointer_tracking: true,
                ignore_keyboard: true,
                ignore_gamepad: true,
                ignore_pointer: true,
            }
        );
        assert_eq!(
            restarted
                .runtime_client()
                .snapshot()
                .random_behavior_settings,
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 9,
            }
        );
        assert!(
            !restarted
                .runtime_client()
                .snapshot()
                .overlay_settings
                .keep_inside_screen
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
            corner_radius_percent: 25,
            hide_on_pointer_hover: true,
            hide_on_pointer_hover_delay_seconds: 1,
            keep_inside_screen: false,
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
            corner_radius_percent: 50,
            hide_on_pointer_hover: false,
            hide_on_pointer_hover_delay_seconds: 0,
            keep_inside_screen: true,
        };
        let error = client
            .set_overlay_settings_blocking(initial_config_revision, stale_settings)
            .expect_err("stale overlay update");
        assert_eq!(error.code(), SettingsErrorCode::SnapshotOutdated);
        assert_eq!(
            error.to_string(),
            "Settings changed elsewhere. Review the latest settings and try again."
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
        let committed = client
            .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
            .expect("commit a persistent setting");
        let committed_config = std::fs::read(&config_path).expect("committed config");

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
                bongocat_ui_protocol::SettingsModelSettings {
                    mirror: true,
                    mirror_pointer_tracking: true,
                    ignore_keyboard: false,
                    ignore_gamepad: false,
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
        assert_eq!(after_stale_model_settings.revision, committed.revision);
        assert_eq!(
            after_stale_model_settings.model_settings,
            bongocat_ui_protocol::SettingsModelSettings::default()
        );
        assert_eq!(
            std::fs::read(&config_path).expect("preserved committed config"),
            committed_config
        );

        let stale_gamepad_error = client
            .set_gamepad_axis_settings_blocking(
                initial_config_revision,
                bongocat_ui_protocol::SettingsGamepadAxisSettings {
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
                    origin: SettingsModelOrigin::BuiltIn,
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
        assert_eq!(after_stale_model.revision, committed.revision);
        assert_eq!(after_stale_model.active_model, initial_active_model);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved committed config"),
            committed_config
        );

        // The fresh v1 configuration is silent (`play_motion_audio: false`), so
        // the stale request has to ask for the opposite value: a request that
        // already matched the committed config could be applied without any
        // observable difference.
        let stale_audio_error = client
            .set_motion_audio_enabled_blocking(initial_config_revision, true)
            .expect_err("stale motion audio update");
        assert_eq!(
            stale_audio_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let after_stale_audio = client
            .read_snapshot_blocking()
            .expect("snapshot after stale audio");
        assert_eq!(after_stale_audio.revision, committed.revision);
        assert!(after_stale_audio.overlay_visible);
        assert!(!after_stale_audio.motion_audio_enabled);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved committed config"),
            committed_config
        );

        let enabled = client
            .set_motion_audio_enabled_blocking(
                committed.config_revision.expect("config revision"),
                true,
            )
            .expect("enable motion audio");
        let enabled_config = std::fs::read(&config_path).expect("enabled config");
        let stale_visibility_error = client
            .set_overlay_visible_blocking(committed.config_revision.expect("config revision"), true)
            .expect_err("stale overlay visibility update");
        assert_eq!(
            stale_visibility_error.code(),
            SettingsErrorCode::SnapshotOutdated
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, enabled.revision);
        assert!(unchanged.overlay_visible);
        assert!(unchanged.motion_audio_enabled);
        assert_eq!(
            std::fs::read(&config_path).expect("preserved enabled config"),
            enabled_config
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
            .set_appearance_theme_blocking(initial_config_revision, SettingsTheme::Dark)
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

    /// Login startup is gated on the build environment, not on the platform.
    ///
    /// Written as an equality so the same test covers both feature sets: the
    /// Development build the workspace tests run as, and the `production` build
    /// the release pipeline compiles.
    #[test]
    fn login_startup_is_gated_on_the_build_environment() {
        assert_eq!(
            startup_item_available(),
            BUILD_ENVIRONMENT == BuildEnvironment::Production,
            "the gate must open for released builds and stay shut for development ones"
        );
    }

    /// A development build reports login startup as unavailable and refuses to change it.
    ///
    /// Only the Development direction is observable here: the released direction
    /// would have to register a real login item on the machine running the test.
    #[cfg(not(feature = "production"))]
    #[test]
    fn a_development_build_reports_login_startup_as_unavailable() {
        let unavailable = SettingsStartupItemState::Unsupported(
            SettingsStartupItemUnsupportedReason::BuildEnvironment,
        );

        assert_eq!(
            system_startup_item_state(),
            SettingsStartupItemStatus::State(unavailable)
        );
        // Both requested directions answer with the capability rather than with a
        // failure: the switch that would send this command renders disabled, and a
        // command that still arrives must not raise an error the user cannot act on.
        for enabled in [true, false] {
            assert_eq!(
                system_set_startup_item_enabled(enabled),
                Ok(unavailable),
                "a development build must not report a failed login-startup write"
            );
        }
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
    fn service_renames_and_covers_a_model_of_either_origin() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let overrides_root = layout.model_overrides.clone();
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        // The store canonicalizes its own root, and on macOS `$TMPDIR` resolves
        // through `/private`, so the expected paths are canonical too. The roots
        // only exist once the application has created them.
        let canonical_models_root = models_root.canonicalize().expect("canonical models root");
        let canonical_overrides_root = overrides_root
            .canonicalize()
            .expect("canonical model overrides root");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                title: "原始名称".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import model");
        let revision = imported.config_revision.expect("config revision");
        let entry = imported
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == SettingsModelOrigin::Imported)
            .expect("installed entry")
            .clone();
        let key = SettingsModelKey {
            id: entry.id.clone(),
            origin: SettingsModelOrigin::Imported,
        };
        // The page needs the package directory and, since this fixture ships no
        // cover, must be told there is none rather than guessing a path.
        assert_eq!(entry.directory, Some(canonical_models_root.join(&entry.id)));
        assert_eq!(entry.cover, None);

        let renamed = client
            .set_model_title_blocking(revision, key.clone(), "我的猫".to_owned())
            .expect("rename model");
        let renamed_entry = renamed
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == entry.id && entry.origin == key.origin)
            .expect("renamed entry");
        assert_eq!(renamed_entry.title, "我的猫");
        // The title is configuration metadata, so it survives as configuration
        // rather than living only in the snapshot the page happens to hold.
        let persisted = std::fs::read_to_string(&config_path).expect("persisted config");
        assert!(persisted.contains("我的猫"));

        let revision = renamed.config_revision.expect("config revision");
        assert_eq!(
            client
                .set_model_title_blocking(revision, key.clone(), "   ".to_owned())
                .expect_err("an empty title is not a name")
                .code(),
            SettingsErrorCode::ModelTitleInvalid
        );

        // A cover is written into the package itself and reported back through
        // the catalog, so the page can render it without knowing the layout.
        let cover_source = base.path().join("cover-source.png");
        let cover_bytes = b"\x89PNG\r\n\x1a\npairing artwork";
        std::fs::write(&cover_source, cover_bytes).expect("cover source");
        let covered = client
            .set_model_cover_blocking(key.clone(), cover_source.clone())
            .expect("replace cover");
        let covered_entry = covered
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == key.id && entry.origin == key.origin)
            .expect("covered entry");
        let expected_cover = canonical_models_root
            .join(&key.id)
            .join("resources/cover.png");
        assert_eq!(covered_entry.cover, Some(expected_cover.clone()));
        assert_eq!(
            std::fs::read(&expected_cover).expect("installed cover"),
            cover_bytes
        );

        // A file that is not a PNG is refused, and the installed cover stays.
        let not_an_image = base.path().join("notes.txt");
        std::fs::write(&not_an_image, b"not an image").expect("plain file");
        assert_eq!(
            client
                .set_model_cover_blocking(key.clone(), not_an_image)
                .expect_err("a non-PNG cover is refused")
                .code(),
            SettingsErrorCode::ModelCoverInvalid
        );
        assert_eq!(
            std::fs::read(&expected_cover).expect("unchanged cover"),
            cover_bytes
        );

        // The same two edits on a model the build ships. Its package lives
        // inside the application bundle, so the rename goes into the preset
        // list and the cover into the user's override root: the preset is
        // customised exactly like an installed model, without either of them
        // being written into the bundle.
        let preset = SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        };
        let bundled_cover = crate::repository_preset_root()
            .canonicalize()
            .expect("canonical preset root")
            .join(&preset.id)
            .join("resources/cover.png");
        let bundled_bytes = std::fs::read(&bundled_cover).expect("bundled preset cover");
        let bundled_entry = covered
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
            .expect("preset entry");
        assert_eq!(
            bundled_entry.title, "standard",
            "a preset that was never renamed is named by the id the build gave it"
        );
        assert_eq!(bundled_entry.cover, Some(bundled_cover.clone()));

        let revision = covered.config_revision.expect("config revision");
        let renamed_preset = client
            .set_model_title_blocking(revision, preset.clone(), "我的预设".to_owned())
            .expect("rename a preset model");
        let renamed_preset_entry = renamed_preset
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
            .expect("renamed preset entry");
        assert_eq!(renamed_preset_entry.title, "我的预设");
        // The preset list is its own id space: the installed model renamed
        // above kept its own record, and the same id in both lists names two
        // different models.
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).expect("persisted config"))
                .expect("config json");
        assert_eq!(
            document["model"]["built_in_models"],
            serde_json::json!([{ "id": "standard", "title": "我的预设" }])
        );

        let covered_preset = client
            .set_model_cover_blocking(preset.clone(), cover_source)
            .expect("replace a preset cover");
        let covered_preset_entry = covered_preset
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.id == preset.id && entry.origin == preset.origin)
            .expect("covered preset entry");
        let expected_preset_cover = canonical_overrides_root
            .join(&preset.id)
            .join("resources/cover.png");
        assert_eq!(
            covered_preset_entry.cover,
            Some(expected_preset_cover.clone()),
            "the replacement must be what the page draws, not the bundled artwork"
        );
        assert_eq!(
            std::fs::read(&expected_preset_cover).expect("stored preset cover"),
            cover_bytes
        );
        assert_eq!(
            std::fs::read(&bundled_cover).expect("bundled cover after the edit"),
            bundled_bytes,
            "a preset's package is app-bundled and must never be written to"
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_opens_the_application_log_directory_without_advancing_revision() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let log_location = Arc::new(TestLogLocation::new());
        let service =
            ApplicationSettingsService::start_with_log_location(application, log_location.clone())
                .expect("service start");
        let client = service.client();
        let before = client.read_snapshot_blocking().expect("initial snapshot");

        let opened = client
            .open_logs_location_blocking()
            .expect("open application log folder");
        assert_eq!(log_location.invocations.load(Ordering::Acquire), 1);
        assert_eq!(
            opened.config_revision, before.config_revision,
            "opening a log folder is not a configuration change"
        );

        log_location.fail.store(true, Ordering::Release);
        assert_eq!(
            client
                .open_logs_location_blocking()
                .expect_err("failed open")
                .code(),
            SettingsErrorCode::LogLocationOpenFailed
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_opens_a_models_own_folder_without_advancing_revision() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let canonical_models_root = models_root.canonicalize().expect("canonical models root");
        let model_location = Arc::new(TestModelLocation::new());
        let service = ApplicationSettingsService::start_with_model_location(
            application,
            model_location.clone(),
        )
        .expect("service start");
        let client = service.client();

        client
            .import_model_blocking(SettingsModelImportRequest {
                title: "我的猫".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import model");
        let snapshot = client.read_snapshot_blocking().expect("snapshot");
        let entry = snapshot
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == SettingsModelOrigin::Imported)
            .expect("installed entry");
        let key = SettingsModelKey {
            id: entry.id.clone(),
            origin: SettingsModelOrigin::Imported,
        };

        let opened = client
            .open_model_location_blocking(key.clone())
            .expect("open model folder");
        assert_eq!(
            model_location.opened(),
            vec![canonical_models_root.join(&key.id)]
        );
        assert_eq!(
            opened.config_revision, snapshot.config_revision,
            "opening a folder is not a configuration change"
        );

        // A file manager that refuses is reported as its own outcome rather than
        // as a silent no-op.
        model_location.fail.store(true, Ordering::Release);
        assert_eq!(
            client
                .open_model_location_blocking(key.clone())
                .expect_err("failed open")
                .code(),
            SettingsErrorCode::ModelLocationOpenFailed
        );

        // A model whose directory is gone has nothing to open.
        std::fs::remove_dir_all(models_root.join(&key.id)).expect("remove model directory");
        assert_eq!(
            client
                .open_model_location_blocking(key)
                .expect_err("missing model directory")
                .code(),
            SettingsErrorCode::ModelLocationOpenFailed
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
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
                title: "送葬人 · 标准模式".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import model");
        assert!(imported.revision > initial.revision);
        assert_eq!(
            imported.active_model, None,
            "import must not implicitly activate the model"
        );
        let first = imported
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == SettingsModelOrigin::Imported)
            .expect("installed entry");
        assert_eq!(first.title, "送葬人 · 标准模式");
        assert_eq!(
            first.input_mode,
            Some(SettingsModelMode::Standard),
            "the ordinary package is classified from its left-keys artwork, not its title"
        );
        assert!(matches!(
            &first.availability,
            SettingsModelAvailability::Ready { .. }
        ));
        assert!(
            bongocat_model::ModelId::parse(&first.id).is_ok(),
            "the store key must be a portable id"
        );
        assert!(models_root.join(&first.id).join("猫.model3.json").is_file());

        // Importing the same source folder again stays independent: both ids
        // are service-generated UUIDs, never derived from titles or names.
        let second = client
            .import_model_blocking(SettingsModelImportRequest {
                title: "经典小键盘 · 标准模式".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("second import of the same source");
        assert!(second.revision > imported.revision);
        let installed: Vec<_> = second
            .model_catalog
            .entries
            .iter()
            .filter(|entry| entry.origin == SettingsModelOrigin::Imported)
            .collect();
        assert_eq!(
            installed.len(),
            2,
            "both imports stay installed side by side"
        );
        assert_ne!(installed[0].id, installed[1].id, "ids are generated UUIDs");
        assert!(
            installed
                .iter()
                .any(|entry| entry.title == "经典小键盘 · 标准模式")
        );
        for entry in &installed {
            assert_eq!(entry.input_mode, Some(SettingsModelMode::Standard));
            assert!(matches!(
                &entry.availability,
                SettingsModelAvailability::Ready { .. }
            ));
            assert!(models_root.join(&entry.id).join("猫.model3.json").is_file());
        }

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
                title: "cancelled-model".to_owned(),
                source_root: source.path().to_owned(),
                selected_mver_modes: Vec::new(),
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
                ModelStoreDiagnostic::SourceConversionFailed,
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
        // Enumerating the cases is only useful while it stays complete, so a new
        // store diagnostic cannot reach the settings service unmapped.
        for diagnostic in ModelStoreDiagnostic::ALL {
            assert!(
                cases.iter().any(|(case, _)| *case == diagnostic),
                "{diagnostic:?} has no import result code"
            );
        }
    }

    #[test]
    fn every_model_store_delete_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::NotFound,
                SettingsErrorCode::ModelNotFound,
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
                ModelStoreDiagnostic::SourceConversionFailed,
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
        for diagnostic in ModelStoreDiagnostic::ALL {
            assert!(
                cases.iter().any(|(case, _)| *case == diagnostic),
                "{diagnostic:?} has no delete result code"
            );
        }
    }

    #[test]
    fn invalid_random_behavior_settings_leave_config_and_runtime_unchanged() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application =
            Application::start_with_layout(layout.clone()).expect("application start");
        let runtime = application.runtime_client();
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config_revision = initial.config_revision.expect("config revision");
        let initial_runtime = runtime.snapshot();
        let initial_bytes = std::fs::read(&layout.config).expect("initial config bytes");

        for interval_seconds in [0, 3_601] {
            client
                .set_random_behavior_settings_blocking(
                    initial_config_revision,
                    SettingsRandomBehavior {
                        enabled: true,
                        interval_seconds,
                    },
                )
                .expect_err("invalid random behavior settings must fail");
            assert_eq!(
                client
                    .read_snapshot_blocking()
                    .expect("snapshot after rejected setting")
                    .config_revision,
                Some(initial_config_revision)
            );
            assert_eq!(
                runtime.snapshot().random_behavior_settings,
                initial_runtime.random_behavior_settings
            );
            assert_eq!(
                std::fs::read(&layout.config).expect("config after rejected setting"),
                initial_bytes
            );
        }
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_deletes_only_unselected_installed_source_identity() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        seed_installed_model(&layout.models, "standard");
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let current = client.read_snapshot_blocking().expect("initial snapshot");

        let selected = client
            .select_model_blocking(
                current.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "standard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
            )
            .expect("select preset duplicate");
        let deleted = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Imported,
            })
            .expect("delete installed duplicate");
        assert!(selected.revision > current.revision);
        assert!(deleted.revision > selected.revision);
        assert_eq!(
            deleted.active_model,
            Some(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            })
        );
        assert!(!deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Imported
        }));
        assert!(deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::BuiltIn
        }));

        let preset_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            })
            .expect_err("preset deletion");
        assert_eq!(
            preset_error.code(),
            SettingsErrorCode::PresetModelCannotBeDeleted
        );
        let missing_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "missing".to_owned(),
                origin: SettingsModelOrigin::Imported,
            })
            .expect_err("missing installed model");
        assert_eq!(missing_error.code(), SettingsErrorCode::ModelNotFound);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_deletes_the_selected_installed_model_and_switches_to_the_preset() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                title: "selected".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import model");
        let installed_id = imported
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == SettingsModelOrigin::Imported)
            .expect("installed entry")
            .id
            .clone();
        let selected = client
            .select_model_blocking(
                imported.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: installed_id.clone(),
                    origin: SettingsModelOrigin::Imported,
                },
            )
            .expect("select installed model");

        let deleted = client
            .delete_model_blocking(SettingsModelKey {
                id: installed_id.clone(),
                origin: SettingsModelOrigin::Imported,
            })
            .expect("selected model deletion switches away first");
        // One snapshot carries both halves: the package is gone from the
        // catalog, and the model that replaced it is the standard preset.
        assert!(deleted.revision >= selected.revision);
        assert!(!deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == installed_id && entry.origin == SettingsModelOrigin::Imported
        }));
        assert_eq!(
            deleted.active_model.as_ref().map(|model| model.origin),
            Some(SettingsModelOrigin::BuiltIn)
        );

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

        // The import title is free-form display text and never becomes the
        // store key: even a path-like title imports cleanly with a
        // service-generated UUID id. Deletion still validates ids strictly.
        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                title: "../escape".to_owned(),
                source_root: model_fixture(),
                selected_mver_modes: Vec::new(),
            })
            .expect("import with a path-like title");
        let imported_entry = imported
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == SettingsModelOrigin::Imported)
            .expect("installed entry");
        assert_eq!(imported_entry.title, "../escape");
        assert!(bongocat_model::ModelId::parse(&imported_entry.id).is_ok());

        let invalid_delete_id = client
            .delete_model_blocking(SettingsModelKey {
                id: "../escape".to_owned(),
                origin: SettingsModelOrigin::Imported,
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
                title: "invalid-package".to_owned(),
                source_root: invalid_package_source.path().to_owned(),
                selected_mver_modes: Vec::new(),
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

        let window_state = WindowStateStore::new(layout.clone())
            .load_or_default()
            .state;
        assert_eq!(
            window_state.settings_window,
            Some(
                WindowPlacement::new(-360, 144, 1040, 760, false)
                    .expect("valid persisted placement")
            )
        );
        assert_eq!(
            window_state.overlay_window,
            Some(
                OverlayWindowPlacement::new(-640, 220, 420, 560).expect("valid overlay placement")
            )
        );
        let selected = client
            .select_model_blocking(
                initial.config_revision.expect("config revision"),
                SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::BuiltIn,
                },
            )
            .expect("select model");
        client
            .set_overlay_visible_blocking(selected.config_revision.expect("config revision"), false)
            .expect("update configuration");

        let persisted_state = WindowStateStore::new(layout).load_or_default().state;
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
    fn corrupt_window_state_never_blocks_configuration_or_runtime_startup() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let loaded = store.load_or_default().expect("default config");
        let config_before = std::fs::read(&layout.config).expect("config bytes");
        std::fs::write(&layout.window_state, b"corrupt-window-state")
            .expect("corrupt state fixture");

        let application = Application::start_with_layout(layout.clone())
            .expect("corrupt state must not block application startup");
        assert_eq!(application.settings_window_placement(), None);
        assert_eq!(application.config(), &loaded.config);
        assert_eq!(
            std::fs::read(&layout.config).expect("config preserved"),
            config_before
        );
        assert_eq!(
            std::fs::read(&layout.window_state).expect("state preserved until flush"),
            b"corrupt-window-state"
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn future_window_state_is_preserved_without_failing_service_shutdown() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        store.load_or_default().expect("default config");
        let future = br#"{"schema_version":2,"settings_window":null,"overlay_window":null}"#;
        std::fs::write(&layout.window_state, future).expect("future state");

        let application = Application::start_with_layout(layout.clone())
            .expect("future state must not block startup");
        let service = ApplicationSettingsService::start(application).expect("service start");
        service
            .client()
            .shutdown_blocking()
            .expect("future state must not fail shutdown");
        service.join().expect("service join");
        assert_eq!(
            std::fs::read(&layout.window_state).expect("future state preserved"),
            future
        );
    }

    #[test]
    fn window_state_write_failure_is_reported_after_runtime_still_stops() {
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
            .open(layout.locks.join(WINDOW_STATE_WRITER_LOCK_FILE_NAME))
            .expect("state writer lock");
        state_lock.lock().expect("hold state writer lock");

        let error = service
            .client()
            .shutdown_blocking()
            .expect_err("state lock must report persistence failure");
        assert_eq!(error.code(), SettingsErrorCode::WindowStatePersistFailed);
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
