//! The level catalogue matches what the application obeys.

use super::*;

#[test]
fn logging_settings_default_and_level_catalog_match_the_native_contract() {
    assert_eq!(
        SettingsLogging::default(),
        SettingsLogging {
            level: SettingsLogLevel::Info,
            retention_days: 7,
        }
    );
    assert_eq!(
        SettingsLogLevel::ALL.map(SettingsLogLevel::as_str),
        ["error", "warn", "info", "debug", "trace"]
    );
}
