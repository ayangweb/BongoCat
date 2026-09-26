//! Starting the worker, publishing revisions and stopping cleanly.

use super::*;

#[test]
fn lifecycle_publishes_typed_snapshots_and_stops_cleanly() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    let ready = client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    assert_eq!(ready.state, RuntimeState::Ready);
    assert_eq!(ready.maximum_fps, DEFAULT_MAXIMUM_FPS);

    let sequence = client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("command accepted");
    let changed = client
        .wait_for_revision(ready.revision + 1, TIMEOUT)
        .expect("updated snapshot");
    assert!(!changed.overlay_visible);
    assert_eq!(changed.last_command_sequence, Some(sequence));
    assert_eq!(changed.command_transport.enqueued, 1);
    assert_eq!(changed.command_transport.queue_full, 0);

    let settings = OverlaySettings {
        click_through: false,
        always_on_top: false,
        scale_percent: 125,
        opacity_percent: 80,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 1,
        keep_inside_screen: false,
    };
    let sequence = client
        .send(RuntimeCommand::SetOverlaySettings(settings))
        .expect("overlay settings command accepted");
    let updated = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("overlay settings snapshot");
    assert_eq!(updated.overlay_settings, settings);
    assert_eq!(updated.last_command_failure, None);

    let sequence = client
        .send(RuntimeCommand::SetMaximumFps(120))
        .expect("maximum FPS command accepted");
    let updated = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("maximum FPS snapshot");
    assert_eq!(updated.maximum_fps, 120);
    assert_eq!(updated.last_command_failure, None);

    let sequence = client
        .send(RuntimeCommand::SetMaximumFps(14))
        .expect("invalid maximum FPS command accepted for typed rejection");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("invalid maximum FPS rejection");
    assert_eq!(rejected.maximum_fps, 120);
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::MaximumFpsInvalid,
        })
    );

    let random_behavior = RandomBehaviorSettings {
        enabled: true,
        interval_seconds: 30,
    };
    let sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(random_behavior))
        .expect("random behavior command accepted");
    let updated = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("random behavior settings snapshot");
    assert_eq!(updated.random_behavior_settings, random_behavior);
    assert_eq!(updated.last_command_failure, None);

    let sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 0,
            },
        ))
        .expect("invalid random behavior command accepted for typed rejection");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("invalid random behavior rejection");
    assert_eq!(rejected.random_behavior_settings, random_behavior);
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::RandomBehaviorSettingsInvalid,
        })
    );

    let invalid = OverlaySettings {
        scale_percent: 0,
        ..settings
    };
    let sequence = client
        .send(RuntimeCommand::SetOverlaySettings(invalid))
        .expect("invalid settings command accepted for typed rejection");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("invalid settings rejection");
    assert_eq!(rejected.overlay_settings, settings);
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
        })
    );

    let invalid = OverlaySettings {
        corner_radius_percent: 51,
        ..settings
    };
    let sequence = client
        .send(RuntimeCommand::SetOverlaySettings(invalid))
        .expect("out-of-range corner radius accepted for typed rejection");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("out-of-range corner radius rejection");
    assert_eq!(rejected.overlay_settings, settings);
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
        })
    );

    let invalid = OverlaySettings {
        hide_on_pointer_hover_delay_seconds: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS + 1,
        ..settings
    };
    let sequence = client
        .send(RuntimeCommand::SetOverlaySettings(invalid))
        .expect("out-of-range hover hide delay accepted for typed rejection");
    let rejected = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("out-of-range hover hide delay rejection");
    assert_eq!(rejected.overlay_settings, settings);
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence,
            code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
        })
    );

    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
}

