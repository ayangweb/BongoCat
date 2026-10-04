//! What one read of the configuration looks like.

use super::*;

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
fn cursor_settings_default_to_the_absolute_cursor() {
    assert_eq!(
        SettingsCursorSettings::default(),
        SettingsCursorSettings { force_move: false }
    );
}
