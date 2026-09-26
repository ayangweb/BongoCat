//! Motion and expression commands, priorities and fades.

use super::*;

#[test]
fn motion_commands_use_priority_identity_and_injectable_time() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_audio_and_clock(
        true,
        true,
        8,
        MotionAudioClient::unavailable(),
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    let activated = report_model_prepared(&client, &consumer, &candidate);
    let audio_rejections_before_motion = activated.motion_audio.rejected_after_shutdown;
    let baseline = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == candidate.model_generation
            && frame.frame_number > candidate.frame_number
    });

    let first = MotionId::new("CAT_motion", 0).expect("motion id");
    let first_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: first.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("start motion");
    let started = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("motion started");
    assert_eq!(
        started.active_motion,
        Some(ActiveMotionSnapshot {
            motion: first.clone(),
            priority: MotionPriority::Normal,
            command_sequence: first_sequence,
            stop_command_sequence: None,
        })
    );
    assert_eq!(
        started.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 1
    );

    let duplicate_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: first.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("duplicate motion request");
    let duplicate = client
        .wait_for_command(duplicate_sequence, TIMEOUT)
        .expect("duplicate motion result");
    assert_eq!(duplicate.active_motion, started.active_motion);
    assert_eq!(
        duplicate.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 1,
        "an idempotent retry must not restart motion audio"
    );

    clock.set(Duration::from_millis(500));
    let tick_sequence = client
        .send(RuntimeCommand::Tick)
        .expect("deterministic tick");
    client
        .wait_for_command(tick_sequence, TIMEOUT)
        .expect("tick completed");
    let animated = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > baseline.transport_sequence
            && frame.snapshot != baseline.snapshot
    });
    assert_ne!(animated.snapshot, baseline.snapshot);

    let second = MotionId::new("CAT_motion", 1).expect("motion id");
    let ignored_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: second.clone(),
            priority: MotionPriority::Idle,
        })
        .expect("lower priority request");
    let ignored = client
        .wait_for_command(ignored_sequence, TIMEOUT)
        .expect("lower priority result");
    assert_eq!(ignored.active_motion, started.active_motion);
    assert_eq!(
        ignored.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 1
    );

    let force_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: second.clone(),
            priority: MotionPriority::Force,
        })
        .expect("force motion");
    let forced = client
        .wait_for_command(force_sequence, TIMEOUT)
        .expect("force motion result");
    assert_eq!(
        forced.active_motion,
        Some(ActiveMotionSnapshot {
            motion: second.clone(),
            priority: MotionPriority::Force,
            command_sequence: force_sequence,
            stop_command_sequence: None,
        })
    );
    assert_eq!(
        forced.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 2
    );

    let invalid_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: MotionId::new("missing", 0).expect("syntactically valid motion id"),
            priority: MotionPriority::Force,
        })
        .expect("invalid resource request");
    let invalid = client
        .wait_for_command(invalid_sequence, TIMEOUT)
        .expect("invalid resource result");
    assert_eq!(
        invalid.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence: invalid_sequence,
            code: RuntimeRenderErrorCode::MotionLoadFailed,
        })
    );
    assert_eq!(invalid.active_motion, forced.active_motion);
    assert_eq!(
        invalid.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 2
    );

    let stale_stop_sequence = client
        .send(RuntimeCommand::StopMotion(first))
        .expect("stale stop");
    let stale_stop = client
        .wait_for_command(stale_stop_sequence, TIMEOUT)
        .expect("stale stop result");
    assert_eq!(stale_stop.active_motion, forced.active_motion);
    assert_eq!(
        stale_stop.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 2
    );

    let stop_sequence = client
        .send(RuntimeCommand::StopMotion(second))
        .expect("current stop");
    let stopped = client
        .wait_for_command(stop_sequence, TIMEOUT)
        .expect("current stop result");
    assert!(stopped.active_motion.is_none());
    assert_eq!(
        stopped.motion_audio.rejected_after_shutdown,
        audio_rejections_before_motion + 3
    );
    let duplicate_stop_sequence = client
        .send(RuntimeCommand::StopMotion(
            MotionId::new("CAT_motion", 1).expect("motion id"),
        ))
        .expect("duplicate stop");
    let duplicate_stop = client
        .wait_for_command(duplicate_stop_sequence, TIMEOUT)
        .expect("duplicate stop result");
    assert!(duplicate_stop.active_motion.is_none());
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
    assert!(matches!(
        client.send(RuntimeCommand::SetOverlayVisible(true)),
        Err(SendError::RuntimeStopped(_))
    ));
    assert_eq!(client.snapshot().command_transport.runtime_stopped, 1);
}

