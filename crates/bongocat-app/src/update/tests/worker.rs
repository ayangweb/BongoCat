//! A check and an install publish every step, and a failure keeps its stage.

use super::*;

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
