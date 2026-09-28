//! A line is written once, readable, and never as a whole log.

use super::*;

#[test]
fn writes_a_single_human_readable_line_with_stable_code() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    handle.record(ApplicationLogEvent::started());
    let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
    assert!(contents.contains(" INFO  [application] application/started | Application started\n"));
    assert!(!contents.contains('{'));
    assert_eq!(handle.diagnostics().written, 1);
}

#[test]
fn level_filter_and_shared_controller_update_are_applied_without_restart() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install_with_settings(
        directory.path(),
        LogSettings {
            level: LogLevel::Error,
            retention_days: 30,
        },
        false,
    )
    .expect("application log");
    handle.record(ApplicationLogEvent::started());
    assert_eq!(handle.diagnostics().written, 0);
    assert_eq!(handle.settings().retention_days, 30);

    handle.replace_settings(LogSettings {
        level: LogLevel::Debug,
        retention_days: 7,
    });
    handle.record(ApplicationLogEvent::started());
    let diagnostics = handle.diagnostics();
    let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
    assert_eq!(diagnostics.written, 2, "{contents}");
    assert_eq!(diagnostics.events.started, 1, "{contents}");
    assert!(contents.contains("logging/settings_changed | Logging settings changed"));
}

#[test]
fn record_once_does_not_consume_a_filtered_event_before_a_later_policy_change() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install_with_settings(
        directory.path(),
        LogSettings {
            level: LogLevel::Error,
            retention_days: 7,
        },
        false,
    )
    .expect("application log");
    handle.record_once(ApplicationLogEvent::started());
    assert_eq!(handle.diagnostics().written, 0);
    handle.replace_settings(LogSettings {
        level: LogLevel::Info,
        retention_days: 7,
    });
    handle.record_once(ApplicationLogEvent::started());
    let diagnostics = handle.diagnostics();
    assert_eq!(diagnostics.written, 2, "settings event plus start event");
    assert_eq!(diagnostics.events.started, 1);
}

#[test]
fn rejects_invalid_directory_without_panicking() {
    let directory = tempdir().expect("temporary directory");
    let file = directory.path().join("not-a-directory");
    fs::write(&file, b"occupied").expect("occupied path");
    assert!(matches!(
        ApplicationLogHandle::install(&file),
        Err(ApplicationLogError::CreateDirectory(_))
    ));
}

#[cfg(unix)]
#[test]
fn application_logs_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    handle.record(ApplicationLogEvent::started());
    let log_path = only_log(directory.path());
    assert_eq!(
        fs::metadata(directory.path())
            .expect("log directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(log_path)
            .expect("active log metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let (marker, _) = handle.begin_run().expect("run marker");
    let marker_path = directory.path().join(RUN_MARKER_NAME);
    assert_eq!(
        fs::metadata(marker_path)
            .expect("marker metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    marker.complete().expect("complete run");
}
