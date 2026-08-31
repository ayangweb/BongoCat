#![forbid(unsafe_code)]

use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_FILE_PREFIX: &str = "application-";
const LOG_FILE_SUFFIX: &str = ".jsonl";
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_LOG_FILES: usize = 8;
const MAX_TOTAL_LOG_BYTES: u64 = 8 * 1024 * 1024;
const RETENTION_DAYS: u64 = 7;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogComponent {
    Application,
    Configuration,
    Input,
    Model,
    Renderer,
    Runtime,
    Settings,
}

impl ApplicationLogComponent {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Configuration => "configuration",
            Self::Input => "input",
            Self::Model => "model",
            Self::Renderer => "renderer",
            Self::Runtime => "runtime",
            Self::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogLevel {
    Info,
    Warn,
    Error,
}

impl ApplicationLogLevel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogCode {
    Started,
    ShutdownStarted,
    ShutdownFailed,
    RuntimeUnavailable,
    DiagnosticsExportFailed,
}

impl ApplicationLogCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::ShutdownStarted => "shutdown_started",
            Self::ShutdownFailed => "shutdown_failed",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::DiagnosticsExportFailed => "diagnostics_export_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ApplicationLogEvent {
    pub component: ApplicationLogComponent,
    pub level: ApplicationLogLevel,
    pub code: ApplicationLogCode,
}

impl ApplicationLogEvent {
    pub const fn started() -> Self {
        Self {
            component: ApplicationLogComponent::Application,
            level: ApplicationLogLevel::Info,
            code: ApplicationLogCode::Started,
        }
    }

    pub const fn shutdown_started() -> Self {
        Self {
            component: ApplicationLogComponent::Application,
            level: ApplicationLogLevel::Info,
            code: ApplicationLogCode::ShutdownStarted,
        }
    }

    pub const fn shutdown_failed() -> Self {
        Self {
            component: ApplicationLogComponent::Application,
            level: ApplicationLogLevel::Error,
            code: ApplicationLogCode::ShutdownFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApplicationLogDiagnostics {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
}

#[derive(Debug)]
pub enum ApplicationLogError {
    CreateDirectory(io::Error),
    OpenFile(io::Error),
}

impl std::fmt::Display for ApplicationLogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDirectory(error) => {
                write!(
                    formatter,
                    "cannot create application log directory: {error}"
                )
            }
            Self::OpenFile(error) => write!(formatter, "cannot open application log file: {error}"),
        }
    }
}

impl std::error::Error for ApplicationLogError {}

#[derive(Debug)]
struct ApplicationLogState {
    directory: PathBuf,
    day: u64,
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    diagnostics: ApplicationLogDiagnostics,
    code_counts: BTreeMap<ApplicationLogEvent, u64>,
}

#[derive(Debug)]
struct ApplicationLogSink {
    state: Mutex<ApplicationLogState>,
}

#[derive(Debug)]
pub struct ApplicationLogHandle {
    sink: Arc<ApplicationLogSink>,
}

#[derive(Serialize)]
struct ApplicationLogRecord {
    component: &'static str,
    level: &'static str,
    code: &'static str,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LogFile {
    day: u64,
    generation: usize,
    path: PathBuf,
    bytes: u64,
}

impl ApplicationLogHandle {
    pub fn install(directory: impl AsRef<Path>) -> Result<Self, ApplicationLogError> {
        Ok(Self {
            sink: Arc::new(ApplicationLogSink::open(directory.as_ref(), current_day())?),
        })
    }

    pub fn record(&self, event: ApplicationLogEvent) {
        self.sink.record(current_day(), event);
    }

    pub fn diagnostics(&self) -> ApplicationLogDiagnostics {
        self.sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .diagnostics
    }
}

impl ApplicationLogSink {
    fn open(directory: &Path, day: u64) -> Result<Self, ApplicationLogError> {
        fs::create_dir_all(directory).map_err(ApplicationLogError::CreateDirectory)?;
        let path = active_path(directory, day);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(ApplicationLogError::OpenFile)?;
        let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let mut state = ApplicationLogState {
            directory: directory.to_owned(),
            day,
            path,
            file: Some(file),
            bytes,
            diagnostics: ApplicationLogDiagnostics {
                bytes,
                retained_files: 1,
                ..ApplicationLogDiagnostics::default()
            },
            code_counts: BTreeMap::new(),
        };
        prune_logs(&mut state, day);
        refresh_totals(&mut state);
        Ok(Self {
            state: Mutex::new(state),
        })
    }

