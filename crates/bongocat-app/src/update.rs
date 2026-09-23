//! The application-owned update worker.
//!
//! One dedicated thread owns the [`UpdateRuntime`] for the lifetime of the process
//! and is the only thing that touches the network. It converts the update
//! subsystem's own types into the UI protocol in `bongocat_ui` and publishes the
//! result into shared state, so the GPUI thread never blocks on a check, a transfer
//! or an install, and a window that is closed or slow cannot stall the worker.
//!
//! Restarting is the one step the worker cannot take: replacing the process image
//! requires the product's shutdown sequence, which belongs to the application. The
//! worker therefore only records the request and the application acts on it.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use bongocat_config::BuildEnvironment;
use bongocat_ui::{
    UpdateClient, UpdateCommand, UpdateErrorCode, UpdateFailureStage, UpdatePhase,
    UpdateProgressInfo, UpdateReleaseInfo, UpdateServiceEndpoint, UpdateSnapshot,
    UpdateStateHandle, UpdateUnavailableReason,
};
use bongocat_update::{
    UpdateDiagnosticsTracker, UpdateError, UpdateEvent, UpdateOutcome, UpdateRelease,
    UpdateRuntime, UpdateUnavailability,
};

const UPDATE_COMMAND_CAPACITY: usize = 8;

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

/// The update pipeline the worker drives.
///
/// The worker's own job is the state machine around the pipeline: which phase is
/// published when, which failures are attributed to which stage, and what happens to
/// the phase the window is rendering. Depending on this trait rather than on
/// [`UpdateRuntime`] directly is what lets that state machine be exercised without a
/// network, a published release or a real install.
trait UpdateEngine: Send + 'static {
    fn unavailability(&self) -> Option<UpdateUnavailability>;
    fn release_page_url(&self, version: &str) -> Option<String>;
    fn check(&self) -> Result<UpdateOutcome, UpdateError>;
    fn install(&self, observe: &dyn Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError>;
}

impl UpdateEngine for UpdateRuntime {
    fn unavailability(&self) -> Option<UpdateUnavailability> {
        UpdateRuntime::unavailability(self)
    }

    fn release_page_url(&self, version: &str) -> Option<String> {
        UpdateRuntime::release_page_url(self, version)
    }

    fn check(&self) -> Result<UpdateOutcome, UpdateError> {
        UpdateRuntime::check(self)
    }

    fn install(&self, observe: &dyn Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError> {
        self.install_with_observer(observe)
    }
}

/// The update worker the product owns.
pub struct ApplicationUpdateService {
    client: UpdateClient,
    state: UpdateStateHandle,
    restart_requested: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
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
    ) -> Result<Self, UpdateServiceError> {
        Self::start_with_engine(
            UpdateRuntime::for_current_build(environment, current_version, diagnostics),
            current_version,
        )
    }

    fn start_with_engine(
        engine: impl UpdateEngine,
        current_version: &'static str,
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
            .spawn(move || run_worker(Box::new(engine), endpoint, worker_state, worker_restart))
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

    fn shutdown_worker(&mut self) -> Result<(), UpdateServiceError> {
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

#[derive(Debug)]
pub enum UpdateServiceError {
    Spawn(std::io::Error),
    Panicked,
}

impl fmt::Display for UpdateServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "failed to start update service: {error}"),
            Self::Panicked => formatter.write_str("update service panicked"),
        }
    }
}

impl std::error::Error for UpdateServiceError {}

fn run_worker(
    engine: Box<dyn UpdateEngine>,
    endpoint: UpdateServiceEndpoint,
    state: UpdateStateHandle,
    restart_requested: Arc<AtomicBool>,
) {
    loop {
        match endpoint.recv_blocking() {
            Ok(UpdateCommand::Check) => run_check(engine.as_ref(), &state),
            Ok(UpdateCommand::Install) => run_install(engine.as_ref(), &state),
            Ok(UpdateCommand::Restart) => {
                restart_requested.store(true, Ordering::Release);
            }
            Ok(UpdateCommand::Shutdown) | Err(_) => break,
        }
    }
}

