#![forbid(unsafe_code)]

//! Shared, bounded, human-readable logging for BongoCat.
//!
//! The application and Cubism Core use separate streams so support can tell
//! vendor messages apart, but both streams share this writer, level filter,
//! daily rollover, per-file size guard, directory budget, and retention
//! implementation. No third-party logging types cross the project boundary.

use bongocat_storage::{create_private_dir_all, set_private_file};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

pub const DEFAULT_RETENTION_DAYS: u64 = 7;
pub const MAX_LOG_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_TOTAL_LOG_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_LOG_FILES: u64 = 32;

const SECONDS_PER_DAY: u64 = 86_400;
const RETENTION_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const MAXIMUM_MODULE_BYTES: usize = 64;
const MAXIMUM_CODE_BYTES: usize = 128;
const MAXIMUM_MESSAGE_BYTES: usize = 512;
const MAXIMUM_CONTEXT_KEY_BYTES: usize = 48;
const MAXIMUM_CONTEXT_VALUE_BYTES: usize = 160;
const MAXIMUM_CONTEXT_FIELDS: usize = 8;
const TRUNCATION_MARKER: &str = "...[truncated]";

/// Severity used both as the record level and as the configured write
/// threshold. Lower numeric values are more severe.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub const fn is_enabled(self, configured: Self) -> bool {
        (self as u8) <= (configured as u8)
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        })
    }
}

/// Runtime policy shared by every writer participating in one application
/// environment. Updating it takes effect for both application and Core logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogSettings {
    pub level: LogLevel,
    pub retention_days: u64,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            level: LogLevel::default(),
            retention_days: DEFAULT_RETENTION_DAYS,
        }
    }
}

struct LogSettingsInner {
    settings: RwLock<LogSettings>,
    application_pruned: AtomicU64,
    core_pruned: AtomicU64,
}

impl fmt::Debug for LogSettingsInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogSettingsInner")
            .field("settings", &*read_settings(&self.settings))
            .field("application_pruned", &self.application_pruned)
            .field("core_pruned", &self.core_pruned)
            .finish()
    }
}

/// Cloneable owner for the active log policy and cross-writer prune counters.
#[derive(Clone, Debug)]
pub struct LogSettingsController {
    inner: Arc<LogSettingsInner>,
}

impl LogSettingsController {
    pub fn new(settings: LogSettings) -> Self {
        Self {
            inner: Arc::new(LogSettingsInner {
                settings: RwLock::new(settings),
                application_pruned: AtomicU64::new(0),
                core_pruned: AtomicU64::new(0),
            }),
        }
    }

    pub fn settings(&self) -> LogSettings {
        *read_settings(&self.inner.settings)
    }

    pub fn replace_settings(&self, settings: LogSettings) -> LogSettings {
        let mut current = write_settings(&self.inner.settings);
        std::mem::replace(&mut *current, settings)
    }

    fn record_pruned(&self, stream: LogStream, count: u64) {
        if count == 0 {
            return;
        }
        let counter = match stream {
            LogStream::Application => &self.inner.application_pruned,
            LogStream::CubismCore => &self.inner.core_pruned,
        };
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(count))
        });
    }

    fn pruned(&self, stream: LogStream) -> u64 {
        let counter = match stream {
            LogStream::Application => &self.inner.application_pruned,
            LogStream::CubismCore => &self.inner.core_pruned,
        };
        counter.load(Ordering::Relaxed)
    }
}

impl Default for LogSettingsController {
    fn default() -> Self {
        Self::new(LogSettings::default())
    }
}

fn read_settings(lock: &RwLock<LogSettings>) -> RwLockReadGuard<'_, LogSettings> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_settings(lock: &RwLock<LogSettings>) -> RwLockWriteGuard<'_, LogSettings> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The two product-owned text streams. Adding a third stream requires making
/// its filename and diagnostics ownership explicit in the same change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogStream {
    Application,
    CubismCore,
}

impl LogStream {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::CubismCore => "cubism-core",
        }
    }

    fn active_file_name(self, date: UtcDate) -> String {
        format!("{}-{}.log", self.prefix(), date.as_string())
    }

    fn rotated_file_name(self, date: UtcDate, generation: u64) -> String {
        format!("{}-{}.{generation}.log", self.prefix(), date.as_string())
    }
}