    fn record(&self, day: u64, event: ApplicationLogEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if day != state.day {
            switch_day(&mut state, day);
        }
        let record = ApplicationLogRecord {
            component: event.component.as_str(),
            level: event.level.as_str(),
            code: event.code.as_str(),
        };
        let Ok(mut line) = serde_json::to_vec(&record) else {
            state.diagnostics.dropped = state.diagnostics.dropped.saturating_add(1);
            return;
        };
        line.push(b'\n');
        let Ok(line_len) = u64::try_from(line.len()) else {
            state.diagnostics.dropped = state.diagnostics.dropped.saturating_add(1);
            return;
        };
        if state.bytes.saturating_add(line_len) > MAX_LOG_BYTES && !rotate_active(&mut state) {
            state.diagnostics.dropped = state.diagnostics.dropped.saturating_add(1);
            return;
        }
        let Some(file) = state.file.as_mut() else {
            state.diagnostics.dropped = state.diagnostics.dropped.saturating_add(1);
            return;
        };
        if file.write_all(&line).is_err() || file.flush().is_err() {
            state.diagnostics.dropped = state.diagnostics.dropped.saturating_add(1);
            return;
        }
        state.bytes = state.bytes.saturating_add(line_len);
        state.diagnostics.written = state.diagnostics.written.saturating_add(1);
        state.diagnostics.bytes = state.diagnostics.bytes.saturating_add(line_len);
        *state.code_counts.entry(event).or_default() = state
            .code_counts
            .get(&event)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        prune_logs(&mut state, day);
        refresh_totals(&mut state);
    }
}

fn switch_day(state: &mut ApplicationLogState, day: u64) {
    if let Some(file) = state.file.take() {
        let _ = file.sync_all();
    }
    state.day = day;
    state.path = active_path(&state.directory, day);
    state.bytes = 0;
    state.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
        .ok();
    if let Some(file) = state.file.as_ref() {
        state.bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    }
    prune_logs(state, day);
    refresh_totals(state);
}

fn rotate_active(state: &mut ApplicationLogState) -> bool {
    let Some(file) = state.file.take() else {
        return false;
    };
    if file.sync_all().is_err() {
        state.file = None;
        return false;
    }
    drop(file);
    for generation in (1..MAX_LOG_FILES).rev() {
        let source = rotated_path(&state.path, generation);
        let destination = rotated_path(&state.path, generation + 1);
        let _ = fs::remove_file(&destination);
        if source.exists() && fs::rename(&source, &destination).is_err() {
            reopen_active(state);
            return false;
        }
    }
    let first = rotated_path(&state.path, 1);
    let _ = fs::remove_file(&first);
    if fs::rename(&state.path, &first).is_err() {
        reopen_active(state);
        return false;
    }
    if !reopen_active(state) {
        let _ = fs::rename(&first, &state.path);
        reopen_active(state);
        return false;
    }
    state.bytes = 0;
    state.diagnostics.rotated = state.diagnostics.rotated.saturating_add(1);
    refresh_totals(state);
    true
}

fn reopen_active(state: &mut ApplicationLogState) -> bool {
    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)
    else {
        return false;
    };
    state.bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    state.file = Some(file);
    true
}

