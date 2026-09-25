#![forbid(unsafe_code)]

use bongocat_log::{
    LogLevel, LogRecord, LogSettings, LogSettingsController, LogStream, TextLogWriter,
};
use bongocat_storage::{set_private_directory, set_private_file};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    fs::OpenOptions,
    io::{self, Write},
    panic::{self, PanicHookInfo},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub use bongocat_log::LogLevel as ApplicationLogLevel;

const RUN_MARKER_NAME: &str = "application-running.marker";
const RUN_MARKER_RUNNING: &[u8] = b"{\"schema_version\":1,\"phase\":\"running\"}\n";
const RUN_MARKER_SHUTTING_DOWN: &[u8] = b"{\"schema_version\":1,\"phase\":\"shutting_down\"}\n";
const RUN_MARKER_PANICKED: &[u8] = b"{\"schema_version\":1,\"phase\":\"panicked\"}\n";
const MAX_RECORDED_ONCE_KEYS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogComponent {
    Application,
    Configuration,
    Filesystem,
    Input,
    Logging,
    Model,
    Network,
    Parser,
    Renderer,
    Runtime,
    Service,
    Settings,
    Ui,
    Update,
    Window,
}

impl ApplicationLogComponent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Configuration => "configuration",
            Self::Filesystem => "filesystem",
            Self::Input => "input",
            Self::Logging => "logging",
            Self::Model => "model",
            Self::Network => "network",
            Self::Parser => "parser",
            Self::Renderer => "renderer",
            Self::Runtime => "runtime",
            Self::Service => "service",
            Self::Settings => "settings",
            Self::Ui => "ui",
            Self::Update => "update",
            Self::Window => "window",
        }
    }
}

/// Closed application event catalog. Code and message are fixed project-owned
/// values; only the small, allow-listed context after the message varies.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogCode {
    Started,
    PreviousRunUnclean,
    ShutdownStarted,
    ShutdownCompleted,
    ShutdownFailed,
    Panicked,
    LoggingSettingsChanged,
    RuntimeUnavailable,
    DiagnosticsExportFailed,
    ModelSelectionFallback,
    StartupFailed,
    StateRecovered,
    StatePersistFailed,
    ServiceDegraded,
    ServiceRecovered,
    ServiceFailed,
    ModelPrepareStarted,
    ModelActivationFailed,
    ModelOperationCompleted,
    ModelOperationFailed,
    WindowVisibilityChanged,
    WindowStatePersisted,
    InputStatusChanged,
    InputPermissionUnavailable,
    UpdateUnavailable,
    UpdateCheckCompleted,
    UpdateCheckFailed,
    UpdatePhaseChanged,
    UpdateInstallStarted,
    UpdateInstallCompleted,
    UpdateInstallFailed,
    SettingsCommandFailed,
    UiTransportFailed,
    FilesystemOperationFailed,
    NetworkOperationFailed,
    ParsingFailed,
}