#[test]
fn shutdown_request_is_nonblocking_and_skips_later_automatic_actions() {
    let signal = Arc::new(ShutdownSignal::default());
    let action_signal = Arc::clone(&signal);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let action = thread::spawn(move || {
        action_signal.run_if_not_shutdown(|| {
            entered_tx.send(()).expect("enter automatic side effect");
            release_rx.recv().expect("release automatic side effect");
        });
    });
    entered_rx
        .recv_timeout(TIMEOUT)
        .expect("automatic side effect entered shutdown gate");

    let request_signal = Arc::clone(&signal);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let request = thread::spawn(move || {
        request_signal.request(7);
        done_tx.send(()).expect("finish shutdown request");
    });
    done_rx
        .recv_timeout(TIMEOUT)
        .expect("shutdown request must not wait for the admitted side effect");
    assert_eq!(signal.sequence(), Some(7));

    let skipped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let skipped_action = Arc::clone(&skipped);
    signal.run_if_not_shutdown(|| skipped_action.store(true, Ordering::Relaxed));
    assert!(!skipped.load(Ordering::Relaxed));

    release_tx.send(()).expect("release automatic side effect");
    action.join().expect("automatic side effect thread");
    request.join().expect("shutdown request thread");
}

#[test]
fn shutdown_rejects_new_commands_and_drains_a_full_queue() {
    let owner = RuntimeOwner::start(true, 1);
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");
    let mut queue_full = false;
    for _ in 0..100_000 {
        match client.send(RuntimeCommand::SetOverlayVisible(false)) {
            Ok(_) => {}
            Err(SendError::QueueFull(_)) => {
                queue_full = true;
                break;
            }
            Err(SendError::RuntimeStopped(_)) => panic!("runtime stopped unexpectedly"),
        }
    }
    assert!(queue_full, "test must observe a full command queue");

    owner.request_shutdown();
    assert!(matches!(
        client.send(RuntimeCommand::SetOverlayVisible(true)),
        Err(SendError::RuntimeStopped(_))
    ));
    let stopped = owner.shutdown(TIMEOUT).expect("shutdown drains queue");
    assert_eq!(stopped.state, RuntimeState::Stopped);
}

#[test]
fn shutdown_timeout_returns_without_waiting_for_worker_join() {
    let owner = RuntimeOwner::start(true, 1);
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");
    client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("queue accepts one command");

    let started = Instant::now();
    let result = owner.shutdown(Duration::ZERO);
    assert_eq!(result, Err(ShutdownError::TimedOut));
    assert_eq!(client.snapshot().shutdown.timed_out, 1);
    assert_eq!(client.snapshot().shutdown.worker_panicked, 0);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "explicit shutdown timeout must bound the caller wait"
    );

    let stopped = client
        .wait_for_state(RuntimeState::Stopped, TIMEOUT)
        .expect("detached worker eventually drains and stops");
    assert_eq!(stopped.state, RuntimeState::Stopped);
}

#[test]
fn shutdown_timeout_bounds_a_blocking_worker_and_eventually_drains() {
    let owner = RuntimeOwner::start_with_shutdown_delay(true, 1, Duration::from_millis(100));
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");
    client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("queued command before shutdown");

    let started = Instant::now();
    assert_eq!(
        owner.shutdown(Duration::from_millis(5)),
        Err(ShutdownError::TimedOut)
    );
    assert!(
        started.elapsed() < Duration::from_millis(75),
        "shutdown caller must not wait for the blocking worker"
    );
    assert_eq!(client.snapshot().shutdown.timed_out, 1);

    let stopped = client
        .wait_for_state(RuntimeState::Stopped, TIMEOUT)
        .expect("blocking worker eventually drains and stops");
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert!(
        !stopped.overlay_visible,
        "shutdown must drain the queued command"
    );
    assert_eq!(client.snapshot().shutdown.worker_panicked, 0);
}

#[test]
fn shutdown_reports_worker_panic_after_stopped_snapshot() {
    let owner = RuntimeOwner::start_with_worker_panic(true, 1);
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");

    assert_eq!(owner.shutdown(TIMEOUT), Err(ShutdownError::WorkerPanicked));
    assert_eq!(client.snapshot().shutdown.worker_panicked, 1);
}

#[test]
fn shutdown_timeout_aggregates_late_worker_panic() {
    let owner = RuntimeOwner::start_with_worker_panic(true, 1);
    let client = owner.client();
    client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("runtime ready");

    assert_eq!(owner.shutdown(Duration::ZERO), Err(ShutdownError::TimedOut));
    client
        .wait_for_state(RuntimeState::Stopped, TIMEOUT)
        .expect("detached worker eventually stops");

    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if client.snapshot().shutdown.worker_panicked == 1 {
            return;
        }
        thread::yield_now();
    }
    panic!("late worker panic was not aggregated");
}