/// One fully bounded text record. `module`, `code`, and `message` are intended
/// to be fixed project values. Context is an allow-listed set of small values;
/// callers must not pass paths, raw I/O errors, input events, or resource text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogRecord {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub module: String,
    pub code: String,
    pub message: String,
    pub context: Vec<(String, String)>,
}

impl LogRecord {
    pub fn new(
        timestamp: SystemTime,
        level: LogLevel,
        module: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            level,
            module: module.into(),
            code: code.into(),
            message: message.into(),
            context: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }
}

/// Strictly parsed prefix of one project text-log record.
///
/// The parser validates the shared line grammar but deliberately does not
/// expose context values. Diagnostics may use the fixed level/module/code and
/// message after matching the code against its own closed catalog, but cannot
/// accidentally copy timestamps or user-adjacent context into an export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsedLogLine<'a> {
    pub timestamp: &'a str,
    pub level: LogLevel,
    pub module: &'a str,
    pub code: &'a str,
    pub message: &'a str,
    pub context_field_count: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextLogStats {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub active_bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetentionReport {
    pub pruned: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Debug)]
struct LogFile {
    path: PathBuf,
    stream: LogStream,
    date: UtcDate,
    generation: Option<u64>,
    bytes: u64,
    modified: SystemTime,
    current: bool,
}

struct RetentionSweep {
    report: RetentionReport,
    application_pruned: u64,
    core_pruned: u64,
}

struct TextLogState {
    directory: PathBuf,
    stream: LogStream,
    day: UtcDate,
    path: Option<PathBuf>,
    file: Option<File>,
    active_bytes: u64,
    written: u64,
    dropped: u64,
    rotated: u64,
    retention_ready: bool,
    last_retention_check: Option<SystemTime>,
}

impl TextLogState {
    fn active_path(&self) -> PathBuf {
        self.directory.join(self.stream.active_file_name(self.day))
    }

    fn open_active(&mut self) -> io::Result<()> {
        if self.file.is_some() {
            if self.path.as_ref().is_some_and(|path| path.is_file()) {
                return Ok(());
            }
            self.file = None;
            self.path = None;
            self.active_bytes = 0;
        }

        let path = self.active_path();
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to append through a log symlink",
            ));
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        set_private_file(&file)?;
        self.active_bytes = fs::metadata(&path)?.len();
        self.path = Some(path);
        self.file = Some(file);
        Ok(())
    }

    fn switch_day(&mut self, next_day: UtcDate) -> io::Result<()> {
        if self.day == next_day {
            return self.open_active();
        }
        if let Some(path) = self.path.take() {
            self.file = None;
            if rotate_path(&self.directory, self.stream, &path).is_ok() {
                self.rotated = self.rotated.saturating_add(1);
            } else {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        self.day = next_day;
        self.open_active()
    }

    fn rotate_for_size(&mut self, incoming_bytes: u64) -> io::Result<()> {
        if self.active_bytes == 0
            || self.active_bytes.saturating_add(incoming_bytes) <= MAX_LOG_FILE_BYTES
        {
            return Ok(());
        }
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        self.file = None;
        rotate_path(&self.directory, self.stream, &path)?;
        self.rotated = self.rotated.saturating_add(1);
        self.open_active()
    }

    fn refresh_retention(&mut self, controller: &LogSettingsController, now: SystemTime) {
        let sweep = enforce_directory_retention_sweep(
            &self.directory,
            now,
            controller.settings().retention_days,
        );
        controller.record_pruned(LogStream::Application, sweep.application_pruned);
        controller.record_pruned(LogStream::CubismCore, sweep.core_pruned);
    }

    fn maybe_refresh_retention(&mut self, controller: &LogSettingsController, now: SystemTime) {
        if !self.retention_ready {
            return;
        }
        let due = self.last_retention_check.is_none_or(|last| {
            now.duration_since(last)
                .is_ok_and(|elapsed| elapsed >= RETENTION_CHECK_INTERVAL)
        });
        if due {
            self.refresh_retention(controller, now);
            self.last_retention_check = Some(now);
        }
    }

    fn refresh_stream_totals(&mut self) {
        if let Some(path) = self.path.as_ref()
            && let Ok(metadata) = fs::metadata(path)
        {
            self.active_bytes = metadata.len();
        }
    }
}

/// Synchronous, bounded text writer shared by one log stream.
///
/// Writes happen only from low-frequency app/service boundaries. The panic
/// hook uses [`Self::try_record`] so lock contention cannot recurse or block.
pub struct TextLogWriter {
    state: Mutex<TextLogState>,
    controller: LogSettingsController,
}

impl fmt::Debug for TextLogWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextLogWriter")
            .field("stream", &self.state.lock().map(|state| state.stream))
            .field("controller", &self.controller)
            .finish()
    }
}

