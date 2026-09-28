//! Shutdown stops the voice, and a blocked device cannot hold up exit.

use super::*;

#[test]
fn shutdown_stops_voice_releases_worker_and_rejects_late_commands() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = MotionAudioService::start_with_backend(
        1,
        Box::new(RecordingBackend {
            events: Arc::clone(&events),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let client = service.client();
    client
        .try_publish(play(9, "voice.flac"))
        .expect("play queued");
    client
        .wait_for_sequence(9, TIMEOUT)
        .expect("play processed");
    let stopped = service.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, MotionAudioState::Stopped);
    assert_eq!(stopped.current_voice_sequence, None);
    assert_eq!(stopped.voices_stopped, 1);
    assert_eq!(
        client.try_publish(play(10, "late.flac")),
        Err(MotionAudioPublishError::ServiceStopped(play(
            10,
            "late.flac"
        )))
    );
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last(),
        Some(&BackendEvent::Stop)
    );
}

#[test]
fn shutdown_timeout_returns_without_waiting_for_a_blocked_backend() {
    let state = Arc::new((Mutex::new(BlockingState::default()), Condvar::new()));
    let service = MotionAudioService::start_with_backend(
        1,
        Box::new(BlockingBackend {
            state: Arc::clone(&state),
        }),
    )
    .expect("audio service");
    let client = service.client();
    client
        .try_publish(play(1, "blocked.flac"))
        .expect("play queued");
    {
        let (lock, changed) = &*state;
        let entered = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (entered, result) = changed
            .wait_timeout_while(entered, TIMEOUT, |state| !state.entered)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!result.timed_out(), "backend did not start processing");
        drop(entered);
    }

    let started = Instant::now();
    let result = service.shutdown(Duration::ZERO);
    assert_eq!(result, Err(MotionAudioShutdownError::TimedOut));
    assert!(started.elapsed() < Duration::from_millis(100));

    let (lock, changed) = &*state;
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.released = true;
    changed.notify_all();
    drop(state);

    let deadline = Instant::now() + TIMEOUT;
    loop {
        let diagnostics = client.diagnostics();
        if diagnostics.state == MotionAudioState::Stopped {
            break;
        }
        assert!(Instant::now() < deadline, "timed-out worker did not stop");
        thread::yield_now();
    }
}

#[test]
fn shutdown_reports_worker_panic_after_stopped_diagnostics() {
    let service = MotionAudioService::start_with_worker_panic(
        1,
        Box::new(RecordingBackend {
            events: Arc::new(Mutex::new(Vec::new())),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let client = service.client();
    let result = service.shutdown(TIMEOUT);
    assert_eq!(result, Err(MotionAudioShutdownError::WorkerPanicked));
    assert_eq!(client.diagnostics().state, MotionAudioState::Stopped);
}
