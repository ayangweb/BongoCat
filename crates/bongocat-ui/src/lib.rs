#![forbid(unsafe_code)]

use async_channel::{Receiver, Sender};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

mod pop_confirm;
mod window;
pub use window::{SettingsView, SettingsWindowHandle, SettingsWindowSeed, open_settings_window};

mod update;
pub use update::{
    UPDATE_STATE_POLL_INTERVAL, UpdateClient, UpdateCommand, UpdateErrorCode, UpdateFailureStage,
    UpdatePhase, UpdateProgressInfo, UpdateReleaseInfo, UpdateServiceClosed, UpdateServiceEndpoint,
    UpdateSnapshot, UpdateStateHandle, UpdateUnavailableReason,
};
mod update_markdown;
mod update_window;
pub use update_window::{UpdateView, UpdateWindowHandle, open_update_window};

const MIN_SETTINGS_WINDOW_WIDTH: u32 = 640;
const MIN_SETTINGS_WINDOW_HEIGHT: u32 = 480;
const MAX_SETTINGS_WINDOW_DIMENSION: u32 = 16_384;
const MAX_SETTINGS_WINDOW_COORDINATE: i32 = 1_000_000;
pub const SETTINGS_PATCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// The archive extension a suggested title should not repeat.
///
/// Nothing in the product reads a model archive today — the picker only returns
/// folders and the store only ingests them (ADR-0036 已撤回) — so this rule only
/// reaches a path that is *not* a directory: a folder may legitimately be called
/// `something.zip` and keeps its name, while anything else drops the suffix so
/// that `名字.zip` and the folder it was made from suggest the same title. It
/// stays because that naming rule is shared with the settings service's fallback,
/// and dropping it would leave the two entrances disagreeing once archive import
/// is built again.
const ARCHIVE_EXTENSION: &str = ".zip";

/// Coalesces rapid typed setting updates while retaining values that were not
/// acknowledged by the settings service.
#[derive(Clone, Debug)]
pub struct SettingsPatchDebouncer<T> {
    last_sent_at: Option<Instant>,
    pending: Option<T>,
    debounce: Duration,
}

impl<T> Default for SettingsPatchDebouncer<T> {
    fn default() -> Self {
        Self {
            last_sent_at: None,
            pending: None,
            debounce: SETTINGS_PATCH_DEBOUNCE,
        }
    }
}

impl<T: Clone + PartialEq> SettingsPatchDebouncer<T> {
    pub fn observe(&mut self, value: T, now: Instant) -> Option<T> {
        self.pending = Some(value);
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= self.debounce)
        {
            self.last_sent_at = Some(now);
            self.pending.clone()
        } else {
            None
        }
    }

    pub fn mark_sent(&mut self, value: &T) {
        if self.pending.as_ref() == Some(value) {
            self.pending = None;
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn ready(&self, now: Instant) -> Option<T> {
        self.pending.as_ref().and_then(|pending| {
            self.last_sent_at
                .is_none_or(|last| now.saturating_duration_since(last) >= self.debounce)
                .then(|| pending.clone())
        })
    }

    pub fn flush(&mut self, now: Instant) -> Option<T> {
        if self.pending.is_some() {
            self.last_sent_at = Some(now);
        }
        self.pending.clone()
    }
}

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
    TransportClosed,
    OverlaySettingsInvalid,
    MaximumFpsInvalid,
    ReleaseFallbackTimeoutInvalid,
}

impl SettingsRuntimeErrorCode {
    pub const ALL: [Self; 9] = [
        Self::ModelLoadFailed,
        Self::ModelEvaluationFailed,
        Self::MotionLoadFailed,
        Self::ExpressionLoadFailed,
        Self::GpuPreparationFailed,
        Self::TransportClosed,
        Self::OverlaySettingsInvalid,
        Self::MaximumFpsInvalid,
        Self::ReleaseFallbackTimeoutInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelLoadFailed => "model_load_failed",
            Self::ModelEvaluationFailed => "model_evaluation_failed",
            Self::MotionLoadFailed => "motion_load_failed",
            Self::ExpressionLoadFailed => "expression_load_failed",
            Self::GpuPreparationFailed => "gpu_preparation_failed",
            Self::TransportClosed => "transport_closed",
            Self::OverlaySettingsInvalid => "overlay_settings_invalid",
            Self::MaximumFpsInvalid => "maximum_fps_invalid",
            Self::ReleaseFallbackTimeoutInvalid => "release_fallback_timeout_invalid",
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
    pub work_budget_exceeded: u64,
    pub last_over_budget_ms: u64,
    pub shutdown_timed_out: u64,
    pub shutdown_worker_panicked: u64,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsBuildEnvironment {
    Development,
    Production,
}

impl SettingsBuildEnvironment {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsBuildInfo {
    pub product_version: String,
    pub environment: SettingsBuildEnvironment,
}

/// Version of the anonymous diagnostics export JSON contract.
pub const DIAGNOSTICS_EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsTheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsLanguage {
    #[default]
    System,
    ChineseSimplified,
    EnglishUnitedStates,
}

impl SettingsLanguage {
    pub const ALL: [Self; 3] = [
        Self::System,
        Self::ChineseSimplified,
        Self::EnglishUnitedStates,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ChineseSimplified => "zh-CN",
            Self::EnglishUnitedStates => "en-US",
        }
    }

    /// Returns the locale used by the embedded rust-i18n catalog.
    ///
    /// The platform resolves `system` before it reaches a UI snapshot. Keeping
    /// the English fallback here also makes pure UI presentation deterministic
    /// when a snapshot is not available yet.
    pub(crate) const fn catalog_locale(self) -> &'static str {
        match self {
            Self::ChineseSimplified => "zh-CN",
            Self::System | Self::EnglishUnitedStates => "en-US",
        }
    }

    pub fn display_name(self, display_language: Self) -> &'static str {
        let locale = display_language.catalog_locale();
        match (self, display_language) {
            (Self::System, _) => {
                bongocat_i18n::text(locale, "settings.appearance.language.options.system")
            }
            (Self::ChineseSimplified, _) => bongocat_i18n::text(
                locale,
                "settings.appearance.language.options.chinese_simplified",
            ),
            (Self::EnglishUnitedStates, _) => bongocat_i18n::text(
                locale,
                "settings.appearance.language.options.english_united_states",
            ),
        }
    }