fn prune_logs(state: &mut ApplicationLogState, current_day: u64) {
    let mut files = collect_log_files(&state.directory);
    let oldest_day = current_day.saturating_sub(RETENTION_DAYS.saturating_sub(1));
    for file in files.iter().filter(|file| file.day < oldest_day) {
        if fs::remove_file(&file.path).is_ok() {
            state.diagnostics.pruned = state.diagnostics.pruned.saturating_add(1);
        }
    }
    files.retain(|file| file.day >= oldest_day);
    files.sort_by(|left, right| {
        right
            .day
            .cmp(&left.day)
            .then_with(|| left.generation.cmp(&right.generation))
    });
    let mut total_bytes = files.iter().map(|file| file.bytes).sum::<u64>();
    for file in files.iter().skip(MAX_LOG_FILES) {
        if file.path == state.path {
            continue;
        }
        if fs::remove_file(&file.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(file.bytes);
            state.diagnostics.pruned = state.diagnostics.pruned.saturating_add(1);
        }
    }
    let mut remaining = collect_log_files(&state.directory);
    remaining.sort_by(|left, right| {
        right
            .day
            .cmp(&left.day)
            .then_with(|| left.generation.cmp(&right.generation))
    });
    total_bytes = remaining.iter().map(|file| file.bytes).sum();
    for file in remaining.iter().rev() {
        if total_bytes <= MAX_TOTAL_LOG_BYTES || file.path == state.path {
            continue;
        }
        if fs::remove_file(&file.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(file.bytes);
            state.diagnostics.pruned = state.diagnostics.pruned.saturating_add(1);
        }
    }
    refresh_totals(state);
}

fn collect_log_files(directory: &Path) -> Vec<LogFile> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let suffix = name.strip_prefix(LOG_FILE_PREFIX)?;
            let (day_text, generation) = if let Some(day) = suffix.strip_suffix(LOG_FILE_SUFFIX) {
                (day, 0)
            } else if let Some((day, generation)) = suffix.split_once(".jsonl.") {
                (day, generation.parse::<usize>().ok()?)
            } else {
                return None;
            };
            let day = day_text.parse::<u64>().ok()?;
            let bytes = entry.metadata().ok()?.len();
            Some(LogFile {
                day,
                generation,
                path,
                bytes,
            })
        })
        .collect()
}

fn refresh_totals(state: &mut ApplicationLogState) {
    let files = collect_log_files(&state.directory);
    state.diagnostics.bytes = files.iter().map(|file| file.bytes).sum::<u64>();
    state.diagnostics.retained_files = files.len() as u64;
}

fn active_path(directory: &Path, day: u64) -> PathBuf {
    directory.join(format!("{LOG_FILE_PREFIX}{day}{LOG_FILE_SUFFIX}"))
}

fn rotated_path(path: &Path, generation: usize) -> PathBuf {
    let mut rotated = path.as_os_str().to_owned();
    rotated.push(format!(".{generation}"));
    PathBuf::from(rotated)
}

fn current_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() / SECONDS_PER_DAY)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sink(directory: &Path, day: u64) -> ApplicationLogSink {
        ApplicationLogSink::open(directory, day).expect("open application log")
    }

    #[test]
    fn writes_only_stable_fields_and_aggregates_codes() {
        let directory = tempdir().expect("temporary directory");
        let sink = sink(directory.path(), 20_000);
        sink.record(20_000, ApplicationLogEvent::started());
        let state = sink.state.lock().expect("state lock");
        assert_eq!(state.diagnostics.written, 1);
        assert_eq!(state.code_counts.len(), 1);
        let contents = fs::read_to_string(&state.path).expect("log contents");
        assert_eq!(
            contents,
            "{\"component\":\"application\",\"level\":\"info\",\"code\":\"started\"}\n"
        );
        assert!(!contents.contains("/"));
    }

    #[test]
    fn switches_day_and_prunes_old_files() {
        let directory = tempdir().expect("temporary directory");
        fs::write(active_path(directory.path(), 1), b"old").expect("old log");
        let sink = sink(directory.path(), 10);
        sink.record(10, ApplicationLogEvent::started());
        let mut state = sink.state.lock().expect("state lock");
        switch_day(&mut state, 11);
        assert!(!active_path(directory.path(), 1).exists());
        assert!(active_path(directory.path(), 11).is_file());
        assert_eq!(state.diagnostics.retained_files, 2);
    }

    #[test]
    fn rotates_at_one_mib_and_keeps_bounded_file_set() {
        let directory = tempdir().expect("temporary directory");
        let path = active_path(directory.path(), 42);
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).expect("seed log");
        let sink = sink(directory.path(), 42);
        sink.record(42, ApplicationLogEvent::shutdown_started());
        let state = sink.state.lock().expect("state lock");
        assert_eq!(state.diagnostics.rotated, 1);
        assert!(rotated_path(&path, 1).is_file());
        assert!(state.diagnostics.retained_files <= MAX_LOG_FILES as u64);
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
}
