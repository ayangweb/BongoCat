//! The check and the install, and the thread that runs them.
//!
//! Restarting is the one step the worker cannot take — replacing the process
//! image requires the product's shutdown sequence, which belongs to the
//! application — so the worker records the request and the application acts on
//! it. Everything else here is ordinary: a check publishes what it found, an
//! install publishes every step as it happens, and a failure keeps the stage and
//! code it failed at rather than collapsing to one error.

use super::*;

pub(crate) fn run_worker(
    engine: Box<dyn UpdateEngine>,
    endpoint: UpdateServiceEndpoint,
    state: UpdateStateHandle,
    restart_requested: Arc<AtomicBool>,
    application_log: Option<ApplicationLogHandle>,
) {
    loop {
        match endpoint.recv_blocking() {
            Ok(UpdateCommand::Check) => {
                run_check(engine.as_ref(), &state, application_log.as_ref())
            }
            Ok(UpdateCommand::Install) => {
                run_install(engine.as_ref(), &state, application_log.as_ref())
            }
            Ok(UpdateCommand::Restart) => {
                restart_requested.store(true, Ordering::Release);
            }
            Ok(UpdateCommand::Shutdown) | Err(_) => break,
        }
    }
}

pub(crate) fn record_update_event(
    application_log: Option<&ApplicationLogHandle>,
    event: ApplicationLogEvent,
) {
    if let Some(application_log) = application_log {
        application_log.record(event);
    }
}

pub(crate) fn run_check(
    engine: &dyn UpdateEngine,
    state: &UpdateStateHandle,
    application_log: Option<&ApplicationLogHandle>,
) {
    if let Some(reason) = engine.unavailability() {
        state.publish(UpdatePhase::Unavailable {
            reason: unavailable_reason(reason),
        });
        record_update_event(
            application_log,
            ApplicationLogEvent::new(ApplicationLogCode::UpdateUnavailable).with_context(
                ApplicationLogContext::Reason(match reason {
                    UpdateUnavailability::DevelopmentChannel => "development_channel",
                    UpdateUnavailability::UnsupportedTarget => "unsupported_target",
                    UpdateUnavailability::SigningKeyMissing => "signing_key_missing",
                }),
            ),
        );
        return;
    }
    state.publish(UpdatePhase::Checking);
    match engine.check() {
        Ok(UpdateOutcome::UpToDate) => {
            state.publish(UpdatePhase::UpToDate);
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateCheckCompleted)
                    .with_context(ApplicationLogContext::Result("up_to_date")),
            );
        }
        Ok(UpdateOutcome::Available { release }) => {
            state.publish(UpdatePhase::Available {
                release: release_info(engine, release),
            });
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateCheckCompleted)
                    .with_context(ApplicationLogContext::Result("available")),
            );
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
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateCheckFailed)
                    .with_context(ApplicationLogContext::Reason("internal")),
            );
        }
        Err(error) => {
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::NetworkOperationFailed)
                    .with_context(ApplicationLogContext::Operation("update_check"))
                    .with_context(ApplicationLogContext::Reason(error.code().as_str())),
            );
            state.publish(failure_phase(error, None));
        }
    }
}

pub(crate) fn run_install(
    engine: &dyn UpdateEngine,
    state: &UpdateStateHandle,
    application_log: Option<&ApplicationLogHandle>,
) {
    if let Some(reason) = engine.unavailability() {
        state.publish(UpdatePhase::Unavailable {
            reason: unavailable_reason(reason),
        });
        record_update_event(
            application_log,
            ApplicationLogEvent::new(ApplicationLogCode::UpdateUnavailable).with_context(
                ApplicationLogContext::Reason(match reason {
                    UpdateUnavailability::DevelopmentChannel => "development_channel",
                    UpdateUnavailability::UnsupportedTarget => "unsupported_target",
                    UpdateUnavailability::SigningKeyMissing => "signing_key_missing",
                }),
            ),
        );
        return;
    }
    // The release to install is the one the last check announced. Without one there
    // is nothing to install, so the state is left untouched rather than guessed at.
    let Some(release) = state.phase().release().cloned() else {
        record_update_event(
            application_log,
            ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallFailed)
                .with_context(ApplicationLogContext::Reason("release_not_available")),
        );
        return;
    };
    record_update_event(
        application_log,
        ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallStarted),
    );
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
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdatePhaseChanged)
                    .with_context(ApplicationLogContext::State("verifying")),
            );
            state.publish(UpdatePhase::Verifying {
                release: observed_release.clone(),
            });
        }
        UpdateEvent::Verified => {
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdatePhaseChanged)
                    .with_context(ApplicationLogContext::State("installing")),
            );
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
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallCompleted),
            );
        }
        Ok(UpdateOutcome::UpToDate) => {
            state.publish(UpdatePhase::UpToDate);
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallFailed)
                    .with_context(ApplicationLogContext::Reason("internal")),
            );
        }
        Ok(UpdateOutcome::Available { .. }) => {
            state.publish(UpdatePhase::Failed {
                stage: UpdateFailureStage::Install,
                code: UpdateErrorCode::Internal,
                release: Some(release),
            });
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallFailed)
                    .with_context(ApplicationLogContext::Reason("internal")),
            );
        }
        Err(error) => {
            record_update_event(
                application_log,
                ApplicationLogEvent::new(ApplicationLogCode::UpdateInstallFailed)
                    .with_context(ApplicationLogContext::Reason(error.code().as_str())),
            );
            state.publish(failure_phase(error, Some(release)));
        }
    }
}
