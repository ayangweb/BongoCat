//! The codes are stable, unique, and readable.

use super::*;

#[test]
fn settings_error_codes_are_stable_and_unique() {
    let mut codes = SettingsErrorCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.iter().all(|code| !code.is_empty()));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), SettingsErrorCode::ALL.len());
    assert_eq!(
        SettingsErrorCode::SnapshotOutdated.as_str(),
        "snapshot_outdated"
    );
    let mut runtime_codes = SettingsRuntimeErrorCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>();
    runtime_codes.sort_unstable();
    runtime_codes.dedup();
    assert_eq!(runtime_codes.len(), SettingsRuntimeErrorCode::ALL.len());
    assert!(!SettingsRandomBehavior::default().enabled);
    assert_eq!(SettingsRandomBehavior::default().interval_seconds, 30);
    assert_eq!(
        SettingsError::new(SettingsErrorCode::ModelSwitchFailed).code(),
        SettingsErrorCode::ModelSwitchFailed
    );
}

#[test]
fn config_write_errors_are_actionable_and_anonymous() {
    for (code, expected) in [
        (
            SettingsErrorCode::SnapshotOutdated,
            "Settings changed elsewhere. Review the latest settings and try again.",
        ),
        (
            SettingsErrorCode::ConfigPermissionDenied,
            "The configuration file cannot be written; check permissions and retry",
        ),
        (
            SettingsErrorCode::ConfigStorageFull,
            "The disk holding the configuration is full; free space and retry",
        ),
        (
            SettingsErrorCode::ConfigTargetOccupied,
            "The configuration location is in use; close the program using it and retry",
        ),
    ] {
        let message = SettingsError::new(code).to_string();
        assert_eq!(message, expected);
        assert!(!message.contains('/') && !message.contains('\\'));
    }
}
