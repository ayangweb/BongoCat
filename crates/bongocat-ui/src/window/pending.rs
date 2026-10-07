//! A change the window has accepted but not yet sent.
//!
//! A control that writes straight through would send a command per keystroke and
//! per drag step, so a control records what it wants in a `SettingValue` and the
//! window sends it once the user stops. The value carries the config revision it
//! was formed against, because a write that lands after someone else changed the
//! configuration has to be refused rather than silently overwrite it.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingOperation {
    Refresh,
    AppearanceTheme,
    Language,
    StatusIconVisibility,
    #[cfg(target_os = "windows")]
    TaskbarIconVisibility,
    #[cfg(target_os = "macos")]
    DockIconVisibility,
    AutomaticUpdateCheck,
    CheckForUpdatesIntervalHours,
    LoggingSettings,
    OverlayVisibility,
    OverlaySettings,
    OverlayScale,
    OverlayOpacity,
    OverlayCornerRadius,
    OverlayHoverHideDelay,
    OverlayIdleHideDelay,
    MotionAudio,
    CommandShortcuts,
    BehaviorShortcuts,
    RandomBehavior,
    MaximumFps,
    ModelSettings,
    GamepadAxisSettings,
    CursorSettings,
    GamepadAutoSwitch,
    RememberLastExpression,
    ToggleRepeatedExpression,
    StartupItem,
    ModelSelection,
    ModelDeletion,
    ModelMetadata,
    ModelBehaviorName,
    ModelLocation,
    OpenLogsLocation,
    SetShortcuts,
    BeginShortcutCapture,
    CancelShortcutCapture,
}

#[derive(Clone)]
pub(crate) enum SettingValue {
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
    #[cfg(target_os = "macos")]
    DockIconVisible {
        expected_config_revision: u64,
        visible: bool,
    },
    CheckForUpdatesAutomatically {
        expected_config_revision: u64,
        enabled: bool,
    },
    CheckForUpdatesIntervalHours {
        expected_config_revision: u64,
        interval_hours: u16,
    },
    LoggingSettings {
        expected_config_revision: u64,
        settings: SettingsLogging,
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
    OverlayIdleHideDelay {
        expected_config_revision: u64,
        hide_on_idle_delay_seconds: u32,
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
    RandomBehaviorSettings {
        expected_config_revision: u64,
        settings: SettingsRandomBehavior,
    },
    MaximumFps {
        expected_config_revision: u64,
        maximum_fps: u16,
    },
    ModelSettings {
        expected_config_revision: u64,
        settings: SettingsModelSettings,
    },
    GamepadAxisSettings {
        expected_config_revision: u64,
        settings: SettingsGamepadAxisSettings,
    },
    CursorSettings {
        expected_config_revision: u64,
        settings: SettingsCursorSettings,
    },
    GamepadAutoSwitch {
        expected_config_revision: u64,
        settings: SettingsGamepadAutoSwitch,
    },
    RememberLastExpression {
        expected_config_revision: u64,
        enabled: bool,
    },
    ToggleRepeatedExpression {
        expected_config_revision: u64,
        enabled: bool,
    },
    ModelBehaviorName {
        expected_config_revision: u64,
        model: SettingsModelKey,
        behavior_id: String,
        name: String,
    },
    StartupItemEnabled(bool),
    Shortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    },
}
