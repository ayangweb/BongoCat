//! One read of everything the settings window renders.
//!
//! The snapshot is the only thing the window reads. It carries a revision so a
//! poller can ask "did anything change" without paying for the model catalog
//! scan a full read performs, which is why the two are separate commands.

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsTheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutomaticUpdateSettings {
    pub enabled: bool,
    pub interval_hours: u16,
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
    /// The macOS Dock icon, projected from `system.show_dock_icon`. It travels
    /// beside `taskbar_icon_visible` rather than inside it because the two
    /// answer different platforms' shell surfaces, and each is only meaningful
    /// on its own.
    pub dock_icon_visible: bool,
    pub check_for_updates_automatically: bool,
    pub check_for_updates_interval_hours: u16,
    pub overlay_visible: bool,
    pub overlay: SettingsOverlay,
    pub motion_audio_enabled: bool,
    /// Whether the application command bindings are allowed to reach the
    /// platform shortcut table. The shortcuts page renders it as the "disable
    /// window shortcuts" switch above the command rows.
    pub command_shortcuts_enabled: bool,
    pub behavior_shortcuts_enabled: bool,
    pub maximum_fps: u16,
    pub random_behavior: SettingsRandomBehavior,
    pub model_settings: SettingsModelSettings,
    pub gamepad_axis_settings: SettingsGamepadAxisSettings,
    pub cursor_settings: SettingsCursorSettings,
    /// The configured gamepad-connection model switch. The settings service is
    /// what acts on it, so the view only reads the gate and the two targets.
    pub gamepad_auto_switch: SettingsGamepadAutoSwitch,
    /// Whether a model returns to the expression the user last chose for it.
    ///
    /// The remembered expressions are not shown: the switch is the whole of the
    /// decision a user makes, and which expression each model happens to be
    /// holding is a fact about what they used rather than a setting.
    pub remember_last_expression: bool,
    /// Whether triggering the expression already showing turns it off.
    ///
    /// A separate switch from the remembered-expression one because the two are
    /// orthogonal: remembering an expression says where a model starts, and this
    /// says how a repeat trigger behaves while it is already wearing one. Neither
    /// expression a model currently shows is shown here, for the same reason the
    /// remembered ones are not.
    pub toggle_repeated_expression: bool,
    pub logging: SettingsLogging,
    pub shortcuts: SettingsShortcuts,
    /// What each of the active model's motions and expressions is called.
    ///
    /// Only the named ones, for the active model only. A behavior with no row here is
    /// one the user has not renamed, and the window draws its numbered label — so the
    /// absence of a name is a state the page renders rather than an error.
    pub model_behavior_names: Vec<SettingsModelBehaviorName>,
    pub startup_item: SettingsStartupItemStatus,
    pub diagnostics_export: Option<SettingsDiagnosticsExportStatus>,
    pub input_diagnostics: SettingsInputDiagnostics,
    pub active_model: Option<SettingsModelKey>,
    pub model_catalog: SettingsModelCatalog,
}

/// One behavior of the active model, with the name the user gave it.
///
/// The window resolves the label from this rather than asking the service for a
/// rendered string, so the same text appears in every place a row is drawn — the
/// shortcut row and the random-playback checkbox share it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelBehaviorName {
    /// The `behavior_id` spelling the configuration persists it under.
    pub behavior_id: String,
    pub name: String,
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
    pub model: SettingsModelKey,
    pub behavior_id: String,
    pub shortcut: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsModelSettings {
    pub mirror: bool,
    pub mirror_pointer_tracking_horizontal: bool,
    pub mirror_pointer_tracking_vertical: bool,
    pub ignore_keyboard: bool,
    pub ignore_gamepad: bool,
    /// Whether the key-image layer stacks every held key with artwork instead of
    /// drawing one image per hand.
    pub show_all_pressed_keys: bool,
    pub ignore_pointer: bool,
}

/// What the idle scheduler may pick on its own, or that it does nothing.
///
/// The catalogue lives in the protocol rather than in the window, the same reason
/// [`SettingsLogLevel`] does: the application is the one that has to obey the
/// choice, so a mode the window could render but the application did not know
/// would be a control that appears to work and does nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsRandomBehaviorMode {
    #[default]
    Off,
    Expressions,
    Motions,
    MotionsAndExpressions,
}

impl SettingsRandomBehaviorMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Expressions,
        Self::Motions,
        Self::MotionsAndExpressions,
    ];

    /// Whether this mode schedules anything at all.
    ///
    /// The window uses it to decide whether the interval row below the dropdown
    /// is live, so the rule that a gated control is inert when its gate is off is
    /// written once here rather than as a `!= Off` comparison at each use.
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsRandomBehavior {
    pub mode: SettingsRandomBehaviorMode,
    pub interval_seconds: u32,
}

impl Default for SettingsRandomBehavior {
    fn default() -> Self {
        Self {
            mode: SettingsRandomBehaviorMode::default(),
            interval_seconds: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsGamepadAxisSettings {
    pub stick_dead_zone_percent: u8,
    pub trigger_dead_zone_percent: u8,
}

/// How the pointer is read before any model sees it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsCursorSettings {
    /// Follow relative device motion instead of the absolute cursor position,
    /// so the model keeps following the pointer in applications that capture it.
    pub force_move: bool,
}

/// Choosing the shown model from gamepad connection state.
///
/// `connected_model` is the model the product shows while at least one gamepad
/// is connected and `disconnected_model` the one it shows while none is; `None`
/// leaves the shown model alone in that direction. The two are complete model
/// identities, so a target survives a restart and a rename.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsGamepadAutoSwitch {
    pub enabled: bool,
    pub connected_model: Option<SettingsModelKey>,
    pub disconnected_model: Option<SettingsModelKey>,
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
    /// The physical modifier key whose hold gives the pointer back to the user.
    ///
    /// The overlay stops passing pointer events through and stops hiding while
    /// this key is down, so a cat that is configured to be unreachable can still
    /// be moved. `None` means no key does this.
    pub hold_modifier_to_interact: Option<ModifierKey>,
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
    /// Hide the overlay after the mouse, keyboard and gamepad stay untouched.
    pub hide_on_idle: bool,
    /// How long input may stay untouched before the idle hide starts, in whole
    /// seconds. `0` hides as soon as input stops.
    pub hide_on_idle_delay_seconds: u32,
    /// Keep the overlay fully on a display. The window is allowed over a
    /// taskbar, Dock or menu bar, and a window dragged off the desktop returns
    /// after the drag ends rather than being pulled back mid-drag.
    pub keep_inside_screen: bool,
}

impl Default for SettingsOverlay {
    fn default() -> Self {
        Self {
            click_through: false,
            hold_modifier_to_interact: None,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            corner_radius_percent: 0,
            hide_on_pointer_hover: false,
            hide_on_pointer_hover_delay_seconds: 0,
            hide_on_idle: false,
            hide_on_idle_delay_seconds: 10,
            keep_inside_screen: true,
        }
    }
}