fn run_check(engine: &dyn UpdateEngine, state: &UpdateStateHandle) {
    if let Some(reason) = engine.unavailability() {
        state.publish(UpdatePhase::Unavailable {
            reason: unavailable_reason(reason),
        });
        return;
    }
    state.publish(UpdatePhase::Checking);
    match engine.check() {
        Ok(UpdateOutcome::UpToDate) => {
            state.publish(UpdatePhase::UpToDate);
        }
        Ok(UpdateOutcome::Available { release }) => {
            state.publish(UpdatePhase::Available {
                release: release_info(engine, release),
            });
        }
        Ok(UpdateOutcome::Installed { .. }) => {
            // A check never installs anything. Reporting it as a completed install
            // would claim a change that did not happen, so it is reported as the
            // internal failure it is.
            state.publish(UpdatePhase::Failed {
                stage: UpdateFailureStage::Check,
                code: UpdateErrorCode::Internal,
                release: None,
            });
        }
        Err(error) => {
            state.publish(failure_phase(error, None));
        }
    }
}

fn run_install(engine: &dyn UpdateEngine, state: &UpdateStateHandle) {
    if let Some(reason) = engine.unavailability() {
        state.publish(UpdatePhase::Unavailable {
            reason: unavailable_reason(reason),
        });
        return;
    }
    // The release to install is the one the last check announced. Without one there
    // is nothing to install, so the state is left untouched rather than guessed at.
    let Some(release) = state.phase().release().cloned() else {
        return;
    };
    state.publish(UpdatePhase::Downloading {
        release: release.clone(),
        progress: UpdateProgressInfo::default(),
    });

    let observed_release = release.clone();
    let result = engine.install(&|event| match event {
        UpdateEvent::Progress(progress) => {
            state.publish(UpdatePhase::Downloading {
                release: observed_release.clone(),
                progress: progress_info(progress),
            });
        }
        UpdateEvent::DownloadFinished => {
            state.publish(UpdatePhase::Verifying {
                release: observed_release.clone(),
            });
        }
        UpdateEvent::Verified => {
            state.publish(UpdatePhase::Installing {
                release: observed_release.clone(),
            });
        }
    });

    match result {
        Ok(UpdateOutcome::Installed { version }) => {
            state.publish(UpdatePhase::Installed {
                version,
                restart_required: restart_required_after_install(),
            });
        }
        Ok(UpdateOutcome::UpToDate) => {
            state.publish(UpdatePhase::UpToDate);
        }
        Ok(UpdateOutcome::Available { .. }) => {
            state.publish(UpdatePhase::Failed {
                stage: UpdateFailureStage::Install,
                code: UpdateErrorCode::Internal,
                release: Some(release),
            });
        }
        Err(error) => {
            state.publish(failure_phase(error, Some(release)));
        }
    }
}

fn failure_phase(error: UpdateError, release: Option<UpdateReleaseInfo>) -> UpdatePhase {
    UpdatePhase::Failed {
        stage: failure_stage(error.stage()),
        code: error_code(error.code()),
        release,
    }
}

fn release_info(engine: &dyn UpdateEngine, release: UpdateRelease) -> UpdateReleaseInfo {
    UpdateReleaseInfo {
        release_page_url: engine.release_page_url(&release.version),
        version: release.version,
        notes: release.notes,
    }
}

const fn progress_info(progress: bongocat_update::UpdateProgress) -> UpdateProgressInfo {
    UpdateProgressInfo {
        downloaded_bytes: progress.downloaded_bytes,
        total_bytes: progress.total_bytes,
    }
}

/// Map the update subsystem's stage onto the UI protocol.
///
/// The match is exhaustive on purpose: a new stage must be given a user-facing
/// meaning before it can reach a window.
const fn failure_stage(stage: bongocat_update::UpdateStage) -> UpdateFailureStage {
    match stage {
        bongocat_update::UpdateStage::Check => UpdateFailureStage::Check,
        bongocat_update::UpdateStage::Download => UpdateFailureStage::Download,
        bongocat_update::UpdateStage::Verify => UpdateFailureStage::Verify,
        bongocat_update::UpdateStage::Install => UpdateFailureStage::Install,
    }
}