impl ApplicationLogCode {
    pub const ALL: &'static [Self] = &[
        Self::Started,
        Self::PreviousRunUnclean,
        Self::ShutdownStarted,
        Self::ShutdownCompleted,
        Self::ShutdownFailed,
        Self::Panicked,
        Self::LoggingSettingsChanged,
        Self::RuntimeUnavailable,
        Self::DiagnosticsExportFailed,
        Self::ModelSelectionFallback,
        Self::StartupFailed,
        Self::StateRecovered,
        Self::StatePersistFailed,
        Self::ServiceDegraded,
        Self::ServiceRecovered,
        Self::ServiceFailed,
        Self::ModelPrepareStarted,
        Self::ModelActivationFailed,
        Self::ModelOperationCompleted,
        Self::ModelOperationFailed,
        Self::WindowVisibilityChanged,
        Self::WindowStatePersisted,
        Self::InputStatusChanged,
        Self::InputPermissionUnavailable,
        Self::UpdateUnavailable,
        Self::UpdateCheckCompleted,
        Self::UpdateCheckFailed,
        Self::UpdatePhaseChanged,
        Self::UpdateInstallStarted,
        Self::UpdateInstallCompleted,
        Self::UpdateInstallFailed,
        Self::SettingsCommandFailed,
        Self::UiTransportFailed,
        Self::FilesystemOperationFailed,
        Self::NetworkOperationFailed,
        Self::ParsingFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "application/started",
            Self::PreviousRunUnclean => "application/previous_run_unclean",
            Self::ShutdownStarted => "application/shutdown_started",
            Self::ShutdownCompleted => "application/shutdown_completed",
            Self::ShutdownFailed => "application/shutdown_failed",
            Self::Panicked => "application/panicked",
            Self::LoggingSettingsChanged => "logging/settings_changed",
            Self::RuntimeUnavailable => "settings/runtime_unavailable",
            Self::DiagnosticsExportFailed => "settings/diagnostics_export_failed",
            Self::ModelSelectionFallback => "model/selection_fallback",
            Self::StartupFailed => "application/startup_failed",
            Self::StateRecovered => "application/state_recovered",
            Self::StatePersistFailed => "application/state_persist_failed",
            Self::ServiceDegraded => "service/degraded",
            Self::ServiceRecovered => "service/recovered",
            Self::ServiceFailed => "service/failed",
            Self::ModelPrepareStarted => "model/prepare_started",
            Self::ModelActivationFailed => "model/activation_failed",
            Self::ModelOperationCompleted => "model/operation_completed",
            Self::ModelOperationFailed => "model/operation_failed",
            Self::WindowVisibilityChanged => "window/visibility_changed",
            Self::WindowStatePersisted => "window/state_persisted",
            Self::InputStatusChanged => "input/status_changed",
            Self::InputPermissionUnavailable => "input/permission_unavailable",
            Self::UpdateUnavailable => "update/unavailable",
            Self::UpdateCheckCompleted => "update/check_completed",
            Self::UpdateCheckFailed => "update/check_failed",
            Self::UpdatePhaseChanged => "update/phase_changed",
            Self::UpdateInstallStarted => "update/install_started",
            Self::UpdateInstallCompleted => "update/install_completed",
            Self::UpdateInstallFailed => "update/install_failed",
            Self::SettingsCommandFailed => "settings/command_failed",
            Self::UiTransportFailed => "ui/transport_failed",
            Self::FilesystemOperationFailed => "filesystem/operation_failed",
            Self::NetworkOperationFailed => "network/operation_failed",
            Self::ParsingFailed => "parser/failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == value)
    }

    pub const fn level(self) -> LogLevel {
        match self {
            Self::Started
            | Self::ShutdownStarted
            | Self::ShutdownCompleted
            | Self::LoggingSettingsChanged
            | Self::StateRecovered
            | Self::ServiceRecovered
            | Self::ModelOperationCompleted
            | Self::WindowVisibilityChanged
            | Self::InputStatusChanged
            | Self::UpdateUnavailable
            | Self::UpdateCheckCompleted
            | Self::UpdatePhaseChanged
            | Self::UpdateInstallStarted
            | Self::UpdateInstallCompleted => LogLevel::Info,
            Self::PreviousRunUnclean
            | Self::ModelSelectionFallback
            | Self::StatePersistFailed
            | Self::ServiceDegraded
            | Self::InputPermissionUnavailable
            | Self::SettingsCommandFailed => LogLevel::Warn,
            Self::ShutdownFailed
            | Self::Panicked
            | Self::RuntimeUnavailable
            | Self::DiagnosticsExportFailed
            | Self::StartupFailed
            | Self::ServiceFailed
            | Self::ModelActivationFailed
            | Self::ModelOperationFailed
            | Self::UpdateCheckFailed
            | Self::UpdateInstallFailed
            | Self::UiTransportFailed
            | Self::FilesystemOperationFailed
            | Self::NetworkOperationFailed
            | Self::ParsingFailed => LogLevel::Error,
            Self::ModelPrepareStarted | Self::WindowStatePersisted => LogLevel::Debug,
        }
    }

    pub const fn component(self) -> ApplicationLogComponent {
        match self {
            Self::Started
            | Self::PreviousRunUnclean
            | Self::ShutdownStarted
            | Self::ShutdownCompleted
            | Self::ShutdownFailed
            | Self::Panicked
            | Self::StartupFailed
            | Self::StateRecovered
            | Self::StatePersistFailed => ApplicationLogComponent::Application,
            Self::LoggingSettingsChanged => ApplicationLogComponent::Logging,
            Self::RuntimeUnavailable
            | Self::DiagnosticsExportFailed
            | Self::SettingsCommandFailed => ApplicationLogComponent::Settings,
            Self::ModelSelectionFallback
            | Self::ModelPrepareStarted
            | Self::ModelActivationFailed
            | Self::ModelOperationCompleted
            | Self::ModelOperationFailed => ApplicationLogComponent::Model,
            Self::ServiceDegraded | Self::ServiceRecovered | Self::ServiceFailed => {
                ApplicationLogComponent::Service
            }
            Self::WindowVisibilityChanged | Self::WindowStatePersisted => {
                ApplicationLogComponent::Window
            }
            Self::InputStatusChanged | Self::InputPermissionUnavailable => {
                ApplicationLogComponent::Input
            }
            Self::UpdateUnavailable
            | Self::UpdateCheckCompleted
            | Self::UpdateCheckFailed
            | Self::UpdatePhaseChanged
            | Self::UpdateInstallStarted
            | Self::UpdateInstallCompleted
            | Self::UpdateInstallFailed => ApplicationLogComponent::Update,
            Self::UiTransportFailed => ApplicationLogComponent::Ui,
            Self::FilesystemOperationFailed => ApplicationLogComponent::Filesystem,
            Self::NetworkOperationFailed => ApplicationLogComponent::Network,
            Self::ParsingFailed => ApplicationLogComponent::Parser,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::Started => "Application started",
            Self::PreviousRunUnclean => "The previous run ended unexpectedly",
            Self::ShutdownStarted => "Application shutdown started",
            Self::ShutdownCompleted => "Application shutdown completed",
            Self::ShutdownFailed => "Application shutdown failed",
            Self::Panicked => "Application panicked",
            Self::LoggingSettingsChanged => "Logging settings changed",
            Self::RuntimeUnavailable => "Runtime service is unavailable",
            Self::DiagnosticsExportFailed => "Diagnostics export failed",
            Self::ModelSelectionFallback => {
                "The selected model was unavailable; restored the standard preset"
            }
            Self::StartupFailed => "Application startup failed",
            Self::StateRecovered => "Application state was recovered",
            Self::StatePersistFailed => "Application state could not be persisted",
            Self::ServiceDegraded => "A service is degraded",
            Self::ServiceRecovered => "A service recovered",
            Self::ServiceFailed => "A service failed",
            Self::ModelPrepareStarted => "Model preparation started",
            Self::ModelActivationFailed => "Model activation failed",
            Self::ModelOperationCompleted => "Model operation completed",
            Self::ModelOperationFailed => "Model operation failed",
            Self::WindowVisibilityChanged => "Model window visibility changed",
            Self::WindowStatePersisted => "Window state was persisted",
            Self::InputStatusChanged => "Input service status changed",
            Self::InputPermissionUnavailable => "Input monitoring permission is unavailable",
            Self::UpdateUnavailable => "Updates are unavailable",
            Self::UpdateCheckCompleted => "Update check completed",
            Self::UpdateCheckFailed => "Update check failed",
            Self::UpdatePhaseChanged => "Update phase changed",
            Self::UpdateInstallStarted => "Update installation started",
            Self::UpdateInstallCompleted => "Update installation completed",
            Self::UpdateInstallFailed => "Update installation failed",
            Self::SettingsCommandFailed => "Settings command failed",
            Self::UiTransportFailed => "Settings service transport failed",
            Self::FilesystemOperationFailed => "Filesystem operation failed",
            Self::NetworkOperationFailed => "Network operation failed",
            Self::ParsingFailed => "Data parsing failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationLogContext {
    Operation(&'static str),
    Phase(&'static str),
    Service(&'static str),
    Reason(&'static str),
    State(&'static str),
    Source(&'static str),
    Result(&'static str),
    Count(u64),
    Bytes(u64),
    Revision(u64),
    PreviousLevel(&'static str),
    CurrentLevel(&'static str),
    PreviousRetentionDays(u64),
    CurrentRetentionDays(u64),
}

impl ApplicationLogContext {
    const fn key(self) -> &'static str {
        match self {
            Self::Operation(_) => "operation",
            Self::Phase(_) => "phase",
            Self::Service(_) => "service",
            Self::Reason(_) => "reason",
            Self::State(_) => "state",
            Self::Source(_) => "source",
            Self::Result(_) => "result",
            Self::Count(_) => "count",
            Self::Bytes(_) => "bytes",
            Self::Revision(_) => "revision",
            Self::PreviousLevel(_) => "previous_level",
            Self::CurrentLevel(_) => "current_level",
            Self::PreviousRetentionDays(_) => "previous_retention_days",
            Self::CurrentRetentionDays(_) => "current_retention_days",
        }
    }

    fn value(self) -> String {
        match self {
            Self::Operation(value)
            | Self::Phase(value)
            | Self::Service(value)
            | Self::Reason(value)
            | Self::State(value)
            | Self::Source(value)
            | Self::Result(value)
            | Self::PreviousLevel(value)
            | Self::CurrentLevel(value) => value.to_owned(),
            Self::Count(value)
            | Self::Bytes(value)
            | Self::Revision(value)
            | Self::PreviousRetentionDays(value)
            | Self::CurrentRetentionDays(value) => value.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationLogEvent {
    code: ApplicationLogCode,
    context: Vec<ApplicationLogContext>,
}

impl ApplicationLogEvent {
    pub const fn new(code: ApplicationLogCode) -> Self {
        Self {
            code,
            context: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_context(mut self, context: ApplicationLogContext) -> Self {
        self.context.push(context);
        self
    }

    pub const fn started() -> Self {
        Self::new(ApplicationLogCode::Started)
    }

    pub const fn shutdown_started() -> Self {
        Self::new(ApplicationLogCode::ShutdownStarted)
    }

    pub const fn previous_run_unclean() -> Self {
        Self::new(ApplicationLogCode::PreviousRunUnclean)
    }

    pub const fn shutdown_completed() -> Self {
        Self::new(ApplicationLogCode::ShutdownCompleted)
    }

    pub const fn shutdown_failed() -> Self {
        Self::new(ApplicationLogCode::ShutdownFailed)
    }

    pub const fn panicked() -> Self {
        Self::new(ApplicationLogCode::Panicked)
    }

    /// The configured selected model was missing or unusable at startup and
    /// the application fell back to the standard preset model.
    pub const fn model_selection_fallback() -> Self {
        Self::new(ApplicationLogCode::ModelSelectionFallback)
    }

    pub(crate) fn logging_settings_changed(previous: LogSettings, current: LogSettings) -> Self {
        Self::new(ApplicationLogCode::LoggingSettingsChanged)
            .with_context(ApplicationLogContext::PreviousLevel(
                previous.level.as_str(),
            ))
            .with_context(ApplicationLogContext::CurrentLevel(current.level.as_str()))
            .with_context(ApplicationLogContext::PreviousRetentionDays(
                previous.retention_days,
            ))
            .with_context(ApplicationLogContext::CurrentRetentionDays(
                current.retention_days,
            ))
    }

    fn to_record(&self, timestamp: SystemTime) -> LogRecord {
        let mut record = LogRecord::new(
            timestamp,
            self.code.level(),
            self.code.component().as_str(),
            self.code.as_str(),
            self.code.message(),
        );
        for context in &self.context {
            record = record.with_context(context.key(), context.value());
        }
        record
    }

    fn once_key(&self) -> String {
        let mut key = self.code.as_str().to_owned();
        for context in &self.context {
            key.push('\u{1f}');
            key.push_str(context.key());
            key.push('=');
            key.push_str(&context.value());
        }
        key
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApplicationLogEventCounts {
    pub started: u64,
    pub previous_run_unclean: u64,
    pub shutdown_started: u64,
    pub shutdown_completed: u64,
    pub shutdown_failed: u64,
    pub panicked: u64,
    pub runtime_unavailable: u64,
    pub diagnostics_export_failed: u64,
    pub model_selection_fallback: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApplicationLogDiagnostics {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
    pub events: ApplicationLogEventCounts,
}

/// Anonymous retention counters supplied by the Cubism Core log owner.
/// The application deliberately receives no Core log path or message data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogDiagnostics {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplicationLogError {
    #[error("cannot create application log directory: {0}")]
    CreateDirectory(io::Error),
    #[error("cannot open application log file: {0}")]
    OpenFile(io::Error),
    #[error("cannot write application run marker: {0}")]
    WriteRunMarker(io::Error),
    #[error("cannot remove application run marker: {0}")]
    RemoveRunMarker(io::Error),
}

#[derive(Debug)]
struct ApplicationLogState {
    directory: PathBuf,
    writer: TextLogWriter,
    controller: LogSettingsController,
    diagnostics: ApplicationLogDiagnostics,
    code_counts: BTreeMap<ApplicationLogCode, u64>,
    recorded_once: BTreeSet<String>,
}

#[derive(Debug)]
struct ApplicationLogSink {
    state: Mutex<ApplicationLogState>,
}

#[derive(Clone, Debug)]
pub struct ApplicationLogHandle {
    sink: Arc<ApplicationLogSink>,
}

#[derive(Debug)]
pub(crate) struct ApplicationRunMarker {
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreviousRunState {
    ForcedOrUnknown,
    Panic,
    ShutdownInterrupted,
}

type PanicHook = dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static;

pub struct ApplicationPanicHook {
    previous: Option<Box<PanicHook>>,
}

impl std::fmt::Debug for ApplicationPanicHook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationPanicHook")
            .finish_non_exhaustive()
    }
}

impl Drop for ApplicationPanicHook {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if let Some(previous) = self.previous.take() {
            panic::set_hook(previous);
        }
    }
}

impl ApplicationLogHandle {
    pub fn install(directory: impl AsRef<Path>) -> Result<Self, ApplicationLogError> {
        Self::install_with_settings(directory, LogSettings::default(), false)
    }

    pub fn install_with_settings(
        directory: impl AsRef<Path>,
        settings: LogSettings,
        deferred_retention: bool,
    ) -> Result<Self, ApplicationLogError> {
        let directory = directory.as_ref();
        fs::create_dir_all(directory).map_err(ApplicationLogError::CreateDirectory)?;
        set_private_directory(directory).map_err(ApplicationLogError::CreateDirectory)?;
        let controller = LogSettingsController::new(settings);
        let writer = if deferred_retention {
            TextLogWriter::open_deferred(directory, LogStream::Application, controller.clone())
        } else {
            TextLogWriter::open(directory, LogStream::Application, controller.clone())
        }
        .map_err(ApplicationLogError::OpenFile)?;
        let mut state = ApplicationLogState {
            directory: directory.to_owned(),
            writer,
            controller,
            diagnostics: ApplicationLogDiagnostics::default(),
            code_counts: BTreeMap::new(),
            recorded_once: BTreeSet::new(),
        };
        refresh_diagnostics(&mut state);
        Ok(Self {
            sink: Arc::new(ApplicationLogSink {
                state: Mutex::new(state),
            }),
        })
    }

    pub fn record(&self, event: ApplicationLogEvent) {
        self.sink.record(event);
    }

    /// Record an event only once for this process. Use this for polling or
    /// retry loops whose error state is already summarized elsewhere.
    pub fn record_once(&self, event: ApplicationLogEvent) {
        self.sink.record_once(event);
    }

    pub fn diagnostics(&self) -> ApplicationLogDiagnostics {
        let mut state = self
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        refresh_diagnostics(&mut state);
        state.diagnostics
    }

    pub fn settings_controller(&self) -> LogSettingsController {
        self.sink.controller()
    }

    pub fn settings(&self) -> LogSettings {
        self.sink.controller().settings()
    }

    /// Apply a successfully persisted policy to the shared controller. When
    /// raising the threshold, the change is recorded under the old level; when
    /// lowering it, the same event is recorded under the new level.
    pub fn replace_settings(&self, settings: LogSettings) {
        self.sink.replace_settings(settings);
    }

    pub fn install_panic_hook(&self) -> ApplicationPanicHook {
        let previous = panic::take_hook();
        let sink = Arc::clone(&self.sink);
        let directory = self
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .directory
            .clone();
        panic::set_hook(Box::new(move |_| {
            sink.try_record(ApplicationLogEvent::panicked());
            let _ = write_run_marker(&directory.join(RUN_MARKER_NAME), RUN_MARKER_PANICKED);
        }));
        ApplicationPanicHook {
            previous: Some(previous),
        }
    }

    pub(crate) fn begin_run(
        &self,
    ) -> Result<(ApplicationRunMarker, Option<PreviousRunState>), ApplicationLogError> {
        let directory = self
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .directory
            .clone();
        let path = directory.join(RUN_MARKER_NAME);
        let previous_run = fs::read(&path)
            .ok()
            .map(|contents| match contents.as_slice() {
                RUN_MARKER_PANICKED => PreviousRunState::Panic,
                RUN_MARKER_SHUTTING_DOWN => PreviousRunState::ShutdownInterrupted,
                _ => PreviousRunState::ForcedOrUnknown,
            });
        write_run_marker(&path, RUN_MARKER_RUNNING)?;
        Ok((ApplicationRunMarker { path }, previous_run))
    }
}

impl ApplicationRunMarker {
    pub(crate) fn mark_shutdown_started(&self) -> Result<(), ApplicationLogError> {
        write_run_marker(&self.path, RUN_MARKER_SHUTTING_DOWN)
    }

    pub(crate) fn complete(self) -> Result<(), ApplicationLogError> {
        fs::remove_file(&self.path).map_err(ApplicationLogError::RemoveRunMarker)
    }
}

fn write_run_marker(path: &Path, contents: &[u8]) -> Result<(), ApplicationLogError> {
    let mut marker = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(ApplicationLogError::WriteRunMarker)?;
    set_private_file(&marker).map_err(ApplicationLogError::WriteRunMarker)?;
    marker
        .write_all(contents)
        .and_then(|()| marker.sync_all())
        .map_err(ApplicationLogError::WriteRunMarker)
}

impl ApplicationLogSink {
    fn controller(&self) -> LogSettingsController {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .controller
            .clone()
    }

    fn replace_settings(&self, settings: LogSettings) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = state.controller.settings();
        if previous == settings {
            state.writer.refresh_policy();
            refresh_diagnostics(&mut state);
            return;
        }

        let event = ApplicationLogEvent::logging_settings_changed(previous, settings);
        if LogLevel::Info.is_enabled(previous.level) {
            record_event_locked(&mut state, event.clone());
        }
        state.controller.replace_settings(settings);
        state.writer.refresh_policy();
        if !LogLevel::Info.is_enabled(previous.level) && LogLevel::Info.is_enabled(settings.level) {
            record_event_locked(&mut state, event);
        }
        refresh_diagnostics(&mut state);
    }

    fn record(&self, event: ApplicationLogEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        record_event_locked(&mut state, event);
    }

    fn record_once(&self, event: ApplicationLogEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !event
            .code
            .level()
            .is_enabled(state.controller.settings().level)
        {
            return;
        }
        let key = event.once_key();
        let should_record = if state.recorded_once.contains(&key) {
            false
        } else if state.recorded_once.len() < MAX_RECORDED_ONCE_KEYS {
            state.recorded_once.insert(key);
            true
        } else {
            false
        };
        if should_record {
            record_event_locked(&mut state, event);
        }
    }

    fn try_record(&self, event: ApplicationLogEvent) {
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        let timestamp = SystemTime::now();
        let record = event.to_record(timestamp);
        if matches!(state.writer.try_record(record), Some(Ok(true))) {
            count_event(&mut state, event.code);
        }
        refresh_diagnostics(&mut state);
    }
}

fn record_event_locked(state: &mut ApplicationLogState, event: ApplicationLogEvent) {
    let timestamp = SystemTime::now();
    let record = event.to_record(timestamp);
    if matches!(state.writer.record(record), Ok(true)) {
        count_event(state, event.code);
    }
    refresh_diagnostics(state);
}

fn count_event(state: &mut ApplicationLogState, code: ApplicationLogCode) {
    let count = state.code_counts.entry(code).or_default();
    *count = count.saturating_add(1);

    let events = &mut state.diagnostics.events;
    let target = match code {
        ApplicationLogCode::Started => &mut events.started,
        ApplicationLogCode::PreviousRunUnclean => &mut events.previous_run_unclean,
        ApplicationLogCode::ShutdownStarted => &mut events.shutdown_started,
        ApplicationLogCode::ShutdownCompleted => &mut events.shutdown_completed,
        ApplicationLogCode::ShutdownFailed => &mut events.shutdown_failed,
        ApplicationLogCode::Panicked => &mut events.panicked,
        ApplicationLogCode::RuntimeUnavailable => &mut events.runtime_unavailable,
        ApplicationLogCode::DiagnosticsExportFailed => &mut events.diagnostics_export_failed,
        ApplicationLogCode::ModelSelectionFallback => &mut events.model_selection_fallback,
        ApplicationLogCode::LoggingSettingsChanged
        | ApplicationLogCode::StartupFailed
        | ApplicationLogCode::StateRecovered
        | ApplicationLogCode::StatePersistFailed
        | ApplicationLogCode::ServiceDegraded
        | ApplicationLogCode::ServiceRecovered
        | ApplicationLogCode::ServiceFailed
        | ApplicationLogCode::ModelPrepareStarted
        | ApplicationLogCode::ModelActivationFailed
        | ApplicationLogCode::ModelOperationCompleted
        | ApplicationLogCode::ModelOperationFailed
        | ApplicationLogCode::WindowVisibilityChanged
        | ApplicationLogCode::WindowStatePersisted
        | ApplicationLogCode::InputStatusChanged
        | ApplicationLogCode::InputPermissionUnavailable
        | ApplicationLogCode::UpdateUnavailable
        | ApplicationLogCode::UpdateCheckCompleted
        | ApplicationLogCode::UpdateCheckFailed
        | ApplicationLogCode::UpdatePhaseChanged
        | ApplicationLogCode::UpdateInstallStarted
        | ApplicationLogCode::UpdateInstallCompleted
        | ApplicationLogCode::UpdateInstallFailed
        | ApplicationLogCode::SettingsCommandFailed
        | ApplicationLogCode::UiTransportFailed
        | ApplicationLogCode::FilesystemOperationFailed
        | ApplicationLogCode::NetworkOperationFailed
        | ApplicationLogCode::ParsingFailed => return,
    };
    *target = target.saturating_add(1);
}

fn refresh_diagnostics(state: &mut ApplicationLogState) {
    let writer = state.writer.stats();
    state.diagnostics.written = writer.written;
    state.diagnostics.dropped = writer.dropped;
    state.diagnostics.rotated = writer.rotated;
    state.diagnostics.pruned = writer.pruned;
    // The historical diagnostics field is the sum of all retained application
    // files, not only the active one. The shared writer exposes the active
    // value separately for Core's two-field contract.
    state.diagnostics.bytes = writer.retained_bytes;
    state.diagnostics.retained_files = writer.retained_files;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn only_log(directory: &Path) -> PathBuf {
        let mut paths = fs::read_dir(directory)
            .expect("read logs")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("application-") && name.ends_with(".log"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths.pop().expect("application log")
    }

    #[test]
    fn writes_a_single_human_readable_line_with_stable_code() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        handle.record(ApplicationLogEvent::started());
        let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
        assert!(
            contents.contains(" INFO  [application] application/started | Application started\n")
        );
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
    fn typed_context_stays_on_one_bounded_line() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        handle.record(
            ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
                .with_context(ApplicationLogContext::Operation("load_model")),
        );
        let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("operation=load_model"));
    }

    #[test]
    fn record_once_suppresses_only_the_same_code_and_context() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        let event = ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
            .with_context(ApplicationLogContext::Operation("start"));
        handle.record_once(event.clone());
        handle.record_once(event.clone());
        handle.record_once(event.clone());
        handle.record_once(event.with_context(ApplicationLogContext::Operation("stop")));
        assert_eq!(handle.diagnostics().written, 2);
    }

    #[test]
    fn code_catalog_round_trips_and_owns_its_severity_and_message() {
        let mut codes = BTreeSet::new();
        let mut messages = BTreeSet::new();
        for code in ApplicationLogCode::ALL {
            assert!(codes.insert(code.as_str()));
            assert!(messages.insert(code.message()));
            assert_eq!(ApplicationLogCode::parse(code.as_str()), Some(*code));
            let record = ApplicationLogEvent::new(*code).to_record(SystemTime::UNIX_EPOCH);
            assert_eq!(record.level, code.level());
            assert_eq!(record.code, code.as_str());
            assert_eq!(record.module, code.component().as_str());
            assert_eq!(record.message, code.message());
        }
        assert_eq!(ApplicationLogCode::parse("unknown/event"), None);
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

    #[test]
    fn panic_hook_writes_only_a_stable_event_and_restores_the_previous_hook() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        let hook = handle.install_panic_hook();
        let panic = std::thread::spawn(|| {
            panic!("private payload /Users/example/secret-model/model3.json")
        })
        .join();
        assert!(panic.is_err());
        drop(hook);

        let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
        assert!(
            contents.contains("ERROR [application] application/panicked | Application panicked")
        );
        assert!(!contents.contains("secret-model"));
        assert!(handle.diagnostics().written >= 1);
    }

    #[test]
    fn panic_record_drops_instead_of_waiting_for_the_log_lock() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        let state = handle.sink.state.lock().expect("state lock");
        handle.sink.try_record(ApplicationLogEvent::panicked());
        assert_eq!(state.diagnostics.written, 0);
    }

    #[test]
    fn run_marker_survives_unclean_drop_and_is_removed_on_completion() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        let (marker, previous) = handle.begin_run().expect("begin first run");
        assert_eq!(previous, None);
        let marker_path = directory.path().join(RUN_MARKER_NAME);
        assert_eq!(
            fs::read(&marker_path).expect("marker bytes"),
            RUN_MARKER_RUNNING
        );
        drop(marker);

        let (marker, previous) = handle.begin_run().expect("begin recovered run");
        assert_eq!(previous, Some(PreviousRunState::ForcedOrUnknown));
        marker.complete().expect("complete run");
        assert!(!marker_path.exists());
    }

    #[test]
    fn run_marker_classifies_panic_and_interrupted_shutdown_without_retaining_them() {
        let directory = tempdir().expect("temporary directory");
        let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
        let marker_path = directory.path().join(RUN_MARKER_NAME);

        fs::write(&marker_path, RUN_MARKER_PANICKED).expect("panic marker");
        let (marker, previous) = handle.begin_run().expect("recover panic");
        assert_eq!(previous, Some(PreviousRunState::Panic));
        assert_eq!(
            fs::read(&marker_path).expect("new running marker"),
            RUN_MARKER_RUNNING
        );
        marker.mark_shutdown_started().expect("mark shutdown start");
        drop(marker);

        let (marker, previous) = handle.begin_run().expect("recover interrupted shutdown");
        assert_eq!(previous, Some(PreviousRunState::ShutdownInterrupted));
        marker.complete().expect("complete recovered run");
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
}
