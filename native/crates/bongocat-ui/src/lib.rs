#![forbid(unsafe_code)]

use async_channel::{Receiver, Sender};
use std::{
    fmt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod window;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use window::{SettingsView, SettingsWindowHandle, open_settings_window};

const MIN_SETTINGS_WINDOW_WIDTH: u32 = 640;
const MIN_SETTINGS_WINDOW_HEIGHT: u32 = 480;
const MAX_SETTINGS_WINDOW_DIMENSION: u32 = 16_384;
const MAX_SETTINGS_WINDOW_COORDINATE: i32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsWindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl SettingsWindowPlacement {
    pub fn new(x: i32, y: i32, width: u32, height: u32, maximized: bool) -> Option<Self> {
        if !(-MAX_SETTINGS_WINDOW_COORDINATE..=MAX_SETTINGS_WINDOW_COORDINATE).contains(&x)
            || !(-MAX_SETTINGS_WINDOW_COORDINATE..=MAX_SETTINGS_WINDOW_COORDINATE).contains(&y)
            || !(MIN_SETTINGS_WINDOW_WIDTH..=MAX_SETTINGS_WINDOW_DIMENSION).contains(&width)
            || !(MIN_SETTINGS_WINDOW_HEIGHT..=MAX_SETTINGS_WINDOW_DIMENSION).contains(&height)
        {
            return None;
        }
        Some(Self {
            x,
            y,
            width,
            height,
            maximized,
        })
    }
}

#[derive(Clone, Default)]
pub struct SettingsWindowState {
    placement: Arc<Mutex<Option<SettingsWindowPlacement>>>,
    change_revision: Arc<AtomicU64>,
    commands: Option<Sender<SettingsCommand>>,
}

impl SettingsWindowState {
    pub fn new(placement: Option<SettingsWindowPlacement>) -> Self {
        Self {
            placement: Arc::new(Mutex::new(placement)),
            change_revision: Arc::new(AtomicU64::new(0)),
            commands: None,
        }
    }

    fn tracked(
        placement: Option<SettingsWindowPlacement>,
        commands: Sender<SettingsCommand>,
    ) -> Self {
        Self {
            placement: Arc::new(Mutex::new(placement)),
            change_revision: Arc::new(AtomicU64::new(0)),
            commands: Some(commands),
        }
    }

    pub fn placement(&self) -> Option<SettingsWindowPlacement> {
        *self
            .placement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn update(&self, placement: SettingsWindowPlacement) -> Option<u64> {
        let mut current = self
            .placement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == Some(placement) {
            return None;
        }
        *current = Some(placement);
        Some(self.change_revision.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub fn request_persist_if_current(&self, revision: u64) -> bool {
        if self.change_revision.load(Ordering::Acquire) != revision {
            return true;
        }
        if let Some(commands) = self.commands.as_ref() {
            return match commands.try_send(SettingsCommand::SettingsWindowPlacementChanged) {
                Ok(()) | Err(async_channel::TrySendError::Closed(_)) => true,
                Err(async_channel::TrySendError::Full(_)) => false,
            };
        }
        true
    }
}

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
    PlatformUnsupported,
    TransportClosed,
    OverlaySettingsInvalid,
}

impl SettingsRuntimeErrorCode {
    pub const ALL: [Self; 8] = [
        Self::ModelLoadFailed,
        Self::ModelEvaluationFailed,
        Self::MotionLoadFailed,
        Self::ExpressionLoadFailed,
        Self::GpuPreparationFailed,
        Self::PlatformUnsupported,
        Self::TransportClosed,
        Self::OverlaySettingsInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLoadFailed => "model_load_failed",
            Self::ModelEvaluationFailed => "model_evaluation_failed",
            Self::MotionLoadFailed => "motion_load_failed",
            Self::ExpressionLoadFailed => "expression_load_failed",
            Self::GpuPreparationFailed => "gpu_preparation_failed",
            Self::PlatformUnsupported => "platform_unsupported",
            Self::TransportClosed => "transport_closed",
            Self::OverlaySettingsInvalid => "overlay_settings_invalid",
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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsInputDiagnostics {
    pub service_status: SettingsInputServiceStatus,
    pub service_start_attempts: u64,
    pub pressed_key_count: usize,
    pub pressed_mouse_button_count: usize,
    pub pressed_gamepad_button_count: usize,
    pub connected_gamepad_count: usize,
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
pub struct SettingsConfigRecovery {
    pub source_schema_version: u32,
    pub skipped_newer_backups: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsDiagnosticsExportStatus {
    pub format_version: u32,
    pub bytes_written: u64,
}

/// Version of the anonymous diagnostics export JSON contract.
pub const DIAGNOSTICS_EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsConfigurationStatus {
    Ready,
    RecoveryRequired { checked_backups: u32 },
    DefaultsRestoredRestartRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub config_revision: Option<u64>,
    pub runtime_health: RuntimeHealth,
    pub runtime_diagnostics: SettingsRuntimeDiagnostics,
    pub overlay_visible: bool,
    pub overlay: SettingsOverlay,
    pub motion_audio_enabled: bool,
    pub model_settings: SettingsModelSettings,
    pub gamepad_axis_settings: SettingsGamepadAxisSettings,
    pub shortcuts: SettingsShortcuts,
    pub startup_item: SettingsStartupItemStatus,
    pub configuration_status: SettingsConfigurationStatus,
    pub config_recovery: Option<SettingsConfigRecovery>,
    pub diagnostics_export: Option<SettingsDiagnosticsExportStatus>,
    pub input_diagnostics: SettingsInputDiagnostics,
    pub active_model: Option<SettingsModelKey>,
    pub model_catalog: SettingsModelCatalog,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsShortcuts {
    pub commands: Vec<SettingsShortcutBinding>,
    pub model_behaviors: Vec<SettingsModelBehaviorBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsShortcutBinding {
    pub command: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelBehaviorBinding {
    pub model_id: String,
    pub behavior_id: String,
    pub shortcut: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsModelSettings {
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    pub ignore_pointer: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsGamepadAxisSettings {
    pub stick_dead_zone_percent: u8,
    pub trigger_dead_zone_percent: u8,
}

impl Default for SettingsGamepadAxisSettings {
    fn default() -> Self {
        Self {
            stick_dead_zone_percent: 15,
            trigger_dead_zone_percent: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsOverlay {
    pub click_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self {
            click_through: false,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemStatus {
    State(SettingsStartupItemState),
    ReadError(SettingsStartupItemError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemState {
    Unsupported(SettingsStartupItemUnsupportedReason),
    Disabled,
    Enabled,
    Stale,
    RequiresApproval,
    NotFound,
}

impl SettingsStartupItemState {
    pub const fn can_set_enabled(self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemUnsupportedReason {
    Platform,
    OperatingSystem,
    BuildEnvironment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsStartupItemError {
    CurrentExecutableUnavailable,
    InvalidExecutablePath,
    BackendUnavailable,
    StateReadFailed,
    EnableFailed,
    DisableFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelKey {
    pub id: String,
    pub origin: SettingsModelOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelImportRequest {
    pub id: String,
    pub source_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SettingsOperationId(u64);

impl SettingsOperationId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelImportStage {
    Preparing,
    Copying,
    Validating,
    Committing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsModelImportProgress {
    pub stage: SettingsModelImportStage,
    pub files_copied: u64,
    pub bytes_copied: u64,
}

pub struct SettingsModelImportFinalResult {
    pub operation_id: SettingsOperationId,
    pub result: Result<SettingsSnapshot, SettingsError>,
}

#[derive(Clone)]
pub struct SettingsModelImportControl {
    operation_id: SettingsOperationId,
    cancelled: Arc<AtomicBool>,
    progress: Arc<Mutex<SettingsModelImportProgress>>,
}

impl SettingsModelImportControl {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.operation_id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn report_progress(&self, progress: SettingsModelImportProgress) -> bool {
        let mut current = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.stage < current.stage
            || progress.files_copied < current.files_copied
            || progress.bytes_copied < current.bytes_copied
        {
            return false;
        }
        *current = progress;
        true
    }
}

pub struct SettingsModelImportOperation {
    control: SettingsModelImportControl,
    result: Receiver<Result<SettingsSnapshot, SettingsError>>,
}

#[derive(Clone)]
pub struct SettingsModelImportMonitor {
    control: SettingsModelImportControl,
}

impl SettingsModelImportMonitor {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.control.operation_id()
    }

    pub fn progress(&self) -> SettingsModelImportProgress {
        *self
            .control
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn cancel(&self) -> bool {
        !self.control.cancelled.swap(true, Ordering::AcqRel)
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }
}

impl SettingsModelImportOperation {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.control.operation_id()
    }

    pub fn progress(&self) -> SettingsModelImportProgress {
        self.monitor().progress()
    }

    pub fn cancel(&self) -> bool {
        self.monitor().cancel()
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }

    pub fn monitor(&self) -> SettingsModelImportMonitor {
        SettingsModelImportMonitor {
            control: self.control.clone(),
        }
    }

    pub async fn final_result(self) -> SettingsModelImportFinalResult {
        let operation_id = self.operation_id();
        let result = self
            .result
            .recv()
            .await
            .unwrap_or_else(|_| Err(SettingsError::new(SettingsErrorCode::ServiceUnavailable)));
        SettingsModelImportFinalResult {
            operation_id,
            result,
        }
    }

    pub fn final_result_blocking(self) -> SettingsModelImportFinalResult {
        let operation_id = self.operation_id();
        let result = self
            .result
            .recv_blocking()
            .unwrap_or_else(|_| Err(SettingsError::new(SettingsErrorCode::ServiceUnavailable)));
        SettingsModelImportFinalResult {
            operation_id,
            result,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsModelCatalog {
    pub entries: Vec<SettingsModelEntry>,
    pub error: Option<SettingsModelCatalogError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelEntry {
    pub id: String,
    pub origin: SettingsModelOrigin,
    pub availability: SettingsModelAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsModelOrigin {
    Preset,
    Installed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsModelAvailability {
    Ready {
        texture_count: usize,
        expression_count: usize,
        motion_count: usize,
    },
    Invalid {
        diagnostic: SettingsModelDiagnostic,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsModelDiagnostic {
    InvalidModelId,
    ModelEntryAmbiguous,
    ModelEntryMissing,
    ModelFileCountExceeded,
    ModelFileTooLarge,
    ModelIoError,
    ModelJsonInvalid,
    ModelJsonTooLarge,
    ModelMocMissing,
    ModelPackageDepthExceeded,
    ModelPackageSizeExceeded,
    ModelReferenceEscapesRoot,
    ModelReferenceInvalid,
    ModelReferenceSymlinkEscape,
    ModelResourceInvalid,
    ModelResourceMissing,
    ModelResourceNotFile,
    ModelSymlinkDirectoryUnsupported,
    ModelTextureDimensionExceeded,
    ModelTextureInvalidPng,
    ModelTextureMissing,
    ModelUnsupportedVersion,
}

impl SettingsModelDiagnostic {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidModelId => "invalid_model_id",
            Self::ModelEntryAmbiguous => "model_entry_ambiguous",
            Self::ModelEntryMissing => "model_entry_missing",
            Self::ModelFileCountExceeded => "model_file_count_exceeded",
            Self::ModelFileTooLarge => "model_file_too_large",
            Self::ModelIoError => "model_io_error",
            Self::ModelJsonInvalid => "model_json_invalid",
            Self::ModelJsonTooLarge => "model_json_too_large",
            Self::ModelMocMissing => "model_moc_missing",
            Self::ModelPackageDepthExceeded => "model_package_depth_exceeded",
            Self::ModelPackageSizeExceeded => "model_package_size_exceeded",
            Self::ModelReferenceEscapesRoot => "model_reference_escapes_root",
            Self::ModelReferenceInvalid => "model_reference_invalid",
            Self::ModelReferenceSymlinkEscape => "model_reference_symlink_escape",
            Self::ModelResourceInvalid => "model_resource_invalid",
            Self::ModelResourceMissing => "model_resource_missing",
            Self::ModelResourceNotFile => "model_resource_not_file",
            Self::ModelSymlinkDirectoryUnsupported => "model_symlink_directory_unsupported",
            Self::ModelTextureDimensionExceeded => "model_texture_dimension_exceeded",
            Self::ModelTextureInvalidPng => "model_texture_invalid_png",
            Self::ModelTextureMissing => "model_texture_missing",
            Self::ModelUnsupportedVersion => "model_unsupported_version",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsModelCatalogError {
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsErrorCode {
    ServiceUnavailable,
    SnapshotOutdated,
    RuntimeUnavailable,
    InvalidGamepadAxisSettings,
    InvalidShortcutBindings,
    ConfigPersistFailed,
    ConfigPermissionDenied,
    ConfigStorageFull,
    ConfigTargetOccupied,
    BackupLocationOpenFailed,
    ConfigurationRecoveryRequired,
    ConfigurationRecoveryFailed,
    ModelUnavailable,
    ModelSwitchFailed,
    InvalidModelId,
    ModelAlreadyInstalled,
    ModelImportInvalidPackage,
    ModelImportSourceInvalid,
    ModelImportSourceChanged,
    ModelImportSourceUnsupported,
    ModelImportCancelled,
    ModelStoreBusy,
    ModelImportFailed,
    PresetModelCannotBeDeleted,
    SelectedModelCannotBeDeleted,
    ModelNotInstalled,
    ModelDeleteFailed,
    DiagnosticsExportFailed,
    StartupItemUpdateFailed,
    WindowUnavailable,
    StatePersistFailed,
    ShutdownFailed,
}

impl SettingsErrorCode {
    pub const ALL: [Self; 32] = [
        Self::ServiceUnavailable,
        Self::SnapshotOutdated,
        Self::RuntimeUnavailable,
        Self::InvalidGamepadAxisSettings,
        Self::InvalidShortcutBindings,
        Self::ConfigPersistFailed,
        Self::ConfigPermissionDenied,
        Self::ConfigStorageFull,
        Self::ConfigTargetOccupied,
        Self::BackupLocationOpenFailed,
        Self::ConfigurationRecoveryRequired,
        Self::ConfigurationRecoveryFailed,
        Self::ModelUnavailable,
        Self::ModelSwitchFailed,
        Self::InvalidModelId,
        Self::ModelAlreadyInstalled,
        Self::ModelImportInvalidPackage,
        Self::ModelImportSourceInvalid,
        Self::ModelImportSourceChanged,
        Self::ModelImportSourceUnsupported,
        Self::ModelImportCancelled,
        Self::ModelStoreBusy,
        Self::ModelImportFailed,
        Self::PresetModelCannotBeDeleted,
        Self::SelectedModelCannotBeDeleted,
        Self::ModelNotInstalled,
        Self::ModelDeleteFailed,
        Self::DiagnosticsExportFailed,
        Self::StartupItemUpdateFailed,
        Self::WindowUnavailable,
        Self::StatePersistFailed,
        Self::ShutdownFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceUnavailable => "service_unavailable",
            Self::SnapshotOutdated => "snapshot_outdated",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::InvalidGamepadAxisSettings => "invalid_gamepad_axis_settings",
            Self::InvalidShortcutBindings => "invalid_shortcut_bindings",
            Self::ConfigPersistFailed => "config_persist_failed",
            Self::ConfigPermissionDenied => "config_permission_denied",
            Self::ConfigStorageFull => "config_storage_full",
            Self::ConfigTargetOccupied => "config_target_occupied",
            Self::BackupLocationOpenFailed => "backup_location_open_failed",
            Self::ConfigurationRecoveryRequired => "configuration_recovery_required",
            Self::ConfigurationRecoveryFailed => "configuration_recovery_failed",
            Self::ModelUnavailable => "model_unavailable",
            Self::ModelSwitchFailed => "model_switch_failed",
            Self::InvalidModelId => "invalid_model_id",
            Self::ModelAlreadyInstalled => "model_already_installed",
            Self::ModelImportInvalidPackage => "model_import_invalid_package",
            Self::ModelImportSourceInvalid => "model_import_source_invalid",
            Self::ModelImportSourceChanged => "model_import_source_changed",
            Self::ModelImportSourceUnsupported => "model_import_source_unsupported",
            Self::ModelImportCancelled => "model_import_cancelled",
            Self::ModelStoreBusy => "model_store_busy",
            Self::ModelImportFailed => "model_import_failed",
            Self::PresetModelCannotBeDeleted => "preset_model_cannot_be_deleted",
            Self::SelectedModelCannotBeDeleted => "selected_model_cannot_be_deleted",
            Self::ModelNotInstalled => "model_not_installed",
            Self::ModelDeleteFailed => "model_delete_failed",
            Self::DiagnosticsExportFailed => "diagnostics_export_failed",
            Self::StartupItemUpdateFailed => "startup_item_update_failed",
            Self::WindowUnavailable => "window_unavailable",
            Self::StatePersistFailed => "state_persist_failed",
            Self::ShutdownFailed => "shutdown_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsError {
    code: SettingsErrorCode,
}

impl SettingsError {
    pub const fn new(code: SettingsErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> SettingsErrorCode {
        self.code
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.code {
            SettingsErrorCode::ServiceUnavailable => "settings service is unavailable",
            SettingsErrorCode::SnapshotOutdated => {
                "settings changed in the background; review the latest values and retry"
            }
            SettingsErrorCode::RuntimeUnavailable => "runtime did not apply the setting",
            SettingsErrorCode::InvalidGamepadAxisSettings => {
                "gamepad dead-zone settings are out of range"
            }
            SettingsErrorCode::InvalidShortcutBindings => {
                "shortcut bindings are invalid or conflict"
            }
            SettingsErrorCode::ConfigPersistFailed => "setting could not be saved",
            SettingsErrorCode::ConfigPermissionDenied => {
                "configuration storage is not writable; check permissions and retry"
            }
            SettingsErrorCode::ConfigStorageFull => {
                "configuration storage is full; free space and retry"
            }
            SettingsErrorCode::ConfigTargetOccupied => {
                "configuration storage is blocked; remove the blocking item and retry"
            }
            SettingsErrorCode::BackupLocationOpenFailed => {
                "configuration backup folder could not be opened"
            }
            SettingsErrorCode::ConfigurationRecoveryRequired => {
                "configuration must be recovered before this action"
            }
            SettingsErrorCode::ConfigurationRecoveryFailed => {
                "default configuration could not be restored"
            }
            SettingsErrorCode::ModelUnavailable => "selected model is unavailable",
            SettingsErrorCode::ModelSwitchFailed => "selected model could not be activated",
            SettingsErrorCode::InvalidModelId => "model id is invalid",
            SettingsErrorCode::ModelAlreadyInstalled => "model id is already installed",
            SettingsErrorCode::ModelImportInvalidPackage => "model package is invalid",
            SettingsErrorCode::ModelImportSourceInvalid => "model source cannot be imported",
            SettingsErrorCode::ModelImportSourceChanged => "model source changed during import",
            SettingsErrorCode::ModelImportSourceUnsupported => {
                "model source contains an unsupported entry"
            }
            SettingsErrorCode::ModelImportCancelled => "model import was cancelled",
            SettingsErrorCode::ModelStoreBusy => "model storage is busy",
            SettingsErrorCode::ModelImportFailed => "model could not be imported",
            SettingsErrorCode::PresetModelCannotBeDeleted => "preset model cannot be deleted",
            SettingsErrorCode::SelectedModelCannotBeDeleted => {
                "selected model must be replaced before deletion"
            }
            SettingsErrorCode::ModelNotInstalled => "installed model was not found",
            SettingsErrorCode::ModelDeleteFailed => "installed model could not be deleted",
            SettingsErrorCode::DiagnosticsExportFailed => "diagnostics could not be exported",
            SettingsErrorCode::StartupItemUpdateFailed => "startup setting could not be updated",
            SettingsErrorCode::WindowUnavailable => "settings window could not be hidden",
            SettingsErrorCode::StatePersistFailed => "window layout could not be saved",
            SettingsErrorCode::ShutdownFailed => "application shutdown did not complete",
        })
    }
}

impl std::error::Error for SettingsError {}

pub struct SettingsReply<T>(Sender<T>);

impl<T> SettingsReply<T> {
    pub fn respond(self, value: T) -> Result<(), SettingsServiceClosed> {
        self.0
            .send_blocking(value)
            .map_err(|_| SettingsServiceClosed)
    }
}

pub enum SettingsCommand {
    SettingsWindowPlacementChanged,
    OverlayWindowPlacementChanged {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    ReadSnapshot {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetOverlayVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetOverlaySettings {
        expected_config_revision: u64,
        settings: SettingsOverlay,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetMotionAudioEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetModelSettings {
        expected_config_revision: u64,
        settings: SettingsModelSettings,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetGamepadAxisSettings {
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetShortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    RestoreDefaultShortcuts {
        expected_config_revision: u64,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    TriggerApplicationShortcut {
        command: SettingsApplicationShortcut,
    },
    SetStartupItemEnabled {
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SelectModel {
        expected_config_revision: u64,
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    ImportModel {
        request: SettingsModelImportRequest,
        operation: SettingsModelImportControl,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    DeleteModel {
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    RestoreDefaultConfiguration {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    OpenConfigBackupLocation {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    ExportDiagnostics {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    Shutdown {
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsApplicationShortcut {
    ToggleOverlay,
    ToggleMirror,
    ToggleClickThrough,
    ToggleAlwaysOnTop,
    OpenSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsServiceClosed;

impl fmt::Display for SettingsServiceClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("settings command channel is closed")
    }
}

impl std::error::Error for SettingsServiceClosed {}

#[derive(Clone)]
pub struct SettingsClient {
    commands: Sender<SettingsCommand>,
    next_operation_id: Arc<AtomicU64>,
}

pub struct SettingsServiceEndpoint {
    commands: Receiver<SettingsCommand>,
}

type PreparedModelImport = (
    SettingsModelImportOperation,
    SettingsModelImportControl,
    SettingsReply<Result<SettingsSnapshot, SettingsError>>,
);

impl SettingsClient {
    pub fn bounded(capacity: usize) -> (Self, SettingsServiceEndpoint) {
        assert!(capacity > 0, "settings command capacity must be positive");
        let (commands, receiver) = async_channel::bounded(capacity);
        (
            Self {
                commands,
                next_operation_id: Arc::new(AtomicU64::new(1)),
            },
            SettingsServiceEndpoint { commands: receiver },
        )
    }

    pub fn track_window_state(
        &self,
        placement: Option<SettingsWindowPlacement>,
    ) -> SettingsWindowState {
        SettingsWindowState::tracked(placement, self.commands.clone())
    }

    pub fn update_overlay_window_placement(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), SettingsServiceClosed> {
        self.commands
            .try_send(SettingsCommand::OverlayWindowPlacementChanged {
                x,
                y,
                width,
                height,
            })
            .map_err(|_| SettingsServiceClosed)
    }

    pub async fn read_snapshot(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ReadSnapshot { reply })
            .await
    }

    pub async fn set_overlay_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetOverlayVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_motion_audio_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMotionAudioEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    pub async fn set_model_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_gamepad_axis_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetGamepadAxisSettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_shortcuts(
        &self,
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetShortcuts {
            expected_config_revision,
            shortcuts,
            reply,
        })
        .await
    }

    pub async fn restore_default_shortcuts(
        &self,
        expected_config_revision: u64,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::RestoreDefaultShortcuts {
            expected_config_revision,
            reply,
        })
        .await
    }

    pub async fn set_overlay_settings(
        &self,
        expected_config_revision: u64,
        settings: SettingsOverlay,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetOverlaySettings {
            expected_config_revision,
            settings,
            reply,
        })
        .await
    }

    pub async fn set_startup_item_enabled(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetStartupItemEnabled { enabled, reply })
            .await
    }

    pub async fn select_model(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SelectModel {
            expected_config_revision,
            model,
            reply,
        })
        .await
    }

    pub async fn import_model(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.start_model_import(request)
            .await?
            .final_result()
            .await
            .result
    }

    pub async fn start_model_import(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsModelImportOperation, SettingsError> {
        let (operation, control, reply) = self.prepare_model_import()?;
        self.commands
            .send(SettingsCommand::ImportModel {
                request,
                operation: control,
                reply,
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        Ok(operation)
    }

    pub async fn delete_model(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::DeleteModel { model, reply })
            .await
    }

    pub async fn restore_default_configuration(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::RestoreDefaultConfiguration { reply })
            .await
    }

    pub async fn open_config_backup_location(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::OpenConfigBackupLocation { reply })
            .await
    }

    pub async fn export_diagnostics(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ExportDiagnostics { reply })
            .await
    }

    pub async fn shutdown(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::Shutdown { reply })
            .await
    }

    pub fn read_snapshot_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ReadSnapshot { reply })
    }

    pub fn set_overlay_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetOverlayVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_motion_audio_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMotionAudioEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_model_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_gamepad_axis_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetGamepadAxisSettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_shortcuts_blocking(
        &self,
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetShortcuts {
            expected_config_revision,
            shortcuts,
            reply,
        })
    }

    pub fn restore_default_shortcuts_blocking(
        &self,
        expected_config_revision: u64,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::RestoreDefaultShortcuts {
            expected_config_revision,
            reply,
        })
    }

    pub fn enqueue_application_shortcut(
        &self,
        command: SettingsApplicationShortcut,
    ) -> Result<(), SettingsServiceClosed> {
        self.commands
            .send_blocking(SettingsCommand::TriggerApplicationShortcut { command })
            .map_err(|_| SettingsServiceClosed)
    }

    pub fn set_overlay_settings_blocking(
        &self,
        expected_config_revision: u64,
        settings: SettingsOverlay,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetOverlaySettings {
            expected_config_revision,
            settings,
            reply,
        })
    }

    pub fn set_startup_item_enabled_blocking(
        &self,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetStartupItemEnabled { enabled, reply })
    }

    pub fn select_model_blocking(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SelectModel {
            expected_config_revision,
            model,
            reply,
        })
    }

    pub fn import_model_blocking(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.start_model_import_blocking(request)?
            .final_result_blocking()
            .result
    }

    pub fn start_model_import_blocking(
        &self,
        request: SettingsModelImportRequest,
    ) -> Result<SettingsModelImportOperation, SettingsError> {
        let (operation, control, reply) = self.prepare_model_import()?;
        self.commands
            .send_blocking(SettingsCommand::ImportModel {
                request,
                operation: control,
                reply,
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        Ok(operation)
    }

    pub fn delete_model_blocking(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::DeleteModel { model, reply })
    }

    pub fn restore_default_configuration_blocking(
        &self,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::RestoreDefaultConfiguration { reply })
    }

    pub fn open_config_backup_location_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::OpenConfigBackupLocation { reply })
    }

    pub fn export_diagnostics_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ExportDiagnostics { reply })
    }

    pub fn shutdown_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::Shutdown { reply })
    }

    async fn request(
        &self,
        command: impl FnOnce(SettingsReply<Result<SettingsSnapshot, SettingsError>>) -> SettingsCommand,
    ) -> Result<SettingsSnapshot, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(command(SettingsReply(reply)))
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    fn request_blocking(
        &self,
        command: impl FnOnce(SettingsReply<Result<SettingsSnapshot, SettingsError>>) -> SettingsCommand,
    ) -> Result<SettingsSnapshot, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(command(SettingsReply(reply)))
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
    }

    fn prepare_model_import(&self) -> Result<PreparedModelImport, SettingsError> {
        let operation_id = self
            .next_operation_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map(SettingsOperationId)
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        let control = SettingsModelImportControl {
            operation_id,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(SettingsModelImportProgress {
                stage: SettingsModelImportStage::Preparing,
                files_copied: 0,
                bytes_copied: 0,
            })),
        };
        let (reply, result) = async_channel::bounded(1);
        Ok((
            SettingsModelImportOperation {
                control: control.clone(),
                result,
            },
            control,
            SettingsReply(reply),
        ))
    }
}

impl SettingsServiceEndpoint {
    pub fn recv_blocking(&self) -> Result<SettingsCommand, SettingsServiceClosed> {
        self.commands
            .recv_blocking()
            .map_err(|_| SettingsServiceClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn settings_error_codes_are_stable_and_unique() {
        let mut codes = SettingsErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| !code.is_empty()));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), SettingsErrorCode::ALL.len());
        assert_eq!(
            SettingsErrorCode::SnapshotOutdated.as_str(),
            "snapshot_outdated"
        );
        assert_eq!(
            SettingsError::new(SettingsErrorCode::ModelSwitchFailed).code(),
            SettingsErrorCode::ModelSwitchFailed
        );
    }

    #[test]
    fn commands_are_bounded_ordered_and_receive_typed_replies() {
        let (client, endpoint) = SettingsClient::bounded(2);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetOverlayVisible {
                expected_config_revision,
                visible,
                reply,
            } = endpoint.recv_blocking().expect("first command")
            else {
                panic!("unexpected first command");
            };
            assert_eq!(expected_config_revision, 1);
            assert!(!visible);
            reply
                .respond(Ok(snapshot(2, false, true)))
                .expect("first reply");

            let SettingsCommand::SetMotionAudioEnabled {
                expected_config_revision,
                enabled,
                reply,
            } = endpoint.recv_blocking().expect("second command")
            else {
                panic!("unexpected second command");
            };
            assert_eq!(expected_config_revision, 2);
            assert!(!enabled);
            reply
                .respond(Ok(snapshot(3, false, false)))
                .expect("second reply");
        });

        let first = client.set_overlay_visible_blocking(1, false);
        let second = client.set_motion_audio_enabled_blocking(2, false);
        assert_eq!(first.expect("first snapshot").revision, 2);
        assert_eq!(second.expect("second snapshot").revision, 3);
        worker.join().expect("worker join");
    }

    #[test]
    fn gamepad_axis_settings_command_preserves_typed_values() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetGamepadAxisSettings {
                expected_config_revision,
                settings,
                reply,
            } = endpoint.recv_blocking().expect("gamepad command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(
                settings,
                SettingsGamepadAxisSettings {
                    stick_dead_zone_percent: 25,
                    trigger_dead_zone_percent: 10,
                }
            );
            let mut result = snapshot(8, true, true);
            result.gamepad_axis_settings = settings;
            reply.respond(Ok(result)).expect("gamepad reply");
        });
        let result = client
            .set_gamepad_axis_settings_blocking(
                7,
                SettingsGamepadAxisSettings {
                    stick_dead_zone_percent: 25,
                    trigger_dead_zone_percent: 10,
                },
            )
            .expect("gamepad snapshot");
        assert_eq!(result.gamepad_axis_settings.stick_dead_zone_percent, 25);
        worker.join().expect("worker join");
    }

    #[test]
    fn shortcut_command_preserves_typed_bindings() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let shortcuts = SettingsShortcuts {
            commands: vec![SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+Alt+B".to_owned(),
            }],
            model_behaviors: vec![SettingsModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: "motion:TapBody:0".to_owned(),
                shortcut: "Control+Alt+M".to_owned(),
            }],
        };
        let expected = shortcuts.clone();
        let worker = thread::spawn(move || {
            let SettingsCommand::SetShortcuts {
                expected_config_revision,
                shortcuts,
                reply,
            } = endpoint.recv_blocking().expect("shortcut command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(shortcuts, expected);
            let mut result = snapshot(8, true, true);
            result.shortcuts = shortcuts;
            reply.respond(Ok(result)).expect("shortcut reply");
        });

        let result = client
            .set_shortcuts_blocking(7, shortcuts)
            .expect("shortcut snapshot");
        assert_eq!(result.shortcuts.commands.len(), 1);
        assert_eq!(result.shortcuts.model_behaviors.len(), 1);
        worker.join().expect("worker join");
    }

    #[test]
    fn application_shortcut_handoff_is_typed_and_fire_and_forget() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::TriggerApplicationShortcut { command } = endpoint
                .recv_blocking()
                .expect("application shortcut command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(command, SettingsApplicationShortcut::ToggleOverlay);
        });
        client
            .enqueue_application_shortcut(SettingsApplicationShortcut::ToggleOverlay)
            .expect("queue application shortcut");
        worker.join().expect("worker join");
    }

    #[test]
    fn restore_default_shortcuts_command_preserves_expected_revision() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::RestoreDefaultShortcuts {
                expected_config_revision,
                reply,
            } = endpoint.recv_blocking().expect("restore shortcut command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 9);
            let mut result = snapshot(10, true, true);
            result.shortcuts = SettingsShortcuts::default();
            reply.respond(Ok(result)).expect("restore shortcut reply");
        });

        let result = client
            .restore_default_shortcuts_blocking(9)
            .expect("restore shortcut snapshot");
        assert_eq!(result.shortcuts, SettingsShortcuts::default());
        worker.join().expect("worker join");
    }

    #[test]
    fn gamepad_axis_settings_default_matches_native_config() {
        assert_eq!(
            SettingsGamepadAxisSettings::default(),
            SettingsGamepadAxisSettings {
                stick_dead_zone_percent: 15,
                trigger_dead_zone_percent: 0,
            }
        );
    }

    #[test]
    fn a_closed_service_returns_a_stable_error() {
        let (client, endpoint) = SettingsClient::bounded(1);
        drop(endpoint);
        let result = client.read_snapshot_blocking();
        assert_eq!(
            result.expect_err("closed service").code(),
            SettingsErrorCode::ServiceUnavailable
        );
    }

    #[test]
    fn config_write_errors_are_actionable_and_anonymous() {
        for (code, expected) in [
            (
                SettingsErrorCode::SnapshotOutdated,
                "settings changed in the background; review the latest values and retry",
            ),
            (
                SettingsErrorCode::ConfigPermissionDenied,
                "configuration storage is not writable; check permissions and retry",
            ),
            (
                SettingsErrorCode::ConfigStorageFull,
                "configuration storage is full; free space and retry",
            ),
            (
                SettingsErrorCode::ConfigTargetOccupied,
                "configuration storage is blocked; remove the blocking item and retry",
            ),
        ] {
            let message = SettingsError::new(code).to_string();
            assert_eq!(message, expected);
            assert!(!message.contains('/') && !message.contains('\\'));
        }
    }

    #[test]
    fn startup_item_command_preserves_the_requested_state() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetStartupItemEnabled { enabled, reply } =
                endpoint.recv_blocking().expect("startup item command")
            else {
                panic!("unexpected command");
            };
            assert!(enabled);
            let mut updated = snapshot(2, true, true);
            updated.startup_item =
                SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled);
            reply.respond(Ok(updated)).expect("startup item reply");
        });

        let updated = client
            .set_startup_item_enabled_blocking(true)
            .expect("startup item snapshot");
        assert_eq!(
            updated.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)
        );
        worker.join().expect("worker join");
    }

    #[test]
    fn only_unsupported_startup_states_reject_mutation() {
        let actionable = [
            SettingsStartupItemState::Disabled,
            SettingsStartupItemState::Enabled,
            SettingsStartupItemState::Stale,
            SettingsStartupItemState::RequiresApproval,
            SettingsStartupItemState::NotFound,
        ];
        assert!(actionable.into_iter().all(|state| state.can_set_enabled()));
        assert!(
            !SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment
            )
            .can_set_enabled()
        );
    }

    #[test]
    fn model_import_command_preserves_the_typed_request() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let expected = SettingsModelImportRequest {
            id: "custom-model".to_owned(),
            source_root: PathBuf::from("selected/model"),
        };
        let worker = thread::spawn({
            let expected = expected.clone();
            move || {
                let SettingsCommand::ImportModel {
                    request,
                    operation,
                    reply,
                } = endpoint.recv_blocking().expect("import command")
                else {
                    panic!("unexpected command");
                };
                assert_eq!(request, expected);
                assert_eq!(operation.operation_id().get(), 1);
                reply
                    .respond(Ok(snapshot(2, true, true)))
                    .expect("import reply");
            }
        });

        let imported = client
            .import_model_blocking(expected)
            .expect("import snapshot");
        assert_eq!(imported.revision, 2);
        worker.join().expect("worker join");
    }

    #[test]
    fn import_operations_share_monotonic_ids_progress_and_cancellation() {
        let (client, _endpoint) = SettingsClient::bounded(2);
        let clone = client.clone();
        let (first, first_control, _) = client.prepare_model_import().expect("first operation");
        let (second, _, _) = clone.prepare_model_import().expect("second operation");

        assert_eq!(first.operation_id().get(), 1);
        assert_eq!(second.operation_id().get(), 2);
        assert_eq!(
            first.progress(),
            SettingsModelImportProgress {
                stage: SettingsModelImportStage::Preparing,
                files_copied: 0,
                bytes_copied: 0,
            }
        );
        assert!(first_control.report_progress(SettingsModelImportProgress {
            stage: SettingsModelImportStage::Copying,
            files_copied: 2,
            bytes_copied: 4_096,
        }));
        assert!(!first_control.report_progress(SettingsModelImportProgress {
            stage: SettingsModelImportStage::Preparing,
            files_copied: 1,
            bytes_copied: 128,
        }));
        assert_eq!(first.progress().files_copied, 2);
        assert_eq!(first.progress().bytes_copied, 4_096);
        let monitor = first.monitor();
        assert_eq!(monitor.operation_id(), first.operation_id());
        assert!(monitor.cancel());
        assert!(!first.cancel());
        assert!(monitor.is_cancelled());
        assert!(first_control.is_cancelled());
    }

    #[test]
    fn import_operation_returns_a_typed_final_result() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::ImportModel {
                operation, reply, ..
            } = endpoint.recv_blocking().expect("import command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(operation.operation_id().get(), 1);
            reply
                .respond(Ok(snapshot(7, true, true)))
                .expect("import reply");
        });

        let operation = client
            .start_model_import_blocking(SettingsModelImportRequest {
                id: "custom-model".to_owned(),
                source_root: PathBuf::from("selected/model"),
            })
            .expect("start import");
        let operation_id = operation.operation_id();
        let final_result = operation.final_result_blocking();
        assert_eq!(final_result.operation_id, operation_id);
        assert_eq!(final_result.result.expect("final snapshot").revision, 7);
        worker.join().expect("worker join");
    }

    #[test]
    fn model_delete_command_preserves_source_identity() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let expected = SettingsModelKey {
            id: "custom-model".to_owned(),
            origin: SettingsModelOrigin::Installed,
        };
        let worker = thread::spawn({
            let expected = expected.clone();
            move || {
                let SettingsCommand::DeleteModel { model, reply } =
                    endpoint.recv_blocking().expect("delete command")
                else {
                    panic!("unexpected command");
                };
                assert_eq!(model, expected);
                reply
                    .respond(Ok(snapshot(3, true, true)))
                    .expect("delete reply");
            }
        });

        let deleted = client
            .delete_model_blocking(expected)
            .expect("delete snapshot");
        assert_eq!(deleted.revision, 3);
        worker.join().expect("worker join");
    }

    #[test]
    fn default_configuration_recovery_is_a_typed_command() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::RestoreDefaultConfiguration { reply } =
                endpoint.recv_blocking().expect("recovery command")
            else {
                panic!("unexpected command");
            };
            let mut recovered = snapshot(2, false, false);
            recovered.configuration_status =
                SettingsConfigurationStatus::DefaultsRestoredRestartRequired;
            reply.respond(Ok(recovered)).expect("recovery reply");
        });

        let recovered = client
            .restore_default_configuration_blocking()
            .expect("recovery snapshot");
        assert_eq!(
            recovered.configuration_status,
            SettingsConfigurationStatus::DefaultsRestoredRestartRequired
        );
        worker.join().expect("worker join");
    }

    #[test]
    fn configuration_backup_location_is_a_typed_command() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::OpenConfigBackupLocation { reply } =
                endpoint.recv_blocking().expect("backup location command")
            else {
                panic!("unexpected command");
            };
            reply
                .respond(Ok(snapshot(11, true, false)))
                .expect("backup location reply");
        });

        let unchanged = client
            .open_config_backup_location_blocking()
            .expect("backup location snapshot");
        assert_eq!(unchanged.revision, 11);
        assert!(unchanged.overlay_visible);
        assert!(!unchanged.motion_audio_enabled);
        worker.join().expect("worker join");
    }

    #[test]
    fn diagnostics_export_is_a_typed_command() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::ExportDiagnostics { reply } = endpoint
                .recv_blocking()
                .expect("diagnostics export command")
            else {
                panic!("unexpected command");
            };
            let mut exported = snapshot(12, true, true);
            exported.diagnostics_export = Some(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 512,
            });
            reply.respond(Ok(exported)).expect("export reply");
        });

        let exported = client
            .export_diagnostics_blocking()
            .expect("diagnostics export snapshot");
        assert_eq!(
            exported.diagnostics_export,
            Some(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 512,
            })
        );
        worker.join().expect("worker join");
    }

    #[test]
    fn settings_window_state_is_validated_and_shared_across_clones() {
        assert!(SettingsWindowPlacement::new(0, 0, 639, 600, false).is_none());
        assert!(SettingsWindowPlacement::new(1_000_001, 0, 800, 600, false).is_none());

        let initial = SettingsWindowPlacement::new(-120, 80, 800, 600, false)
            .expect("valid initial placement");
        let updated = SettingsWindowPlacement::new(240, 160, 1024, 768, true)
            .expect("valid updated placement");
        let state = SettingsWindowState::new(Some(initial));
        let cloned = state.clone();
        cloned.update(updated);
        assert_eq!(state.placement(), Some(updated));
    }

    fn snapshot(
        revision: u64,
        overlay_visible: bool,
        motion_audio_enabled: bool,
    ) -> SettingsSnapshot {
        SettingsSnapshot {
            revision,
            config_revision: Some(revision),
            runtime_health: RuntimeHealth::Ready,
            runtime_diagnostics: SettingsRuntimeDiagnostics::default(),
            overlay_visible,
            overlay: SettingsOverlay::default(),
            motion_audio_enabled,
            model_settings: SettingsModelSettings::default(),
            gamepad_axis_settings: SettingsGamepadAxisSettings::default(),
            shortcuts: SettingsShortcuts::default(),
            startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            configuration_status: SettingsConfigurationStatus::Ready,
            config_recovery: None,
            diagnostics_export: None,
            input_diagnostics: SettingsInputDiagnostics::default(),
            active_model: Some(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            }),
            model_catalog: SettingsModelCatalog::default(),
        }
    }
}
