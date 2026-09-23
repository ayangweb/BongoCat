use crate::{
    RuntimeHealth, SettingsBuildEnvironment, SettingsBuildInfo, SettingsClient, SettingsError,
    SettingsErrorCode, SettingsGamepadAxisSettings, SettingsLanguage, SettingsModelAvailability,
    SettingsModelBehavior, SettingsModelBehaviorBinding, SettingsModelDiagnostic,
    SettingsModelEntry, SettingsModelImportMonitor, SettingsModelImportOperation,
    SettingsModelImportRequest, SettingsModelKey, SettingsModelOrigin, SettingsModelSettings,
    SettingsModelSourceContent, SettingsMverMode, SettingsOperationId, SettingsOverlay,
    SettingsShortcutBinding, SettingsShortcuts, SettingsSnapshot, SettingsStartupItemState,
    SettingsStartupItemStatus, SettingsStartupItemUnsupportedReason, SettingsTheme,
    SettingsWindowPlacement, SettingsWindowState,
};
use bongocat_config::ShortcutChord;
use bongocat_platform::{
    ModelSourcePickerError, ModelSourcePickerOutcome, pick_model_cover, pick_model_folder,
};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Root, Theme, ThemeMode, ThemeStyled,
    WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{Dialog, DialogButtonProps},
    group_box::GroupBoxVariant,
    input::{Input, InputEvent, InputState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    setting::{
        NumberFieldOptions, RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage,
        Settings,
    },
    switch::Switch,
    tag::Tag,
};

use gpui_kit::{
    Anchor, App, AppContext, Axis, Bounds, Context, DisplayId, Div, Entity, FocusHandle, Focusable,
    Hsla, ImageSource, KeyDownEvent, KeyUpEvent, Modifiers, ObjectFit, Pixels, Render,
    SharedString, Stateful, TitlebarOptions, VisualContext, WeakEntity, Window, WindowAppearance,
    WindowBounds, WindowHandle, WindowOptions, base::StyledExt, div, img, point, prelude::*, px,
    size,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

mod presentation;
use presentation::*;
mod about;
use about::ABOUT_SECTIONS;
mod lifecycle;
mod localization;
mod model_actions;
mod model_import_card;
mod model_mver_dialog;
use model_import_card::ModelImportCard;
use model_mver_dialog::build_mver_mode_dialog;
mod models;
mod render;
mod setting_gate;
use setting_gate::SettingGate;
mod settings;
mod shortcuts;
mod shortcuts_page;
mod smoke;
mod view_state;
use crate::pop_confirm::PopConfirm;
pub use lifecycle::open_settings_window;
use localization::{
    build_info_detail, model_invalid_summary, runtime_status, settings_error,
    shortcut_behavior_name, shortcut_command_name, shortcut_conflict_message,
};
#[cfg(test)]
mod tests;

const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;
const WINDOW_MIN_WIDTH: f32 = crate::MIN_SETTINGS_WINDOW_WIDTH as f32;
const WINDOW_MIN_HEIGHT: f32 = crate::MIN_SETTINGS_WINDOW_HEIGHT as f32;

/// Tab indices of the controls on an editing model card. Only one card can be
/// editing at a time, so they sit above the per-card action range instead of
/// joining its stride.
const MODEL_EDIT_TITLE_TAB_INDEX: isize = 70;
const MODEL_EDIT_COVER_TAB_INDEX: isize = 71;
const MODEL_EDIT_SAVE_TAB_INDEX: isize = 72;
const MODEL_EDIT_CANCEL_TAB_INDEX: isize = 73;

struct SettingsServiceErrorNotification;

struct ShortcutConflictNotification;

/// Marks the notification pushed when the model catalog cannot be read.
///
/// A notification is a prompt, and a catalog that stays unreadable would repeat
/// that prompt on every snapshot, so the view remembers that it already spoke
/// and only the transition back to a readable catalog re-arms it.
struct ModelCatalogErrorNotification;

fn accepts_snapshot_revision(current: Option<u64>, incoming: u64) -> bool {
    current.is_none_or(|current| incoming >= current)
}

/// Element id of the login-startup switch.
///
/// The switch is built by hand rather than through `SettingField::switch`, which
/// names every packaged switch `check`, so this is the only settings switch with
/// an id of its own. That keeps its focus handle and its thumb spring distinct
/// from the other rows' switches.
const STARTUP_ITEM_SWITCH_ID: &str = "open-at-login-switch";

/// A request the settings window forwards to the application rather than acting
/// on itself.
///
/// `None` means the window has no owner for that request, so the matching control
/// is not offered at all.
pub(crate) type SettingsWindowRequest = Rc<dyn Fn(&mut App)>;

type LanguageSelectState = SelectState<SearchableVec<&'static str>>;
type ThemeSelectState = SelectState<SearchableVec<&'static str>>;

#[derive(Clone, Copy)]
pub(crate) struct Tokens {
    pub(crate) canvas: Hsla,
    pub(crate) border: Hsla,
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) accent: Hsla,
    /// The colour of an action that destroys something. Nothing in the page
    /// chrome uses it; it is here for the confirmation surfaces, which are the
    /// only place the page says "this cannot be undone".
    pub(crate) danger: Hsla,
}