    pub fn from_display_name(name: &str, display_language: Self) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.display_name(display_language) == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub config_revision: Option<u64>,
    pub build_info: SettingsBuildInfo,
    pub runtime_health: RuntimeHealth,
    pub runtime_diagnostics: SettingsRuntimeDiagnostics,
    pub appearance_theme: SettingsTheme,
    pub language: SettingsLanguage,
    pub resolved_language: SettingsLanguage,
    pub status_icon_visible: bool,
    pub taskbar_icon_visible: bool,
    pub check_for_updates_automatically: bool,
    pub overlay_visible: bool,
    pub overlay: SettingsOverlay,
    pub motion_audio_enabled: bool,
    /// Whether the application command bindings are allowed to reach the
    /// platform shortcut table. The shortcuts page renders it as the "disable
    /// window shortcuts" switch above the command rows.
    pub command_shortcuts_enabled: bool,
    pub behavior_shortcuts_enabled: bool,
    pub maximum_fps: u16,
    pub release_fallback_timeout_ms: u32,
    pub model_settings: SettingsModelSettings,
    pub gamepad_axis_settings: SettingsGamepadAxisSettings,
    pub shortcuts: SettingsShortcuts,
    pub startup_item: SettingsStartupItemStatus,
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
    /// Overlay window corner radius as a percentage of the window width and
    /// height, matching the legacy `border-radius: N%` window setting.
    pub corner_radius_percent: u8,
    /// Hide the overlay while the pointer rests on it, matching the legacy
    /// `window.hideOnHover` switch.
    pub hide_on_pointer_hover: bool,
    /// How long the pointer must rest on the overlay before the hover hide
    /// starts, in whole seconds. `0` hides as soon as the pointer enters.
    pub hide_on_pointer_hover_delay_seconds: u32,
    /// Keep the overlay fully on a display. The window is allowed over a
    /// taskbar, Dock or menu bar, and a window dragged off the desktop returns
    /// after the drag ends rather than being pulled back mid-drag.
    pub keep_inside_screen: bool,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self {
            click_through: false,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            corner_radius_percent: 0,
            hide_on_pointer_hover: false,
            hide_on_pointer_hover_delay_seconds: 0,
            keep_inside_screen: true,
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

/// The display name a chosen model source suggests.
///
/// The page pre-fills this name and the settings service falls back to it, so one
/// rule answers both entrances. The source is the folder a user picked, and a
/// folder keeps its full name — because a folder may legitimately be called
/// `something.zip`. A path that is *not* a directory drops the archive extension
/// instead, so the folder a user exports and the `名字.zip` made from it suggest
/// the same title; that branch has no caller in the product today (ADR-0036
/// 已撤回) and is kept so the rule is not lost with the feature.
///
/// The name is display-only: the portable store key stays a service-generated
/// UUID and is never derived from it.
pub fn model_source_display_name(source_root: &Path) -> Option<String> {
    let name = source_root.file_name()?.to_str()?;
    let name = match source_root.is_dir() {
        true => name,
        false => strip_archive_extension(name),
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Drop a trailing `.zip`, whatever its case.
///
/// The suffix is compared and removed through the bounds-checked `str` accessors
/// rather than by slicing at a byte offset, because a name whose last four bytes
/// are only *part* of a multi-byte character must leave the name untouched
/// instead of panicking.
fn strip_archive_extension(name: &str) -> &str {
    let Some(offset) = name.len().checked_sub(ARCHIVE_EXTENSION.len()) else {
        return name;
    };
    match name.get(offset..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(ARCHIVE_EXTENSION) => {
            name.get(..offset).unwrap_or(name)
        }
        _ => name,
    }
}

/// One selectable conversion mode of a BongoCat Mver source.
///
/// Mirrors the model crate's mode set at the UI boundary so the settings page
/// can offer choices without depending on the model crate. A source that
/// supports conversion in any of these carries a matching converted model; the
/// order here is the order a request reports modes in.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SettingsMverMode {
    Standard,
    Keyboard,
    Gamepad,
}

/// What inspecting a user-picked source turned out to be.
///
/// `Package` is a single BongoCat model package: nothing converts and any
/// selection is ignored. `Mver` is a BongoCat Mver source, and `modes` is
/// exactly the conversions it actually carries, in report order — it is the
/// set the UI shows checkboxes for and the set a request can ask for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsModelSourceContent {
    Package,
    Mver { modes: Vec<SettingsMverMode> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelImportRequest {
    /// User-chosen display name for the imported model; the store key is a
    /// service-generated UUID and never derived from this value.
    pub title: String,
    pub source_root: PathBuf,
    /// The legacy modes to convert when the source turns out to be a BongoCat
    /// Mver source, in the caller's own selection order.
    ///
    /// Ignored for package sources. The request only ever names modes the
    /// inspection reported; the store still refuses a mode the source does not
    /// actually carry.
    pub selected_mver_modes: Vec<SettingsMverMode>,
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
    /// User-facing display name. The app layer falls back to the stable id
    /// when no editable title metadata exists (presets and legacy records).
    pub title: String,
    pub origin: SettingsModelOrigin,
    pub availability: SettingsModelAvailability,
    /// The package directory the model's files live in, when it is present.
    /// The settings page opens it, and derives nothing else from it: every
    /// other model fact already has its own field.
    pub directory: Option<PathBuf>,
    /// The cover image the package ships, if it ships one. A package without a
    /// cover is an ordinary package, so the page renders a placeholder instead
    /// of treating the entry as degraded.
    pub cover: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsModelOrigin {
    Preset,
    Installed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsModelAvailability {
    Ready {
        behaviors: Vec<SettingsModelBehavior>,
    },
    Invalid {
        diagnostic: SettingsModelDiagnostic,
    },
}

/// A behavior declared by a validated model package. Settings uses this
/// strongly typed identity for preview and shortcut operations.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelBehavior {
    Motion { group: String, index: usize },
    Expression { name: String },
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
    InvalidShortcutBindings,
    ConfigPersistFailed,
    ConfigPermissionDenied,
    ConfigStorageFull,
    ConfigTargetOccupied,
    BackupLocationOpenFailed,
    ModelUnavailable,
    ModelSwitchFailed,
    ModelBehaviorPreviewUnavailable,
    ModelBehaviorPreviewFailed,
    ModelTitleInvalid,
    ModelCoverInvalid,
    ModelCoverUpdateFailed,
    ModelSourcePickerUnavailable,
    ModelLocationOpenFailed,
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
    ModelNotFound,
    ModelDeleteFailed,
    DiagnosticsExportFailed,
    StartupItemUpdateFailed,
    StatusIconUpdateFailed,
    TaskbarIconUpdateFailed,
    WindowHideFailed,
    StatePersistFailed,
    ShutdownFailed,
}

impl SettingsErrorCode {
    pub const ALL: [Self; 37] = [
        Self::ServiceUnavailable,
        Self::SnapshotOutdated,
        Self::RuntimeUnavailable,
        Self::InvalidShortcutBindings,
        Self::ConfigPersistFailed,
        Self::ConfigPermissionDenied,
        Self::ConfigStorageFull,
        Self::ConfigTargetOccupied,
        Self::BackupLocationOpenFailed,
        Self::ModelUnavailable,
        Self::ModelSwitchFailed,
        Self::ModelBehaviorPreviewUnavailable,
        Self::ModelBehaviorPreviewFailed,
        Self::ModelTitleInvalid,
        Self::ModelCoverInvalid,
        Self::ModelCoverUpdateFailed,
        Self::ModelSourcePickerUnavailable,
        Self::ModelLocationOpenFailed,
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
        Self::ModelNotFound,
        Self::ModelDeleteFailed,
        Self::DiagnosticsExportFailed,
        Self::StartupItemUpdateFailed,
        Self::StatusIconUpdateFailed,
        Self::TaskbarIconUpdateFailed,
        Self::WindowHideFailed,
        Self::StatePersistFailed,
        Self::ShutdownFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServiceUnavailable => "service_unavailable",
            Self::SnapshotOutdated => "snapshot_outdated",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::InvalidShortcutBindings => "invalid_shortcut_bindings",
            Self::ConfigPersistFailed => "config_persist_failed",
            Self::ConfigPermissionDenied => "config_permission_denied",
            Self::ConfigStorageFull => "config_storage_full",
            Self::ConfigTargetOccupied => "config_target_occupied",
            Self::BackupLocationOpenFailed => "backup_location_open_failed",
            Self::ModelUnavailable => "model_unavailable",
            Self::ModelSwitchFailed => "model_switch_failed",
            Self::ModelBehaviorPreviewUnavailable => "model_behavior_preview_unavailable",
            Self::ModelBehaviorPreviewFailed => "model_behavior_preview_failed",
            Self::ModelTitleInvalid => "model_title_invalid",
            Self::ModelCoverInvalid => "model_cover_invalid",
            Self::ModelCoverUpdateFailed => "model_cover_update_failed",
            Self::ModelSourcePickerUnavailable => "model_source_picker_unavailable",
            Self::ModelLocationOpenFailed => "model_location_open_failed",
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
            Self::ModelNotFound => "model_not_found",
            Self::ModelDeleteFailed => "model_delete_failed",
            Self::DiagnosticsExportFailed => "diagnostics_export_failed",
            Self::StartupItemUpdateFailed => "startup_item_update_failed",
            Self::StatusIconUpdateFailed => "status_icon_update_failed",
            Self::TaskbarIconUpdateFailed => "taskbar_icon_update_failed",
            Self::WindowHideFailed => "window_hide_failed",
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
            SettingsErrorCode::ServiceUnavailable => "Settings service is unavailable",
            SettingsErrorCode::SnapshotOutdated => {
                "Settings changed in the background; review the latest settings and retry"
            }
            SettingsErrorCode::RuntimeUnavailable => "The setting did not take effect",
            SettingsErrorCode::InvalidShortcutBindings => {
                "Shortcut bindings are invalid or conflict"
            }
            SettingsErrorCode::ConfigPersistFailed => "Setting could not be saved",
            SettingsErrorCode::ConfigPermissionDenied => {
                "The configuration file cannot be written; check permissions and retry"
            }
            SettingsErrorCode::ConfigStorageFull => {
                "The disk holding the configuration is full; free space and retry"
            }
            SettingsErrorCode::ConfigTargetOccupied => {
                "The configuration location is in use; close the program using it and retry"
            }
            SettingsErrorCode::BackupLocationOpenFailed => {
                "Configuration backup folder could not be opened"
            }
            SettingsErrorCode::ModelUnavailable => "Selected model is unavailable",
            SettingsErrorCode::ModelSwitchFailed => "Selected model could not be activated",
            SettingsErrorCode::ModelBehaviorPreviewUnavailable => {
                "The behavior is not available for the model in use"
            }
            SettingsErrorCode::ModelBehaviorPreviewFailed => "The behavior could not be played",
            SettingsErrorCode::ModelTitleInvalid => "Model name is not usable",
            SettingsErrorCode::ModelCoverInvalid => "Cover image must be a PNG file",
            SettingsErrorCode::ModelCoverUpdateFailed => "Model cover could not be updated",
            SettingsErrorCode::ModelSourcePickerUnavailable => {
                "The file dialog could not be opened"
            }
            SettingsErrorCode::ModelLocationOpenFailed => "The model folder could not be opened",
            SettingsErrorCode::InvalidModelId => "Model id is invalid",
            SettingsErrorCode::ModelAlreadyInstalled => "This model is already installed",
            SettingsErrorCode::ModelImportInvalidPackage => "Model package is invalid",
            SettingsErrorCode::ModelImportSourceInvalid => "The selected folder contains the model library itself; choose a specific model folder instead.",
            SettingsErrorCode::ModelImportSourceChanged => "Model source changed during import",
            SettingsErrorCode::ModelImportSourceUnsupported => {
                "The model source contains an unsupported file"
            }
            SettingsErrorCode::ModelImportCancelled => "Model import was cancelled",
            SettingsErrorCode::ModelStoreBusy => "Models are busy with another operation; try again in a moment",
            SettingsErrorCode::ModelImportFailed => "Model could not be imported",
            SettingsErrorCode::PresetModelCannotBeDeleted => "Preset model cannot be deleted",
            SettingsErrorCode::ModelNotFound => "The model was not found",
            SettingsErrorCode::ModelDeleteFailed => "Installed model could not be deleted",
            SettingsErrorCode::DiagnosticsExportFailed => "Diagnostics could not be exported",
            SettingsErrorCode::StartupItemUpdateFailed => "Startup setting could not be updated",
            SettingsErrorCode::StatusIconUpdateFailed => {
                "The status icon display could not be updated"
            }
            SettingsErrorCode::TaskbarIconUpdateFailed => {
                "The taskbar icon display could not be updated"
            }
            SettingsErrorCode::WindowHideFailed => "Settings window could not be hidden",
            SettingsErrorCode::StatePersistFailed => "Window layout could not be saved",
            SettingsErrorCode::ShutdownFailed => "Application shutdown did not complete",
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
    /// The snapshot revision alone, for pollers that only need to know whether anything
    /// changed.
    ///
    /// A full [`Self::ReadSnapshot`] also scans the model catalog, which is filesystem
    /// work a poller cannot act on. Poll this until it moves, then read the snapshot.
    ReadSnapshotRevision {
        reply: SettingsReply<u64>,
    },
    SetOverlayVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetAppearanceTheme {
        expected_config_revision: u64,
        theme: SettingsTheme,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetLanguage {
        expected_config_revision: u64,
        language: SettingsLanguage,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetStatusIconVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetTaskbarIconVisible {
        expected_config_revision: u64,
        visible: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetCheckForUpdatesAutomatically {
        expected_config_revision: u64,
        enabled: bool,
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
    SetBehaviorShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetCommandShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetMaximumFps {
        expected_config_revision: u64,
        maximum_fps: u16,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetReleaseFallbackTimeout {
        expected_config_revision: u64,
        timeout_ms: u32,
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
    SuspendShortcutCapture {
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    ResumeShortcutCapture {
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
    SetModelTitle {
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Play one declared behavior of the model in use, without persisting
    /// anything.
    ///
    /// The shortcut rows carry this because a recorded chord is only meaningful
    /// against the motion or expression it fires: the row that holds the
    /// binding is the one place a user can hear and see what it does. The
    /// command is deliberately revision-free — it changes no configuration, so
    /// there is nothing for a stale revision to protect.
    PreviewModelBehavior {
        model: SettingsModelKey,
        behavior: SettingsModelBehavior,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    SetModelCover {
        model: SettingsModelKey,
        source: PathBuf,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Install cover bytes the product captured from the model itself.
    ///
    /// A user-chosen cover travels as a path ([`Self::SetModelCover`]) because the
    /// file already exists. A captured cover does not: it is produced in memory by
    /// the renderer, which writes the window it captured from and nothing else. Both
    /// end at the same store replacement, under the same PNG contract.
    ReplaceModelCover {
        model: SettingsModelKey,
        png: Vec<u8>,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    OpenModelLocation {
        model: SettingsModelKey,
        reply: SettingsReply<Result<SettingsSnapshot, SettingsError>>,
    },
    /// Describe a user-picked folder without installing it.
    ///
    /// The settings page calls this before showing a BongoCat Mver conversion
    /// dialog, because only the service can ask the model crate what a source
    /// actually carries.
    InspectModelSource {
        source_root: PathBuf,
        reply: SettingsReply<Result<SettingsModelSourceContent, SettingsError>>,
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

    /// The snapshot revision, without building the snapshot.
    ///
    /// The answer costs the service one comparison against state it already holds, so a
    /// change detector can poll it at a display cadence instead of rebuilding and
    /// comparing whole snapshots.
    pub async fn read_snapshot_revision(&self) -> Result<u64, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(SettingsCommand::ReadSnapshotRevision {
                reply: SettingsReply(reply),
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))
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

    pub async fn set_appearance_theme(
        &self,
        expected_config_revision: u64,
        theme: SettingsTheme,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetAppearanceTheme {
            expected_config_revision,
            theme,
            reply,
        })
        .await
    }

    pub async fn set_language(
        &self,
        expected_config_revision: u64,
        language: SettingsLanguage,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetLanguage {
            expected_config_revision,
            language,
            reply,
        })
        .await
    }

    pub async fn set_status_icon_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetStatusIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_taskbar_icon_visible(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetTaskbarIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
        .await
    }

    pub async fn set_check_for_updates_automatically(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetCheckForUpdatesAutomatically {
            expected_config_revision,
            enabled,
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

    pub async fn set_behavior_shortcuts_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetBehaviorShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    /// Whether the application command bindings reach the platform table.
    ///
    /// The recorded bindings stay in the configuration and the model behaviour
    /// gate is untouched: this switch only decides whether the command half of
    /// the live table is populated.
    pub async fn set_command_shortcuts_enabled(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetCommandShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
        .await
    }

    pub async fn set_maximum_fps(
        &self,
        expected_config_revision: u64,
        maximum_fps: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetMaximumFps {
            expected_config_revision,
            maximum_fps,
            reply,
        })
        .await
    }

    pub async fn set_release_fallback_timeout(
        &self,
        expected_config_revision: u64,
        timeout_ms: u32,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetReleaseFallbackTimeout {
            expected_config_revision,
            timeout_ms,
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

    pub async fn suspend_shortcut_capture(
        &self,
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SuspendShortcutCapture {
            expected_config_revision,
            shortcuts_without_capture_target,
            reply,
        })
        .await
    }

    pub async fn resume_shortcut_capture(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ResumeShortcutCapture { reply })
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

    pub async fn set_model_title(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelTitle {
            expected_config_revision,
            model,
            title,
            reply,
        })
        .await
    }

    pub async fn preview_model_behavior(
        &self,
        model: SettingsModelKey,
        behavior: SettingsModelBehavior,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::PreviewModelBehavior {
            model,
            behavior,
            reply,
        })
        .await
    }

    pub async fn set_model_cover(
        &self,
        model: SettingsModelKey,
        source: PathBuf,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::SetModelCover {
            model,
            source,
            reply,
        })
        .await
    }

    /// Install a cover the product captured itself.
    ///
    /// Called from the thread that owns the overlay windows, right after it rendered
    /// the model into that cover.
    pub async fn replace_model_cover(
        &self,
        model: SettingsModelKey,
        png: Vec<u8>,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::ReplaceModelCover { model, png, reply })
            .await
    }

    pub async fn open_model_location(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request(|reply| SettingsCommand::OpenModelLocation { model, reply })
            .await
    }

    pub async fn inspect_model_source(
        &self,
        source_root: PathBuf,
    ) -> Result<SettingsModelSourceContent, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send(SettingsCommand::InspectModelSource {
                source_root,
                reply: SettingsReply(reply),
            })
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv()
            .await
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
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

    /// The snapshot revision, without building the snapshot.
    pub fn read_snapshot_revision_blocking(&self) -> Result<u64, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(SettingsCommand::ReadSnapshotRevision {
                reply: SettingsReply(reply),
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))
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

    pub fn set_appearance_theme_blocking(
        &self,
        expected_config_revision: u64,
        theme: SettingsTheme,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetAppearanceTheme {
            expected_config_revision,
            theme,
            reply,
        })
    }

    pub fn set_language_blocking(
        &self,
        expected_config_revision: u64,
        language: SettingsLanguage,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetLanguage {
            expected_config_revision,
            language,
            reply,
        })
    }

    pub fn set_status_icon_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetStatusIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_taskbar_icon_visible_blocking(
        &self,
        expected_config_revision: u64,
        visible: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetTaskbarIconVisible {
            expected_config_revision,
            visible,
            reply,
        })
    }

    pub fn set_check_for_updates_automatically_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetCheckForUpdatesAutomatically {
            expected_config_revision,
            enabled,
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

    pub fn set_behavior_shortcuts_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetBehaviorShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_command_shortcuts_enabled_blocking(
        &self,
        expected_config_revision: u64,
        enabled: bool,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetCommandShortcutsEnabled {
            expected_config_revision,
            enabled,
            reply,
        })
    }

    pub fn set_maximum_fps_blocking(
        &self,
        expected_config_revision: u64,
        maximum_fps: u16,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetMaximumFps {
            expected_config_revision,
            maximum_fps,
            reply,
        })
    }

    pub fn set_release_fallback_timeout_blocking(
        &self,
        expected_config_revision: u64,
        timeout_ms: u32,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetReleaseFallbackTimeout {
            expected_config_revision,
            timeout_ms,
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

    pub fn suspend_shortcut_capture_blocking(
        &self,
        expected_config_revision: u64,
        shortcuts_without_capture_target: SettingsShortcuts,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SuspendShortcutCapture {
            expected_config_revision,
            shortcuts_without_capture_target,
            reply,
        })
    }

    pub fn resume_shortcut_capture_blocking(&self) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ResumeShortcutCapture { reply })
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

    pub fn set_model_title_blocking(
        &self,
        expected_config_revision: u64,
        model: SettingsModelKey,
        title: String,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelTitle {
            expected_config_revision,
            model,
            title,
            reply,
        })
    }

    pub fn preview_model_behavior_blocking(
        &self,
        model: SettingsModelKey,
        behavior: SettingsModelBehavior,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::PreviewModelBehavior {
            model,
            behavior,
            reply,
        })
    }

    pub fn set_model_cover_blocking(
        &self,
        model: SettingsModelKey,
        source: PathBuf,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::SetModelCover {
            model,
            source,
            reply,
        })
    }

    pub fn replace_model_cover_blocking(
        &self,
        model: SettingsModelKey,
        png: Vec<u8>,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::ReplaceModelCover { model, png, reply })
    }

    pub fn open_model_location_blocking(
        &self,
        model: SettingsModelKey,
    ) -> Result<SettingsSnapshot, SettingsError> {
        self.request_blocking(|reply| SettingsCommand::OpenModelLocation { model, reply })
    }

    pub fn inspect_model_source_blocking(
        &self,
        source_root: PathBuf,
    ) -> Result<SettingsModelSourceContent, SettingsError> {
        let (reply, receiver) = async_channel::bounded(1);
        self.commands
            .send_blocking(SettingsCommand::InspectModelSource {
                source_root,
                reply: SettingsReply(reply),
            })
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?;
        receiver
            .recv_blocking()
            .map_err(|_| SettingsError::new(SettingsErrorCode::ServiceUnavailable))?
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

    #[cfg(test)]
    fn try_recv(&self) -> Result<SettingsCommand, SettingsServiceClosed> {
        self.commands.try_recv().map_err(|_| SettingsServiceClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn model_source_display_names_are_extension_aware_but_never_panic() {
        assert_eq!(
            model_source_display_name(&PathBuf::from("/models/我的猫 · 标准模式.zip")).as_deref(),
            Some("我的猫 · 标准模式")
        );
        // The extension match is case-insensitive and never assumed to be ASCII
        // adjacent: a name ending in four bytes of a multi-byte character must
        // be returned as it stands instead of being sliced mid-character.
        assert_eq!(
            model_source_display_name(&PathBuf::from("/models/ARCHIVE.ZIP")).as_deref(),
            Some("ARCHIVE")
        );
        assert_eq!(
            model_source_display_name(&PathBuf::from("/models/猫猫猫")).as_deref(),
            Some("猫猫猫")
        );
        assert_eq!(
            model_source_display_name(&PathBuf::from("/models/model.moc3")).as_deref(),
            Some("model.moc3")
        );
        assert_eq!(model_source_display_name(&PathBuf::from("/")), None);
        assert_eq!(
            model_source_display_name(&PathBuf::from("/models/   ")).as_deref(),
            None
        );
    }

    #[test]
    fn a_directory_keeps_its_archive_like_name() {
        let root = std::env::temp_dir().join("bongocat-model-source-name-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("directory named like an archive");
        // A folder may legitimately be called `something.zip`; only a file
        // carries the extension that the suggestion drops.
        assert_eq!(
            model_source_display_name(&root).as_deref(),
            Some("bongocat-model-source-name-test")
        );
        std::fs::write(root.with_extension("zip"), b"PK\x03\x04").expect("archive");
        assert_eq!(
            model_source_display_name(&root.with_extension("zip")).as_deref(),
            Some("bongocat-model-source-name-test")
        );
        std::fs::remove_file(root.with_extension("zip")).expect("remove archive");
        std::fs::remove_dir_all(&root).expect("remove directory");
    }

    #[test]
    fn settings_patch_debouncer_coalesces_and_confirms_latest_value() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();

        assert_eq!(debouncer.observe(10_u16, origin), Some(10));
        debouncer.mark_sent(&10);
        assert_eq!(
            debouncer.observe(20, origin + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            debouncer.observe(30, origin + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            debouncer.observe(30, origin + Duration::from_millis(150)),
            Some(30)
        );
        debouncer.mark_sent(&30);
        assert_eq!(debouncer.flush(origin + Duration::from_millis(200)), None);
    }

    #[test]
    fn settings_patch_debouncer_retains_unconfirmed_value_for_retry_and_flush() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();

        assert_eq!(debouncer.observe("first", origin), Some("first"));
        assert_eq!(
            debouncer.observe("latest", origin + Duration::from_millis(25)),
            None
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(30)),
            Some("latest")
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(31)),
            Some("latest")
        );
        debouncer.mark_sent(&"latest");
        assert_eq!(debouncer.flush(origin + Duration::from_millis(32)), None);
    }

    #[test]
    fn settings_patch_debouncer_waits_for_the_stable_window_before_retry() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();
        assert_eq!(debouncer.observe(10_u16, origin), Some(10));
        assert_eq!(
            debouncer.observe(20, origin + Duration::from_millis(50)),
            None
        );
        assert_eq!(debouncer.ready(origin + Duration::from_millis(149)), None);
        assert_eq!(
            debouncer.ready(origin + Duration::from_millis(150)),
            Some(20)
        );
        assert!(debouncer.is_pending());
    }

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
    fn appearance_theme_command_preserves_typed_selection() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetAppearanceTheme {
                expected_config_revision,
                theme,
                reply,
            } = endpoint.recv_blocking().expect("appearance theme command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(theme, SettingsTheme::Dark);
            let mut result = snapshot(8, true, true);
            result.appearance_theme = theme;
            reply.respond(Ok(result)).expect("appearance theme reply");
        });
        let result = client
            .set_appearance_theme_blocking(7, SettingsTheme::Dark)
            .expect("appearance theme snapshot");
        assert_eq!(result.appearance_theme, SettingsTheme::Dark);
        worker.join().expect("worker join");
    }

    #[test]
    fn language_command_preserves_typed_selection() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetLanguage {
                expected_config_revision,
                language,
                reply,
            } = endpoint.recv_blocking().expect("language command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(language, SettingsLanguage::ChineseSimplified);
            let mut result = snapshot(8, true, true);
            result.language = language;
            reply.respond(Ok(result)).expect("language reply");
        });
        let result = client
            .set_language_blocking(7, SettingsLanguage::ChineseSimplified)
            .expect("language snapshot");
        assert_eq!(result.language, SettingsLanguage::ChineseSimplified);
        worker.join().expect("worker join");
    }

    #[test]
    fn language_preferences_have_stable_codes_and_localized_names() {
        let expected = [
            ("system", "System"),
            ("zh-CN", "简体中文"),
            ("en-US", "English"),
        ];
        for (language, (code, display_name)) in SettingsLanguage::ALL.into_iter().zip(expected) {
            assert_eq!(language.code(), code);
            assert_eq!(
                language.display_name(SettingsLanguage::EnglishUnitedStates),
                display_name
            );
            assert_eq!(
                SettingsLanguage::from_display_name(
                    display_name,
                    SettingsLanguage::EnglishUnitedStates
                ),
                Some(language)
            );
        }
        assert_eq!(
            SettingsLanguage::System.display_name(SettingsLanguage::ChineseSimplified),
            "跟随系统"
        );
        assert_eq!(
            SettingsLanguage::from_display_name("Deutsch", SettingsLanguage::EnglishUnitedStates),
            None
        );
    }

    #[test]
    fn status_icon_command_preserves_typed_visibility() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetStatusIconVisible {
                expected_config_revision,
                visible,
                reply,
            } = endpoint.recv_blocking().expect("status icon command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert!(!visible);
            let mut result = snapshot(8, true, true);
            result.status_icon_visible = visible;
            reply.respond(Ok(result)).expect("status icon reply");
        });
        let result = client
            .set_status_icon_visible_blocking(7, false)
            .expect("status icon snapshot");
        assert!(!result.status_icon_visible);
        worker.join().expect("worker join");
    }

    #[test]
    fn taskbar_icon_command_preserves_typed_visibility() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetTaskbarIconVisible {
                expected_config_revision,
                visible,
                reply,
            } = endpoint.recv_blocking().expect("taskbar icon command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert!(!visible);
            let mut result = snapshot(8, true, true);
            result.taskbar_icon_visible = visible;
            reply.respond(Ok(result)).expect("taskbar icon reply");
        });
        let result = client
            .set_taskbar_icon_visible_blocking(7, false)
            .expect("taskbar icon snapshot");
        assert!(!result.taskbar_icon_visible);
        worker.join().expect("worker join");
    }