impl TextLogWriter {
    pub fn open(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
    ) -> io::Result<Self> {
        Self::open_impl(directory, stream, controller, true)
    }

    /// Open a stream without deleting historical files.
    ///
    /// Application startup uses this while the persisted configuration is still
    /// being loaded, so a user-selected retention period longer than the
    /// bootstrap default cannot be irreversibly shortened before it is known.
    /// Call [`Self::refresh_policy`] immediately after loading the settings.
    pub fn open_deferred(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
    ) -> io::Result<Self> {
        Self::open_impl(directory, stream, controller, false)
    }

    fn open_impl(
        directory: impl AsRef<Path>,
        stream: LogStream,
        controller: LogSettingsController,
        enforce_retention: bool,
    ) -> io::Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        create_private_dir_all(&directory)?;
        let now = SystemTime::now();
        let day = UtcDate::from_system_time(now);
        let mut retired_at_open = 0;
        if enforce_retention {
            retired_at_open = retire_other_day_bases(&directory, stream, day);
            let sweep = enforce_directory_retention_sweep(
                &directory,
                now,
                controller.settings().retention_days,
            );
            controller.record_pruned(LogStream::Application, sweep.application_pruned);
            controller.record_pruned(LogStream::CubismCore, sweep.core_pruned);
        }

