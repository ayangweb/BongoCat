#![allow(unsafe_code)]

use crate::sys;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_LOG_FILES: u32 = 8;
const MAX_ROTATED_LOG_FILES: u32 = MAX_TOTAL_LOG_FILES - 1;
const MAX_MESSAGE_BYTES: usize = 512;
const CALLBACK_QUEUE_CAPACITY: usize = 128;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const RETENTION_DAYS: u64 = 7;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug)]
pub enum CoreLogError {
    CreateDirectory(io::Error),
    OpenFile(io::Error),
    StartWorker(io::Error),
}

impl std::fmt::Display for CoreLogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectory(error) => {
                write!(formatter, "cannot create Core log directory: {error}")
            }
            Self::OpenFile(error) => write!(formatter, "cannot open Core log file: {error}"),
            Self::StartWorker(error) => write!(formatter, "cannot start Core log worker: {error}"),
        }
    }
}

impl std::error::Error for CoreLogError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogStats {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
}

#[derive(Debug)]
struct CoreLogState {
    file: Option<File>,
    path: PathBuf,
    bytes: u64,
    stats: CoreLogStats,
}

#[derive(Clone, Copy)]
struct CoreLogMessage {
    bytes: [u8; MAX_MESSAGE_BYTES],
    length: usize,
}

#[derive(Debug)]
struct CoreLogSink {
    state: Mutex<CoreLogState>,
    sender: SyncSender<CoreLogMessage>,
    accepting: AtomicBool,
    callback_dropped: AtomicU64,
    global_drop_baseline: u64,
}

#[derive(Serialize)]
struct CoreLogRecord<'a> {
    component: &'static str,
    level: &'static str,
    message: &'a str,
}

static CORE_LOG_SINK: OnceLock<Mutex<Option<Arc<CoreLogSink>>>> = OnceLock::new();
static CORE_LOG_CALLBACK_DROPS: AtomicU64 = AtomicU64::new(0);

fn sink_slot() -> &'static Mutex<Option<Arc<CoreLogSink>>> {
    CORE_LOG_SINK.get_or_init(|| Mutex::new(None))
}

/// Owns the process-wide Cubism Core callback installation.
///
/// Cubism exposes one global callback and no user-data pointer. The handle
/// keeps the sink alive until it is dropped and removes the callback before
/// releasing the sink, so the FFI callback cannot observe freed Rust state.
#[derive(Debug)]
pub struct CoreLogHandle {
    sink: Arc<CoreLogSink>,
    path: PathBuf,
    worker: Option<JoinHandle<()>>,
}

/// Read-only access to the anonymous retention counters maintained by the
/// process-wide Core log callback.
#[derive(Clone, Debug)]
pub struct CoreLogReporter {
    sink: Arc<CoreLogSink>,
}

