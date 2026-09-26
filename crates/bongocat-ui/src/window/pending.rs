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
    AutomaticUpdateCheck,
    CheckForUpdatesIntervalHours,
    LoggingSettings,
    OverlayVisibility,
    OverlaySettings,
    OverlayScale,
    OverlayOpacity,
    OverlayCornerRadius,
    OverlayHoverHideDelay,
    MotionAudio,
    CommandShortcuts,
    BehaviorShortcuts,
    RandomBehavior,
    MaximumFps,
    ReleaseFallbackTimeout,
    ModelSettings,
    GamepadAxisSettings,
    GamepadAutoSwitch,
    StartupItem,
    ModelSelection,
    ModelDeletion,
    ModelMetadata,
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
    GamepadAutoSwitch {
        expected_config_revision: u64,
        settings: SettingsGamepadAutoSwitch,
    },
    StartupItemEnabled(bool),
    Shortcuts {
        expected_config_revision: u64,
        shortcuts: SettingsShortcuts,
    },
}
