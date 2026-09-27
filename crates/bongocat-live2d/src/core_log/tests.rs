//! The bridge's tests: what a Cubism line may say, and what the callback does
//! when the process is busy, saturated, or already stopped.
//!
//! They share a lock because Cubism's callback is process-wide: two tests with
//! it installed at once would measure each other's writes, and the saturation
//! and drop counters are per-slot rather than per-test.

use super::*;

use bongocat_log::LogSettings;
use std::ffi::CString;
use tempfile::tempdir;

static CORE_LOG_INSTALL_TEST_LOCK: Mutex<()> = Mutex::new(());

fn debug_settings() -> LogSettingsController {
    LogSettingsController::new(LogSettings {
        level: LogLevel::Debug,
        retention_days: bongocat_log::DEFAULT_RETENTION_DAYS,
    })
}

fn core_log_path(directory: &Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(directory)
        .expect("read Core logs")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("cubism-core-") && name.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.pop().expect("Core log")
}

#[test]
fn sanitizes_paths_and_bounds_message_bytes() {
    let message =
        sanitize_message(b"model /Users/example/private\nC:\\Users\\name\\model.moc3 stable");
    assert_eq!(message, "model <redacted> <redacted> stable");
    assert!(sanitize_message(&vec![b'x'; MAX_MESSAGE_BYTES + 20]).len() <= MAX_MESSAGE_BYTES);
    assert_eq!(
        sanitize_message(b"https://example.invalid/private"),
        "<redacted>"
    );
    assert_eq!(sanitize_message(b"token=secret"), "<redacted>");
    assert_eq!(sanitize_message(b"token secret"), "<redacted>");
    assert_eq!(
        sanitize_message(b"Authorization: Bearer abc123"),
        "<redacted>"
    );
    assert_eq!(sanitize_message(b"Permission denied"), "<redacted>");
    assert_eq!(sanitize_message(b"clipboard"), "<redacted>");
}

#[test]
fn default_info_filter_keeps_untyped_core_messages_out_of_the_user_log() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle = CoreLogHandle::install(directory.path(), LogSettingsController::default())
        .expect("install Core logger");
    let message = CString::new("routine Core message").expect("message");
    // SAFETY: the CString is null-terminated and remains alive for this
    // synchronous callback invocation.
    unsafe { core_log_callback(message.as_ptr()) };
    drop(handle);
    let contents = fs::read_to_string(core_log_path(directory.path())).expect("read log");
    assert!(!contents.contains("routine Core message"));
}

#[test]
fn installed_callback_writes_one_sanitized_debug_line_and_is_removed_on_drop() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle =
        CoreLogHandle::install(directory.path(), debug_settings()).expect("install Core logger");
    let message = CString::new("Core warning /private/model.moc3").expect("message");
    // SAFETY: the CString is null-terminated and remains alive for the
    // synchronous callback invocation.
    unsafe { core_log_callback(message.as_ptr()) };
    wait_for_written(&handle, 1);
    let path = core_log_path(directory.path());
    assert_eq!(handle.stats().retained_files, 1);
    let contents = fs::read_to_string(&path).expect("read log");
    assert!(contents.contains("DEBUG [cubism.core] cubism/core/message | Core warning <redacted>"));
    assert!(!contents.contains("/private/model.moc3"));
    drop(handle);
    assert!(
        sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    );
}

#[test]
fn callback_drops_when_the_global_slot_is_busy_without_blocking() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle =
        CoreLogHandle::install(directory.path(), debug_settings()).expect("install Core logger");
    let message = CString::new("callback slot contention").expect("message");
    let slot = sink_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: the CString is null-terminated and remains alive for this
    // synchronous callback invocation.
    unsafe { core_log_callback(message.as_ptr()) };
    drop(slot);
    assert_eq!(handle.stats().written, 0);
    assert_eq!(handle.stats().dropped, 1);
}

#[test]
fn callback_queue_saturation_is_counted_and_shutdown_drains_accepted_records() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle =
        CoreLogHandle::install(directory.path(), debug_settings()).expect("install Core logger");
    let path = core_log_path(directory.path());
    let message = CString::new("queue saturation").expect("message");
    let state = handle
        .sink
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The worker may already have dequeued one message and be waiting for
    // this same state lock, so exceed both the in-flight slot and queue.
    for _ in 0..=(CALLBACK_QUEUE_CAPACITY + 1) {
        // SAFETY: the CString is null-terminated and remains alive for
        // each synchronous callback invocation.
        unsafe { core_log_callback(message.as_ptr()) };
    }
    drop(state);
    assert!(handle.stats().dropped >= 1);
    drop(handle);
    let contents = fs::read_to_string(path).expect("drained log");
    assert!(contents.contains("queue saturation"));
}

#[test]
fn callback_after_stop_is_counted_without_writing() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle =
        CoreLogHandle::install(directory.path(), debug_settings()).expect("install Core logger");
    handle.sink.accepting.store(false, Ordering::Release);
    let message = CString::new("late callback").expect("message");
    // SAFETY: the CString is null-terminated and remains alive for this
    // synchronous callback invocation.
    unsafe { core_log_callback(message.as_ptr()) };
    assert_eq!(handle.stats().written, 0);
    assert_eq!(handle.stats().dropped, 1);
}

#[test]
fn stats_refresh_after_another_writer_prunes_a_rotated_file() {
    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let handle =
        CoreLogHandle::install(directory.path(), debug_settings()).expect("install Core logger");
    let active = core_log_path(directory.path());
    let stem = active
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("Core log stem");
    let rotated = directory.path().join(format!("{stem}.1.log"));
    fs::write(&rotated, b"rotated").expect("seed rotated log");
    assert_eq!(handle.stats().retained_files, 2);
    fs::remove_file(rotated).expect("prune rotated log");
    assert_eq!(handle.stats().retained_files, 1);
    assert_eq!(
        handle.stats().retained_bytes,
        fs::metadata(active).expect("active metadata").len()
    );
}

fn wait_for_written(handle: &CoreLogHandle, expected: u64) {
    for _ in 0..100 {
        if handle.stats().written >= expected {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("Core log worker did not write {expected} records");
}

#[cfg(unix)]
#[test]
fn core_log_storage_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempdir().expect("temporary directory");
    let logs = directory.path().join("logs");
    let handle = CoreLogHandle::install(&logs, debug_settings()).expect("install Core logger");
    let path = core_log_path(&logs);
    assert_eq!(
        fs::metadata(&logs)
            .expect("log directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&path)
            .expect("log file metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(handle);
}
