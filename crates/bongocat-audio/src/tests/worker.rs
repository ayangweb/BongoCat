//! An overflow discards the backlog and keeps what matters.

use super::*;

#[test]
fn accepted_commands_replace_one_voice_in_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = MotionAudioService::start_with_backend(
        4,
        Box::new(RecordingBackend {
            events: Arc::clone(&events),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let client = service.client();
    client
        .try_publish(play(1, "first.flac"))
        .expect("first play");
    client
        .try_publish(play(2, "second.flac"))
        .expect("replacement play");
    client
        .try_publish(MotionAudioCommand::Stop {
            sequence: 3,
            reason: MotionAudioStopReason::MotionStopped,
        })
        .expect("stop");
    let diagnostics = client
        .wait_for_sequence(3, TIMEOUT)
        .expect("commands processed");
    assert_eq!(diagnostics.playback_starts, 2);
    assert_eq!(diagnostics.voices_stopped, 2);
    assert_eq!(diagnostics.current_voice_sequence, None);
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![
            BackendEvent::Play(PathBuf::from("first.flac")),
            BackendEvent::Stop,
            BackendEvent::Play(PathBuf::from("second.flac")),
            BackendEvent::Stop,
        ]
    );
    let stopped = service.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, MotionAudioState::Stopped);
}

#[test]
fn prepared_resources_are_available_before_the_first_play() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = MotionAudioService::start_with_backend(
        4,
        Box::new(RecordingBackend {
            events: Arc::clone(&events),
            failures: VecDeque::new(),
            playing: false,
        }),
    )
    .expect("audio service");
    let client = service.client();
    client
        .try_publish(prepare(1, &["first.flac"]))
        .expect("prepare queued");
    client
        .wait_for_sequence(1, TIMEOUT)
        .expect("prepare processed");
    client
        .try_publish(MotionAudioCommand::ActivatePrepared {
            sequence: 1,
            paths: vec![PathBuf::from("first.flac")],
        })
        .expect("activation queued");
    client
        .try_publish(play(2, "first.flac"))
        .expect("play queued");
    let diagnostics = client
        .wait_for_sequence(2, TIMEOUT)
        .expect("play processed");

    assert_eq!(diagnostics.prepare_requests, 1);
    assert_eq!(diagnostics.prepared_resources, 1);
    assert_eq!(diagnostics.playback_starts, 1);
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![
            BackendEvent::Prepare(vec![PathBuf::from("first.flac")]),
            BackendEvent::Play(PathBuf::from("first.flac")),
        ]
    );
    service.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn queue_overflow_discards_untrusted_backlog_and_stops_the_voice() {
    let state = Arc::new((Mutex::new(BlockingState::default()), Condvar::new()));
    let service = MotionAudioService::start_with_backend(
        1,
        Box::new(BlockingBackend {
            state: Arc::clone(&state),
        }),
    )
    .expect("audio service");
    let client = service.client();
    client.try_publish(play(1, "one.flac")).expect("first play");
    {
        let (lock, changed) = &*state;
        let entered = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut entered, result) = changed
            .wait_timeout_while(entered, TIMEOUT, |state| !state.entered)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!result.timed_out(), "backend did not start processing");
        client
            .try_publish(play(2, "two.flac"))
            .expect("queued replacement");
        assert_eq!(
            client.try_publish(play(3, "overflow.flac")),
            Err(MotionAudioPublishError::QueueFull(play(3, "overflow.flac")))
        );
        entered.released = true;
        changed.notify_all();
    }

    let deadline = Instant::now() + TIMEOUT;
    let recovered = loop {
        let diagnostics = client.diagnostics();
        if diagnostics.discarded_commands == 1 && diagnostics.current_voice_sequence.is_none() {
            break diagnostics;
        }
        assert!(Instant::now() < deadline, "overflow recovery timed out");
        thread::yield_now();
    };
    assert_eq!(recovered.queue_overflows, 1);
    assert_eq!(recovered.enqueued_commands, 2);
    assert_eq!(recovered.processed_commands, 1);
    assert_eq!(recovered.voices_stopped, 1);
    service.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn overflow_recovery_retains_model_prepare_and_rejects_late_commands() {
    let state = Arc::new((Mutex::new(BlockingState::default()), Condvar::new()));
    let service = MotionAudioService::start_with_backend(
        1,
        Box::new(BlockingBackend {
            state: Arc::clone(&state),
        }),
    )
    .expect("audio service");
    let client = service.client();
    client.try_publish(play(1, "one.flac")).expect("first play");
    {
        let (lock, changed) = &*state;
        let entered = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut entered, result) = changed
            .wait_timeout_while(entered, TIMEOUT, |state| !state.entered)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!result.timed_out(), "backend did not start processing");
        client
            .try_publish(prepare(2, &["prepared.flac"]))
            .expect("prepare queued before overflow");
        assert!(matches!(
            client.try_publish(play(3, "overflow.flac")),
            Err(MotionAudioPublishError::QueueFull(_))
        ));
        assert!(matches!(
            client.try_publish_with_sequence(|sequence| prepare(sequence, &["late.flac"])),
            Err(MotionAudioPublishError::RecoveryPending(_))
        ));
        entered.released = true;
        changed.notify_all();
    }

    let recovered = client
        .wait_for_sequence(2, TIMEOUT)
        .expect("retained prepare processed after overflow recovery");
    assert_eq!(recovered.prepare_requests, 1);
    assert_eq!(recovered.discarded_commands, 0);
    assert_eq!(recovered.queue_overflows, 1);
    service.shutdown(TIMEOUT).expect("clean shutdown");
}
