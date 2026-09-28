//! Retention is bounded on both axes, and touches only its own files.

use super::*;

#[test]
fn deferred_startup_retires_a_previous_day_base_only_after_policy_load() {
    let directory = tempdir().expect("log directory");
    let now = SystemTime::now();
    let current = UtcDate::from_system_time(now);
    let previous = UtcDate::from_system_time(
        now.checked_sub(Duration::from_secs(SECONDS_PER_DAY))
            .expect("previous timestamp"),
    );
    let previous_path = directory
        .path()
        .join(LogStream::Application.active_file_name(previous));
    fs::write(&previous_path, b"previous").expect("previous log");
    let writer = TextLogWriter::open_deferred(
        directory.path(),
        LogStream::Application,
        LogSettingsController::default(),
    )
    .expect("deferred writer");
    assert!(previous_path.is_file());
    writer.refresh_policy();
    assert!(!previous_path.exists());
    assert!(
        directory
            .path()
            .join(LogStream::Application.rotated_file_name(previous, 1))
            .is_file()
    );
    assert!(
        directory
            .path()
            .join(LogStream::Application.active_file_name(current))
            .is_file()
    );
}

#[test]
fn shared_retention_uses_configured_days_and_total_budget() {
    let directory = tempdir().expect("log directory");
    let old_day = UtcDate::from_system_time(
        SystemTime::now()
            .checked_sub(Duration::from_secs(3 * SECONDS_PER_DAY))
            .expect("old timestamp"),
    );
    fs::write(
        directory
            .path()
            .join(LogStream::Application.rotated_file_name(old_day, 1)),
        b"old",
    )
    .expect("old log");
    let now = SystemTime::now()
        .checked_add(Duration::from_secs(4 * SECONDS_PER_DAY))
        .expect("retention clock");
    let report = enforce_directory_retention(directory.path(), now, 1);
    assert_eq!(report.pruned, 1);
    assert_eq!(report.retained_files, 0);

    let today = UtcDate::from_system_time(now);
    for generation in 1..=MAX_TOTAL_LOG_FILES + 2 {
        fs::write(
            directory
                .path()
                .join(LogStream::CubismCore.rotated_file_name(today, generation)),
            b"x",
        )
        .expect("rotated log");
    }
    let report = enforce_directory_retention(directory.path(), now, 7);
    assert!(report.retained_files <= MAX_TOTAL_LOG_FILES);
    assert!(report.retained_bytes <= MAX_TOTAL_LOG_BYTES);
}

#[test]
fn unknown_files_and_symlinks_stay_outside_retention() {
    let directory = tempdir().expect("log directory");
    let unknown = directory.path().join("user-data.bin");
    let impossible_date = directory.path().join("application-2026-02-30.log");
    fs::write(&unknown, vec![b'u'; MAX_TOTAL_LOG_BYTES as usize]).expect("unknown");
    fs::write(&impossible_date, b"user data").expect("impossible date");
    let report = enforce_directory_retention(directory.path(), SystemTime::now(), 7);
    assert!(unknown.exists());
    assert!(impossible_date.exists());
    assert_eq!(report.retained_bytes, 0);

    #[cfg(unix)]
    {
        let date = UtcDate::from_system_time(SystemTime::now());
        let symlink = directory
            .path()
            .join(LogStream::Application.rotated_file_name(date, 1));
        std::os::unix::fs::symlink(&unknown, &symlink).expect("symlink");
        let report = enforce_directory_retention(directory.path(), SystemTime::now(), 7);
        assert!(
            symlink
                .symlink_metadata()
                .expect("metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(report.retained_bytes, 0);
    }
}