        let mut state = TextLogState {
            directory,
            stream,
            day,
            path: None,
            file: None,
            active_bytes: 0,
            written: 0,
            dropped: 0,
            rotated: retired_at_open,
            retention_ready: enforce_retention,
            last_retention_check: enforce_retention.then_some(now),
        };
        state.open_active()?;
        Ok(Self {
            state: Mutex::new(state),
            controller,
        })
    }

    /// Record `record` if the shared level filter admits it.
    ///
    /// `Ok(false)` means the record was intentionally filtered. `Ok(true)`
    /// means it was written and flushed.
    pub fn record(&self, record: LogRecord) -> io::Result<bool> {
        let mut state = self.lock_state();
        self.record_locked(&mut state, record)
    }

    /// Non-blocking variant used by the process panic hook.
    pub fn try_record(&self, record: LogRecord) -> Option<io::Result<bool>> {
        let mut state = self.state.try_lock().ok()?;
        Some(self.record_locked(&mut state, record))
    }

    fn record_locked(&self, state: &mut TextLogState, record: LogRecord) -> io::Result<bool> {
        let settings = self.controller.settings();
        if !record.level.is_enabled(settings.level) {
            return Ok(false);
        }

        let line = format_log_line(&record);
        let bytes = line.as_bytes();
        let record_day = UtcDate::from_system_time(record.timestamp);
        if let Err(error) = state.switch_day(record_day) {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }
        if let Err(error) = state.rotate_for_size(bytes.len() as u64) {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }

        let write_result = state
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file is not open"))
            .and_then(|file| {
                file.write_all(bytes)?;
                file.flush()
            });
        if let Err(error) = write_result {
            state.dropped = state.dropped.saturating_add(1);
            return Err(error);
        }

        state.active_bytes = state.active_bytes.saturating_add(bytes.len() as u64);
        state.written = state.written.saturating_add(1);
        state.maybe_refresh_retention(&self.controller, SystemTime::now());
        Ok(true)
    }

    /// Apply a newly replaced shared settings controller immediately. Rotation
    /// and cleanup remain best effort and never fail a settings transaction.
    pub fn refresh_policy(&self) {
        let mut state = self.lock_state();
        let now = SystemTime::now();
        let retired = retire_other_day_bases(&state.directory, state.stream, state.day);
        state.rotated = state.rotated.saturating_add(retired);
        state.refresh_retention(&self.controller, now);
        state.retention_ready = true;
        state.last_retention_check = Some(now);
    }

    pub fn stats(&self) -> TextLogStats {
        let mut state = self.lock_state();
        state.refresh_stream_totals();
        let today = UtcDate::from_system_time(SystemTime::now());
        let files = collect_log_files(&state.directory, today)
            .into_iter()
            .filter(|file| file.stream == state.stream);
        let mut retained_files = 0_u64;
        let mut retained_bytes = 0_u64;
        for file in files {
            retained_files = retained_files.saturating_add(1);
            retained_bytes = retained_bytes.saturating_add(file.bytes);
        }
        TextLogStats {
            written: state.written,
            dropped: state.dropped,
            rotated: state.rotated,
            pruned: self.controller.pruned(state.stream),
            active_bytes: state.active_bytes,
            retained_files,
            retained_bytes,
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, TextLogState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Enforce the shared log-directory budget without reading log contents.
///
/// Only the two known product-owned `.log` naming schemes are considered.
/// Unknown files and symlinks are ignored. Current UTC-day active files are
/// retained; expired or budget-breaking rotated files are removed oldest first.
pub fn enforce_directory_retention(
    directory: &Path,
    now: SystemTime,
    retention_days: u64,
) -> RetentionReport {
    enforce_directory_retention_sweep(directory, now, retention_days).report
}

fn enforce_directory_retention_sweep(
    directory: &Path,
    now: SystemTime,
    retention_days: u64,
) -> RetentionSweep {
    let today = UtcDate::from_system_time(now);
    let expiration = now.checked_sub(Duration::from_secs(
        retention_days.saturating_mul(SECONDS_PER_DAY),
    ));
    let mut application_pruned = 0_u64;
    let mut core_pruned = 0_u64;
    let mut files = collect_log_files(directory, today);

    files.retain(|file| {
        let expired = expiration.is_some_and(|deadline| file.modified < deadline);
        if expired && !file.current && fs::remove_file(&file.path).is_ok() {
            match file.stream {
                LogStream::Application => application_pruned = application_pruned.saturating_add(1),
                LogStream::CubismCore => core_pruned = core_pruned.saturating_add(1),
            }
            false
        } else {
            true
        }
    });

    let mut total_bytes = files
        .iter()
        .fold(0_u64, |total, file| total.saturating_add(file.bytes));
    files.sort_by_key(|file| (file.modified, file.path.clone()));
    let mut kept = Vec::with_capacity(files.len());
    for file in files {
        let over_budget =
            total_bytes > MAX_TOTAL_LOG_BYTES || kept.len() as u64 + 1 > MAX_TOTAL_LOG_FILES;
        if over_budget && !file.current && fs::remove_file(&file.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(file.bytes);
            match file.stream {
                LogStream::Application => application_pruned = application_pruned.saturating_add(1),
                LogStream::CubismCore => core_pruned = core_pruned.saturating_add(1),
            }
        } else {
            kept.push(file);
        }
    }

    let remaining = collect_log_files(directory, today);
    let report = RetentionReport {
        pruned: application_pruned.saturating_add(core_pruned),
        retained_files: remaining.len() as u64,
        retained_bytes: remaining
            .iter()
            .fold(0_u64, |total, file| total.saturating_add(file.bytes)),
    };
    RetentionSweep {
        report,
        application_pruned,
        core_pruned,
    }
}

fn collect_log_files(directory: &Path, today: UtcDate) -> Vec<LogFile> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.file_type().is_file() {
                return None;
            }
            let (stream, date, generation) = parse_log_file_name(&path)?;
            let current = generation.is_none() && date == today;
            Some(LogFile {
                path,
                stream,
                date,
                generation,
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                current,
            })
        })
        .collect()
}

