//! The service owns the thread, and a restart is observable once.

use super::*;

/// An install with nothing checked is not an error, but it must not invent one.
#[test]
fn update_worker_records_bounded_phase_and_failure_events() {
    let directory = tempdir().expect("temporary log directory");
    let log = ApplicationLogHandle::install_with_settings(
        directory.path(),
        bongocat_log::LogSettings::default(),
        false,
    )
    .expect("application log");
    let engine = ScriptedEngine::installing(
        Err(UpdateError::at(
            UpdateStage::Verify,
            SourceCode::SignatureInvalid,
        )),
        vec![UpdateEvent::DownloadFinished],
    );
    let service =
        ApplicationUpdateService::start_with_engine_and_log(engine, "1.1.0", Some(log.clone()))
            .expect("start the worker");
    let state = service.state();
    service.client().request_check().expect("queue a check");
    let (_, revision) = wait_for_settled(&state, state.snapshot().revision);
    service
        .client()
        .request_install()
        .expect("queue an install");
    wait_for_settled(&state, revision);
    service.join().expect("join the worker");

    let mut paths = fs::read_dir(directory.path())
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
    let contents = fs::read_to_string(paths.pop().expect("application log")).expect("log text");
    assert!(contents.contains(ApplicationLogCode::UpdateCheckCompleted.as_str()));
    assert!(contents.contains(ApplicationLogCode::UpdateInstallStarted.as_str()));
    assert!(contents.contains(ApplicationLogCode::UpdatePhaseChanged.as_str()));
    assert!(contents.contains(ApplicationLogCode::UpdateInstallFailed.as_str()));
    assert!(!contents.contains("https://"));
    assert!(!contents.contains("1.2.0"));
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
