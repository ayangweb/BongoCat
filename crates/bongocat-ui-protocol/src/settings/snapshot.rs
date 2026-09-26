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
    pub release_fallback_timeout_ms: u32,
    pub random_behavior: SettingsRandomBehavior,
    pub model_settings: SettingsModelSettings,
    pub gamepad_axis_settings: SettingsGamepadAxisSettings,
    /// The configured gamepad-connection model switch. The settings service is
    /// what acts on it, so the view only reads the gate and the two targets.
    pub gamepad_auto_switch: SettingsGamepadAutoSwitch,
    pub logging: SettingsLogging,
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
    pub model: SettingsModelKey,
    pub behavior_id: String,
    pub shortcut: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsModelSettings {
    pub mirror: bool,
    pub mirror_pointer_tracking: bool,
    pub ignore_keyboard: bool,
    pub ignore_gamepad: bool,
    pub ignore_pointer: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsRandomBehavior {
    pub enabled: bool,
    pub interval_seconds: u32,
}

impl Default for SettingsRandomBehavior {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsGamepadAxisSettings {
    pub stick_dead_zone_percent: u8,
    pub trigger_dead_zone_percent: u8,
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
