//! The settings worker: the one owner of the `Application` once the GPUI thread
//! hands it over.
//!
//! Starting the service, the threads it owns and its shutdown boundary live here.
//! What those threads do is split across this directory: the command loop in
//! [`worker`], the snapshot and its revision clock in [`snapshot`], the seams to
//! the system in [`capabilities`], and the projections onto the settings protocol
//! in [`projection`], [`model_projection`] and [`error_mapping`]. One file
//! holding all of it was the reason the settings boundary was the hardest part of
//! this crate to follow.

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

mod capabilities;
mod error_mapping;
mod model_projection;
mod projection;
mod snapshot;
#[cfg(test)]
mod tests;
mod worker;

use capabilities::{
    BackupLocationCapability, DiagnosticsExportCapability, LogLocationCapability,
    ModelLocationCapability, StartupItemCapability, SystemBackupLocation, SystemDiagnosticsExport,
    SystemLogLocation, SystemModelLocation, SystemStartupItem, UnavailableStatusIcon,
    UnavailableTaskbarIcon, VisibilityCapabilities,
};
pub use capabilities::{StatusIconCapability, TaskbarIconCapability};
#[cfg(test)]
use capabilities::{UnavailableLogLocation, UnavailableModelLocation};
use projection::settings_shortcut;
use worker::{run_service, settings_window_placement};

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
