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