/// Return whether `file_name` exactly belongs to the requested product-owned
/// text-log naming scheme. Unknown files and legacy formats return `false`.
pub fn is_log_file_name(stream: LogStream, file_name: &str) -> bool {
    parse_log_file_name(Path::new(file_name)).is_some_and(|(parsed, _, _)| parsed == stream)
}

fn parse_log_file_name(path: &Path) -> Option<(LogStream, UtcDate, Option<u64>)> {
    let name = path.file_name()?.to_str()?;
    for stream in [LogStream::Application, LogStream::CubismCore] {
        let Some(rest) = name.strip_prefix(&format!("{}-", stream.prefix())) else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(".log") else {
            continue;
        };
        if let Some((date, generation)) = rest.rsplit_once('.') {
            let generation = generation.parse::<u64>().ok()?;
            if generation == 0 {
                return None;
            }
            return UtcDate::parse(date).map(|date| (stream, date, Some(generation)));
        }
        return UtcDate::parse(rest).map(|date| (stream, date, None));
    }
    None
}

fn retire_other_day_bases(directory: &Path, stream: LogStream, current: UtcDate) -> u64 {
    let candidates = collect_log_files(directory, current)
        .into_iter()
        .filter(|file| file.stream == stream && file.generation.is_none() && file.date != current)
        .collect::<Vec<_>>();
    let mut rotated = 0_u64;
    for file in candidates {
        if rotate_path(directory, stream, &file.path).is_ok() {
            rotated = rotated.saturating_add(1);
        }
    }
    rotated
}

fn rotate_path(directory: &Path, stream: LogStream, active: &Path) -> io::Result<PathBuf> {
    let file_name = active
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path has no file name"))?;
    let stem = file_name
        .strip_suffix(".log")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path is not .log"))?;
    let prefix = format!("{}-", stream.prefix());
    let date_text = stem.strip_prefix(&prefix).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "active log path belongs to another stream",
        )
    })?;
    let date = UtcDate::parse(date_text).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "active log path has no valid date",
        )
    })?;
    let destination = next_rotation_path(directory, stream, date)?;
    fs::rename(active, &destination)?;
    Ok(destination)
}

fn next_rotation_path(directory: &Path, stream: LogStream, date: UtcDate) -> io::Result<PathBuf> {
    let highest = collect_log_files(directory, date)
        .into_iter()
        .filter(|file| file.stream == stream && file.date == date)
        .filter_map(|file| file.generation)
        .max()
        .unwrap_or(0);
    let generation = highest
        .checked_add(1)
        .ok_or_else(|| io::Error::other("log rotation generation overflow"))?;
    let path = directory.join(stream.rotated_file_name(date, generation));
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "next log rotation path already exists",
        ));
    }
    Ok(path)
}

/// Parse one line produced by the shared text writer without exposing its
/// context values to the caller.
pub fn parse_log_line(line: &str) -> Option<ParsedLogLine<'_>> {
    let bytes = line.as_bytes();
    if bytes.len() < 40 || bytes[23] != b'Z' || bytes[24] != b' ' {
        return None;
    }
    let timestamp = line.get(..24)?;
    if !is_valid_timestamp(timestamp) {
        return None;
    }
    let level = parse_padded_level(line.get(25..30)?)?;
    if bytes[30] != b' ' || bytes[31] != b'[' {
        return None;
    }

    let module_remainder = line.get(32..)?;
    let module_end = module_remainder.find("] ")?;
    let module = &module_remainder[..module_end];
    if !is_stable_fragment(module, MAXIMUM_MODULE_BYTES) {
        return None;
    }
    let payload_start = 32 + module_end + 2;
    let (code, remainder) = line.get(payload_start..)?.split_once(" | ")?;
    if !is_stable_fragment(code, MAXIMUM_CODE_BYTES) {
        return None;
    }
    let (message, context) = remainder
        .split_once(" | ")
        .map_or((remainder, ""), |(message, context)| (message, context));
    if !is_bounded_sanitized_text(message, MAXIMUM_MESSAGE_BYTES) {
        return None;
    }

    let mut context_field_count = 0;
    if !context.is_empty() {
        for field in context.split(" | ") {
            context_field_count += 1;
            if context_field_count > MAXIMUM_CONTEXT_FIELDS {
                return None;
            }
            let (key, value) = field.split_once('=')?;
            if !is_stable_fragment(key, MAXIMUM_CONTEXT_KEY_BYTES)
                || !is_bounded_sanitized_text(value, MAXIMUM_CONTEXT_VALUE_BYTES)
            {
                return None;
            }
        }
    }

    Some(ParsedLogLine {
        timestamp,
        level,
        module,
        code,
        message,
        context_field_count,
    })
}

