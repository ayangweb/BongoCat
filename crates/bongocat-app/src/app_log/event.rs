//! One event, and the counts a diagnostics report reads.
//!
//! The counts are kept rather than derived from the file, because a file that has
//! been rotated or trimmed would report a history the application never had. They
//! are refreshed under the same lock the events are, so a report cannot observe a
//! count that belongs to a moment the log was not in.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationLogEvent {
    pub(crate) code: ApplicationLogCode,
    pub(crate) context: Vec<ApplicationLogContext>,
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

    pub(crate) fn to_record(&self, timestamp: SystemTime) -> LogRecord {
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

    pub(crate) fn once_key(&self) -> String {
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