#[test]
fn model_settings_command_is_revisioned_and_published() {
    let owner = RuntimeOwner::start(true, 8);
    let client = owner.client();
    let ready = client
        .wait_for_state(RuntimeState::Ready, TIMEOUT)
        .expect("ready snapshot");
    assert_eq!(ready.model_settings, ModelSettings::default());

    let settings = ModelSettings {
        mirror: true,
        mirror_pointer_tracking: true,
        ignore_keyboard: false,
        ignore_gamepad: false,
        ignore_pointer: true,
    };
    let sequence = client
        .send(RuntimeCommand::SetModelSettings(settings))
        .expect("model settings command accepted");
    let updated = client
        .wait_for_command(sequence, TIMEOUT)
        .expect("model settings snapshot");
    assert_eq!(updated.model_settings, settings);
    assert_eq!(updated.last_command_failure, None);

    let stopped = owner.shutdown(TIMEOUT).expect("clean shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
    assert_eq!(stopped.model_settings, settings);
}

#[test]
fn random_behavior_setting_starts_a_model_behavior_after_one_interval() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(true, 8, clock.clone());
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let settings = RandomBehaviorSettings {
        enabled: true,
        interval_seconds: 1,
    };
    let sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(settings))
        .expect("random behavior setting accepted");
    client
        .wait_for_command(sequence, TIMEOUT)
        .expect("random behavior setting published");

    let model = Arc::new(preset_model("standard"));
    let sequence = client
        .send(RuntimeCommand::ActivateModel(model))
        .expect("model activation accepted");
    let frame = wait_for_prepared_model(&client, &consumer, sequence);
    let committed = report_model_prepared(&client, &consumer, &frame);
    assert!(committed.active_model.is_some());

    clock.set(Duration::from_secs(1));
    let tick = client
        .send(RuntimeCommand::Tick)
        .expect("random tick accepted");
    let mut snapshot = client
        .wait_for_command(tick, TIMEOUT)
        .expect("random behavior tick published");
    let deadline = Instant::now() + TIMEOUT;
    while snapshot.active_motion.is_none()
        && snapshot.active_expression.is_none()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(2));
        snapshot = client.snapshot();
    }
    assert!(
        snapshot.active_motion.is_some() || snapshot.active_expression.is_some(),
        "the due scheduler must select one declared model behavior"
    );
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn random_behavior_does_not_replace_a_live_normal_product_motion() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(true, 8, clock.clone());
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let settings_sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(
            RandomBehaviorSettings {
                enabled: true,
                interval_seconds: 1,
            },
        ))
        .expect("random behavior setting accepted");
    client
        .wait_for_command(settings_sequence, TIMEOUT)
        .expect("random behavior setting published");

    let (_catalog, model) = preset_model_without_expressions();
    let activation = client
        .send(RuntimeCommand::ActivateModel(Arc::new(model)))
        .expect("model activation accepted");
    let frame = wait_for_prepared_model(&client, &consumer, activation);
    report_model_prepared(&client, &consumer, &frame);
    let manual_motion = MotionId::new("CAT_motion", 0).expect("manual motion");
    let manual_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: manual_motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("manual motion accepted");
    let manual = client
        .wait_for_command(manual_sequence, TIMEOUT)
        .expect("manual motion published");
    let manual_active = manual.active_motion.expect("manual active motion");

    clock.set(Duration::from_secs(1));
    let tick = client
        .send(RuntimeCommand::Tick)
        .expect("random tick accepted");
    client
        .wait_for_command(tick, TIMEOUT)
        .expect("tick published");
    let deadline = Instant::now() + TIMEOUT;
    let mut after_random = client.snapshot();
    while after_random.revision == manual.revision && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
        after_random = client.snapshot();
    }
    assert_eq!(after_random.active_motion, Some(manual_active));
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}