const fn error_code(code: bongocat_update::UpdateErrorCode) -> UpdateErrorCode {
    use bongocat_update::UpdateErrorCode as Source;
    match code {
        Source::NotConfigured => UpdateErrorCode::NotConfigured,
        Source::EnvironmentDisabled => UpdateErrorCode::EnvironmentDisabled,
        Source::SignatureKeyMissing => UpdateErrorCode::SignatureKeyMissing,
        Source::ReleaseFetchFailed => UpdateErrorCode::ReleaseFetchFailed,
        Source::ReleaseManifestInvalid => UpdateErrorCode::ReleaseManifestInvalid,
        Source::NoMatchingAsset => UpdateErrorCode::NoMatchingAsset,
        Source::DownloadTransportFailed => UpdateErrorCode::DownloadTransportFailed,
        Source::ChecksumMismatch => UpdateErrorCode::ChecksumMismatch,
        Source::SignatureInvalid => UpdateErrorCode::SignatureInvalid,
        Source::ArchiveInvalid => UpdateErrorCode::ArchiveInvalid,
        Source::InstallPathNotWritable => UpdateErrorCode::InstallPathNotWritable,
        Source::InstallFailed => UpdateErrorCode::InstallFailed,
        Source::RestartFailed => UpdateErrorCode::RestartFailed,
        Source::Internal => UpdateErrorCode::Internal,
    }
}

