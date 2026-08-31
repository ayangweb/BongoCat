#![allow(unsafe_code)]

use crate::sys;
use serde::Serialize;
use std::{
    ffi::CStr,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_LOG_FILES: u32 = 4;
const MAX_MESSAGE_BYTES: usize = 512;

#[derive(Debug)]
pub enum CoreLogError {
    CreateDirectory(io::Error),
    OpenFile(io::Error),
}

impl std::fmt::Display for CoreLogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectory(error) => {
                write!(formatter, "cannot create Core log directory: {error}")
            }
            Self::OpenFile(error) => write!(formatter, "cannot open Core log file: {error}"),
        }
    }
}

impl std::error::Error for CoreLogError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogStats {
    pub written: u64,
    pub dropped: u64,
    pub bytes: u64,
}

#[derive(Debug)]
struct CoreLogState {
    file: Option<File>,
    path: PathBuf,
    bytes: u64,
    stats: CoreLogStats,
}

#[derive(Debug)]
struct CoreLogSink {
    state: Mutex<CoreLogState>,
}

#[derive(Serialize)]
struct CoreLogRecord<'a> {
    component: &'static str,
    level: &'static str,
    message: &'a str,
}

static CORE_LOG_SINK: OnceLock<Mutex<Option<Arc<CoreLogSink>>>> = OnceLock::new();

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
}

impl CoreLogHandle {
    pub fn install(path: impl AsRef<Path>) -> Result<Self, CoreLogError> {
        let path = path.as_ref().to_owned();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(CoreLogError::CreateDirectory)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(CoreLogError::OpenFile)?;
        let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let sink = Arc::new(CoreLogSink {
            state: Mutex::new(CoreLogState {
                file: Some(file),
                path: path.clone(),
                bytes,
                stats: CoreLogStats {
                    bytes,
                    ..CoreLogStats::default()
                },
            }),
        });
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
        Ok(Self { sink, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn stats(&self) -> CoreLogStats {
        self.sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stats
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
    }
}

unsafe extern "C" fn core_log_callback(message: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if message.is_null() {
            return;
        }
        // SAFETY: Cubism documents a valid null-terminated message for the
        // duration of the callback; it is copied before returning to Core.
        let bytes = unsafe { CStr::from_ptr(message).to_bytes() };
        let Some(sink) = sink_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .cloned()
        else {
            return;
        };
        sink.record(bytes);
    }));
}

impl CoreLogSink {
    fn record(&self, bytes: &[u8]) {
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
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.bytes.saturating_add(line_len) > MAX_LOG_BYTES && !rotate_logs(&mut state) {
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
}

fn rotate_logs(state: &mut CoreLogState) -> bool {
    let Some(file) = state.file.take() else {
        return false;
    };
    drop(file);

    for generation in (1..MAX_LOG_FILES).rev() {
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
    let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    state.file = Some(file);
    state.bytes = bytes;
    state.stats.bytes = bytes;
    true
}

fn rotated_log_path(path: &Path, generation: u32) -> PathBuf {
    let mut rotated = path.as_os_str().to_owned();
    rotated.push(format!(".{generation}"));
    PathBuf::from(rotated)
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

    #[test]
    fn sanitizes_paths_and_bounds_message_bytes() {
        let message =
            sanitize_message(b"model /Users/example/private\nC:\\Users\\name\\model.moc3 stable");
        assert_eq!(message, "model <redacted-path> <redacted-path> stable");
        assert!(sanitize_message(&vec![b'x'; MAX_MESSAGE_BYTES + 20]).len() <= MAX_MESSAGE_BYTES);
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
        let sink = CoreLogSink {
            state: Mutex::new(CoreLogState {
                file: Some(file),
                path: path.clone(),
                bytes: MAX_LOG_BYTES - 1,
                stats: CoreLogStats {
                    bytes: MAX_LOG_BYTES - 1,
                    ..CoreLogStats::default()
                },
            }),
        };
        sink.record(b"one");
        let stats = sink.state.lock().expect("state lock").stats;
        assert_eq!(stats.written, 1);
        assert_eq!(stats.dropped, 0);
        assert!(stats.bytes < MAX_LOG_BYTES);
        assert!(rotated_log_path(&path, 1).is_file());
        assert!(fs::metadata(&path).expect("active log").len() > 0);
    }

    #[test]
    fn rotation_retains_only_the_configured_number_of_files() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("seed active log");
        for generation in 1..=MAX_LOG_FILES {
            fs::write(rotated_log_path(&path, generation), b"old").expect("seed rotated log");
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("log file");
        let sink = CoreLogSink {
            state: Mutex::new(CoreLogState {
                file: Some(file),
                path: path.clone(),
                bytes: MAX_LOG_BYTES,
                stats: CoreLogStats {
                    bytes: MAX_LOG_BYTES,
                    ..CoreLogStats::default()
                },
            }),
        };
        sink.record(b"rotation");
        assert!(rotated_log_path(&path, MAX_LOG_FILES).is_file());
        assert!(!rotated_log_path(&path, MAX_LOG_FILES + 1).exists());
        assert!(fs::read(&path).expect("active contents").contains(&b'\n'));
    }

    #[test]
    fn missing_active_file_handle_can_be_reopened_after_a_rotation_failure() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        fs::write(&path, b"active").expect("seed active log");
        let sink = CoreLogSink {
            state: Mutex::new(CoreLogState {
                file: None,
                path: path.clone(),
                bytes: 0,
                stats: CoreLogStats {
                    bytes: 0,
                    ..CoreLogStats::default()
                },
            }),
        };

        let mut state = sink.state.lock().expect("state lock");
        assert!(reopen_active_log(&mut state));
        assert!(state.file.is_some());
        assert_eq!(state.bytes, b"active".len() as u64);
    }

    #[test]
    fn installed_callback_writes_structured_record_and_is_removed_on_drop() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("core.jsonl");
        let handle = CoreLogHandle::install(&path).expect("install Core logger");
        let message = CString::new("Core warning /private/model.moc3").expect("message");
        // SAFETY: the CString is null-terminated and remains alive for the
        // synchronous callback invocation.
        unsafe { core_log_callback(message.as_ptr()) };
        assert_eq!(handle.stats().written, 1);
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
}
