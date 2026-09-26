//! The writer's file, filter, rollover and size guard.

use super::*;

#[test]
fn writer_creates_a_dated_log_and_honors_the_shared_filter() {
    let directory = tempdir().expect("log directory");
    let controller = LogSettingsController::default();
    let writer = TextLogWriter::open(directory.path(), LogStream::Application, controller.clone())
        .expect("writer");
    let now = SystemTime::now();
    let date = UtcDate::from_system_time(now);
    let error = LogRecord::new(
        now,
        LogLevel::Error,
        "application",
        "application/failed",
        "A key operation failed",
    );
    assert!(writer.record(error).expect("error record"));
    assert!(
        !writer
            .record(LogRecord::new(
                now,
                LogLevel::Debug,
                "runtime",
                "runtime/prepared",
                "Candidate prepared",
            ))
            .expect("filtered record")
    );

    controller.replace_settings(LogSettings {
        level: LogLevel::Debug,
        retention_days: 7,
    });
    assert!(
        writer
            .record(LogRecord::new(
                now,
                LogLevel::Debug,
                "runtime",
                "runtime/prepared",
                "Candidate prepared",
            ))
            .expect("debug record")
    );

    let path = directory
        .path()
        .join(LogStream::Application.active_file_name(date));
    let mut contents = String::new();
    File::open(path)
        .expect("open log")
        .read_to_string(&mut contents)
        .expect("read log");
    assert!(contents.contains("ERROR [application] application/failed"));
    assert!(contents.contains("DEBUG [runtime] runtime/prepared"));
    assert!(!contents.contains('{'));
    assert_eq!(writer.stats().written, 2);
}

#[test]
fn application_and_core_writers_share_one_live_policy() {
    let directory = tempdir().expect("log directory");
    let controller = LogSettingsController::default();
    let application =
        TextLogWriter::open(directory.path(), LogStream::Application, controller.clone())
            .expect("application writer");
    let core = TextLogWriter::open(directory.path(), LogStream::CubismCore, controller.clone())
        .expect("core writer");
    let now = SystemTime::now();

    controller.replace_settings(LogSettings {
        level: LogLevel::Debug,
        retention_days: 14,
    });
    for (stream, writer) in [
        (LogStream::Application, &application),
        (LogStream::CubismCore, &core),
    ] {
        assert!(
            writer
                .record(LogRecord::new(
                    now,
                    LogLevel::Debug,
                    stream.prefix(),
                    "test/shared_policy",
                    "Debug record",
                ))
                .expect("shared debug policy")
        );
    }

    controller.replace_settings(LogSettings {
        level: LogLevel::Error,
        retention_days: 1,
    });
    for writer in [&application, &core] {
        assert!(
            !writer
                .record(LogRecord::new(
                    now,
                    LogLevel::Warn,
                    "test",
                    "test/shared_policy",
                    "Filtered record",
                ))
                .expect("shared error policy")
        );
    }
    assert_eq!(controller.settings().retention_days, 1);
}

#[test]
fn daily_rollover_keeps_the_previous_day_as_a_numbered_log() {
    let directory = tempdir().expect("log directory");
    let controller = LogSettingsController::default();
    let writer =
        TextLogWriter::open(directory.path(), LogStream::Application, controller).expect("writer");
    let now = SystemTime::now();
    let next_day = now
        .checked_add(Duration::from_secs(SECONDS_PER_DAY))
        .expect("next day");
    writer
        .record(LogRecord::new(
            now,
            LogLevel::Info,
            "application",
            "application/first",
            "First day",
        ))
        .expect("first");
    writer
        .record(LogRecord::new(
            next_day,
            LogLevel::Info,
            "application",
            "application/second",
            "Second day",
        ))
        .expect("second");

    let first = UtcDate::from_system_time(now);
    let second = UtcDate::from_system_time(next_day);
    assert!(
        directory
            .path()
            .join(LogStream::Application.rotated_file_name(first, 1))
            .is_file()
    );
    assert!(
        directory
            .path()
            .join(LogStream::Application.active_file_name(second))
            .is_file()
    );
    assert!(writer.stats().rotated >= 1);
}

#[test]
fn per_file_size_guard_creates_a_bounded_segment() {
    let directory = tempdir().expect("log directory");
    let writer = TextLogWriter::open(
        directory.path(),
        LogStream::Application,
        LogSettingsController::default(),
    )
    .expect("writer");
    let now = SystemTime::now();
    let message = "x".repeat(MAXIMUM_MESSAGE_BYTES - TRUNCATION_MARKER.len());
    for _ in 0..2_100 {
        assert!(
            writer
                .record(LogRecord::new(
                    now,
                    LogLevel::Info,
                    "runtime",
                    "runtime/test",
                    message.clone(),
                ))
                .expect("record")
        );
    }
    let date = UtcDate::from_system_time(now);
    assert!(
        directory
            .path()
            .join(LogStream::Application.rotated_file_name(date, 1))
            .is_file()
    );
    for entry in fs::read_dir(directory.path()).expect("read directory") {
        let metadata = entry.expect("entry").metadata().expect("metadata");
        assert!(metadata.len() <= MAX_LOG_FILE_BYTES);
    }
}