fn is_valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        })
        && UtcDate::parse(&value[..10]).is_some()
        && parse_decimal(&bytes[11..13]).is_some_and(|hour| hour < 24)
        && parse_decimal(&bytes[14..16]).is_some_and(|minute| minute < 60)
        && parse_decimal(&bytes[17..19]).is_some_and(|second| second < 60)
        && parse_decimal(&bytes[20..23]).is_some_and(|millisecond| millisecond < 1_000)
}

fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse::<u32>().ok()
}

fn parse_padded_level(value: &str) -> Option<LogLevel> {
    match value.as_bytes() {
        b"ERROR" | b"ERROR " => Some(LogLevel::Error),
        b"WARN " => Some(LogLevel::Warn),
        b"INFO " => Some(LogLevel::Info),
        b"DEBUG" | b"DEBUG " => Some(LogLevel::Debug),
        b"TRACE" | b"TRACE " => Some(LogLevel::Trace),
        _ => None,
    }
}

fn is_stable_fragment(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn is_bounded_sanitized_text(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && !value.chars().any(char::is_control)
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| byte != b'|' || index > 0 && value.as_bytes()[index - 1] == b'\\')
}

fn format_log_line(record: &LogRecord) -> String {
    let timestamp = format_timestamp(record.timestamp);
    let level = record.level.to_string();
    let module = sanitize_fragment(&record.module, MAXIMUM_MODULE_BYTES, false);
    let code = sanitize_fragment(&record.code, MAXIMUM_CODE_BYTES, false);
    let message = sanitize_fragment(&record.message, MAXIMUM_MESSAGE_BYTES, true);
    let mut line = format!("{timestamp} {level:<5} [{module}] {code} | {message}");

    for (key, value) in record.context.iter().take(MAXIMUM_CONTEXT_FIELDS) {
        let key = sanitize_fragment(key, MAXIMUM_CONTEXT_KEY_BYTES, false);
        let value = sanitize_fragment(value, MAXIMUM_CONTEXT_VALUE_BYTES, true);
        if !key.is_empty() && !value.is_empty() {
            line.push_str(" | ");
            line.push_str(&key);
            line.push('=');
            line.push_str(&value);
        }
    }
    line.push('\n');
    line
}

