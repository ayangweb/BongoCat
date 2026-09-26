//! The seams the settings worker reaches the system through.
//!
//! Opening a folder, toggling a login item and showing a tray or taskbar icon all
//! happen off the worker thread, and none of them can be asserted from a unit test
//! that must not actually launch a window manager or move a real icon. Each
//! therefore sits behind a small capability, with a system implementation and,
//! where refusing is a real outcome the page reports, one that refuses.

// The settings vocabulary. `super` already imports every type this module's code
// names; what follows are the sibling modules whose values it reads.
use super::*;

use super::projection::*;

pub(super) trait StartupItemCapability: Send + Sync + 'static {
    fn state(&self) -> SettingsStartupItemStatus;

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError>;
}

pub trait StatusIconCapability: Send + Sync + 'static {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError>;
}

pub trait TaskbarIconCapability: Send + Sync + 'static {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError>;
}

pub(super) struct VisibilityCapabilities {
    pub(super) status_icon: Arc<dyn StatusIconCapability>,
    pub(super) taskbar_icon: Arc<dyn TaskbarIconCapability>,
}

pub(super) trait BackupLocationCapability: Send + Sync + 'static {
    fn open(&self) -> Result<(), SettingsError>;
}

/// Opening a model's own folder in the system file manager.
///
/// The same seam the configuration backup folder uses: a unit test must be able
/// to assert the outcome of "open model folder" without actually launching a
/// window manager, so the system call sits behind a capability.
pub(super) trait ModelLocationCapability: Send + Sync + 'static {
    fn open(&self, path: &Path) -> Result<(), SettingsError>;
}

pub(super) trait DiagnosticsExportCapability: Send + Sync + 'static {
    fn export(
        &self,
        snapshot: &SettingsSnapshot,
        application_logs: ApplicationLogDiagnostics,
        core_logs: Option<CoreLogDiagnostics>,
        update: Option<UpdateDiagnostics>,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError>;
}

pub(super) struct SystemStartupItem;

pub(super) struct UnavailableStatusIcon;

pub(super) struct UnavailableTaskbarIcon;

pub(super) struct SystemBackupLocation {
    pub(super) path: PathBuf,
}

pub(super) struct SystemModelLocation;

/// The test seam for a file manager that is not there. Refusing is a real
/// outcome the page reports, and it keeps a test from opening a real window.
#[cfg(test)]
pub(super) struct UnavailableModelLocation;

/// Opening the application-owned log directory in the system file manager.
///
/// The path is kept in the settings worker so the UI never needs to know a
/// storage root, just as it does for the configuration backup location.
pub(super) trait LogLocationCapability: Send + Sync + 'static {
    fn open(&self) -> Result<(), SettingsError>;
}

#[cfg(test)]
pub(super) struct UnavailableLogLocation;

pub(super) struct SystemLogLocation {
    pub(super) path: PathBuf,
}

pub(super) struct SystemDiagnosticsExport {
    pub(super) path: PathBuf,
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

pub(super) fn system_open_backup_location(path: &std::path::Path) -> Result<(), SettingsError> {
    open_directory(path)
        .map_err(|_| SettingsError::new(SettingsErrorCode::BackupLocationOpenFailed))
}

pub(super) fn system_open_model_location(path: &std::path::Path) -> Result<(), SettingsError> {
    open_directory(path).map_err(|_| SettingsError::new(SettingsErrorCode::ModelLocationOpenFailed))
}
