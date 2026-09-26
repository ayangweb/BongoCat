#![allow(unsafe_code)]

use crate::sys;
use bongocat_log::{LogLevel, LogRecord, LogSettingsController, LogStream, TextLogWriter};
use bongocat_storage::set_private_directory;
use std::{
    ffi::CStr,
    fs,
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

const MAX_MESSAGE_BYTES: usize = 512;
const MAX_SAFE_CORE_TOKEN_BYTES: usize = 64;
const REDACTED_CORE_TOKEN: &str = "<redacted>";
const CALLBACK_QUEUE_CAPACITY: usize = 128;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, thiserror::Error)]
pub enum CoreLogError {
    #[error("cannot create Core log directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("cannot open Core log file: {0}")]
    OpenFile(std::io::Error),
    #[error("cannot start Core log worker: {0}")]
    StartWorker(std::io::Error),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogStats {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Debug)]
struct CoreLogState {
    writer: TextLogWriter,
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
    worker: Option<JoinHandle<()>>,
}

/// Read-only access to the anonymous retention counters maintained by the
/// process-wide Core log callback.
#[derive(Clone, Debug)]
pub struct CoreLogReporter {
    sink: Arc<CoreLogSink>,
}

impl CoreLogHandle {
    pub fn install(
        directory: impl AsRef<Path>,
        settings: LogSettingsController,
    ) -> Result<Self, CoreLogError> {
        let directory = directory.as_ref();
        fs::create_dir_all(directory).map_err(CoreLogError::CreateDirectory)?;
        set_private_directory(directory).map_err(CoreLogError::CreateDirectory)?;
        let writer = TextLogWriter::open(directory, LogStream::CubismCore, settings)
            .map_err(CoreLogError::OpenFile)?;
        let (sender, receiver) = sync_channel(CALLBACK_QUEUE_CAPACITY);
        let sink = Arc::new(CoreLogSink {
            state: Mutex::new(CoreLogState { writer }),
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
            worker: Some(worker),
        })
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
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let writer = state.writer.stats();
        let mut stats = CoreLogStats {
            written: writer.written,
            dropped: writer.dropped,
            rotated: writer.rotated,
            pruned: writer.pruned,
            bytes: writer.active_bytes,
            retained_files: writer.retained_files,
            retained_bytes: writer.retained_bytes,
        };
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
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let message = sanitize_message(&message.bytes[..message.length]);
        // Cubism's callback has no typed severity. Treating every vendor
        // message as debug prevents routine Core chatter from becoming the
        // default user log; actual Core/renderer failures are logged by the
        // app owner with a stable error or warning code.
        let _ = state.writer.record(LogRecord::new(
            SystemTime::now(),
            LogLevel::Debug,
            "cubism.core",
            "cubism/core/message",
            message,
        ));
    }
}

impl CoreLogMessage {
    unsafe fn copy_from_callback(message: *const c_char) -> Self {
        let mut copied = Self {
            bytes: [0; MAX_MESSAGE_BYTES],
            length: 0,
        };
        // SAFETY: the Core callback contract supplies a readable,
        // null-terminated string for this invocation. `CStr::from_ptr` is the
        // standard wrapper for that C boundary; the subsequent copy is capped
        // at MAX_MESSAGE_BYTES and never exposes the original pointer to the
        // queue.
        let source = unsafe { CStr::from_ptr(message) }.to_bytes();
        copied.length = source.len().min(MAX_MESSAGE_BYTES);
        copied.bytes[..copied.length].copy_from_slice(&source[..copied.length]);
        copied
    }
}

fn sanitize_message(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_MESSAGE_BYTES)]);
    let tokens = text.split_whitespace().collect::<Vec<_>>();

    // A Core callback has no typed field boundary. Keep only short, plain
    // diagnostic words and numbers; punctuation-bearing tokens are commonly
    // paths, URLs, assignments, or serialized resource fragments. Redacting the
    // whole message when it contains a sensitive marker also prevents a value
    // following a word such as `token` or `password` from being retained.
    if tokens.iter().any(|token| is_sensitive_core_token(token)) {
        return REDACTED_CORE_TOKEN.to_owned();
    }

    let mut output = String::new();
    for token in tokens {
        let rendered = if is_safe_core_token(token) {
            token
        } else {
            REDACTED_CORE_TOKEN
        };
        if !output.is_empty() {
            if output.len().saturating_add(1) >= MAX_MESSAGE_BYTES {
                break;
            }
            output.push(' ');
        }
        let remaining = MAX_MESSAGE_BYTES.saturating_sub(output.len());
        if remaining == 0 {
            break;
        }
        if rendered.len() > remaining {
            output.push_str(&rendered[..remaining]);
            break;
        }
        output.push_str(rendered);
    }
    if output.is_empty() {
        REDACTED_CORE_TOKEN.to_owned()
    } else {
        output
    }
}

fn is_safe_core_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_SAFE_CORE_TOKEN_BYTES
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_sensitive_core_token(token: &str) -> bool {
    let normalized = token.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "clipboard",
        "url",
        "uri",
        "path",
        "file",
        "config",
        "payload",
        "content",
        "signature",
        "authorization",
        "bearer",
        "permission",
        "denied",
        "error",
        "failed",
        "failure",
        "invalid",
        "corrupt",
        "unavailable",
        "errno",
        "exception",
        "network",
        "connection",
        "socket",
        "address",
        "dns",
        "http",
        "https",
        "proxy",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

#[cfg(test)]
mod tests {
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
        let handle = CoreLogHandle::install(directory.path(), debug_settings())
            .expect("install Core logger");
        let message = CString::new("Core warning /private/model.moc3").expect("message");
        // SAFETY: the CString is null-terminated and remains alive for the
        // synchronous callback invocation.
        unsafe { core_log_callback(message.as_ptr()) };
        wait_for_written(&handle, 1);
        let path = core_log_path(directory.path());
        assert_eq!(handle.stats().retained_files, 1);
        let contents = fs::read_to_string(&path).expect("read log");
        assert!(
            contents.contains("DEBUG [cubism.core] cubism/core/message | Core warning <redacted>")
        );
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
        let handle = CoreLogHandle::install(directory.path(), debug_settings())
            .expect("install Core logger");
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
        let handle = CoreLogHandle::install(directory.path(), debug_settings())
            .expect("install Core logger");
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
        let handle = CoreLogHandle::install(directory.path(), debug_settings())
            .expect("install Core logger");
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
        let handle = CoreLogHandle::install(directory.path(), debug_settings())
            .expect("install Core logger");
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
}
