//! The settings window: the view, the handle the application holds, and the wiring.
//!
//! This module is the window's own state and the small pure helpers the pages
//! beside it share. The per-page logic, the rendering and the debounced write
//! path are the modules under `window/`.

use crate::{
    SettingsBuildEnvironment, SettingsBuildInfo, SettingsClient, SettingsError, SettingsErrorCode,
    SettingsGamepadAutoSwitch, SettingsGamepadAxisSettings, SettingsLanguage, SettingsLogLevel,
    SettingsLogging, SettingsModelAvailability, SettingsModelBehavior,
    SettingsModelBehaviorBinding, SettingsModelDiagnostic, SettingsModelEntry,
    SettingsModelImportMonitor, SettingsModelImportOperation, SettingsModelImportRequest,
    SettingsModelKey, SettingsModelMode, SettingsModelOrigin, SettingsModelSettings,
    SettingsModelSourceContent, SettingsMverMode, SettingsOperationId, SettingsOverlay,
    SettingsRandomBehavior, SettingsShortcutBinding, SettingsShortcuts, SettingsSnapshot,
    SettingsStartupItemState, SettingsStartupItemStatus, SettingsStartupItemUnsupportedReason,
    SettingsTheme, SettingsWindowPlacement, SettingsWindowState,
};
use bongocat_config::ShortcutChord;
use bongocat_platform::{
    ModelSourcePickerError, ModelSourcePickerOutcome, pick_model_cover, pick_model_folder,
    validate_model_folder,
};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IndexPath, Root, Theme, ThemeMode, ThemeStyled, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{Dialog, DialogButtonProps},
    group_box::GroupBoxVariant,
    input::{Input, InputEvent, InputState},
    notification::{Notification, NotificationType},
    searchable_list::SearchableListItem,
    select::{SearchableVec, Select, SelectEvent, SelectState},
    setting::{
        NumberFieldOptions, RenderOptions, SelectIndex, SettingField, SettingGroup, SettingItem,
        SettingPage, Settings,
    },
};

