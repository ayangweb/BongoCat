//! The application-owned update worker.
//!
//! One dedicated thread owns the [`UpdateRuntime`] for the lifetime of the process
//! and is the only thing that touches the network. It converts the update
//! subsystem's own types into the UI protocol in `bongocat-ui-protocol` and publishes
//! the result into shared state, so the GPUI thread never blocks on a check, a transfer
//! or an install, and a window that is closed or slow cannot stall the worker.
//!
//! Restarting is the one step the worker cannot take: replacing the process image
//! requires the product's shutdown sequence, which belongs to the application. The
//! worker therefore only records the request and the application acts on it.

use crate::app_log::{
    ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent, ApplicationLogHandle,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use bongocat_config::BuildEnvironment;
use bongocat_ui_protocol::{
    UpdateClient, UpdateCommand, UpdateErrorCode, UpdateFailureStage, UpdatePhase,
    UpdateProgressInfo, UpdateReleaseInfo, UpdateServiceEndpoint, UpdateSnapshot,
    UpdateStateHandle, UpdateUnavailableReason,
};
use bongocat_update::{
    UpdateDiagnosticsTracker, UpdateError, UpdateEvent, UpdateOutcome, UpdateRelease,
    UpdateRuntime, UpdateUnavailability,
};

mod engine;
#[cfg(test)]
mod tests;
mod translate;
mod worker;

// The three modules are one pipeline: the service starts a worker, the worker
// drives an engine, and both publish through the translation. They reach each
// other through this one prelude rather than naming two modules apiece.
pub(crate) use engine::UpdateEngine;
pub(crate) use translate::*;
pub(crate) use worker::run_worker;

pub(crate) const UPDATE_COMMAND_CAPACITY: usize = 8;

/// Whether the running process keeps executing the previous release after an install.
///
/// macOS replaces the `.app` in place, so the process has to be replaced before the
/// new build is used. Windows hands the payload to the NSIS installer, which
/// replaces the files and relaunches the application itself; the update library
/// exits the process before returning there, so a completed install is never
/// observed on that platform.
pub const fn restart_required_after_install() -> bool {
    cfg!(target_os = "macos")
}

/// The update worker the product owns.
pub struct ApplicationUpdateService {
    pub(crate) client: UpdateClient,
    pub(crate) state: UpdateStateHandle,
    pub(crate) restart_requested: Arc<AtomicBool>,
    pub(crate) worker: Option<thread::JoinHandle<()>>,
}

impl ApplicationUpdateService {
    /// Start the worker for this build.
    ///
    /// The runtime is derived from the immutable build environment and the compiled
    /// version, so no runtime input can retarget an update.
    pub fn start(
        environment: BuildEnvironment,
        current_version: &'static str,
        diagnostics: UpdateDiagnosticsTracker,
        application_log: ApplicationLogHandle,
    ) -> Result<Self, UpdateServiceError> {
        Self::start_with_engine_and_log(
            UpdateRuntime::for_current_build(environment, current_version, diagnostics),
            current_version,
            Some(application_log),
        )
    }

    #[cfg(test)]
    pub(crate) fn start_with_engine(
        engine: impl UpdateEngine,
        current_version: &'static str,
    ) -> Result<Self, UpdateServiceError> {
        Self::start_with_engine_and_log(engine, current_version, None)
    }

    pub(crate) fn start_with_engine_and_log(
        engine: impl UpdateEngine,
        current_version: &'static str,
        application_log: Option<ApplicationLogHandle>,
    ) -> Result<Self, UpdateServiceError> {
        let initial_phase = match engine.unavailability() {
            Some(reason) => UpdatePhase::Unavailable {
                reason: unavailable_reason(reason),
            },
            None => UpdatePhase::Idle,
        };
        let state = UpdateStateHandle::new(UpdateSnapshot::new(current_version, initial_phase));
        let (client, endpoint) = UpdateClient::bounded(UPDATE_COMMAND_CAPACITY);
        let client = client.track_state(state.clone());
        let restart_requested = Arc::new(AtomicBool::new(false));
        let worker_restart = Arc::clone(&restart_requested);
        let worker_state = state.clone();
        let worker = thread::Builder::new()
            .name("bongocat-update-service".to_owned())
            .spawn(move || {
                run_worker(
                    Box::new(engine),
                    endpoint,
                    worker_state,
                    worker_restart,
                    application_log,
                )
            })
            .map_err(UpdateServiceError::Spawn)?;
        Ok(Self {
            client,
            state,
            restart_requested,
            worker: Some(worker),
        })
    }

    pub fn client(&self) -> UpdateClient {
        self.client.clone()
    }

    pub fn state(&self) -> UpdateStateHandle {
        self.state.clone()
    }

    /// Take a pending restart request, clearing it.
    ///
    /// The window asks for a restart through the command channel; the application
    /// polls this because only it can run the product shutdown sequence.
    pub fn take_restart_request(&self) -> bool {
        self.restart_requested.swap(false, Ordering::AcqRel)
    }

    pub fn join(mut self) -> Result<(), UpdateServiceError> {
        self.shutdown_worker()
    }

    pub(crate) fn shutdown_worker(&mut self) -> Result<(), UpdateServiceError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let _ = self.client.request_shutdown();
        worker.join().map_err(|_| UpdateServiceError::Panicked)
    }
}

impl Drop for ApplicationUpdateService {
    fn drop(&mut self) {
        let _ = self.shutdown_worker();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateServiceError {
    #[error("failed to start update service: {0}")]
    Spawn(std::io::Error),
    #[error("update service panicked")]
    Panicked,
}
