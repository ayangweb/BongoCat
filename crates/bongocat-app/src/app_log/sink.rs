//! The state behind the lock, and the sink that writes it.
//!
//! One lock covers the filter, the record-once set and the counters together,
//! because a filter that changed between deciding and writing would emit a line the
//! user had just turned off. The sink is what holds the open file; the handle is
//! what the application holds, and neither knows about the other.

use super::*;

pub(crate) const MAX_RECORDED_ONCE_KEYS: usize = 256;

#[derive(Debug)]
pub(crate) struct ApplicationLogState {
    pub(crate) directory: PathBuf,
    pub(crate) writer: TextLogWriter,
    pub(crate) controller: LogSettingsController,
    pub(crate) diagnostics: ApplicationLogDiagnostics,
    pub(crate) code_counts: BTreeMap<ApplicationLogCode, u64>,
    pub(crate) recorded_once: BTreeSet<String>,
}

#[derive(Debug)]
pub(crate) struct ApplicationLogSink {
    pub(crate) state: Mutex<ApplicationLogState>,
}

impl ApplicationLogSink {
    pub(crate) fn controller(&self) -> LogSettingsController {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .controller
            .clone()
    }

    pub(crate) fn replace_settings(&self, settings: LogSettings) {
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

    pub(crate) fn record(&self, event: ApplicationLogEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        record_event_locked(&mut state, event);
    }

    pub(crate) fn record_once(&self, event: ApplicationLogEvent) {
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

    pub(crate) fn try_record(&self, event: ApplicationLogEvent) {
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

pub(crate) fn record_event_locked(state: &mut ApplicationLogState, event: ApplicationLogEvent) {
    let timestamp = SystemTime::now();
    let record = event.to_record(timestamp);
    if matches!(state.writer.record(record), Ok(true)) {
        count_event(state, event.code);
    }
    refresh_diagnostics(state);
}

pub(crate) fn count_event(state: &mut ApplicationLogState, code: ApplicationLogCode) {
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

pub(crate) fn refresh_diagnostics(state: &mut ApplicationLogState) {
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