impl Tokens {
    pub(crate) fn from_theme(cx: &App) -> Self {
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
    AppearanceTheme,
    Language,
    StatusIconVisibility,
    #[cfg(target_os = "windows")]
    TaskbarIconVisibility,
    AutomaticUpdateCheck,
    OverlayVisibility,
    OverlaySettings,
    OverlayScale,
    OverlayOpacity,
    OverlayCornerRadius,
    OverlayHoverHideDelay,
    MotionAudio,
    CommandShortcuts,
    BehaviorShortcuts,
    MaximumFps,
    ReleaseFallbackTimeout,
    ModelSettings,
    GamepadAxisSettings,
    StartupItem,
    ModelSelection,
    ModelDeletion,
    ModelMetadata,
    ModelLocation,
    SetShortcuts,
    BeginShortcutCapture,
    CancelShortcutCapture,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ShortcutCaptureTarget {
    Command(String),
    ModelBehavior {
        model_id: String,
        behavior_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ShortcutCapture {
    target: ShortcutCaptureTarget,
    modifiers: Modifiers,
    keys: BTreeSet<String>,
}

impl ShortcutCapture {
    fn new(target: ShortcutCaptureTarget) -> Self {
        Self {
            target,
            modifiers: Modifiers::default(),
            keys: BTreeSet::new(),
        }
    }

    fn clear_temporary_input(&mut self) {
        self.modifiers = Modifiers::default();
        self.keys.clear();
    }
}

/// What the single import card is doing.
///
/// Every failure is reported through a notification and then returns the card to
/// [`ModelImportState::Idle`], so no state here holds an error: a state that kept
/// one would be a second copy of a message the user has already read.
enum ModelImportState {
    /// Nothing is running; the card is the clickable upload prompt.
    Idle,
    /// A native source dialog is open.
    Picking,
    /// The chosen folder is being classified as a package or a Mver source.
    Inspecting,
    Starting {
        cancel_requested: bool,
    },
    Running(SettingsModelImportMonitor),
    /// The model is installed and the product is rendering its own cover. The
    /// card shows the capture step — replacing the import line rather than adding
    /// to it — and the cards the run installed stay out of the grid until the
    /// capture finishes.
    Capturing,
}

/// The BongoCat Mver conversion-mode dialog's own state.
///
/// The dialog only exists after inspection reported the modes a source actually
/// carries, so `available` is exactly what the checkboxes show. `checked` is
/// the user's selection and reaches a request in `available` order; the set has
/// no default of the whole list, only the single priority mode
/// [`default_checked_mver_mode`] names.
struct MverModeDialog {
    /// The conversions inspection reported, in the store's report order.
    available: Vec<SettingsMverMode>,
    /// The conversions the user has checked; always a subset of `available`.
    checked: BTreeSet<SettingsMverMode>,
    /// Set when the dialog is built during render; the window's dialog system
    /// owns the surface itself, and this records what the view put into it.
    open: bool,
}

impl MverModeDialog {
    fn from_available(available: Vec<SettingsMverMode>) -> Self {
        let checked = default_checked_mver_mode(&available).into_iter().collect();
        Self {
            available,
            checked,
            open: false,
        }
    }

    /// The checked modes in the order inspection reported them.
    ///
    /// The request is built from this rather than `checked` itself so the
    /// selection reads top to bottom and the checkbox order decides, not
    /// [`SettingsMverMode`]'s declaration order.
    #[cfg(test)]
    fn checked_in_order(&self) -> Vec<SettingsMverMode> {
        self.available
            .iter()
            .copied()
            .filter(|mode| self.checked.contains(mode))
            .collect()
    }

    #[cfg(test)]
    fn can_confirm(&self) -> bool {
        !self.checked.is_empty()
    }

    fn toggle(&mut self, mode: SettingsMverMode, checked: bool) {
        if !self.available.contains(&mode) {
            return;
        }
        if checked {
            self.checked.insert(mode);
        } else {
            self.checked.remove(&mode);
        }
    }
}

/// A render-safe copy of one Mver conversion dialog.
///
/// A dialog builder runs while `SettingsView` is borrowed for rendering, so it
/// must not read the entity back through `App`. This snapshot carries exactly the
/// state the pane needs: the options to draw, the current checks, and whether
/// confirmation is available.
#[derive(Clone)]
pub(super) struct MverDialogSnapshot {
    available: Vec<SettingsMverMode>,
    checked: BTreeSet<SettingsMverMode>,
}

impl MverDialogSnapshot {
    fn from_dialog(dialog: &MverModeDialog) -> Self {
        Self {
            available: dialog.available.clone(),
            checked: dialog.checked.clone(),
        }
    }

    pub(super) fn available(&self) -> &[SettingsMverMode] {
        &self.available
    }

    pub(super) fn is_checked(&self, mode: SettingsMverMode) -> bool {
        self.checked.contains(&mode)
    }

    pub(super) fn set(&mut self, mode: SettingsMverMode, checked: bool) {
        if !self.available.contains(&mode) {
            return;
        }
        if checked {
            self.checked.insert(mode);
        } else {
            self.checked.remove(&mode);
        }
    }

    pub(super) fn checked_in_order(&self) -> Vec<SettingsMverMode> {
        self.available
            .iter()
            .copied()
            .filter(|mode| self.checked.contains(mode))
            .collect()
    }

    pub(super) fn can_confirm(&self) -> bool {
        !self.checked.is_empty()
    }
}

/// The one mode checked when the conversion dialog first opens.
///
/// The order here is the user's priority, not the enum's: standard first, then
/// keyboard, then gamepad, and `None` only when the source has no convertible
/// mode at all (which inspection never reports, but the page does not assume it).
fn default_checked_mver_mode(available: &[SettingsMverMode]) -> Option<SettingsMverMode> {
    [
        SettingsMverMode::Standard,
        SettingsMverMode::Keyboard,
        SettingsMverMode::Gamepad,
    ]
    .into_iter()
    .find(|mode| available.contains(mode))
}

struct ModelImportDraft {
    /// The display name an import will use, derived from the chosen source.
    title: String,
    source_root: Option<PathBuf>,
    state: ModelImportState,
    /// The conversion choices for a Mver source. `Some` only while the dialog
    /// for this source is still open — the draft keeps no selection once the run
    /// starts, because the request has already carried it.
    mver_mode_dialog: Option<MverModeDialog>,
    /// The models that existed when the run started, so the cards the run
    /// installed can be told apart from the ones that were already there and
    /// held back until their cover capture finishes.
    baseline_models: BTreeSet<ModelRowKey>,
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
    open_location: FocusHandle,
    edit: FocusHandle,
    delete: FocusHandle,
}

/// The one model card that is open for editing.
///
/// The draft owns the title field and a cover the user picked but has not saved
/// yet, so cancelling is dropping this value: nothing reaches the settings
/// service until save, and a half-finished edit never appears in the catalog.
struct ModelEditDraft {
    model: SettingsModelKey,
    title: String,
    /// A cover chosen in this edit, still to be written to the model's package.
    cover: Option<PathBuf>,
    input: Entity<InputState>,
    input_focus: FocusHandle,
    cover_focus: FocusHandle,
    save_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// A cover dialog is open for this draft.
    picking: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActions {
    active: bool,
    can_activate: bool,
    /// An imported model is deletable even while it is the one on screen: the
    /// runtime switches to the standard preset before the package goes away, so
    /// no card the user imported ever loses its delete control.
    can_delete: bool,
    /// Every model in the catalog can be renamed and have its cover replaced,
    /// whichever origin it came from: a preset's name and cover are recorded on
    /// the user's side rather than written into the package the build owns, so
    /// the row offers the same edit affordance either way.
    can_edit: bool,
    can_open_location: bool,
}

#[derive(Clone, Copy)]
enum ModelRowAction {
    Activate,
    OpenLocation,
    Edit,
    /// Delete the model. Only reachable from the confirmation surface, so
    /// reaching this variant *is* the confirmation.
    Delete,
}

impl Default for ModelImportDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            source_root: None,
            state: ModelImportState::Idle,
            mver_mode_dialog: None,
            baseline_models: BTreeSet::new(),
        }
    }
}

impl ModelImportDraft {
    /// Whether the card is showing progress rather than the upload prompt.
    ///
    /// The capture is part of the run: the model is installed, but the card has
    /// not handed over to the catalog yet, so the page keeps treating the run as
    /// in flight for every command gate.
    fn is_running(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. }
                | ModelImportState::Running(_)
                | ModelImportState::Capturing
        )
    }

