//! The update worker's tests, split by the module they cover.
//!
//! The imports below are the worker's own, rewritten for where they now live:
//! everything the tests read is either the service or something the root already
//! re-exports.

use super::{
    ApplicationUpdateService, UpdateEngine, UpdateErrorCode, UpdateFailureStage, UpdatePhase,
    error_code, failure_stage, progress_info, restart_required_after_install, unavailable_reason,
};
use crate::app_log::{ApplicationLogCode, ApplicationLogHandle};
use bongocat_ui_protocol::{UpdateCommand, UpdateStateHandle, UpdateUnavailableReason};
use bongocat_update::{
    UpdateError, UpdateErrorCode as SourceCode, UpdateEvent, UpdateOutcome, UpdateProgress,
    UpdateRelease, UpdateStage, UpdateUnavailability,
};
use std::{
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tempfile::tempdir;

/// A scripted stand-in for the update pipeline.
///
/// The worker's contract is the sequence of phases it publishes, so the engine is
/// scripted rather than faked: the test says exactly which events and which outcome
/// the pipeline produces and then asserts what the window would have seen.
struct ScriptedEngine {
    unavailability: Option<UpdateUnavailability>,
    check_outcome: Result<UpdateOutcome, UpdateError>,
    install_outcome: Result<UpdateOutcome, UpdateError>,
    events: Vec<UpdateEvent>,
    installs: Arc<Mutex<u32>>,
}

impl ScriptedEngine {
    /// A build whose check produces `outcome` and whose install is never entered.
    fn available(outcome: Result<UpdateOutcome, UpdateError>) -> Self {
        Self {
            unavailability: None,
            check_outcome: outcome,
            install_outcome: Ok(UpdateOutcome::UpToDate),
            events: Vec::new(),
            installs: Arc::new(Mutex::new(0)),
        }
    }

    /// A build whose check finds `release()` and whose install produces `outcome`
    /// after replaying `events`.
    fn installing(outcome: Result<UpdateOutcome, UpdateError>, events: Vec<UpdateEvent>) -> Self {
        Self {
            unavailability: None,
            check_outcome: Ok(UpdateOutcome::Available { release: release() }),
            install_outcome: outcome,
            events,
            installs: Arc::new(Mutex::new(0)),
        }
    }
}

impl UpdateEngine for ScriptedEngine {
    fn unavailability(&self) -> Option<UpdateUnavailability> {
        self.unavailability
    }

    fn release_page_url(&self, version: &str) -> Option<String> {
        Some(format!("https://example.invalid/v{version}"))
    }

    fn check(&self) -> Result<UpdateOutcome, UpdateError> {
        self.check_outcome.clone()
    }

    fn install(&self, observe: &dyn Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError> {
        *self.installs.lock().expect("install counter") += 1;
        for event in &self.events {
            observe(*event);
        }
        self.install_outcome.clone()
    }
}

fn release() -> UpdateRelease {
    UpdateRelease {
        version: "1.2.0".to_owned(),
        notes: Some("- a change".to_owned()),
    }
}

/// Run one command through the real worker and return the phase it settles on.
///
/// The worker runs on its own thread exactly as it does in the product, so this
/// covers the channel, the published state and the phase mapping together.
fn settled_phase(engine: ScriptedEngine, command: UpdateCommand) -> (UpdatePhase, u64) {
    let service =
        ApplicationUpdateService::start_with_engine(engine, "1.1.0").expect("start the worker");
    let client = service.client();
    let state = service.state();
    assert!(
        !matches!(state.phase(), UpdatePhase::Unavailable { .. }),
        "the scripted engine is available"
    );
    let mut settled_from = state.snapshot().revision;
    match command {
        UpdateCommand::Check => client.request_check().expect("queue a check"),
        UpdateCommand::Install => {
            // An install needs a release to install, so a check runs first.
            client.request_check().expect("queue a check");
            settled_from = wait_for_settled(&state, settled_from).1;
            client.request_install().expect("queue an install");
        }
        other => panic!("unsupported scripted command {other:?}"),
    }
    let (phase, revision) = wait_for_settled(&state, settled_from);
    service.join().expect("join the worker");
    (phase, revision)
}

/// Wait until the worker has published something new *and* stopped working.
///
/// Waiting for "not busy" alone would return the idle phase the command has not
/// replaced yet; waiting for a revision bump alone could catch an intermediate
/// step of a multi-step install.
fn wait_for_settled(state: &UpdateStateHandle, from: u64) -> (UpdatePhase, u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = state.snapshot();
        if snapshot.revision > from && !snapshot.phase.is_busy() {
            return (snapshot.phase, snapshot.revision);
        }
        assert!(
            Instant::now() < deadline,
            "the worker did not settle, last snapshot was {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

mod root;
mod translate;
mod worker;
