use crate::{
    RuntimeHealth, SettingsClient, SettingsConfigRecovery, SettingsConfigurationStatus,
    SettingsError, SettingsErrorCode, SettingsGamepadAxisSettings, SettingsInputDiagnostics,
    SettingsInputServiceStatus, SettingsModelAvailability, SettingsModelDiagnostic,
    SettingsModelEntry, SettingsModelImportMonitor, SettingsModelImportOperation,
    SettingsModelImportRequest, SettingsModelImportStage, SettingsModelKey, SettingsModelOrigin,
    SettingsModelSettings, SettingsOperationId, SettingsOverlay, SettingsRuntimeDiagnostics,
    SettingsRuntimeErrorCode, SettingsShortcuts, SettingsSnapshot, SettingsStartupItemState,
    SettingsStartupItemStatus, SettingsStartupItemUnsupportedReason, SettingsWindowPlacement,
    SettingsWindowState,
};
use bongocat_config::ShortcutChord;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityNode, AccessibilityNodeId,
    AccessibilityRole, AccessibilityToggle, AccessibilityTree, SettingsAccessibilityBridge,
};
use bongocat_platform::{DirectoryPickerError, DirectoryPickerOutcome, pick_model_directory};
use gpui_kit::component::{
    ActiveTheme, Disableable, Root, Theme,
    button::Button,
    group_box::{GroupBox, GroupBoxVariant, GroupBoxVariants},
    input::{Input, InputEvent, InputState, NumberInputEvent, StepAction},
    setting::{
        NumberFieldOptions, RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage,
        Settings,
    },
    tag::Tag,
};
use gpui_kit::{
    App, AppContext, Axis, Bounds, Context, DisplayId, Div, Entity, FocusHandle, Hsla,
    KeyDownEvent, Pixels, Render, SharedString, Stateful, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, div, point, prelude::*, px, size,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::HasWindowHandle;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

mod presentation;
use presentation::*;
mod accessibility;
mod diagnostics;
mod lifecycle;
mod model_actions;
mod models;
mod render;
mod settings;
mod shortcuts;
mod smoke;
mod view_state;
pub use lifecycle::open_settings_window;
#[cfg(test)]
mod tests;

const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;
const WINDOW_MIN_WIDTH: f32 = crate::MIN_SETTINGS_WINDOW_WIDTH as f32;
const WINDOW_MIN_HEIGHT: f32 = crate::MIN_SETTINGS_WINDOW_HEIGHT as f32;

#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_ROOT: AccessibilityNodeId = AccessibilityNodeId::new(1);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_GENERAL: AccessibilityNodeId = AccessibilityNodeId::new(2);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_MODELS: AccessibilityNodeId = AccessibilityNodeId::new(3);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_DIAGNOSTICS: AccessibilityNodeId = AccessibilityNodeId::new(4);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY: AccessibilityNodeId = AccessibilityNodeId::new(10);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_AUDIO: AccessibilityNodeId = AccessibilityNodeId::new(11);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_STARTUP: AccessibilityNodeId = AccessibilityNodeId::new(12);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_TOPMOST: AccessibilityNodeId = AccessibilityNodeId::new(13);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_CLICK_THROUGH: AccessibilityNodeId = AccessibilityNodeId::new(14);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_SCALE_DECREASE: AccessibilityNodeId = AccessibilityNodeId::new(15);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_SCALE_INCREASE: AccessibilityNodeId = AccessibilityNodeId::new(16);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_OPACITY_DECREASE: AccessibilityNodeId = AccessibilityNodeId::new(17);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OVERLAY_OPACITY_INCREASE: AccessibilityNodeId = AccessibilityNodeId::new(18);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_OPEN_BACKUPS: AccessibilityNodeId = AccessibilityNodeId::new(28);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_RESTORE_DEFAULTS: AccessibilityNodeId = AccessibilityNodeId::new(29);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_REFRESH: AccessibilityNodeId = AccessibilityNodeId::new(30);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_QUIT: AccessibilityNodeId = AccessibilityNodeId::new(31);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_EXPORT_DIAGNOSTICS: AccessibilityNodeId = AccessibilityNodeId::new(32);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_RESTORE_SHORTCUTS: AccessibilityNodeId = AccessibilityNodeId::new(33);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_CLEAR_SHORTCUTS: AccessibilityNodeId = AccessibilityNodeId::new(34);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_SHORTCUT_CAPTURE_BASE: u64 = 1_000;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_MIRROR: AccessibilityNodeId = AccessibilityNodeId::new(19);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_MIRROR_POINTER: AccessibilityNodeId = AccessibilityNodeId::new(20);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_IGNORE_POINTER: AccessibilityNodeId = AccessibilityNodeId::new(21);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_STICK_DEAD_ZONE: AccessibilityNodeId = AccessibilityNodeId::new(22);
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ACCESSIBILITY_TRIGGER_DEAD_ZONE: AccessibilityNodeId = AccessibilityNodeId::new(23);

#[derive(Clone, Copy)]
struct Tokens {
    canvas: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    accent: Hsla,
    danger: Hsla,
}

impl Tokens {
    fn from_theme(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            canvas: theme.background,
            border: theme.border,
            text: theme.foreground,
            muted: theme.muted_foreground,
            accent: theme.primary,
            danger: theme.danger,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingOperation {
    Refresh,
    OverlayVisibility,
    OverlaySettings,
    MotionAudio,
    ModelSettings,
    GamepadAxisSettings,
    StartupItem,
    ModelSelection,
    ModelDeletion,
    OpenConfigBackupLocation,
    RestoreDefaultConfiguration,
    RestoreDefaultShortcuts,
    ClearShortcuts,
    SetShortcuts,
    ExportDiagnostics,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ShortcutCaptureTarget {
    Command(String),
    ModelBehavior {
        model_id: String,
        behavior_id: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsPage {
    #[default]
    General,
    Models,
    Diagnostics,
}

enum ModelImportState {
    Empty,
    Ready,
    Picking,
    PickerCancelled,
    PickerFailed(DirectoryPickerError),
    Starting { cancel_requested: bool },
    Running(SettingsModelImportMonitor),
    Succeeded,
    Failed(SettingsError),
    Cancelled,
}

struct ModelImportDraft {
    id: String,
    source_root: Option<PathBuf>,
    state: ModelImportState,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModelRowKey {
    origin_rank: u8,
    id: String,
}

impl ModelRowKey {
    fn new(origin: SettingsModelOrigin, id: &str) -> Self {
        Self {
            origin_rank: match origin {
                SettingsModelOrigin::Preset => 0,
                SettingsModelOrigin::Installed => 1,
            },
            id: id.to_owned(),
        }
    }
}

#[derive(Clone)]
struct ModelRowFocus {
    activate: FocusHandle,
    delete: FocusHandle,
    cancel_delete: FocusHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActions {
    active: bool,
    can_activate: bool,
    can_delete: bool,
}

#[derive(Clone, Copy)]
enum ModelRowAction {
    Activate,
    Delete,
    CancelDelete,
}

impl Default for ModelImportDraft {
    fn default() -> Self {
        Self {
            id: String::new(),
            source_root: None,
            state: ModelImportState::Empty,
        }
    }
}

impl ModelImportDraft {
    fn is_running(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. } | ModelImportState::Running(_)
        )
    }

    fn can_import(&self) -> bool {
        self.source_root.is_some()
            && !self.id.is_empty()
            && !self.is_running()
            && !self.is_picker_open()
    }

    fn is_picker_open(&self) -> bool {
        matches!(self.state, ModelImportState::Picking)
    }

    fn running_operation_id(&self) -> Option<SettingsOperationId> {
        match &self.state {
            ModelImportState::Running(monitor) => Some(monitor.operation_id()),
            _ => None,
        }
    }

    fn apply_starting_cancellation(&self, operation: &SettingsModelImportOperation) {
        if matches!(
            self.state,
            ModelImportState::Starting {
                cancel_requested: true
            }
        ) {
            operation.cancel();
        }
    }

    fn reset_result_state(&mut self) {
        self.state = if self.source_root.is_some() {
            ModelImportState::Ready
        } else {
            ModelImportState::Empty
        };
    }
}

pub struct SettingsView {
    client: SettingsClient,
    snapshot: Option<SettingsSnapshot>,
    pending: Option<PendingOperation>,
    error: Option<SettingsError>,
    page: SettingsPage,
    model_import: ModelImportDraft,
    model_delete_confirmation: Option<SettingsModelKey>,
    model_row_focus: BTreeMap<ModelRowKey, ModelRowFocus>,
    shortcut_capture: Option<ShortcutCaptureTarget>,
    shortcut_capture_error: Option<String>,
    shortcut_row_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    window_hidden: bool,
    request_quit: Rc<dyn Fn(&mut App)>,
    general_focus: FocusHandle,
    models_focus: FocusHandle,
    diagnostics_focus: FocusHandle,
    overlay_focus: FocusHandle,
    overlay_topmost_focus: FocusHandle,
    overlay_click_through_focus: FocusHandle,
    overlay_scale_decrease_focus: FocusHandle,
    overlay_scale_increase_focus: FocusHandle,
    overlay_opacity_decrease_focus: FocusHandle,
    overlay_opacity_increase_focus: FocusHandle,
    audio_focus: FocusHandle,
    mirror_focus: FocusHandle,
    mirror_pointer_focus: FocusHandle,
    ignore_pointer_focus: FocusHandle,
    stick_dead_zone_focus: FocusHandle,
    trigger_dead_zone_focus: FocusHandle,
    startup_item_focus: FocusHandle,
    model_id_focus: FocusHandle,
    choose_model_focus: FocusHandle,
    import_model_focus: FocusHandle,
    open_backups_focus: FocusHandle,
    restore_defaults_focus: FocusHandle,
    restore_shortcuts_focus: FocusHandle,
    clear_shortcuts_focus: FocusHandle,
    export_diagnostics_focus: FocusHandle,
    refresh_focus: FocusHandle,
    quit_focus: FocusHandle,
    overlay_scale_input: Entity<InputState>,
    overlay_opacity_input: Entity<InputState>,
    stick_dead_zone_input: Entity<InputState>,
    trigger_dead_zone_input: Entity<InputState>,
    model_id_input: Entity<InputState>,
    syncing_component_inputs: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    accessibility: Option<SettingsAccessibilityBridge>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    accessibility_focus: Option<AccessibilityNodeId>,
}
#[derive(Clone)]
pub struct SettingsWindowHandle {
    window: WindowHandle<Root>,
    view: WeakEntity<SettingsView>,
}

impl SettingsWindowHandle {
    pub fn read(&self, cx: &App) -> gpui_kit::Result<()> {
        self.window.read(cx)?;
        self.view
            .upgrade()
            .map(|_| ())
            .ok_or_else(|| gpui_kit::private::anyhow::anyhow!("settings view was released"))
    }

    pub fn update<C, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut SettingsView, &mut Window, &mut Context<SettingsView>) -> R,
    ) -> gpui_kit::Result<R>
    where
        C: AppContext,
    {
        self.window.update(cx, |_, window, cx| {
            self.view.update(cx, |view, cx| update(view, window, cx))
        })?
    }
}

impl PartialEq for SettingsWindowHandle {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window
    }
}

impl Eq for SettingsWindowHandle {}

impl SettingsView {
    fn start_request(
        &mut self,
        operation: PendingOperation,
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(operation);
        self.error = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = match value {
                None => client.read_snapshot().await,
                Some(SettingValue::OverlayVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_overlay_visible(expected_config_revision, visible)
                        .await
                }
                Some(SettingValue::OverlaySettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::MotionAudioEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_motion_audio_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::ModelSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_model_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::GamepadAxisSettings {
                    expected_config_revision,
                    settings,
                }) => {
                    client
                        .set_gamepad_axis_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::StartupItemEnabled(enabled)) => {
                    client.set_startup_item_enabled(enabled).await
                }
                Some(SettingValue::OpenConfigBackupLocation) => {
                    client.open_config_backup_location().await
                }
                Some(SettingValue::RestoreDefaultConfiguration) => {
                    client.restore_default_configuration().await
                }
                Some(SettingValue::RestoreDefaultShortcuts {
                    expected_config_revision,
                }) => {
                    client
                        .restore_default_shortcuts(expected_config_revision)
                        .await
                }
                Some(SettingValue::Shortcuts {
                    expected_config_revision,
                    shortcuts,
                }) => {
                    client
                        .set_shortcuts(expected_config_revision, shortcuts)
                        .await
                }
                Some(SettingValue::ExportDiagnostics) => client.export_diagnostics().await,
            };
            let refreshed = if result
                .as_ref()
                .is_err_and(|error| error.code() == SettingsErrorCode::SnapshotOutdated)
            {
                client.read_snapshot().await.ok()
            } else {
                None
            };
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                if let Some(snapshot) = refreshed.filter(|snapshot| {
                    view.snapshot
                        .as_ref()
                        .is_none_or(|current| snapshot.revision >= current.revision)
                }) {
                    view.snapshot = Some(snapshot);
                }
                match result {
                    Ok(snapshot)
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision) =>
                    {
                        if snapshot.configuration_status != SettingsConfigurationStatus::Ready {
                            view.page = SettingsPage::Diagnostics;
                        }
                        view.snapshot = Some(snapshot);
                    }
                    Ok(_) => {}
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn sanitize_model_id_input(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        .take(64)
        .map(char::from)
        .collect()
}

#[derive(Clone)]
enum SettingValue {
    OverlayVisible {
        expected_config_revision: u64,
        visible: bool,
    },
    OverlaySettings {
        expected_config_revision: u64,
        settings: SettingsOverlay,
    },
    MotionAudioEnabled {
        expected_config_revision: u64,
        enabled: bool,
    },
    ModelSettings {
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    },
    GamepadAxisSettings {
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    },
    StartupItemEnabled(bool),
    OpenConfigBackupLocation,
    RestoreDefaultConfiguration,
    RestoreDefaultShortcuts {
        expected_config_revision: u64,
    },
    Shortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    },
    ExportDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupItemAction {
    None,
    Retry,
    SetEnabled(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StartupItemPresentation {
    description: &'static str,
    enabled: bool,
    action: StartupItemAction,
}

fn startup_item_presentation(
    status: Option<SettingsStartupItemStatus>,
    blocked: bool,
) -> StartupItemPresentation {
    let mut presentation = match status {
        None => StartupItemPresentation {
            description: "Checking login startup...",
            enabled: false,
            action: StartupItemAction::None,
        },
        Some(SettingsStartupItemStatus::ReadError(_)) => StartupItemPresentation {
            description: "Status unavailable; activate to retry",
            enabled: false,
            action: StartupItemAction::Retry,
        },
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)) => {
            StartupItemPresentation {
                description: "Open BongoCat when you sign in",
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)) => {
            StartupItemPresentation {
                description: "BongoCat opens when you sign in",
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)) => {
            StartupItemPresentation {
                description: "Saved app location changed; enable to repair",
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval)) => {
            StartupItemPresentation {
                description: "Approval required in System Settings",
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound)) => {
            StartupItemPresentation {
                description: "App login item is missing; enable to repair",
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(reason))) => {
            StartupItemPresentation {
                description: match reason {
                    SettingsStartupItemUnsupportedReason::Platform => {
                        "Login startup is unavailable on this platform"
                    }
                    SettingsStartupItemUnsupportedReason::OperatingSystem => {
                        "Login startup requires macOS 13 or later"
                    }
                    SettingsStartupItemUnsupportedReason::BuildEnvironment => {
                        "Login startup is unavailable in development builds"
                    }
                },
                enabled: false,
                action: StartupItemAction::None,
            }
        }
    };
    if blocked {
        presentation.action = StartupItemAction::None;
    }
    presentation
}

fn diagnostic_group(
    title: &'static str,
    metrics: &[(&'static str, u64)],
    tokens: Tokens,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .pb_3()
        .mb_3()
        .child(div().pb_2().text_sm().text_color(tokens.muted).child(title))
        .children(metrics.iter().map(|(label, value)| {
            div()
                .h(px(30.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .text_sm()
                .child(div().min_w_0().flex_1().child(*label))
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.muted)
                        .child(value.to_string()),
                )
        }))
        .child(div().border_b_1().border_color(tokens.border))
}

fn suggested_model_id(source_root: &Path) -> String {
    let name = source_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut suggestion = String::with_capacity(name.len().min(64));
    let mut separator_pending = false;
    for byte in name.bytes() {
        if suggestion.len() >= 64 {
            break;
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            if separator_pending && !suggestion.is_empty() && suggestion.len() < 64 {
                suggestion.push('-');
            }
            separator_pending = false;
            suggestion.push(char::from(byte.to_ascii_lowercase()));
        } else {
            separator_pending = true;
        }
    }
    let trimmed = suggestion
        .trim_matches(|character| matches!(character, '.' | '-' | '_'))
        .to_owned();
    if trimmed.is_empty() {
        return "custom-model".to_owned();
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed.as_str());
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|value| stem.eq_ignore_ascii_case(value))
        || (stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        let maximum_tail = 64 - "model-".len();
        let tail = trimmed[..trimmed.len().min(maximum_tail)].trim_end_matches('.');
        format!("model-{tail}")
    } else {
        trimmed
    }
}

fn model_row_actions(
    entry: &SettingsModelEntry,
    active_model: Option<&SettingsModelKey>,
    commands_blocked: bool,
) -> ModelRowActions {
    let model = SettingsModelKey {
        id: entry.id.clone(),
        origin: entry.origin,
    };
    let active = active_model == Some(&model);
    let ready = matches!(entry.availability, SettingsModelAvailability::Ready { .. });
    ModelRowActions {
        active,
        can_activate: ready && !active && !commands_blocked,
        can_delete: entry.origin == SettingsModelOrigin::Installed && !active && !commands_blocked,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActionTabIndices {
    activate: isize,
    delete: isize,
    cancel_delete: isize,
}

fn model_row_action_tab_indices(
    first_tab_index: isize,
    confirming_delete: bool,
) -> ModelRowActionTabIndices {
    let second = first_tab_index.saturating_add(1);
    let third = first_tab_index.saturating_add(2);
    ModelRowActionTabIndices {
        activate: first_tab_index,
        delete: if confirming_delete { third } else { second },
        cancel_delete: if confirming_delete { second } else { third },
    }
}

fn model_delete_confirmation_is_valid(
    entries: &[SettingsModelEntry],
    active_model: Option<&SettingsModelKey>,
    model: &SettingsModelKey,
) -> bool {
    model.origin == SettingsModelOrigin::Installed
        && active_model != Some(model)
        && entries
            .iter()
            .any(|entry| entry.origin == model.origin && entry.id == model.id)
}

fn model_availability_status(entry: &SettingsModelEntry, active: bool) -> SharedString {
    let origin = match entry.origin {
        SettingsModelOrigin::Preset => "Preset",
        SettingsModelOrigin::Installed => "Installed",
    };
    let active = if active { " · Active" } else { "" };
    match entry.availability {
        SettingsModelAvailability::Ready {
            texture_count,
            expression_count,
            motion_count,
        } => format!(
            "{origin}{active} · {texture_count} textures · {expression_count} expressions · {motion_count} motions"
        )
        .into(),
        SettingsModelAvailability::Invalid { diagnostic } => {
            let diagnostic = match diagnostic {
                SettingsModelDiagnostic::InvalidModelId
                | SettingsModelDiagnostic::ModelEntryAmbiguous
                | SettingsModelDiagnostic::ModelEntryMissing
                | SettingsModelDiagnostic::ModelReferenceEscapesRoot
                | SettingsModelDiagnostic::ModelReferenceInvalid
                | SettingsModelDiagnostic::ModelReferenceSymlinkEscape
                | SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
                    "Package layout is invalid"
                }
                SettingsModelDiagnostic::ModelFileCountExceeded
                | SettingsModelDiagnostic::ModelFileTooLarge
                | SettingsModelDiagnostic::ModelJsonTooLarge
                | SettingsModelDiagnostic::ModelPackageDepthExceeded
                | SettingsModelDiagnostic::ModelPackageSizeExceeded
                | SettingsModelDiagnostic::ModelTextureDimensionExceeded => {
                    "Package exceeds safety limits"
                }
                SettingsModelDiagnostic::ModelJsonInvalid
                | SettingsModelDiagnostic::ModelUnsupportedVersion => {
                    "Model definition is unsupported"
                }
                SettingsModelDiagnostic::ModelTextureInvalidPng
                | SettingsModelDiagnostic::ModelTextureMissing => "Texture is invalid",
                SettingsModelDiagnostic::ModelIoError => "Model files are unavailable",
                SettingsModelDiagnostic::ModelMocMissing
                | SettingsModelDiagnostic::ModelResourceInvalid
                | SettingsModelDiagnostic::ModelResourceMissing
                | SettingsModelDiagnostic::ModelResourceNotFile => "Model resource is invalid",
            };
            format!("{origin} · {diagnostic}").into()
        }
    }
}

fn model_import_status(draft: &ModelImportDraft) -> (SharedString, bool) {
    match &draft.state {
        ModelImportState::Empty => ("No folder selected".into(), false),
        ModelImportState::Ready => ("Folder selected".into(), false),
        ModelImportState::Picking => ("Choosing folder...".into(), false),
        ModelImportState::PickerCancelled if draft.source_root.is_some() => (
            "Selection cancelled; previous folder retained".into(),
            false,
        ),
        ModelImportState::PickerCancelled => ("Selection cancelled".into(), false),
        ModelImportState::PickerFailed(error) => {
            let message = match error {
                DirectoryPickerError::WrongThread => "Folder picker requires the UI thread",
                DirectoryPickerError::SelectionInvalid => "Selected folder is unavailable",
                DirectoryPickerError::UnsupportedPlatform
                | DirectoryPickerError::BackendUnavailable
                | DirectoryPickerError::SelectionUnavailable => "Folder picker is unavailable",
            };
            (message.into(), true)
        }
        ModelImportState::Starting {
            cancel_requested: true,
        } => ("Cancelling import...".into(), false),
        ModelImportState::Starting {
            cancel_requested: false,
        } => ("Starting import...".into(), false),
        ModelImportState::Running(monitor) if monitor.is_cancelled() => {
            ("Cancelling import...".into(), false)
        }
        ModelImportState::Running(monitor) => {
            let progress = monitor.progress();
            let stage = match progress.stage {
                SettingsModelImportStage::Preparing => "Preparing",
                SettingsModelImportStage::Copying => "Copying",
                SettingsModelImportStage::Validating => "Validating",
                SettingsModelImportStage::Committing => "Committing",
            };
            (
                format!(
                    "{stage} · {} files · {} bytes",
                    progress.files_copied, progress.bytes_copied
                )
                .into(),
                false,
            )
        }
        ModelImportState::Succeeded => ("Import complete".into(), false),
        ModelImportState::Failed(error) => (error.to_string().into(), true),
        ModelImportState::Cancelled => ("Import cancelled".into(), false),
    }
}

fn install_component_theme(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}

fn stepped_overlay_scale(mut settings: SettingsOverlay, delta: i16) -> SettingsOverlay {
    let next = i32::from(settings.scale_percent) + i32::from(delta);
    settings.scale_percent = next.clamp(25, 400) as u16;
    settings
}

fn stepped_overlay_opacity(mut settings: SettingsOverlay, delta: i16) -> SettingsOverlay {
    let next = i16::from(settings.opacity_percent) + delta;
    settings.opacity_percent = next.clamp(1, 100) as u8;
    settings
}

fn command_button(
    label: &'static str,
    focus: &FocusHandle,
    tab_index: isize,
    _window: &Window,
    _tokens: Tokens,
    disabled: bool,
) -> Div {
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(tab_index)
        .child(Button::new(label).label(label).disabled(disabled))
}
