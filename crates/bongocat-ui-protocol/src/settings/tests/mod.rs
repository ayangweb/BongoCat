//! The settings contract's tests, split by the module they cover.
//!
//! The commands are tested by sending them through a real bounded channel
//! rather than by calling the service, so the test is the same round trip the
//! window makes.

use super::*;

use std::thread;

/// A snapshot with the fields a settings test does not care about left at
/// their defaults. `pub(crate)` so the settings-window tests can seed a view
/// with one catalog instead of restating every field.
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
            cubism_core_version: "6.0.1".to_owned(),
        },
        runtime_health: RuntimeHealth::Ready,
        runtime_diagnostics: SettingsRuntimeDiagnostics::default(),
        appearance_theme: SettingsTheme::System,
        language: SettingsLanguage::System,
        resolved_language: SettingsLanguage::English,
        status_icon_visible: true,
        taskbar_icon_visible: true,
        dock_icon_visible: false,
        check_for_updates_automatically: false,
        check_for_updates_interval_hours: 24,
        overlay_visible,
        overlay: SettingsOverlay::default(),
        motion_audio_enabled,
        command_shortcuts_enabled: true,
        behavior_shortcuts_enabled: true,
        maximum_fps: 60,
        random_behavior: SettingsRandomBehavior::default(),
        model_settings: SettingsModelSettings::default(),
        gamepad_axis_settings: SettingsGamepadAxisSettings::default(),
        cursor_settings: SettingsCursorSettings::default(),
        gamepad_auto_switch: SettingsGamepadAutoSwitch::default(),
        remember_last_expression: false,
        toggle_repeated_expression: false,
        logging: SettingsLogging::default(),
        shortcuts: SettingsShortcuts::default(),
        model_behavior_names: Vec::new(),
        startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
        diagnostics_export: None,
        input_diagnostics: SettingsInputDiagnostics::default(),
        active_model: Some(SettingsModelKey {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::BuiltIn,
        }),
        model_catalog: SettingsModelCatalog::default(),
    }
}

mod command;
mod error;
mod logging;
mod model_import;
mod service;
mod snapshot;
mod startup;
mod window;
