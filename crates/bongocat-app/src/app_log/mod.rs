//! The application's own log.
//!
//! Distinct from the shared logger: this one has a stable code per event, a
//! bounded context, a record-once filter, and a marker saying how the last run
//! ended. Those are the things a support thread reads, so they are the things that
//! live here.

#![forbid(unsafe_code)]

use bongocat_log::{
    LogLevel, LogRecord, LogSettings, LogSettingsController, LogStream, TextLogWriter,
};
use bongocat_storage::{set_private_directory, set_private_file};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub use bongocat_log::LogLevel as ApplicationLogLevel;

const RUN_MARKER_NAME: &str = "application-running.marker";
const RUN_MARKER_RUNNING: &[u8] = b"{\"schema_version\":1,\"phase\":\"running\"}\n";
const RUN_MARKER_SHUTTING_DOWN: &[u8] = b"{\"schema_version\":1,\"phase\":\"shutting_down\"}\n";

mod code;
mod component;
mod context;
mod core_diagnostics;
mod error;
mod event;
mod panic_hook;
mod run_marker;
mod sink;
#[cfg(test)]
mod tests;
pub(crate) use run_marker::*;
pub(crate) use sink::*;
// The rest are reached by the `pub use` lines below: they are already visible
// to the crate, and a `pub(crate)` glob would narrow them to nothing.

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use code::ApplicationLogCode;
pub use component::ApplicationLogComponent;
pub use context::ApplicationLogContext;
pub use core_diagnostics::CoreLogDiagnostics;
pub use error::ApplicationLogError;
pub use event::{ApplicationLogDiagnostics, ApplicationLogEvent, ApplicationLogEventCounts};
pub use panic_hook::ApplicationPanicHook;

#[derive(Clone, Debug)]
pub struct ApplicationLogHandle {
    pub(crate) sink: Arc<ApplicationLogSink>,
}

impl ApplicationLogHandle {
    pub fn install(directory: impl AsRef<Path>) -> Result<Self, ApplicationLogError> {
        Self::install_with_settings(directory, LogSettings::default(), false)
    }
}

impl ApplicationLogHandle {
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
}

impl ApplicationLogHandle {
    pub fn record(&self, event: ApplicationLogEvent) {
        self.sink.record(event);
    }
}

impl ApplicationLogHandle {
    /// Record an event only once for this process. Use this for polling or
    /// retry loops whose error state is already summarized elsewhere.
    pub fn record_once(&self, event: ApplicationLogEvent) {
        self.sink.record_once(event);
    }
}

impl ApplicationLogHandle {
    pub fn diagnostics(&self) -> ApplicationLogDiagnostics {
        let mut state = self
            .sink
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        refresh_diagnostics(&mut state);
        state.diagnostics
    }
}

impl ApplicationLogHandle {
    pub fn settings_controller(&self) -> LogSettingsController {
        self.sink.controller()
    }
}

impl ApplicationLogHandle {
    pub fn settings(&self) -> LogSettings {
        self.sink.controller().settings()
    }
}

impl ApplicationLogHandle {
    /// Apply a successfully persisted policy to the shared controller. When
    /// raising the threshold, the change is recorded under the old level; when
    /// lowering it, the same event is recorded under the new level.
    pub fn replace_settings(&self, settings: LogSettings) {
        self.sink.replace_settings(settings);
    }
}

impl ApplicationLogHandle {
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
