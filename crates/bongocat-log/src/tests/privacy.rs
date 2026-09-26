//! What a log writes about the user, and with which permissions.

#[cfg(unix)]
#[test]
fn created_logs_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("log directory");
    let writer = TextLogWriter::open(
        directory.path(),
        LogStream::Application,
        LogSettingsController::default(),
    )
    .expect("writer");
    writer
        .record(LogRecord::new(
            SystemTime::now(),
            LogLevel::Info,
            "application",
            "application/started",
            "Started",
        ))
        .expect("record");
    let date = UtcDate::from_system_time(SystemTime::now());
    let mode = fs::metadata(
        directory
            .path()
            .join(LogStream::Application.active_file_name(date)),
    )
    .expect("metadata")
    .permissions()
    .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