impl CoreLogHandle {
    pub fn install(path: impl AsRef<Path>) -> Result<Self, CoreLogError> {
        let path = path.as_ref().to_owned();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(CoreLogError::CreateDirectory)?;
            set_private_directory(parent).map_err(CoreLogError::CreateDirectory)?;
        }
        let pruned = prune_expired_rotated_logs(&path, SystemTime::now());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(CoreLogError::OpenFile)?;
        set_private_file(&file).map_err(CoreLogError::OpenFile)?;
        let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let (sender, receiver) = sync_channel(CALLBACK_QUEUE_CAPACITY);
        let sink = Arc::new(CoreLogSink {
            state: Mutex::new(CoreLogState {
                file: Some(file),
                path: path.clone(),
                bytes,
                stats: CoreLogStats {
                    pruned,
                    bytes,
                    retained_files: retained_log_files(&path),
                    ..CoreLogStats::default()
                },
            }),
            sender,
            accepting: AtomicBool::new(true),
            callback_dropped: AtomicU64::new(0),
            global_drop_baseline: CORE_LOG_CALLBACK_DROPS.load(Ordering::Relaxed),
        });
        let worker_sink = Arc::clone(&sink);
        let worker = thread::Builder::new()
            .name("bongocat-core-log".to_owned())
            .spawn(move || worker_sink.run_worker(receiver))
            .map_err(CoreLogError::StartWorker)?;
        let mut slot = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Core has one callback slot. Replacing an existing sink is safe only
        // after the old callback is disabled and its Arc is removed.
        // SAFETY: the callback is process-global and no Rust pointer is passed
        // through it; setting it to None prevents future callback entry.
        unsafe { sys::csmSetLogFunction(None) };
        *slot = Some(Arc::clone(&sink));
        // SAFETY: `core_log_callback` has the ABI generated for this exact
        // Cubism Core header and never lets a panic cross the FFI boundary.
        unsafe { sys::csmSetLogFunction(Some(core_log_callback)) };
        drop(slot);
        Ok(Self {
            sink,
            path,
            worker: Some(worker),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn stats(&self) -> CoreLogStats {
        self.sink.stats()
    }

    pub fn reporter(&self) -> CoreLogReporter {
        CoreLogReporter {
            sink: Arc::clone(&self.sink),
        }
    }
}

impl CoreLogReporter {
    pub fn stats(&self) -> CoreLogStats {
        self.sink.stats()
    }
}

impl Drop for CoreLogHandle {
    fn drop(&mut self) {
        let mut slot = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.sink))
        {
            // SAFETY: disabling the callback happens before the final Arc is
            // released, so Core cannot enter Rust with a dangling sink.
            unsafe { sys::csmSetLogFunction(None) };
            *slot = None;
        }
        drop(slot);
        self.sink.accepting.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

unsafe extern "C" fn core_log_callback(message: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if message.is_null() {
            return;
        }
        let sink = match sink_slot().try_lock() {
            Ok(slot) => slot.as_ref().cloned(),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner().as_ref().cloned(),
            Err(std::sync::TryLockError::WouldBlock) => {
                CORE_LOG_CALLBACK_DROPS.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        let Some(sink) = sink else {
            return;
        };
        // SAFETY: Cubism documents a valid null-terminated message for the
        // callback duration. This copies at most MAX_MESSAGE_BYTES without
        // allocating or reading past that fixed bound.
        let message = unsafe { CoreLogMessage::copy_from_callback(message) };
        sink.enqueue(message);
    }));
}

impl CoreLogSink {
    fn stats(&self) -> CoreLogStats {
        let mut stats = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stats;
        let local_drops = self.callback_dropped.load(Ordering::Relaxed);
        let global_drops = CORE_LOG_CALLBACK_DROPS
            .load(Ordering::Relaxed)
            .saturating_sub(self.global_drop_baseline);
        stats.dropped = stats
            .dropped
            .saturating_add(local_drops)
            .saturating_add(global_drops);
        stats
    }