    #[test]
    fn automatic_update_check_command_preserves_typed_preference() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetCheckForUpdatesAutomatically {
                expected_config_revision,
                enabled,
                reply,
            } = endpoint.recv_blocking().expect("automatic update command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert!(!enabled);
            let mut result = snapshot(8, true, true);
            result.check_for_updates_automatically = enabled;
            reply.respond(Ok(result)).expect("automatic update reply");
        });
        let result = client
            .set_check_for_updates_automatically_blocking(7, false)
            .expect("automatic update snapshot");
        assert!(!result.check_for_updates_automatically);
        worker.join().expect("worker join");
    }

    #[test]
    fn maximum_fps_command_preserves_typed_value() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetMaximumFps {
                expected_config_revision,
                maximum_fps,
                reply,
            } = endpoint.recv_blocking().expect("maximum FPS command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(maximum_fps, 120);
            let mut result = snapshot(8, true, true);
            result.maximum_fps = maximum_fps;
            reply.respond(Ok(result)).expect("maximum FPS reply");
        });
        let result = client
            .set_maximum_fps_blocking(7, 120)
            .expect("maximum FPS snapshot");
        assert_eq!(result.maximum_fps, 120);
        worker.join().expect("worker join");
    }

    #[test]
    fn release_fallback_timeout_command_preserves_typed_value() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetReleaseFallbackTimeout {
                expected_config_revision,
                timeout_ms,
                reply,
            } = endpoint
                .recv_blocking()
                .expect("release fallback timeout command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert_eq!(timeout_ms, 1_500);
            let mut result = snapshot(8, true, true);
            result.release_fallback_timeout_ms = timeout_ms;
            reply
                .respond(Ok(result))
                .expect("release fallback timeout reply");
        });
        let result = client
            .set_release_fallback_timeout_blocking(7, 1_500)
            .expect("release fallback timeout snapshot");
        assert_eq!(result.release_fallback_timeout_ms, 1_500);
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
                "Settings changed in the background; review the latest settings and retry",
            ),
            (
                SettingsErrorCode::ConfigPermissionDenied,
                "The configuration file cannot be written; check permissions and retry",
            ),
            (
                SettingsErrorCode::ConfigStorageFull,
                "The disk holding the configuration is full; free space and retry",
            ),
            (
                SettingsErrorCode::ConfigTargetOccupied,
                "The configuration location is in use; close the program using it and retry",
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
            title: "custom-model".to_owned(),
            source_root: PathBuf::from("selected/model"),
            selected_mver_modes: vec![SettingsMverMode::Keyboard],
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
    fn inspect_model_source_command_returns_the_typed_content() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::InspectModelSource { source_root, reply } =
                endpoint.recv_blocking().expect("inspect command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(source_root, PathBuf::from("selected/model"));
            reply
                .respond(Ok(SettingsModelSourceContent::Mver {
                    modes: vec![SettingsMverMode::Standard, SettingsMverMode::Gamepad],
                }))
                .expect("inspect reply");
        });

        let content = client
            .inspect_model_source_blocking(PathBuf::from("selected/model"))
            .expect("inspect content");
        assert_eq!(
            content,
            SettingsModelSourceContent::Mver {
                modes: vec![SettingsMverMode::Standard, SettingsMverMode::Gamepad,],
            }
        );
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
                title: "custom-model".to_owned(),
                source_root: PathBuf::from("selected/model"),
                selected_mver_modes: Vec::new(),
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
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 768,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 2,
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
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 768,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 2,
            })
        );
        worker.join().expect("worker join");
    }

    #[test]
    fn behavior_shortcuts_command_preserves_typed_state() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetBehaviorShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } = endpoint
                .recv_blocking()
                .expect("behavior shortcuts command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 7);
            assert!(!enabled);
            let mut result = snapshot(8, true, true);
            result.behavior_shortcuts_enabled = false;
            reply.respond(Ok(result)).expect("behavior shortcuts reply");
        });

        let result = client
            .set_behavior_shortcuts_enabled_blocking(7, false)
            .expect("behavior shortcuts snapshot");
        assert!(!result.behavior_shortcuts_enabled);
        worker.join().expect("worker join");
    }

    #[test]
    fn command_shortcuts_command_preserves_typed_state() {
        let (client, endpoint) = SettingsClient::bounded(1);
        let worker = thread::spawn(move || {
            let SettingsCommand::SetCommandShortcutsEnabled {
                expected_config_revision,
                enabled,
                reply,
            } = endpoint.recv_blocking().expect("command shortcuts command")
            else {
                panic!("unexpected command");
            };
            assert_eq!(expected_config_revision, 9);
            assert!(!enabled);
            let mut result = snapshot(10, true, true);
            result.command_shortcuts_enabled = false;
            reply.respond(Ok(result)).expect("command shortcuts reply");
        });

        let result = client
            .set_command_shortcuts_enabled_blocking(9, false)
            .expect("command shortcuts snapshot");
        assert!(!result.command_shortcuts_enabled);
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

    #[test]
    fn settings_window_state_coalesces_stale_persist_requests() {
        let (client, endpoint) = SettingsClient::bounded(2);
        let initial =
            SettingsWindowPlacement::new(0, 0, 800, 600, false).expect("valid initial placement");
        let first =
            SettingsWindowPlacement::new(10, 20, 800, 600, false).expect("valid first placement");
        let latest =
            SettingsWindowPlacement::new(30, 40, 1024, 768, false).expect("valid latest placement");
        let state = client.track_window_state(Some(initial));
        let first_revision = state.update(first).expect("first placement changed");
        let latest_revision = state.update(latest).expect("latest placement changed");

        assert!(state.request_persist_if_current(first_revision));
        assert!(
            endpoint.try_recv().is_err(),
            "stale timer must not enqueue a write"
        );
        assert!(state.request_persist_if_current(latest_revision));
        assert!(matches!(
            endpoint.try_recv(),
            Ok(SettingsCommand::SettingsWindowPlacementChanged)
        ));
        assert!(
            endpoint.try_recv().is_err(),
            "only the latest timer may enqueue"
        );
    }

    /// A snapshot with the fields a settings test does not care about left at
    /// their defaults. `pub(crate)` so the settings-window tests can seed a view
    /// with one catalog instead of restating all twenty-eight fields.
    pub(crate) fn snapshot(
        revision: u64,
        overlay_visible: bool,
        motion_audio_enabled: bool,
    ) -> SettingsSnapshot {
        SettingsSnapshot {
            revision,
            config_revision: Some(revision),
            build_info: SettingsBuildInfo {
                product_version: env!("CARGO_PKG_VERSION").to_owned(),
                environment: SettingsBuildEnvironment::Development,
            },
            runtime_health: RuntimeHealth::Ready,
            runtime_diagnostics: SettingsRuntimeDiagnostics::default(),
            appearance_theme: SettingsTheme::System,
            language: SettingsLanguage::System,
            resolved_language: SettingsLanguage::EnglishUnitedStates,
            status_icon_visible: true,
            taskbar_icon_visible: true,
            check_for_updates_automatically: true,
            overlay_visible,
            overlay: SettingsOverlay::default(),
            motion_audio_enabled,
            command_shortcuts_enabled: true,
            behavior_shortcuts_enabled: true,
            maximum_fps: 60,
            release_fallback_timeout_ms: 500,
            model_settings: SettingsModelSettings::default(),
            gamepad_axis_settings: SettingsGamepadAxisSettings::default(),
            shortcuts: SettingsShortcuts::default(),
            startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
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