    /// Whether the card offers a cancel control for the step it is showing.
    ///
    /// The capture that follows a successful import cannot be cancelled: the
    /// model is already installed, and aborting the render would only leave it
    /// without its cover.
    fn shows_cancel(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. } | ModelImportState::Running(_)
        )
    }

    /// Whether a cancel request would still reach a live operation.
    fn is_cancellable(&self) -> bool {
        match &self.state {
            ModelImportState::Starting { cancel_requested } => !cancel_requested,
            ModelImportState::Running(monitor) => !monitor.is_cancelled(),
            _ => false,
        }
    }

    fn can_import(&self) -> bool {
        self.source_root.is_some()
            && !self.title.is_empty()
            && !self.is_running()
            && !self.is_source_surface_open()
    }

    fn is_picker_open(&self) -> bool {
        matches!(self.state, ModelImportState::Picking)
    }

    /// Whether a window-owned source or conversion surface is still up.
    ///
    /// The folder picker and the conversion-mode dialog both stop the card from
    /// starting a second run, so command gates use this rather than picking one.
    fn is_source_surface_open(&self) -> bool {
        self.is_picker_open() || self.is_inspecting() || self.has_open_mver_mode_dialog()
    }

    /// Whether the conversion-mode dialog owns the source the user chose.
    fn has_open_mver_mode_dialog(&self) -> bool {
        self.mver_mode_dialog.is_some()
    }

    /// Whether the conversion-mode dialog for the chosen Mver source is open.
    fn is_inspecting(&self) -> bool {
        matches!(self.state, ModelImportState::Inspecting)
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

    /// Return to the upload prompt, keeping nothing about the run that ended.
    fn reset(&mut self) {
        self.state = ModelImportState::Idle;
        self.source_root = None;
        self.mver_mode_dialog = None;
        self.baseline_models.clear();
    }
}

/// The appearance a settings window paints its first frame with.
///
/// The window is created and shown before the settings service answers its first
/// snapshot, and a frame rendered in that gap has no snapshot to read: the
/// language and theme used to fall back to the built-in defaults, so a user whose
/// language is Simplified Chinese watched the window redraw from English once the
/// snapshot landed. The window is therefore opened with the values the product
/// already knows — the effective language, which the application resolves from the
/// configured preference and the system language at startup, and the configured
/// theme — and renders them until the first snapshot replaces them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsWindowSeed {
    /// The resolved display language, not the stored preference: `system` is
    /// already resolved against the system language by the application.
    pub language: SettingsLanguage,
    pub appearance_theme: SettingsTheme,
}

pub struct SettingsView {
    client: SettingsClient,
    seed: SettingsWindowSeed,
    snapshot: Option<SettingsSnapshot>,
    pending: Option<PendingOperation>,
    pending_notification: Option<SettingsError>,
    model_import: ModelImportDraft,
    overlay_scale_debouncer: crate::SettingsPatchDebouncer<u16>,
    overlay_scale_timer_generation: u64,
    overlay_opacity_debouncer: crate::SettingsPatchDebouncer<u8>,
    overlay_opacity_timer_generation: u64,
    overlay_corner_radius_debouncer: crate::SettingsPatchDebouncer<u8>,
    overlay_corner_radius_timer_generation: u64,
    overlay_hover_hide_delay_debouncer: crate::SettingsPatchDebouncer<u32>,
    overlay_hover_hide_delay_timer_generation: u64,
    gamepad_dead_zone_debouncer: crate::SettingsPatchDebouncer<SettingsGamepadAxisSettings>,
    gamepad_dead_zone_timer_generation: u64,
    maximum_fps_debouncer: crate::SettingsPatchDebouncer<u16>,
    maximum_fps_timer_generation: u64,
    release_fallback_timeout_debouncer: crate::SettingsPatchDebouncer<u32>,
    release_fallback_timeout_timer_generation: u64,
    flush_pending_requested: bool,
    quit_after_flush: bool,
    model_delete_confirmation: Option<SettingsModelKey>,
    model_row_focus: BTreeMap<ModelRowKey, ModelRowFocus>,
    model_edit: Option<ModelEditDraft>,
    /// Whether the unreadable-catalog notification has already been pushed for
    /// the current failure, so it is not repeated on every snapshot.
    model_catalog_error_reported: bool,
    shortcut_capture: Option<ShortcutCapture>,
    shortcut_capture_blur_subscription: Option<gpui_kit::Subscription>,
    shortcut_row_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    shortcut_clear_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    window_hidden: bool,
    applied_theme: Option<SettingsTheme>,
    language_select: Entity<LanguageSelectState>,
    theme_select: Entity<ThemeSelectState>,
    request_quit: Rc<dyn Fn(&mut App)>,
    /// Opens the update window and starts a check.
    request_update: SettingsWindowRequest,
    overlay_focus: FocusHandle,
    /// Focus handle of the import card while it is the upload prompt.
    import_card_focus: FocusHandle,
    /// The models the current run installed whose cover capture has not
    /// finished yet.
    ///
    /// The import result lands before the product has rendered the model's own
    /// cover, so these entries are withheld from the grid: the card would
    /// otherwise flicker through the source package's placeholder picture and
    /// then swap it. An entry leaves this set when its capture reports back,
    /// whether it produced a cover or not, so a failed capture still publishes
    /// the model.
    pending_model_reveal: BTreeSet<ModelRowKey>,
    /// Captures that finished before the run they belong to reported its result.
    ///
    /// The capture runs on a different thread from the settings worker, so it
    /// can win the race against the import reply. Recording its completion here
    /// is what keeps the gate from waiting on a signal that already happened.
    completed_model_cover_captures: BTreeSet<ModelRowKey>,
    syncing_component_inputs: bool,
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
            .ok_or_else(|| std::io::Error::other("settings view was released").into())
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

    /// Requests application shutdown after all unacknowledged debounced patches
    /// have been submitted successfully.
    pub fn request_quit_after_flush(&self, cx: &mut App) -> gpui_kit::Result<()> {
        self.update(cx, |view, _, cx| {
            view.request_quit_after_flush(cx);
        })
    }

    pub fn flush_pending_settings(&self, cx: &mut App) -> gpui_kit::Result<()> {
        self.update(cx, |view, _, cx| {
            view.flush_pending_settings(cx);
        })
    }
}

impl PartialEq for SettingsWindowHandle {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window
    }
}

impl Eq for SettingsWindowHandle {}