    fn enqueue(&self, message: CoreLogMessage) {
        if !self.accepting.load(Ordering::Acquire) {
            self.callback_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match self.sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.callback_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn run_worker(&self, receiver: Receiver<CoreLogMessage>) {
        loop {
            match receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(message) => self.record(message),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !self.accepting.load(Ordering::Acquire) {
                        self.drain_worker_queue(&receiver);
                        return;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn drain_worker_queue(&self, receiver: &Receiver<CoreLogMessage>) {
        loop {
            match receiver.try_recv() {
                Ok(message) => self.record(message),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    fn record(&self, message: CoreLogMessage) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        record_message(&mut state, &message.bytes[..message.length]);
    }
}

impl CoreLogMessage {
    unsafe fn copy_from_callback(message: *const c_char) -> Self {
        let mut copied = Self {
            bytes: [0; MAX_MESSAGE_BYTES],
            length: 0,
        };
        for index in 0..MAX_MESSAGE_BYTES {
            // SAFETY: the Core callback contract supplies a readable
            // null-terminated string for this invocation. The bounded loop
            // reads no more than MAX_MESSAGE_BYTES bytes before returning.
            let byte = unsafe { *message.add(index) } as u8;
            if byte == 0 {
                break;
            }
            copied.bytes[index] = byte;
            copied.length = index + 1;
        }
        copied
    }
}

fn record_message(state: &mut CoreLogState, bytes: &[u8]) {
    let message = sanitize_message(bytes);
    let record = CoreLogRecord {
        component: "cubism_core",
        level: "info",
        message: &message,
    };
    let Ok(mut line) = serde_json::to_vec(&record) else {
        return;
    };
    line.push(b'\n');
    let Ok(line_len) = u64::try_from(line.len()) else {
        return;
    };
    if state.bytes.saturating_add(line_len) > MAX_LOG_BYTES && !rotate_logs(state) {
        state.stats.dropped = state.stats.dropped.saturating_add(1);
        return;
    }
    let Some(file) = state.file.as_mut() else {
        state.stats.dropped = state.stats.dropped.saturating_add(1);
        return;
    };
    if file.write_all(&line).is_err() || file.flush().is_err() {
        state.stats.dropped = state.stats.dropped.saturating_add(1);
        return;
    }
    state.bytes = state.bytes.saturating_add(line_len);
    state.stats.written = state.stats.written.saturating_add(1);
    state.stats.bytes = state.bytes;
}

fn rotate_logs(state: &mut CoreLogState) -> bool {
    let Some(file) = state.file.take() else {
        return false;
    };
    drop(file);

    for generation in (1..MAX_ROTATED_LOG_FILES).rev() {
        let source = rotated_log_path(&state.path, generation);
        let destination = rotated_log_path(&state.path, generation + 1);
        let _ = fs::remove_file(&destination);
        if source.exists() && fs::rename(&source, &destination).is_err() {
            reopen_active_log(state);
            return false;
        }
    }
    let first = rotated_log_path(&state.path, 1);
    let _ = fs::remove_file(&first);
    if fs::rename(&state.path, &first).is_err() {
        reopen_active_log(state);
        return false;
    }

    // Re-open the active path after moving the old file into the rotation set.
    if reopen_active_log(state) {
        state.bytes = 0;
        state.stats.bytes = 0;
        state.stats.rotated = state.stats.rotated.saturating_add(1);
        state.stats.pruned = state
            .stats
            .pruned
            .saturating_add(prune_expired_rotated_logs(&state.path, SystemTime::now()));
        state.stats.retained_files = retained_log_files(&state.path);
        true
    } else {
        let _ = fs::rename(&first, &state.path);
        reopen_active_log(state);
        false
    }
}

fn reopen_active_log(state: &mut CoreLogState) -> bool {
    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
    else {
        return false;
    };
    if set_private_file(&file).is_err() {
        return false;
    }
    let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    state.file = Some(file);
    state.bytes = bytes;
    state.stats.bytes = bytes;
    state.stats.retained_files = retained_log_files(&state.path);
    true
}

fn set_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn set_private_file(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn rotated_log_path(path: &Path, generation: u32) -> PathBuf {
    let mut rotated = path.as_os_str().to_owned();
    rotated.push(format!(".{generation}"));
    PathBuf::from(rotated)
}

fn prune_expired_rotated_logs(path: &Path, now: SystemTime) -> u64 {
    (1..=MAX_ROTATED_LOG_FILES)
        .filter_map(|generation| {
            let rotated = rotated_log_path(path, generation);
            let modified = fs::metadata(&rotated).ok()?.modified().ok()?;
            is_expired(modified, now).then(|| fs::remove_file(rotated).is_ok())
        })
        .filter(|removed| *removed)
        .count() as u64
}

fn retained_log_files(path: &Path) -> u64 {
    u64::from(path.is_file())
        + (1..=MAX_ROTATED_LOG_FILES)
            .filter(|generation| rotated_log_path(path, *generation).is_file())
            .count() as u64
}

fn is_expired(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age.as_secs() >= RETENTION_DAYS.saturating_mul(SECONDS_PER_DAY))
}

fn sanitize_message(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_MESSAGE_BYTES)]);
    text.split_whitespace()
        .map(|token| {
            if token.contains('/')
                || token.contains('\\')
                || token.starts_with("~")
                || token.as_bytes().get(1).is_some_and(|byte| *byte == b':')
            {
                "<redacted-path>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use tempfile::tempdir;

    static CORE_LOG_INSTALL_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn sanitizes_paths_and_bounds_message_bytes() {
        let message =
            sanitize_message(b"model /Users/example/private\nC:\\Users\\name\\model.moc3 stable");
        assert_eq!(message, "model <redacted-path> <redacted-path> stable");
        assert!(sanitize_message(&vec![b'x'; MAX_MESSAGE_BYTES + 20]).len() <= MAX_MESSAGE_BYTES);
    }

    #[test]
    fn expiration_keeps_recent_and_clock_regressed_rotated_logs() {
        let now = std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(RETENTION_DAYS * SECONDS_PER_DAY);
        assert!(is_expired(std::time::UNIX_EPOCH, now));
        assert!(!is_expired(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1),
            now
        ));
        assert!(!is_expired(now, std::time::UNIX_EPOCH));
    }

    #[test]
    fn sink_rotates_before_dropping_records_at_the_file_limit() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("logs/core.jsonl");
        fs::create_dir_all(path.parent().expect("log parent")).expect("log directory");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("log file");
        let mut state = CoreLogState {
            file: Some(file),
            path: path.clone(),
            bytes: MAX_LOG_BYTES - 1,
            stats: CoreLogStats {
                bytes: MAX_LOG_BYTES - 1,
                ..CoreLogStats::default()
            },
        };
        record_message(&mut state, b"one");
        let stats = state.stats;
        assert_eq!(stats.written, 1);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.rotated, 1);
        assert_eq!(stats.retained_files, 2);
        assert!(stats.bytes < MAX_LOG_BYTES);
        assert!(rotated_log_path(&path, 1).is_file());
        assert!(fs::metadata(&path).expect("active log").len() > 0);
    }

    #[test]
    fn rotation_retains_only_the_configured_number_of_files() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("seed active log");
        for generation in 1..=MAX_ROTATED_LOG_FILES {
            fs::write(rotated_log_path(&path, generation), b"old").expect("seed rotated log");
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("log file");
        let mut state = CoreLogState {
            file: Some(file),
            path: path.clone(),
            bytes: MAX_LOG_BYTES,
            stats: CoreLogStats {
                bytes: MAX_LOG_BYTES,
                ..CoreLogStats::default()
            },
        };
        record_message(&mut state, b"rotation");
        let stats = state.stats;
        assert_eq!(stats.rotated, 1);
        assert_eq!(stats.retained_files, u64::from(MAX_TOTAL_LOG_FILES));
        assert!(rotated_log_path(&path, MAX_ROTATED_LOG_FILES).is_file());
        assert!(!rotated_log_path(&path, MAX_ROTATED_LOG_FILES + 1).exists());
        let retained_bytes = (0..=MAX_ROTATED_LOG_FILES)
            .map(|generation| {
                let retained = if generation == 0 {
                    path.clone()
                } else {
                    rotated_log_path(&path, generation)
                };
                fs::metadata(retained).expect("retained log metadata").len()
            })
            .sum::<u64>();
        assert!(retained_bytes <= MAX_TOTAL_LOG_FILES as u64 * MAX_LOG_BYTES);
        assert!(fs::read(&path).expect("active contents").contains(&b'\n'));
    }

    #[test]
    fn missing_active_file_handle_can_be_reopened_after_a_rotation_failure() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        fs::write(&path, b"active").expect("seed active log");
        let mut state = CoreLogState {
            file: None,
            path: path.clone(),
            bytes: 0,
            stats: CoreLogStats {
                bytes: 0,
                ..CoreLogStats::default()
            },
        };
        assert!(reopen_active_log(&mut state));
        assert!(state.file.is_some());
        assert_eq!(state.bytes, b"active".len() as u64);
        assert_eq!(state.stats.retained_files, 1);
    }

    #[test]
    fn installed_callback_writes_structured_record_and_is_removed_on_drop() {
        let _install_guard = CORE_LOG_INSTALL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        let handle = CoreLogHandle::install(&path).expect("install Core logger");
        let message = CString::new("Core warning /private/model.moc3").expect("message");
        // SAFETY: the CString is null-terminated and remains alive for the
        // synchronous callback invocation.
        unsafe { core_log_callback(message.as_ptr()) };
        wait_for_written(&handle, 1);
        assert_eq!(handle.stats().retained_files, 1);
        let contents = fs::read_to_string(&path).expect("read log");
        assert!(contents.contains("cubism_core"));
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
        let handle = CoreLogHandle::install(directory.path().join("core.jsonl"))
            .expect("install Core logger");
        let message = CString::new("callback slot contention").expect("message");
        let slot = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: the CString is null-terminated and remains alive for the
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
        let path = directory.path().join("core.jsonl");
        let handle = CoreLogHandle::install(&path).expect("install Core logger");
        let message = CString::new("queue saturation").expect("message");
        let state = handle
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The worker may already be holding one dequeued message while it
        // waits for `state`, so exceed both that in-flight slot and the queue.
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
        let handle = CoreLogHandle::install(directory.path().join("core.jsonl"))
            .expect("install Core logger");
        handle.sink.accepting.store(false, Ordering::Release);
        let message = CString::new("late callback").expect("message");
        // SAFETY: the CString is null-terminated and remains alive for the
        // synchronous callback invocation.
        unsafe { core_log_callback(message.as_ptr()) };
        assert_eq!(handle.stats().written, 0);
        assert_eq!(handle.stats().dropped, 1);
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
        let path = directory.path().join("logs/core.jsonl");
        let handle = CoreLogHandle::install(&path).expect("install Core logger");
        assert_eq!(
            fs::metadata(path.parent().expect("log parent"))
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
}
