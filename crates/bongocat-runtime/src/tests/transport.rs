//! The command queue, its sequence tracker and overflow recovery.

use super::*;

#[test]
fn sequence_wait_predicate_handles_wraparound() {
    assert!(sequence_reached(42, 42));
    assert!(sequence_reached(0, u64::MAX));
    assert!(sequence_reached(1, u64::MAX));
    assert!(!sequence_reached(u64::MAX, 0));
    assert!(!sequence_reached(10, 11));
}

#[test]
fn zero_capacity_is_rejected() {
    let panic = std::panic::catch_unwind(|| RuntimeOwner::start(true, 0));
    assert!(panic.is_err());
}

#[test]
fn full_queue_returns_the_original_typed_command() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let motion_audio = MotionAudioClient::unavailable();
    let producer = Arc::new(Producer {
        sender,
        next_sequence: Mutex::new(0),
        command_transport: Arc::new(CommandTransportCounters::default()),
        accepting: Arc::new(AtomicBool::new(true)),
    });
    let client = RuntimeClient {
        input_producer: InputProducer::new(Arc::new(RuntimeInputSubmitter {
            producer: Arc::clone(&producer),
        })),
        cursor_producer: CursorProducer::new(),
        gamepad_axis_producer: GamepadAxisProducer::with_capacity(DEFAULT_GAMEPAD_AXIS_CAPACITY),
        producer,
        snapshot: Arc::new(SnapshotCell {
            value: Mutex::new(RuntimeSnapshot::starting(
                true,
                false,
                motion_audio.diagnostics(),
            )),
            changed: Condvar::new(),
        }),
        platform_input_diagnostics: PlatformInputDiagnosticsProducer::default(),
        motion_audio,
        shutdown_diagnostics: Arc::new(ShutdownDiagnosticsCounters::default()),
    };
    client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("first command accepted");
    assert_eq!(
        client.send(RuntimeCommand::ResetInput(InputResetReason::QueueOverflow,)),
        Err(SendError::QueueFull(RuntimeCommand::ResetInput(
            InputResetReason::QueueOverflow,
        )))
    );
    assert_eq!(
        client.snapshot().command_transport,
        RuntimeCommandTransportDiagnostics {
            enqueued: 1,
            queue_full: 1,
            runtime_stopped: 0,
            sequence_gap_count: 0,
            missing_sequence_count: 0,
            duplicate_sequence_count: 0,
            out_of_order_sequence_count: 0,
        }
    );
}

#[test]
fn command_sequence_tracker_classifies_gaps_duplicates_and_wraparound() {
    let mut tracker = CommandSequenceTracker::default();
    assert_eq!(tracker.observe(7), CommandSequenceDisposition::First);
    assert_eq!(
        tracker.observe(9),
        CommandSequenceDisposition::Gap { missing: 1 }
    );
    assert_eq!(tracker.observe(9), CommandSequenceDisposition::Duplicate);
    assert_eq!(tracker.observe(8), CommandSequenceDisposition::OutOfOrder);

    let mut wrapping = CommandSequenceTracker {
        last: Some(u64::MAX),
        ..Default::default()
    };
    assert_eq!(wrapping.observe(0), CommandSequenceDisposition::InOrder);
    assert_eq!(
        wrapping.observe(2),
        CommandSequenceDisposition::Gap { missing: 1 }
    );
    assert_eq!(wrapping.observe(2), CommandSequenceDisposition::Duplicate);
    assert_eq!(
        wrapping.observe(u64::MAX),
        CommandSequenceDisposition::OutOfOrder
    );
}

#[test]
fn input_producer_overflow_is_observable_and_recovery_resets_state() {
    let (sender, receiver) = mpsc::sync_channel(2);
    let motion_audio = MotionAudioClient::unavailable();
    let runtime_producer = Arc::new(Producer {
        sender,
        next_sequence: Mutex::new(0),
        command_transport: Arc::new(CommandTransportCounters::default()),
        accepting: Arc::new(AtomicBool::new(true)),
    });
    let client = RuntimeClient {
        producer: Arc::clone(&runtime_producer),
        snapshot: Arc::new(SnapshotCell {
            value: Mutex::new(RuntimeSnapshot::starting(
                true,
                false,
                motion_audio.diagnostics(),
            )),
            changed: Condvar::new(),
        }),
        input_producer: InputProducer::new(Arc::new(RuntimeInputSubmitter {
            producer: runtime_producer,
        })),
        cursor_producer: CursorProducer::new(),
        gamepad_axis_producer: GamepadAxisProducer::with_capacity(DEFAULT_GAMEPAD_AXIS_CAPACITY),
        platform_input_diagnostics: PlatformInputDiagnosticsProducer::default(),
        motion_audio,
        shutdown_diagnostics: Arc::new(ShutdownDiagnosticsCounters::default()),
    };
    let producer = client.input_producer.clone();
    let sibling_producer = producer.clone();
    let down = InputEvent::Edge {
        control: InputControl::Key(PhysicalKey::KEY_A),
        edge: InputEdge::Down,
        source: InputSource::Capture,
        at: MonotonicMillis::new(0),
    };
    producer.publish(down).expect("down enqueued");
    client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("queue filler");
    let release = InputEvent::Edge {
        control: InputControl::Key(PhysicalKey::KEY_A),
        edge: InputEdge::Up,
        source: InputSource::Capture,
        at: MonotonicMillis::new(1),
    };
    assert_eq!(
        sibling_producer.publish(release.clone()),
        Err(InputPublishError::QueueFull(release))
    );

    let first = receiver.recv().expect("queued input");
    let WorkerCommand::Product(RuntimeCommand::ApplyInput(first)) = first.command else {
        panic!("first command must be input");
    };
    let mut state = InputState::default();
    state.apply(Arc::unwrap_or_clone(first));
    assert_eq!(state.snapshot().pressed_key_count, 1);
    receiver.recv().expect("queue filler");

    producer
        .recover(InputResetReason::QueueOverflow, MonotonicMillis::new(2))
        .expect("recovery enqueued");
    let recovery = receiver.recv().expect("recovery input");
    let WorkerCommand::Product(RuntimeCommand::ApplyInput(recovery)) = recovery.command else {
        panic!("recovery command must be input");
    };
    assert_eq!(
        state.apply(Arc::unwrap_or_clone(recovery)),
        InputDisposition::AppliedAfterSequenceGap { missing: 1 }
    );
    let snapshot = state.snapshot();
    assert_eq!(snapshot.pressed_key_count, 0);
    assert_eq!(
        snapshot.last_reset_reason,
        Some(InputResetReason::QueueOverflow)
    );
    assert_eq!(snapshot.diagnostics.reset_count, 1);
    assert_eq!(
        producer.diagnostics(),
        InputTransportDiagnostics {
            enqueued: 2,
            queue_full: 1,
            recovered_after_overflow: 1,
            runtime_stopped: 0,
        }
    );
    assert_eq!(client.snapshot().input.transport, producer.diagnostics());
}