#[test]
fn shortcut_action_dispatch_reuses_typed_motion_and_expression_commands() {
    let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);

    let motion = MotionId::new("CAT_motion", 0).expect("motion id");
    let start_sequence = client
        .trigger_shortcut(ShortcutAction::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("shortcut start");
    let started = client
        .wait_for_command(start_sequence, TIMEOUT)
        .expect("shortcut motion result");
    assert_eq!(
        started.active_motion.as_ref().map(|active| &active.motion),
        Some(&motion)
    );

    let expression = ExpressionId::new("live2d_expression0.exp3.json").expect("expression id");
    let expression_sequence = client
        .trigger_shortcut(ShortcutAction::SetExpression(expression.clone()))
        .expect("shortcut expression");
    let expressed = client
        .wait_for_command(expression_sequence, TIMEOUT)
        .expect("shortcut expression result");
    assert_eq!(
        expressed
            .active_expression
            .as_ref()
            .map(|active| &active.expression),
        Some(&expression)
    );

    let stop_sequence = client
        .trigger_shortcut(ShortcutAction::StopMotion(motion))
        .expect("shortcut stop");
    let stopped = client
        .wait_for_command(stop_sequence, TIMEOUT)
        .expect("shortcut stop result");
    assert!(stopped.active_motion.is_none());
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

/// A behaviour shortcut is one visible run of the clip. The preset motions
/// declare `Meta.Loop: true` and last 1.633s, so a runtime that honored the
/// clip flag would keep the model animating for as long as the app runs.
/// The one-shot run settles on its final pose and remains the current
/// motion until another request replaces it or an explicit stop removes it.
#[test]
fn shortcut_motion_holds_its_final_pose_after_one_cycle() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);

    let motion = MotionId::new("CAT_motion", 0).expect("motion id");
    let first_sequence = client
        .trigger_shortcut(ShortcutAction::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("shortcut start");
    let started = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("shortcut motion result");
    assert_eq!(
        started.active_motion.as_ref().map(|active| &active.motion),
        Some(&motion)
    );

    // A repeat press while the run is in flight is idempotent: the active
    // identity keeps the first command sequence, so the clip is neither
    // restarted nor its audio replayed.
    let repeat_sequence = client
        .trigger_shortcut(ShortcutAction::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("repeat shortcut start");
    let repeated = client
        .wait_for_command(repeat_sequence, TIMEOUT)
        .expect("repeat shortcut result");
    assert_eq!(
        repeated.active_motion.as_ref().map(|active| &active.motion),
        Some(&motion)
    );
    assert_eq!(
        repeated
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(first_sequence)
    );

    clock.set(Duration::from_secs(2));
    let tick_sequence = client.send(RuntimeCommand::Tick).expect("one-shot tick");
    let completed = client
        .wait_for_command(tick_sequence, TIMEOUT)
        .expect("one-shot completion");
    assert_eq!(
        completed
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(first_sequence),
        "a completed motion must hold its final pose instead of advancing or clearing"
    );

    // The next press, after the run completed, plays the clip again.
    let second_sequence = client
        .trigger_shortcut(ShortcutAction::StartMotion {
            motion,
            priority: MotionPriority::Normal,
        })
        .expect("second shortcut start");
    let restarted = client
        .wait_for_command(second_sequence, TIMEOUT)
        .expect("second shortcut result");
    assert_eq!(
        restarted
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(second_sequence)
    );

    clock.set(Duration::from_secs(4));
    let second_completion_sequence = client
        .send(RuntimeCommand::Tick)
        .expect("second completion");
    let second_completed = client
        .wait_for_command(second_completion_sequence, TIMEOUT)
        .expect("second completion accepted");
    assert_eq!(
        second_completed
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(second_sequence)
    );

    let lower_after_completion = MotionId::new("CAT_motion_lock", 0).expect("motion id");
    let replacement_sequence = client
        .trigger_shortcut(ShortcutAction::StartMotion {
            motion: lower_after_completion,
            priority: MotionPriority::Idle,
        })
        .expect("completed motion must release its priority reservation");
    let replaced = client
        .wait_for_command(replacement_sequence, TIMEOUT)
        .expect("completed motion replacement");
    assert_eq!(
        replaced
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(replacement_sequence)
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn elapsed_one_shot_settles_before_the_next_rendered_frame() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);

    let motion = MotionId::new("CAT_motion", 0).expect("motion id");
    let first_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("start motion");
    let first = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("motion started");
    assert_eq!(
        first
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(first_sequence)
    );

    clock.set(Duration::from_secs(2));
    let replay_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("replay after elapsed duration without an intervening frame");
    let replayed = client
        .wait_for_command(replay_sequence, TIMEOUT)
        .expect("elapsed motion replayed");
    assert_eq!(
        replayed
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(replay_sequence),
        "clock-derived completion must not wait for renderer.evaluate"
    );

    clock.set(Duration::from_secs(4));
    let lower = MotionId::new("CAT_motion_lock", 0).expect("motion id");
    let replacement_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: lower,
            priority: MotionPriority::Idle,
        })
        .expect("lower-priority replacement after elapsed duration");
    let replaced = client
        .wait_for_command(replacement_sequence, TIMEOUT)
        .expect("elapsed motion released priority");
    assert_eq!(
        replaced
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(replacement_sequence),
        "a completed motion must release priority without requiring another frame"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn preview_motion_holds_its_final_pose_after_one_cycle() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);

    let motion = MotionId::new("CAT_motion", 0).expect("motion id");
    let preview_sequence = client
        .send(RuntimeCommand::PreviewMotion(motion.clone()))
        .expect("preview motion");
    let started = client
        .wait_for_command(preview_sequence, TIMEOUT)
        .expect("preview started");
    assert_eq!(
        started.active_motion.as_ref().map(|active| &active.motion),
        Some(&motion)
    );

    clock.set(Duration::from_secs(10));
    let tick_sequence = client.send(RuntimeCommand::Tick).expect("preview tick");
    let completed = client
        .wait_for_command(tick_sequence, TIMEOUT)
        .expect("preview completed");
    assert_eq!(
        completed
            .active_motion
            .as_ref()
            .map(|active| active.command_sequence),
        Some(preview_sequence),
        "a completed preview must remain on its final pose"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn stopped_motion_fades_without_a_jump_and_duplicate_stop_does_not_restart_it() {
    let (_catalog, model) = preset_model_with_motion_fade_out(1.0);
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(model)))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);
    let baseline = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == candidate.model_generation
            && frame.frame_number > candidate.frame_number
    });

    let motion = MotionId::new("CAT_motion", 0).expect("motion id");
    let start_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("start motion");
    client
        .wait_for_command(start_sequence, TIMEOUT)
        .expect("motion started");
    clock.set(Duration::from_millis(500));
    let before_stop = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > baseline.transport_sequence
            && frame.snapshot != baseline.snapshot
    });

    let stop_sequence = client
        .send(RuntimeCommand::StopMotion(motion.clone()))
        .expect("stop motion");
    let stopping = client
        .wait_for_command(stop_sequence, TIMEOUT)
        .expect("motion stopping");
    assert_eq!(
        stopping.active_motion,
        Some(ActiveMotionSnapshot {
            motion: motion.clone(),
            priority: MotionPriority::Normal,
            command_sequence: start_sequence,
            stop_command_sequence: Some(stop_sequence),
        })
    );
    let first_fade_frame = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > before_stop.transport_sequence
    });
    assert_same_render_content(&before_stop.snapshot, &first_fade_frame.snapshot);

    clock.set(Duration::from_millis(700));
    let duplicate_sequence = client
        .send(RuntimeCommand::StopMotion(motion))
        .expect("duplicate stop");
    let duplicate = client
        .wait_for_command(duplicate_sequence, TIMEOUT)
        .expect("duplicate stop completed");
    assert_eq!(
        duplicate
            .active_motion
            .as_ref()
            .and_then(|active| active.stop_command_sequence),
        Some(stop_sequence)
    );

    clock.set(Duration::from_secs(1));
    let half_faded = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > first_fade_frame.transport_sequence
            && frame.snapshot != first_fade_frame.snapshot
            && frame.snapshot != baseline.snapshot
    });
    assert_ne!(half_faded.snapshot, before_stop.snapshot);

    clock.set(Duration::from_millis(1500));
    let completed = client
        .wait_for_revision(duplicate.revision.saturating_add(1), TIMEOUT)
        .expect("fade completion");
    assert!(completed.active_motion.is_none());
    let final_frame = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > half_faded.transport_sequence
    });
    assert_eq!(
        final_frame.snapshot.model_opacity,
        baseline.snapshot.model_opacity
    );
    assert_eq!(
        final_frame.snapshot.drawables.len(),
        baseline.snapshot.drawables.len()
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn expression_commands_crossfade_and_preserve_the_active_expression_on_error() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);
    let baseline = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == candidate.model_generation
            && frame.frame_number > candidate.frame_number
    });

    let first = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
    let first_sequence = client
        .send(RuntimeCommand::SetExpression(first.clone()))
        .expect("set first expression");
    let first_active = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("first expression active");
    assert_eq!(
        first_active.active_expression,
        Some(ActiveExpressionSnapshot {
            expression: first.clone(),
            command_sequence: first_sequence,
        })
    );

    clock.set(Duration::from_millis(400));
    let first_frame = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > baseline.transport_sequence
            && frame.snapshot != baseline.snapshot
    });

    let second = ExpressionId::new("live2d_expression2.exp3.json").expect("expression id");
    let second_sequence = client
        .send(RuntimeCommand::SetExpression(second.clone()))
        .expect("replace expression");
    let second_active = client
        .wait_for_command(second_sequence, TIMEOUT)
        .expect("replacement expression active");
    assert_eq!(
        second_active.active_expression,
        Some(ActiveExpressionSnapshot {
            expression: second.clone(),
            command_sequence: second_sequence,
        })
    );
    clock.set(Duration::from_millis(650));
    let crossfaded = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > first_frame.transport_sequence
            && frame.snapshot != first_frame.snapshot
    });
    assert_ne!(crossfaded.snapshot, first_frame.snapshot);

    clock.set(Duration::from_secs(2));
    let persistence_tick = client
        .send(RuntimeCommand::Tick)
        .expect("expression persistence");
    let persisted = client
        .wait_for_command(persistence_tick, TIMEOUT)
        .expect("latest expression remains active after both fades complete");
    assert_eq!(persisted.active_expression, second_active.active_expression);
    let persisted_frame = wait_for_render_frame(&consumer, |frame| {
        frame.transport_sequence > crossfaded.transport_sequence
    });
    assert_ne!(persisted_frame.snapshot, baseline.snapshot);

    let invalid_sequence = client
        .send(RuntimeCommand::SetExpression(
            ExpressionId::new("missing").expect("syntactically valid expression id"),
        ))
        .expect("invalid expression request");
    let invalid = client
        .wait_for_command(invalid_sequence, TIMEOUT)
        .expect("invalid expression result");
    assert_eq!(
        invalid.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence: invalid_sequence,
            code: RuntimeRenderErrorCode::ExpressionLoadFailed,
        })
    );
    assert_eq!(invalid.active_expression, second_active.active_expression);

    let stopped = owner.shutdown(TIMEOUT).expect("runtime shutdown");
    assert!(stopped.active_expression.is_none());
}