use gpui_kit::{
    Anchor, App, AppContext, Bounds, Context, DisplayId, Div, DragMoveEvent, ElementId, Entity,
    ExternalPaths, FocusHandle, Focusable, Hsla, ImageSource, KeyDownEvent, KeyUpEvent, Modifiers,
    MouseButton, ObjectFit, Pixels, Render, SharedString, Stateful, TitlebarOptions, VisualContext,
    WeakEntity, Window, WindowAppearance, WindowBounds, WindowHandle, WindowOptions,
    base::StyledExt, div, img, point, prelude::*, px, size,
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
mod lifecycle;
mod localization;
mod model_actions;
mod model_drag_overlay;
use model_drag_overlay::ModelDragOverlayState;
mod drag;
mod edit;
mod import;
mod model_import_card;
mod model_mver_dialog;
mod mver;
mod row;
mod source;
use model_import_card::ModelImportCard;
use model_mver_dialog::build_mver_mode_dialog;
mod models;
mod navigation;
pub use navigation::SettingsNavigationMemory;
use navigation::{SettingsNavigationPage, model_library_search_keywords};
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
    build_info_detail, model_invalid_summary, settings_error, shortcut_behavior_name,
    shortcut_command_name, shortcut_conflict_message,
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

struct ModelImportSuccessNotification;

/// Marks the confirmation shown after the About page copies software info.
struct AboutCopySuccessNotification;

/// Marks the notification pushed when a model could not be prepared for
/// display. It is a failure of the import itself, not of a later command: the
/// capture renders the model through the same GPU path the overlay uses, so a
/// model that cannot be captured is a model that cannot be activated, and the
/// run is abandoned instead of publishing a card for it.
struct ModelImportFailedNotification;

fn accepts_snapshot_revision(current: Option<u64>, incoming: u64) -> bool {
    current.is_none_or(|current| incoming >= current)
}

mod controls;
mod import_draft;
mod model_edit_draft;
mod model_row;
mod overlay_step;
mod pending;
mod setting_patch;
mod setting_request;
mod shortcut_capture;
mod startup_item;
mod theme;
mod tokens;

// Every module the split added reaches its neighbours through this one prelude
// rather than naming each of them: the window's items are one vocabulary, and a
// list per module would be the same list thirteen times. The page modules that
// were here first keep their own `pub(super)` items and are not re-exported.
// `pub(super)` is the widest honest visibility here — the window is private, and
// anything wider would expose types that are themselves `pub(super)`.
use controls::*;
use import_draft::*;
use model_edit_draft::*;
#[cfg(test)]
use model_mver_dialog::mver_mode_label_key;
use model_mver_dialog::{MverDialogSnapshot, MverModeDialog};
use model_row::*;
use overlay_step::*;
use pending::*;
use shortcut_capture::*;
use startup_item::*;
pub(crate) use theme::apply_component_theme;
use theme::*;
pub(crate) use tokens::Tokens;

/// A request the settings window forwards to the application rather than acting
/// on itself.
///
/// `None` means the window has no owner for that request, so the matching control
/// is not offered at all.
pub(crate) type SettingsWindowRequest = Rc<dyn Fn(&mut App)>;

pub(crate) type LanguageSelectState = SelectState<SearchableVec<&'static str>>;

pub(crate) type ThemeSelectState = SelectState<SearchableVec<&'static str>>;

pub(crate) type LoggingLevelSelectState = SelectState<SearchableVec<&'static str>>;

pub(crate) type GamepadModelSelectState = SelectState<SearchableVec<GamepadModelChoice>>;

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
    pub(crate) client: SettingsClient,
    pub(crate) seed: SettingsWindowSeed,
    pub(crate) snapshot: Option<SettingsSnapshot>,
    pub(crate) pending: Option<PendingOperation>,
    pub(crate) pending_notification: Option<SettingsError>,
    pub(crate) model_import_success_pending: bool,
    /// Set when the run's cover capture reported that the model could not be
    /// prepared. The model is removed instead of published, and the user is
    /// told the import failed rather than being handed a card that cannot be
    /// activated.
    pub(crate) model_import_failed_pending: bool,
    /// Set after the About page successfully copies the privacy-safe build
    /// summary. It is consumed by the next frame and never becomes page state.
    pub(crate) about_copy_success_pending: bool,
    pub(crate) model_import: ModelImportDraft,
    /// The whole-window file-drop affordance currently shown over every settings
    /// page. It is temporary view state only; the selected path is handed to the
    /// existing settings-service import contract after validation.
    pub(crate) model_drag: Option<ModelDragOverlayState>,
    pub(crate) check_for_updates_interval_debouncer: crate::SettingsPatchDebouncer<u16>,
    pub(crate) check_for_updates_interval_timer_generation: u64,
    pub(crate) overlay_scale_debouncer: crate::SettingsPatchDebouncer<u16>,
    pub(crate) overlay_scale_timer_generation: u64,
    pub(crate) overlay_opacity_debouncer: crate::SettingsPatchDebouncer<u8>,
    pub(crate) overlay_opacity_timer_generation: u64,
    pub(crate) overlay_corner_radius_debouncer: crate::SettingsPatchDebouncer<u8>,
    pub(crate) overlay_corner_radius_timer_generation: u64,
    pub(crate) overlay_hover_hide_delay_debouncer: crate::SettingsPatchDebouncer<u32>,
    pub(crate) overlay_hover_hide_delay_timer_generation: u64,
    pub(crate) gamepad_dead_zone_debouncer:
        crate::SettingsPatchDebouncer<SettingsGamepadAxisSettings>,
    pub(crate) gamepad_dead_zone_timer_generation: u64,
    pub(crate) maximum_fps_debouncer: crate::SettingsPatchDebouncer<u16>,
    pub(crate) maximum_fps_timer_generation: u64,
    pub(crate) release_fallback_timeout_debouncer: crate::SettingsPatchDebouncer<u32>,
    pub(crate) release_fallback_timeout_timer_generation: u64,
    pub(crate) random_behavior_debouncer: crate::SettingsPatchDebouncer<SettingsRandomBehavior>,
    pub(crate) random_behavior_timer_generation: u64,
    pub(crate) logging_settings_debouncer: crate::SettingsPatchDebouncer<SettingsLogging>,
    pub(crate) logging_settings_timer_generation: u64,
    pub(crate) flush_pending_requested: bool,
    pub(crate) quit_after_flush: bool,
    pub(crate) model_delete_confirmation: Option<SettingsModelKey>,
    pub(crate) model_row_focus: BTreeMap<ModelRowKey, ModelRowFocus>,
    pub(crate) model_edit: Option<ModelEditDraft>,
    /// Whether the unreadable-catalog notification has already been pushed for
    /// the current failure, so it is not repeated on every snapshot.
    pub(crate) model_catalog_error_reported: bool,
    pub(crate) shortcut_capture: Option<ShortcutCapture>,
    pub(crate) shortcut_capture_blur_subscription: Option<gpui_kit::Subscription>,
    pub(crate) shortcut_row_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    /// Focus handles of the play controls, one per row.
    ///
    /// The map covers every row, not only the model behaviors that render a
    /// play control, so the entry can be looked up by target without asking
    /// whether this row has one — the same way `shortcut_clear_focus` covers
    /// rows whose binding is empty.
    pub(crate) shortcut_play_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    pub(crate) shortcut_clear_focus: BTreeMap<ShortcutCaptureTarget, FocusHandle>,
    pub(crate) window_hidden: bool,
    pub(crate) navigation_memory: SettingsNavigationMemory,
    pub(crate) applied_theme: Option<SettingsTheme>,
    pub(crate) language_select: Entity<LanguageSelectState>,
    pub(crate) theme_select: Entity<ThemeSelectState>,
    pub(crate) logging_level_select: Entity<LoggingLevelSelectState>,
    /// The model each gamepad connection state switches to. Their option lists
    /// come from the model catalog, so they are rebuilt on every snapshot.
    pub(crate) gamepad_connected_model_select: Entity<GamepadModelSelectState>,
    pub(crate) gamepad_disconnected_model_select: Entity<GamepadModelSelectState>,
    pub(crate) request_quit: Rc<dyn Fn(&mut App)>,
    /// Opens the update window and starts a check.
    pub(crate) request_update: SettingsWindowRequest,
    pub(crate) overlay_focus: FocusHandle,
    /// Focus handle of the import card while it is the upload prompt.
    pub(crate) import_card_focus: FocusHandle,
    /// The models the current run installed whose cover capture has not
    /// finished yet.
    ///
    /// The import result lands before the product has rendered the model's own
    /// cover, so these entries are withheld from the grid: the card would
    /// otherwise flicker through the source package's placeholder picture and
    /// then swap it. An entry leaves this set when its capture reports back,
    /// whether it produced a cover or not, so a failed capture still publishes
    /// the model.
    pub(crate) pending_model_reveal: BTreeSet<ModelRowKey>,
    /// Captures that finished before the run they belong to reported its result.
    ///
    /// The capture runs on a different thread from the settings worker, so it
    /// can win the race against the import reply. Recording its completion here
    /// is what keeps the gate from waiting on a signal that already happened.
    pub(crate) completed_model_cover_captures: BTreeSet<ModelRowKey>,
    pub(crate) syncing_component_inputs: bool,
}

#[derive(Clone)]
pub struct SettingsWindowHandle {
    pub(crate) window: WindowHandle<Root>,
    pub(crate) view: WeakEntity<SettingsView>,
}

impl SettingsWindowHandle {
    pub fn read(&self, cx: &App) -> gpui_kit::Result<()> {
        self.window.read(cx)?;
        self.view
            .upgrade()
            .map(|_| ())
            .ok_or_else(|| std::io::Error::other("settings view was released").into())
    }

    /// Whether the view entity is still alive, without borrowing the app.
    pub fn is_open(&self) -> bool {
        self.view.upgrade().is_some()
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