fn sanitize_fragment(value: &str, maximum_bytes: usize, escape_pipe: bool) -> String {
    let mut output = String::with_capacity(value.len().min(maximum_bytes));
    let mut truncated = false;
    for character in value.chars() {
        let rendered = match character {
            '\r' => "\\r".to_owned(),
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\u{2028}' => "\\u{2028}".to_owned(),
            '\u{2029}' => "\\u{2029}".to_owned(),
            '|' if escape_pipe => "\\|".to_owned(),
            character if character.is_control() => "\u{fffd}".to_owned(),
            character => character.to_string(),
        };
        if output.len().saturating_add(rendered.len()) > maximum_bytes {
            truncated = true;
            break;
        }
        output.push_str(&rendered);
    }
    if truncated {
        let marker = TRUNCATION_MARKER.len().min(maximum_bytes);
        while output.len().saturating_add(marker) > maximum_bytes {
            output.pop();
        }
        output.push_str(&TRUNCATION_MARKER[..marker]);
    }
    output
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct UtcDate {
    year: i64,
    month: u8,
    day: u8,
}

impl UtcDate {
    fn from_system_time(timestamp: SystemTime) -> Self {
        let duration = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
        let days = total_seconds.div_euclid(SECONDS_PER_DAY as i64);
        Self::from_days(days)
    }

    fn from_days(days_since_epoch: i64) -> Self {
        let shifted = days_since_epoch + 719_468;
        let era = if shifted >= 0 {
            shifted
        } else {
            shifted - 146_096
        }
        .div_euclid(146_097);
        let day_of_era = shifted - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_position = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_position + 2) / 5 + 1;
        let month = if month_position < 10 {
            month_position + 3
        } else {
            month_position - 9
        };
        if month <= 2 {
            year += 1;
        }
        Self {
            year,
            month: month as u8,
            day: day as u8,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        {
            return None;
        }
        let year = value[0..4].parse::<i64>().ok()?;
        let month = value[5..7].parse::<u8>().ok()?;
        let day = value[8..10].parse::<u8>().ok()?;
        if !(1..=12).contains(&month) || day == 0 {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => unreachable!("month range checked above"),
        };
        if day > days_in_month {
            return None;
        }
        Some(Self { year, month, day })
    }

    fn as_string(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn format_timestamp(timestamp: SystemTime) -> String {
    let duration = timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = duration.as_secs();
    let day = UtcDate::from_days((total_seconds / SECONDS_PER_DAY) as i64);
    let seconds_of_day = total_seconds % SECONDS_PER_DAY;
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    let second = seconds_of_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        day.year,
        day.month,
        day.day,
        duration.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

    #[test]
    fn levels_are_ordered_from_error_to_trace() {
        assert!(LogLevel::Error.is_enabled(LogLevel::Trace));
        assert!(LogLevel::Warn.is_enabled(LogLevel::Info));
        assert!(LogLevel::Info.is_enabled(LogLevel::Info));
        assert!(!LogLevel::Debug.is_enabled(LogLevel::Info));
        assert_eq!(LogLevel::default(), LogLevel::Info);
        assert_eq!(LogLevel::Trace.to_string(), "TRACE");
    }

    #[test]
    fn text_format_is_stable_and_escapes_multiline_context() {
        let record = LogRecord::new(
            SystemTime::UNIX_EPOCH,
            LogLevel::Info,
            "application",
            "application/started",
            "Application started",
        )
        .with_context("model_id", "standard\nnext");
        assert_eq!(
            format_log_line(&record),
            "1970-01-01T00:00:00.000Z INFO  [application] application/started | Application started | model_id=standard\\nnext\n"
        );
    }

    #[test]
    fn strict_parser_returns_catalog_fields_without_context_values() {
        let record = LogRecord::new(
            SystemTime::UNIX_EPOCH,
            LogLevel::Warn,
            "application",
            "application/previous_run_unclean",
            "The previous run ended unexpectedly",
        )
        .with_context("reason", "forced")
        .with_context("private", "must-not-be-exposed");
        let line = format_log_line(&record);
        let parsed = parse_log_line(line.trim_end_matches('\n')).expect("parse log line");
        assert_eq!(parsed.timestamp, "1970-01-01T00:00:00.000Z");
        assert_eq!(parsed.level, LogLevel::Warn);
        assert_eq!(parsed.module, "application");
        assert_eq!(parsed.code, "application/previous_run_unclean");
        assert_eq!(parsed.message, "The previous run ended unexpectedly");
        assert_eq!(parsed.context_field_count, 2);

        assert!(parse_log_line("not a log line").is_none());
        assert!(
            parse_log_line(&line.replace("application/previous", "application/unknown")).is_none()
        );
        assert!(parse_log_line(&line.replace("WARN ", "WARNX")).is_none());
        assert!(!is_log_file_name(
            LogStream::Application,
            "application-2026-02-30.log"
        ));
        assert!(!is_log_file_name(
            LogStream::Application,
            "application-2026-02-28.0.log"
        ));
        assert!(is_log_file_name(
            LogStream::Application,
            "application-2026-02-28.7.log"
        ));
    }

    #[test]
    fn writer_creates_a_dated_log_and_honors_the_shared_filter() {
        let directory = tempdir().expect("log directory");
        let controller = LogSettingsController::default();
        let writer =
            TextLogWriter::open(directory.path(), LogStream::Application, controller.clone())
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
        let writer = TextLogWriter::open(directory.path(), LogStream::Application, controller)
            .expect("writer");
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
}