const fn unavailable_reason(reason: UpdateUnavailability) -> UpdateUnavailableReason {
    match reason {
        UpdateUnavailability::DevelopmentChannel => UpdateUnavailableReason::DevelopmentBuild,
        UpdateUnavailability::SigningKeyMissing => UpdateUnavailableReason::SigningKeyMissing,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationUpdateService, UpdateEngine, UpdateErrorCode, UpdateFailureStage, UpdatePhase,
        error_code, failure_stage, progress_info, restart_required_after_install,
        unavailable_reason,
    };
    use bongocat_ui::UpdateWindowHandle;
    use bongocat_ui::{UpdateCommand, UpdateStateHandle, UpdateUnavailableReason};
    use bongocat_update::{
        UpdateError, UpdateErrorCode as SourceCode, UpdateEvent, UpdateOutcome, UpdateProgress,
        UpdateRelease, UpdateStage, UpdateUnavailability,
    };
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    /// The two code catalogs are a contract: a code that exists in one and not the
    /// other would surface in a window as a missing translation.
    #[test]
    fn every_source_error_code_has_a_ui_code() {
        for code in SourceCode::ALL {
            let mapped = error_code(code);
            assert_eq!(mapped.as_str(), code.as_str());
        }
        assert_eq!(UpdateErrorCode::ALL.len(), SourceCode::ALL.len());
    }

    #[test]
    fn every_source_stage_has_a_ui_stage() {
        let stages = [
            (UpdateStage::Check, UpdateFailureStage::Check),
            (UpdateStage::Download, UpdateFailureStage::Download),
            (UpdateStage::Verify, UpdateFailureStage::Verify),
            (UpdateStage::Install, UpdateFailureStage::Install),
        ];
        for (source, expected) in stages {
            assert_eq!(failure_stage(source), expected);
            assert_eq!(failure_stage(source).as_str(), source.as_str());
        }
    }

    #[test]
    fn unavailability_reasons_are_preserved() {
        assert_eq!(
            unavailable_reason(UpdateUnavailability::DevelopmentChannel),
            UpdateUnavailableReason::DevelopmentBuild
        );
        assert_eq!(
            unavailable_reason(UpdateUnavailability::SigningKeyMissing),
            UpdateUnavailableReason::SigningKeyMissing
        );
    }

    #[test]
    fn progress_is_carried_over_verbatim() {
        let mapped = progress_info(UpdateProgress {
            downloaded_bytes: 4096,
            total_bytes: Some(8192),
        });
        assert_eq!(mapped.downloaded_bytes, 4096);
        assert_eq!(mapped.total_bytes, Some(8192));
        assert_eq!(mapped.percent(), Some(50));
    }

    /// The restart requirement is a platform fact, not a runtime decision.
    #[test]
    fn restart_requirement_matches_the_platform() {
        assert_eq!(
            restart_required_after_install(),
            cfg!(target_os = "macos"),
            "macOS replaces the running bundle; Windows hands off to the installer"
        );
    }

    #[test]
    fn an_installed_phase_only_offers_a_restart_where_one_is_needed() {
        let restarting = UpdatePhase::Installed {
            version: "1.1.0".to_owned(),
            restart_required: true,
        };
        assert!(restarting.offers_restart());
        let relaunching = UpdatePhase::Installed {
            version: "1.1.0".to_owned(),
            restart_required: false,
        };
        assert!(!relaunching.offers_restart());
    }

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
        fn installing(
            outcome: Result<UpdateOutcome, UpdateError>,
            events: Vec<UpdateEvent>,
        ) -> Self {
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

    #[test]
    fn a_check_that_finds_a_release_publishes_its_metadata() {
        let (phase, _) = settled_phase(
            ScriptedEngine::available(Ok(UpdateOutcome::Available { release: release() })),
            UpdateCommand::Check,
        );
        let UpdatePhase::Available { release } = phase else {
            panic!("expected an available release, got {phase:?}");
        };
        assert_eq!(release.version, "1.2.0");
        assert_eq!(release.notes.as_deref(), Some("- a change"));
        assert_eq!(
            release.release_page_url.as_deref(),
            Some("https://example.invalid/v1.2.0"),
            "the release identity, not the window, decides where the notes link goes"
        );
    }

    #[test]
    fn a_check_that_finds_nothing_reports_up_to_date() {
        let (phase, _) = settled_phase(
            ScriptedEngine::available(Ok(UpdateOutcome::UpToDate)),
            UpdateCommand::Check,
        );
        assert_eq!(phase, UpdatePhase::UpToDate);
    }

    /// A check cannot install, so claiming it did would be a lie the window repeats.
    #[test]
    fn a_check_that_reports_an_install_is_an_internal_failure() {
        let (phase, _) = settled_phase(
            ScriptedEngine::available(Ok(UpdateOutcome::Installed {
                version: "1.2.0".to_owned(),
            })),
            UpdateCommand::Check,
        );
        assert_eq!(
            phase,
            UpdatePhase::Failed {
                stage: UpdateFailureStage::Check,
                code: UpdateErrorCode::Internal,
                release: None,
            }
        );
    }

    #[test]
    fn a_failed_check_keeps_its_stage_and_code() {
        let (phase, _) = settled_phase(
            ScriptedEngine::available(Err(UpdateError::at(
                UpdateStage::Check,
                SourceCode::ReleaseFetchFailed,
            ))),
            UpdateCommand::Check,
        );
        assert_eq!(
            phase,
            UpdatePhase::Failed {
                stage: UpdateFailureStage::Check,
                code: UpdateErrorCode::ReleaseFetchFailed,
                release: None,
            }
        );
    }

    /// The install pipeline publishes one phase per step, and the window depends on
    /// that order to show progress, then verification, then the install.
    #[test]
    fn an_install_publishes_every_step_in_order() {
        let engine = ScriptedEngine::installing(
            Ok(UpdateOutcome::Installed {
                version: "1.2.0".to_owned(),
            }),
            vec![
                UpdateEvent::Progress(UpdateProgress {
                    downloaded_bytes: 512,
                    total_bytes: Some(1024),
                }),
                UpdateEvent::DownloadFinished,
                UpdateEvent::Verified,
            ],
        );
        let installs = Arc::clone(&engine.installs);
        let (phase, revision) = settled_phase(engine, UpdateCommand::Install);
        assert_eq!(*installs.lock().expect("install counter"), 1);
        assert_eq!(
            phase,
            UpdatePhase::Installed {
                version: "1.2.0".to_owned(),
                restart_required: restart_required_after_install(),
            }
        );
        assert!(
            revision >= 5,
            "a check, a download, a verification, an install and the result are five \
             distinct phases, got revision {revision}"
        );
    }

    #[test]
    fn a_failed_install_keeps_the_release_it_was_about() {
        let engine = ScriptedEngine::installing(
            Err(UpdateError::at(
                UpdateStage::Verify,
                SourceCode::SignatureInvalid,
            )),
            vec![UpdateEvent::DownloadFinished],
        );
        let (phase, _) = settled_phase(engine, UpdateCommand::Install);
        let UpdatePhase::Failed {
            stage,
            code,
            release,
        } = phase
        else {
            panic!("expected a failure, got {phase:?}");
        };
        assert_eq!(stage, UpdateFailureStage::Verify);
        assert_eq!(code, UpdateErrorCode::SignatureInvalid);
        assert_eq!(
            release.map(|release| release.version),
            Some("1.2.0".to_owned()),
            "a failed install still names the release it was about"
        );
    }

    /// An install with nothing checked is not an error, but it must not invent one.
    #[test]
    fn an_install_without_a_checked_release_leaves_the_state_alone() {
        let engine = ScriptedEngine::available(Ok(UpdateOutcome::Installed {
            version: "1.2.0".to_owned(),
        }));
        let installs = Arc::clone(&engine.installs);
        let service =
            ApplicationUpdateService::start_with_engine(engine, "1.1.0").expect("start the worker");
        let client = service.client();
        let state = service.state();
        client.request_install().expect("queue an install");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && state.snapshot().revision == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            state.phase(),
            UpdatePhase::Idle,
            "an install with nothing checked must not invent a release"
        );
        assert_eq!(*installs.lock().expect("install counter"), 0);
        service.join().expect("join the worker");
    }

    #[test]
    fn an_unavailable_build_never_enters_the_pipeline() {
        let engine = ScriptedEngine {
            unavailability: Some(UpdateUnavailability::DevelopmentChannel),
            ..ScriptedEngine::available(Ok(UpdateOutcome::UpToDate))
        };
        let service =
            ApplicationUpdateService::start_with_engine(engine, "1.1.0").expect("start the worker");
        let state = service.state();
        assert_eq!(
            state.phase(),
            UpdatePhase::Unavailable {
                reason: UpdateUnavailableReason::DevelopmentBuild
            },
            "the initial phase explains why the entry point is absent"
        );
        let client = service.client();
        client.request_check().expect("queue a check");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && state.snapshot().revision == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            state.phase(),
            UpdatePhase::Unavailable {
                reason: UpdateUnavailableReason::DevelopmentBuild
            },
            "a disabled build re-publishes the reason instead of touching the network"
        );
        service.join().expect("join the worker");
    }

    #[test]
    fn a_restart_request_is_observable_once() {
        let engine = ScriptedEngine::available(Ok(UpdateOutcome::UpToDate));
        let service =
            ApplicationUpdateService::start_with_engine(engine, "1.1.0").expect("start the worker");
        assert!(!service.take_restart_request());
        service.client().request_restart().expect("queue a restart");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !service.take_restart_request() {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !service.take_restart_request(),
            "the request is consumed, so the application restarts once"
        );
        service.join().expect("join the worker");
    }

    /// Dropping the service must stop and join its thread rather than leak it.
    #[test]
    fn dropping_the_service_stops_the_worker() {
        let engine = ScriptedEngine::available(Ok(UpdateOutcome::UpToDate));
        let service =
            ApplicationUpdateService::start_with_engine(engine, "1.1.0").expect("start the worker");
        drop(service);
    }

    /// The application stores the window handle next to the worker, so it has to be
    /// movable across the thread boundary the coordinator is built on.
    #[test]
    fn the_update_window_handle_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Option<UpdateWindowHandle>>();
    }
}