impl SettingsView {
    fn schedule_overlay_scale_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_scale_timer_generation = self.overlay_scale_timer_generation.saturating_add(1);
        let generation = self.overlay_scale_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_scale_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(scale_percent) = view.overlay_scale_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.scale_percent = scale_percent;
                view.start_request(
                    PendingOperation::OverlayScale,
                    Some(SettingValue::OverlayScale {
                        expected_config_revision,
                        scale_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_overlay_opacity_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_opacity_timer_generation =
            self.overlay_opacity_timer_generation.saturating_add(1);
        let generation = self.overlay_opacity_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_opacity_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(opacity_percent) = view.overlay_opacity_debouncer.ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.opacity_percent = opacity_percent;
                view.start_request(
                    PendingOperation::OverlayOpacity,
                    Some(SettingValue::OverlayOpacity {
                        expected_config_revision,
                        opacity_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_overlay_corner_radius_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_corner_radius_timer_generation = self
            .overlay_corner_radius_timer_generation
            .saturating_add(1);
        let generation = self.overlay_corner_radius_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_corner_radius_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(corner_radius_percent) =
                    view.overlay_corner_radius_debouncer.ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.corner_radius_percent = corner_radius_percent;
                view.start_request(
                    PendingOperation::OverlayCornerRadius,
                    Some(SettingValue::OverlayCornerRadius {
                        expected_config_revision,
                        corner_radius_percent,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_overlay_hover_hide_delay_flush(&mut self, cx: &mut Context<Self>) {
        self.overlay_hover_hide_delay_timer_generation = self
            .overlay_hover_hide_delay_timer_generation
            .saturating_add(1);
        let generation = self.overlay_hover_hide_delay_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.overlay_hover_hide_delay_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(hide_on_pointer_hover_delay_seconds) = view
                    .overlay_hover_hide_delay_debouncer
                    .ready(Instant::now())
                else {
                    return;
                };
                let Some(snapshot) = view.snapshot.as_ref() else {
                    return;
                };
                let Some(expected_config_revision) = snapshot.config_revision else {
                    return;
                };
                let mut settings = snapshot.overlay;
                settings.hide_on_pointer_hover_delay_seconds = hide_on_pointer_hover_delay_seconds;
                view.start_request(
                    PendingOperation::OverlayHoverHideDelay,
                    Some(SettingValue::OverlayHoverHideDelay {
                        expected_config_revision,
                        hide_on_pointer_hover_delay_seconds,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_gamepad_dead_zone_flush(&mut self, cx: &mut Context<Self>) {
        self.gamepad_dead_zone_timer_generation =
            self.gamepad_dead_zone_timer_generation.saturating_add(1);
        let generation = self.gamepad_dead_zone_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.gamepad_dead_zone_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(settings) = view.gamepad_dead_zone_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::GamepadAxisSettings,
                    Some(SettingValue::GamepadAxisSettings {
                        expected_config_revision,
                        settings,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_maximum_fps_flush(&mut self, cx: &mut Context<Self>) {
        self.maximum_fps_timer_generation = self.maximum_fps_timer_generation.saturating_add(1);
        let generation = self.maximum_fps_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.maximum_fps_timer_generation != generation || view.pending.is_some() {
                    return;
                }
                let Some(maximum_fps) = view.maximum_fps_debouncer.ready(Instant::now()) else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::MaximumFps,
                    Some(SettingValue::MaximumFps {
                        expected_config_revision,
                        maximum_fps,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn schedule_release_fallback_timeout_flush(&mut self, cx: &mut Context<Self>) {
        self.release_fallback_timeout_timer_generation = self
            .release_fallback_timeout_timer_generation
            .saturating_add(1);
        let generation = self.release_fallback_timeout_timer_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.release_fallback_timeout_timer_generation != generation
                    || view.pending.is_some()
                {
                    return;
                }
                let Some(timeout_ms) = view
                    .release_fallback_timeout_debouncer
                    .ready(Instant::now())
                else {
                    return;
                };
                let Some(expected_config_revision) = view
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.config_revision)
                else {
                    return;
                };
                view.start_request(
                    PendingOperation::ReleaseFallbackTimeout,
                    Some(SettingValue::ReleaseFallbackTimeout {
                        expected_config_revision,
                        timeout_ms,
                    }),
                    cx,
                );
            });
        })
        .detach();
    }

    fn flush_pending_setting_patches(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let now = Instant::now();
        let Some(snapshot) = self.snapshot.clone() else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
            return;
        };
        let Some(expected_config_revision) = snapshot.config_revision else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
            return;
        };
        if let Some(scale_percent) = self.overlay_scale_debouncer.flush(now) {
            let mut settings = snapshot.overlay;
            settings.scale_percent = scale_percent;
            self.start_request(
                PendingOperation::OverlayScale,
                Some(SettingValue::OverlayScale {
                    expected_config_revision,
                    scale_percent,
                    settings,
                }),
                cx,
            );
        } else if let Some(opacity_percent) = self.overlay_opacity_debouncer.flush(now) {
            let mut settings = snapshot.overlay;
            settings.opacity_percent = opacity_percent;
            self.start_request(
                PendingOperation::OverlayOpacity,
                Some(SettingValue::OverlayOpacity {
                    expected_config_revision,
                    opacity_percent,
                    settings,
                }),
                cx,
            );
        } else if let Some(corner_radius_percent) = self.overlay_corner_radius_debouncer.flush(now)
        {
            let mut settings = snapshot.overlay;
            settings.corner_radius_percent = corner_radius_percent;
            self.start_request(
                PendingOperation::OverlayCornerRadius,
                Some(SettingValue::OverlayCornerRadius {
                    expected_config_revision,
                    corner_radius_percent,
                    settings,
                }),
                cx,
            );
        } else if let Some(hide_on_pointer_hover_delay_seconds) =
            self.overlay_hover_hide_delay_debouncer.flush(now)
        {
            let mut settings = snapshot.overlay;
            settings.hide_on_pointer_hover_delay_seconds = hide_on_pointer_hover_delay_seconds;
            self.start_request(
                PendingOperation::OverlayHoverHideDelay,
                Some(SettingValue::OverlayHoverHideDelay {
                    expected_config_revision,
                    hide_on_pointer_hover_delay_seconds,
                    settings,
                }),
                cx,
            );
        } else if let Some(settings) = self.gamepad_dead_zone_debouncer.flush(now) {
            self.start_request(
                PendingOperation::GamepadAxisSettings,
                Some(SettingValue::GamepadAxisSettings {
                    expected_config_revision,
                    settings,
                }),
                cx,
            );
        } else if let Some(maximum_fps) = self.maximum_fps_debouncer.flush(now) {
            self.start_request(
                PendingOperation::MaximumFps,
                Some(SettingValue::MaximumFps {
                    expected_config_revision,
                    maximum_fps,
                }),
                cx,
            );
        } else if let Some(timeout_ms) = self.release_fallback_timeout_debouncer.flush(now) {
            self.start_request(
                PendingOperation::ReleaseFallbackTimeout,
                Some(SettingValue::ReleaseFallbackTimeout {
                    expected_config_revision,
                    timeout_ms,
                }),
                cx,
            );
        } else {
            self.flush_pending_requested = false;
            let should_quit = self.quit_after_flush;
            self.quit_after_flush = false;
            if should_quit {
                (self.request_quit)(cx);
            }
        }
    }

    pub(super) fn flush_pending_settings(&mut self, cx: &mut Context<Self>) {
        self.flush_pending_requested = true;
        self.flush_pending_setting_patches(cx);
    }

    pub(super) fn request_quit_after_flush(&mut self, cx: &mut Context<Self>) {
        self.flush_pending_requested = true;
        self.quit_after_flush = true;
        self.flush_pending_setting_patches(cx);
    }

    /// The one visual-gate predicate every settings page reads (ADR-0053).
    ///
    /// True only where editing is structurally impossible: no snapshot yet, a
    /// model import running, or any source or conversion surface
    /// covering the window. The transient in-flight `pending` flag deliberately never
    /// feeds it — it flips on and off around every command, so gating on it
    /// dims and re-enables a whole page on each control change, which reads
    /// as the page refreshing. Re-entrancy is refused by the command guards
    /// instead (`start_request`, the model command methods), and the header
    /// status is the saving indicator.
    fn editing_blocked(&self, snapshot: Option<&SettingsSnapshot>) -> bool {
        snapshot.is_none()
            || self.model_import.is_running()
            || self.model_import.is_source_surface_open()
    }

    fn start_request(
        &mut self,
        operation: PendingOperation,
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let is_refresh = operation == PendingOperation::Refresh;
        if !is_refresh {
            self.pending = Some(operation);
            cx.notify();
        }
        let client = self.client.clone();
        let sent_overlay_scale = match value.as_ref() {
            Some(SettingValue::OverlayScale { scale_percent, .. }) => Some(*scale_percent),
            _ => None,
        };
        let sent_overlay_opacity = match value.as_ref() {
            Some(SettingValue::OverlayOpacity {
                opacity_percent, ..
            }) => Some(*opacity_percent),
            _ => None,
        };
        let sent_overlay_corner_radius = match value.as_ref() {
            Some(SettingValue::OverlayCornerRadius {
                corner_radius_percent,
                ..
            }) => Some(*corner_radius_percent),
            _ => None,
        };
        let sent_overlay_hover_hide_delay = match value.as_ref() {
            Some(SettingValue::OverlayHoverHideDelay {
                hide_on_pointer_hover_delay_seconds,
                ..
            }) => Some(*hide_on_pointer_hover_delay_seconds),
            _ => None,
        };
        let sent_gamepad_dead_zone = match value.as_ref() {
            Some(SettingValue::GamepadAxisSettings { settings, .. }) => Some(*settings),
            _ => None,
        };
        let sent_maximum_fps = match value.as_ref() {
            Some(SettingValue::MaximumFps { maximum_fps, .. }) => Some(*maximum_fps),
            _ => None,
        };
        let sent_release_fallback_timeout = match value.as_ref() {
            Some(SettingValue::ReleaseFallbackTimeout { timeout_ms, .. }) => Some(*timeout_ms),
            _ => None,
        };
        cx.spawn(async move |this, cx| {
            let result = match value {
                None => client.read_snapshot().await,
                Some(SettingValue::AppearanceTheme {
                    expected_config_revision,
                    theme,
                }) => {
                    client
                        .set_appearance_theme(expected_config_revision, theme)
                        .await
                }
                Some(SettingValue::Language {
                    expected_config_revision,
                    language,
                }) => {
                    client
                        .set_language(expected_config_revision, language)
                        .await
                }
                Some(SettingValue::StatusIconVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_status_icon_visible(expected_config_revision, visible)
                        .await
                }
                #[cfg(target_os = "windows")]
                Some(SettingValue::TaskbarIconVisible {
                    expected_config_revision,
                    visible,
                }) => {
                    client
                        .set_taskbar_icon_visible(expected_config_revision, visible)
                        .await
                }
                Some(SettingValue::CheckForUpdatesAutomatically {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_check_for_updates_automatically(expected_config_revision, enabled)
                        .await
                }
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
                Some(SettingValue::OverlayScale {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayOpacity {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayCornerRadius {
                    expected_config_revision,
                    settings,
                    ..
                }) => {
                    client
                        .set_overlay_settings(expected_config_revision, settings)
                        .await
                }
                Some(SettingValue::OverlayHoverHideDelay {
                    expected_config_revision,
                    settings,
                    ..
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
                Some(SettingValue::CommandShortcutsEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_command_shortcuts_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::BehaviorShortcutsEnabled {
                    expected_config_revision,
                    enabled,
                }) => {
                    client
                        .set_behavior_shortcuts_enabled(expected_config_revision, enabled)
                        .await
                }
                Some(SettingValue::MaximumFps {
                    expected_config_revision,
                    maximum_fps,
                }) => {
                    client
                        .set_maximum_fps(expected_config_revision, maximum_fps)
                        .await
                }
                Some(SettingValue::ReleaseFallbackTimeout {
                    expected_config_revision,
                    timeout_ms,
                }) => {
                    client
                        .set_release_fallback_timeout(expected_config_revision, timeout_ms)
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
                Some(SettingValue::Shortcuts {
                    expected_config_revision,
                    shortcuts,
                }) => {
                    client
                        .set_shortcuts(expected_config_revision, shortcuts)
                        .await
                }
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
                let mut snapshot_changed = false;
                if !is_refresh {
                    view.pending = None;
                }
                if result.is_ok()
                    && let Some(scale_percent) = sent_overlay_scale
                {
                    view.overlay_scale_debouncer.mark_sent(&scale_percent);
                    if view.overlay_scale_debouncer.is_pending() {
                        view.schedule_overlay_scale_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(opacity_percent) = sent_overlay_opacity
                {
                    view.overlay_opacity_debouncer.mark_sent(&opacity_percent);
                    if view.overlay_opacity_debouncer.is_pending() {
                        view.schedule_overlay_opacity_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(corner_radius_percent) = sent_overlay_corner_radius
                {
                    view.overlay_corner_radius_debouncer
                        .mark_sent(&corner_radius_percent);
                    if view.overlay_corner_radius_debouncer.is_pending() {
                        view.schedule_overlay_corner_radius_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(hide_on_pointer_hover_delay_seconds) = sent_overlay_hover_hide_delay
                {
                    view.overlay_hover_hide_delay_debouncer
                        .mark_sent(&hide_on_pointer_hover_delay_seconds);
                    if view.overlay_hover_hide_delay_debouncer.is_pending() {
                        view.schedule_overlay_hover_hide_delay_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(settings) = sent_gamepad_dead_zone
                {
                    view.gamepad_dead_zone_debouncer.mark_sent(&settings);
                    if view.gamepad_dead_zone_debouncer.is_pending() {
                        view.schedule_gamepad_dead_zone_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(maximum_fps) = sent_maximum_fps
                {
                    view.maximum_fps_debouncer.mark_sent(&maximum_fps);
                    if view.maximum_fps_debouncer.is_pending() {
                        view.schedule_maximum_fps_flush(cx);
                    }
                }
                if result.is_ok()
                    && let Some(timeout_ms) = sent_release_fallback_timeout
                {
                    view.release_fallback_timeout_debouncer
                        .mark_sent(&timeout_ms);
                    if view.release_fallback_timeout_debouncer.is_pending() {
                        view.schedule_release_fallback_timeout_flush(cx);
                    }
                }
                if result.is_err() {
                    if operation == PendingOperation::AppearanceTheme {
                        view.applied_theme = None;
                    }
                    // Keep failed debounced patches alive and retry after the stable window.
                    // The debouncer only clears a value after a successful acknowledgement.
                    if sent_overlay_scale.is_some() {
                        view.schedule_overlay_scale_flush(cx);
                    }
                    if sent_overlay_opacity.is_some() {
                        view.schedule_overlay_opacity_flush(cx);
                    }
                    if sent_overlay_corner_radius.is_some() {
                        view.schedule_overlay_corner_radius_flush(cx);
                    }
                    if sent_overlay_hover_hide_delay.is_some() {
                        view.schedule_overlay_hover_hide_delay_flush(cx);
                    }
                    if sent_gamepad_dead_zone.is_some() {
                        view.schedule_gamepad_dead_zone_flush(cx);
                    }
                    if sent_maximum_fps.is_some() {
                        view.schedule_maximum_fps_flush(cx);
                    }
                    if sent_release_fallback_timeout.is_some() {
                        view.schedule_release_fallback_timeout_flush(cx);
                    }
                }
                if let Some(snapshot) = refreshed
                    && accepts_snapshot_revision(
                        view.snapshot.as_ref().map(|current| current.revision),
                        snapshot.revision,
                    )
                    && view.snapshot.as_ref() != Some(&snapshot)
                {
                    view.snapshot = Some(snapshot);
                    snapshot_changed = true;
                }
                match result {
                    Ok(ref snapshot)
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) =>
                    {
                        if view.snapshot.as_ref() != Some(snapshot) {
                            view.snapshot = Some(snapshot.clone());
                            snapshot_changed = true;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if view.pending_notification.as_ref() != Some(&error) {
                            snapshot_changed = true;
                        }
                        view.pending_notification = Some(error);
                    }
                }
                // The next shutdown patch must use the revision returned by this request.
                if view.flush_pending_requested {
                    if result.is_ok() {
                        view.flush_pending_setting_patches(cx);
                    } else {
                        view.flush_pending_requested = false;
                        view.quit_after_flush = false;
                    }
                }
                if !is_refresh || snapshot_changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// The title is free-form display text: control characters are dropped and
/// the value is trimmed and bounded to the metadata title limit. The store
/// key never comes from this field.
fn sanitize_model_title_input(value: &str) -> String {
    let filtered: String = value.chars().filter(|c| !c.is_control()).collect();
    let filtered = filtered.trim();
    filtered.chars().take(128).collect()
}

#[derive(Clone)]
enum SettingValue {
    AppearanceTheme {
        expected_config_revision: u64,
        theme: SettingsTheme,
    },
    Language {
        expected_config_revision: u64,
        language: SettingsLanguage,
    },
    StatusIconVisible {
        expected_config_revision: u64,
        visible: bool,
    },
    #[cfg(target_os = "windows")]
    TaskbarIconVisible {
        expected_config_revision: u64,
        visible: bool,
    },
    CheckForUpdatesAutomatically {
        expected_config_revision: u64,
        enabled: bool,
    },
    OverlayVisible {
        expected_config_revision: u64,
        visible: bool,
    },
    OverlaySettings {
        expected_config_revision: u64,
        settings: SettingsOverlay,
    },
    OverlayScale {
        expected_config_revision: u64,
        scale_percent: u16,
        settings: SettingsOverlay,
    },
    OverlayOpacity {
        expected_config_revision: u64,
        opacity_percent: u8,
        settings: SettingsOverlay,
    },
    OverlayCornerRadius {
        expected_config_revision: u64,
        corner_radius_percent: u8,
        settings: SettingsOverlay,
    },
    OverlayHoverHideDelay {
        expected_config_revision: u64,
        hide_on_pointer_hover_delay_seconds: u32,
        settings: SettingsOverlay,
    },
    MotionAudioEnabled {
        expected_config_revision: u64,
        enabled: bool,
    },
    CommandShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
    },
    BehaviorShortcutsEnabled {
        expected_config_revision: u64,
        enabled: bool,
    },
    MaximumFps {
        expected_config_revision: u64,
        maximum_fps: u16,
    },
    ReleaseFallbackTimeout {
        expected_config_revision: u64,
        timeout_ms: u32,
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
    Shortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupItemAction {
    None,
    Retry,
    SetEnabled(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StartupItemPresentation {
    /// Row copy for the states the switch position cannot explain on its own: a
    /// read that is still in flight, a login item that needs repair, or a
    /// platform that cannot offer login startup at all.
    ///
    /// `Disabled` and `Enabled` carry none: the switch already shows that state,
    /// so a second line repeating it would only add height to the common case.
    description: Option<&'static str>,
    enabled: bool,
    action: StartupItemAction,
    /// Present only when this build cannot offer login startup at all.
    ///
    /// The now-disabled switch explains itself with this text on hover, because
    /// the reason is a property of the build and not something the user can
    /// resolve from the settings window.
    unavailable_hint: Option<&'static str>,
}

impl StartupItemPresentation {
    /// Whether this build cannot offer login startup at all.
    ///
    /// This is a fact about the build, not about the current moment, so it is the
    /// one thing that greys the switch out. Whether the control can act *right
    /// now* is a separate question answered by `action`, and it stays with the
    /// Keeping
    /// the two apart is what lets a released build keep the switch normally
    /// available while the snapshot is still loading.
    fn switch_disabled(self) -> bool {
        self.unavailable_hint.is_some()
    }
}

fn startup_item_presentation(
    status: Option<SettingsStartupItemStatus>,
    blocked: bool,
    language: SettingsLanguage,
) -> StartupItemPresentation {
    let mut presentation = match status {
        None => StartupItemPresentation {
            description: Some(bongocat_i18n::text(
                language.catalog_locale(),
                "settings.application.startup.checking",
            )),
            enabled: false,
            action: StartupItemAction::None,
            unavailable_hint: None,
        },
        Some(SettingsStartupItemStatus::ReadError(_)) => StartupItemPresentation {
            description: Some(bongocat_i18n::text(
                language.catalog_locale(),
                "settings.application.startup.unavailable",
            )),
            enabled: false,
            action: StartupItemAction::Retry,
            unavailable_hint: None,
        },
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)) => {
            StartupItemPresentation {
                description: None,
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                unavailable_hint: None,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)) => {
            StartupItemPresentation {
                description: None,
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
                unavailable_hint: None,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.startup.stale",
                )),
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                unavailable_hint: None,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.startup.requires_approval",
                )),
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
                unavailable_hint: None,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.application.startup.not_found",
                )),
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                unavailable_hint: None,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(reason))) => {
            let (description, unavailable_hint) = match reason {
                SettingsStartupItemUnsupportedReason::Platform => (
                    Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.application.startup.unsupported_platform",
                    )),
                    None,
                ),
                SettingsStartupItemUnsupportedReason::OperatingSystem => (
                    Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.application.startup.unsupported_os",
                    )),
                    None,
                ),
                // The one unsupported reason that is a property of this build
                // rather than of the machine, so the control has to say so
                // itself: the same text explains the row and the disabled
                // switch, and neither copy can drift from the other.
                SettingsStartupItemUnsupportedReason::BuildEnvironment => {
                    let text = bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.application.startup.unsupported_build",
                    );
                    (Some(text), Some(text))
                }
            };
            StartupItemPresentation {
                description,
                enabled: false,
                action: StartupItemAction::None,
                unavailable_hint,
            }
        }
    };
    if blocked {
        presentation.action = StartupItemAction::None;
    }
    presentation
}

/// The import suggestion shown to the user is the chosen folder's own name.
///
/// The rule lives in `model_source_display_name` because the settings service's
/// fallback title has to agree with what the page pre-filled. The portable store
/// id is allocated by the settings service at import time, so the displayed name
/// never needs ASCII folding; hand-typed edits are still sanitized by
/// `sanitize_model_title_input`.
fn suggested_model_title(source_root: &Path) -> String {
    crate::model_source_display_name(source_root).unwrap_or_else(|| "custom-model".to_owned())
}

/// The catalog key naming one BongoCat Mver conversion mode.
///
/// The mode keys are shared with the legacy model list, so the dialog says
/// "Keyboard mode" in the same words the card for a converted model will.
fn mver_mode_label_key(mode: SettingsMverMode) -> &'static str {
    match mode {
        SettingsMverMode::Standard => "models.legacy.mode.standard",
        SettingsMverMode::Keyboard => "models.legacy.mode.keyboard",
        SettingsMverMode::Gamepad => "models.legacy.mode.gamepad",
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
    let ready = matches!(&entry.availability, SettingsModelAvailability::Ready { .. });
    let installed = entry.origin == SettingsModelOrigin::Installed;
    ModelRowActions {
        active,
        can_activate: ready && !active && !commands_blocked,
        can_delete: installed && !commands_blocked,
        // Editing keeps no origin exception: a preset is renamed and re-covered
        // through the same user-side records an installed model uses, so only
        // deleting is still reserved for what the user installed.
        can_edit: !commands_blocked,
        can_open_location: entry.directory.is_some() && !commands_blocked,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModelRowActionTabIndices {
    activate: isize,
    open_location: isize,
    edit: isize,
    delete: isize,
}

/// The tab position of each action on a card.
///
/// Every card action is rendered at all times — the delete confirmation is a
/// surface anchored to the delete control, not a replacement for the row — so
/// the positions do not move when a confirmation opens.
fn model_row_action_tab_indices(first_tab_index: isize) -> ModelRowActionTabIndices {
    ModelRowActionTabIndices {
        activate: first_tab_index,
        open_location: first_tab_index.saturating_add(1),
        edit: first_tab_index.saturating_add(2),
        delete: first_tab_index.saturating_add(3),
    }
}

/// Whether an open delete question should survive a re-projection of the catalog.
///
/// The question is drawn by the card's delete control, and the card only draws
/// that control while deleting is possible, so the question is only meaningful
/// under exactly the conditions the control needs: an installed model, still in
/// the catalog, and no structural command block (an import running or a picker
/// open). An in-flight command is deliberately not one of those conditions —
/// `pending` never feeds a visual gate (ADR-0053), so the control stays drawn
/// and the question stays valid while it waits, and the command methods refuse
/// a second command instead. Asking [`model_row_actions`] keeps the question
/// and the control from drifting.
fn model_delete_confirmation_is_valid(
    entries: &[SettingsModelEntry],
    active_model: Option<&SettingsModelKey>,
    commands_blocked: bool,
    model: &SettingsModelKey,
) -> bool {
    entries
        .iter()
        .find(|entry| entry.origin == model.origin && entry.id == model.id)
        .is_some_and(|entry| model_row_actions(entry, active_model, commands_blocked).can_delete)
}

fn model_availability_status(
    entry: &SettingsModelEntry,
    language: SettingsLanguage,
) -> Option<SharedString> {
    // A ready model has nothing to say that the card does not already show, so
    // only a diagnostic earns a status line.
    match &entry.availability {
        SettingsModelAvailability::Ready { .. } => None,
        SettingsModelAvailability::Invalid { diagnostic } => {
            let diagnostic = match diagnostic {
                SettingsModelDiagnostic::InvalidModelId
                | SettingsModelDiagnostic::ModelEntryAmbiguous
                | SettingsModelDiagnostic::ModelEntryMissing
                | SettingsModelDiagnostic::ModelReferenceEscapesRoot
                | SettingsModelDiagnostic::ModelReferenceInvalid
                | SettingsModelDiagnostic::ModelReferenceSymlinkEscape
                | SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
                    "models.validation.package_layout_invalid"
                }
                SettingsModelDiagnostic::ModelFileCountExceeded
                | SettingsModelDiagnostic::ModelFileTooLarge
                | SettingsModelDiagnostic::ModelJsonTooLarge
                | SettingsModelDiagnostic::ModelPackageDepthExceeded
                | SettingsModelDiagnostic::ModelPackageSizeExceeded
                | SettingsModelDiagnostic::ModelTextureDimensionExceeded => {
                    "models.validation.package_safety_limits_exceeded"
                }
                SettingsModelDiagnostic::ModelJsonInvalid
                | SettingsModelDiagnostic::ModelUnsupportedVersion => {
                    "models.validation.model_definition_unsupported"
                }
                SettingsModelDiagnostic::ModelTextureInvalidPng
                | SettingsModelDiagnostic::ModelTextureMissing => {
                    "models.validation.texture_invalid"
                }
                SettingsModelDiagnostic::ModelIoError => "models.validation.files_unavailable",
                SettingsModelDiagnostic::ModelMocMissing
                | SettingsModelDiagnostic::ModelResourceInvalid
                | SettingsModelDiagnostic::ModelResourceMissing
                | SettingsModelDiagnostic::ModelResourceNotFile => {
                    "models.validation.resource_invalid"
                }
            };
            Some(
                model_invalid_summary(
                    language,
                    entry.origin,
                    bongocat_i18n::text(language.catalog_locale(), diagnostic),
                )
                .into(),
            )
        }
    }
}
pub(crate) fn sync_system_component_theme(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}

/// The component-library mode a preference pins, or `None` when it follows the system.
const fn pinned_theme_mode(theme: SettingsTheme) -> Option<ThemeMode> {
    match theme {
        SettingsTheme::System => None,
        SettingsTheme::Light => Some(ThemeMode::Light),
        SettingsTheme::Dark => Some(ThemeMode::Dark),
    }
}

/// The native appearance a preference pins, or `None` when it follows the system.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn pinned_native_theme(theme: SettingsTheme) -> Option<bongocat_platform::AppTheme> {
    match theme {
        SettingsTheme::System => None,
        SettingsTheme::Light => Some(bongocat_platform::AppTheme::Light),
        SettingsTheme::Dark => Some(bongocat_platform::AppTheme::Dark),
    }
}

/// Hands the preference to the platform layer, which owns every native surface that has
/// to follow it: the window frame, the alerts, the tray and context menus, and the
/// open/save panels.
///
/// A failure is not raised. The surfaces that can refuse are the ones that cannot follow
/// an application theme at all on that platform, and their documented fallback is the
/// system appearance — which is what refusing leaves them on. The product still applies
/// the choice to everything it paints itself, so the user's selection is never lost to a
/// cosmetic failure.
fn apply_native_theme(theme: SettingsTheme, window: &Window) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let _ = bongocat_platform::apply_theme(window, pinned_native_theme(theme));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (theme, window);
}

/// The appearance the operating system is using, for the "follow the system" choice.
///
/// macOS asks the platform layer rather than gpui, for two separate reasons:
///
/// - `Window::appearance()` is a cached field, refreshed from a deferred
///   `appearance_changed` callback. On the frame where the application override is
///   cleared the cache still reports the value that was just cleared, and `SettingsView`
///   remembers that it already applied the preference, so it would never correct itself.
/// - `App::window_appearance()` is live — it and the platform query both read
///   `NSApplication.effectiveAppearance` — but its name mapping only recognises `Aqua`,
///   `DarkAqua`, `VibrantLight` and `VibrantDark`, and falls through to `Light` (printing
///   to stdout) for anything else. With "Increase contrast" on, AppKit reports
///   `AccessibilityHighContrastDarkAqua`, so gpui would call a dark system light and the
///   product would paint a light UI inside a dark one. The platform query knows that name.
///
/// `window` is only consulted off macOS, and only when there is one. A caller without a
/// window — the smoke assertions — falls back to the application appearance, which is the
/// same platform query.
fn system_appearance(window: Option<&Window>, cx: &App) -> WindowAppearance {
    #[cfg(target_os = "macos")]
    {
        // Neither argument is consulted: the whole point of this branch is to avoid the
        // value gpui holds, and the platform query needs no window.
        let _ = (window, cx);
        match bongocat_platform::system_appearance() {
            bongocat_platform::SystemAppearance::Light => WindowAppearance::Light,
            bongocat_platform::SystemAppearance::Dark => WindowAppearance::Dark,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.map_or_else(|| cx.window_appearance(), Window::appearance)
    }
}

/// The component-library mode a preference resolves to.
///
/// One definition for the whole crate. The render path, the optimistic path and the smoke
/// assertions all have to agree on what a preference means, and the way to make them agree
/// is to have only one of them compute it — the smoke exists to prove what the product
/// does, so it must not re-derive the answer with a second formula.
fn resolved_theme_mode(theme: SettingsTheme, window: Option<&Window>, cx: &App) -> ThemeMode {
    match pinned_theme_mode(theme) {
        Some(mode) => mode,
        None => component_theme_mode(theme, system_appearance(window, cx)),
    }
}

/// Applies the preference to the component colours before the configuration roundtrip,
/// so the user sees the switch on the frame they clicked rather than one snapshot later.
///
/// `System` resolves against the live system appearance, which on macOS is only the
/// truth once the application override a pinned Light/Dark installed has been dropped —
/// so the override is dropped first, mirroring the ordering of `apply_component_theme`
/// (native first, then resolve). Windows has no process override to drop; its frame is
/// corrected by the roundtrip's `apply_component_theme` right after this.
fn apply_optimistic_component_theme(theme: SettingsTheme, cx: &mut App) {
    if theme == SettingsTheme::System {
        let _ = bongocat_platform::apply_process_theme(None);
    }
    let mode = resolved_theme_mode(theme, None, cx);
    if cx.theme().mode != mode {
        Theme::change(mode, None, cx);
    }
}

/// Applies a preference to both halves of the appearance: the native surfaces the
/// platform draws, and the component colours the product draws.
///
/// The order is not interchangeable. On macOS the native call installs the very override
/// that the component mode is derived from when the preference is `System`, so asking
/// for the mode first would resolve against the previous override.
pub(crate) fn apply_component_theme(theme: SettingsTheme, window: &mut Window, cx: &mut App) {
    apply_native_theme(theme, window);
    let mode = resolved_theme_mode(theme, Some(window), cx);
    if cx.theme().mode != mode {
        Theme::change(mode, Some(window), cx);
    }
}

fn component_theme_mode(theme: SettingsTheme, system_appearance: WindowAppearance) -> ThemeMode {
    match theme {
        SettingsTheme::System => system_appearance.into(),
        SettingsTheme::Light => ThemeMode::Light,
        SettingsTheme::Dark => ThemeMode::Dark,
    }
}

const fn theme_index(theme: SettingsTheme) -> usize {
    match theme {
        SettingsTheme::System => 0,
        SettingsTheme::Light => 1,
        SettingsTheme::Dark => 2,
    }
}

fn theme_options(language: SettingsLanguage) -> [&'static str; 3] {
    [
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.system",
        ),
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.light",
        ),
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.dark",
        ),
    ]
}

fn theme_display_name(theme: SettingsTheme, language: SettingsLanguage) -> &'static str {
    theme_options(language)[theme_index(theme)]
}

fn theme_from_display_name(name: &str, language: SettingsLanguage) -> Option<SettingsTheme> {
    theme_options(language)
        .into_iter()
        .position(|option| option == name)
        .and_then(theme_from_index)
}

const fn theme_from_index(index: usize) -> Option<SettingsTheme> {
    match index {
        0 => Some(SettingsTheme::System),
        1 => Some(SettingsTheme::Light),
        2 => Some(SettingsTheme::Dark),
        _ => None,
    }
}

#[cfg(test)]
fn stepped_overlay_scale(mut settings: SettingsOverlay, delta: i16) -> SettingsOverlay {
    let next = i32::from(settings.scale_percent) + i32::from(delta);
    settings.scale_percent = next.clamp(25, 400) as u16;
    settings
}

#[cfg(test)]
fn stepped_overlay_opacity(mut settings: SettingsOverlay, delta: i16) -> SettingsOverlay {
    let next = i16::from(settings.opacity_percent) + delta;
    settings.opacity_percent = next.clamp(1, 100) as u8;
    settings
}

/// Whether the hover hide delay applies to the model window right now.
///
/// The overlay only reads the delay while "hide on pointer hover" is on — both
/// platform backends arm the behaviour with
/// `options.hide_on_pointer_hover && input_running` — so the delay row and its two
/// steppers are inert the rest of the time. The recorded value is deliberately left
/// alone: turning the switch back on restores the delay the user chose instead of a
/// default, which is why nothing here resets it.
fn hover_hide_delay_applies(overlay: SettingsOverlay) -> bool {
    overlay.hide_on_pointer_hover
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

fn icon_command_button(
    id: &'static str,
    label: &'static str,
    icon: impl Into<Icon>,
    focus: &FocusHandle,
    tab_index: isize,
    disabled: bool,
) -> Div {
    div()
        .key_context("SettingsControl")
        .track_focus(focus)
        .tab_index(tab_index)
        .child(Button::new(id).icon(icon).tooltip(label).disabled(disabled))
}
