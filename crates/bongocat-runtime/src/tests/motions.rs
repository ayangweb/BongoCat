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

/// A remembered expression is a fact about what the user chose, so it has to
/// outlive the model switch that clears the expression actually on screen — and
/// only a command may produce one, because the idle scheduler plays expressions
/// through the renderer rather than through the command queue.
#[test]
fn only_an_expression_command_is_remembered_and_the_record_outlives_the_model() {
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

    // Nothing has been chosen yet, so there is nothing for a reader to collect.
    assert_eq!(client.unrecorded_user_expression(None), None);
    assert_eq!(client.snapshot().user_expression_memory, None);

    // A name the model does not declare is a failed request, so it is not a
    // choice and must not become the face the model is restored to.
    let rejected_sequence = client
        .send(RuntimeCommand::SetExpression(
            ExpressionId::new("not-declared.exp3.json").expect("syntactically valid name"),
        ))
        .expect("undeclared expression request");
    let rejected = client
        .wait_for_command(rejected_sequence, TIMEOUT)
        .expect("undeclared expression result");
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence: rejected_sequence,
            code: RuntimeRenderErrorCode::ExpressionLoadFailed,
        })
    );
    assert_eq!(rejected.user_expression_memory, None);

    let chosen = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
    let chosen_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("choose expression");
    let remembered = client
        .wait_for_command(chosen_sequence, TIMEOUT)
        .expect("chosen expression active")
        .user_expression_memory
        .expect("a command records the choice");
    assert_eq!(remembered.expression, chosen);
    assert_eq!(remembered.model.as_str(), "standard");
    assert_eq!(remembered.model_origin, ModelOrigin::Preset);
    assert_eq!(remembered.command_sequence, chosen_sequence);

    // The reader collects a record once. Passing back the sequence it already
    // has is how the common case stays a comparison rather than a copy.
    assert_eq!(
        client.unrecorded_user_expression(None),
        Some(remembered.clone())
    );
    assert_eq!(
        client.unrecorded_user_expression(Some(chosen_sequence)),
        None
    );
    assert_eq!(
        client.unrecorded_user_expression(Some(chosen_sequence - 1)),
        Some(remembered.clone())
    );

    // Switching models clears what is displayed, and the choice that produced it
    // still has to reach the configuration that remembers it per model.
    let second_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "keyboard",
        ))))
        .expect("second activation command");
    let second = wait_for_prepared_model(&client, &consumer, second_sequence);
    let switched = report_model_prepared(&client, &consumer, &second);
    assert_eq!(switched.active_expression, None);
    assert_eq!(
        switched.user_expression_memory,
        Some(remembered.clone()),
        "the first model's choice survives the switch that cleared the display"
    );
    assert_eq!(
        client.unrecorded_user_expression(Some(chosen_sequence)),
        None,
        "a record already collected is not offered again"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

/// The idle scheduler picks expressions on its own. A pick the user never made
/// must not be recorded, or the next launch would restore a random face and call
/// it the one the user chose.
#[test]
fn an_automatic_expression_is_never_remembered() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let settings_sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(
            RandomBehaviorSettings {
                mode: RandomBehaviorMode::Expressions,
                interval_seconds: 1,
            },
        ))
        .expect("random behavior setting accepted");
    client
        .wait_for_command(settings_sequence, TIMEOUT)
        .expect("random behavior setting published");

    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("model activation accepted");
    let frame = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &frame);

    clock.set(Duration::from_secs(1));
    let tick = client
        .send(RuntimeCommand::Tick)
        .expect("random tick accepted");
    let mut snapshot = client
        .wait_for_command(tick, TIMEOUT)
        .expect("random behavior tick published");
    let deadline = Instant::now() + TIMEOUT;
    while snapshot.active_expression.is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
        snapshot = client.snapshot();
    }
    assert!(
        snapshot.active_expression.is_some(),
        "the due scheduler must select a declared expression"
    );
    assert_eq!(
        snapshot.user_expression_memory, None,
        "an automatic pick is not a choice the user made"
    );
    assert_eq!(client.unrecorded_user_expression(None), None);

    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

/// With the toggle off — the default — a repeat of the expression already showing
/// is what it has always been: the same face applied again.
///
/// This is the direction an existing configuration keeps, so it is the direction
/// that has to be provably unchanged rather than assumed.
#[test]
fn a_repeated_expression_is_applied_again_while_the_toggle_is_off() {
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

    let chosen = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
    let first_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("choose expression");
    let first = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("chosen expression active");
    assert_eq!(
        first.active_expression,
        Some(ActiveExpressionSnapshot {
            expression: chosen.clone(),
            command_sequence: first_sequence,
        })
    );

    let repeat_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("repeat the same expression");
    let repeat = client
        .wait_for_command(repeat_sequence, TIMEOUT)
        .expect("repeated expression answered");
    assert_eq!(
        repeat.active_expression,
        Some(ActiveExpressionSnapshot {
            expression: chosen,
            command_sequence: repeat_sequence,
        }),
        "the repeat is applied, not turned into an off request"
    );
    assert!(
        repeat.user_expression_memory.is_some(),
        "applying the same face again is still the user choosing it"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

/// With the toggle on, a repeat of the expression in effect takes it off and the
/// model returns to its own default face.
///
/// Both trigger sources reach the runtime as this one command, so the decision
/// lives where the runtime knows which expression is in effect. A request for a
/// *different* expression is unaffected: turning one face off is not a general
/// "next trigger clears the model" rule.
#[test]
fn a_repeated_expression_turns_itself_off_while_the_toggle_is_on() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let toggle_sequence = client
        .send(RuntimeCommand::SetModelSettings(ModelSettings {
            toggle_repeated_expression: true,
            ..ModelSettings::default()
        }))
        .expect("toggle accepted");
    let toggled = client
        .wait_for_command(toggle_sequence, TIMEOUT)
        .expect("toggle published");
    assert!(toggled.model_settings.toggle_repeated_expression);

    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);
    wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == candidate.model_generation
            && frame.frame_number > candidate.frame_number
    });

    let chosen = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
    let first_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("choose expression");
    let first = client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("chosen expression active");
    assert_eq!(
        first
            .active_expression
            .as_ref()
            .map(|active| &active.expression),
        Some(&chosen)
    );
    let remembered_sequence = first.user_expression_memory.as_ref().map(|memory| {
        assert_eq!(memory.expression, chosen);
        memory.command_sequence
    });

    clock.set(Duration::from_millis(400));
    wait_for_render_frame(&consumer, |consumer_frame| {
        consumer_frame.model_generation == candidate.model_generation
    });

    // The repeat is the off request, and it deliberately records nothing: a
    // remembered expression that the user just turned off would come back on the
    // next launch as if they had chosen it.
    let repeat_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("repeat the same expression");
    let repeat = client
        .wait_for_command(repeat_sequence, TIMEOUT)
        .expect("repeated expression answered");
    assert_eq!(
        repeat.active_expression, None,
        "the expression in effect is turned off rather than re-applied"
    );
    assert_eq!(
        repeat
            .user_expression_memory
            .as_ref()
            .map(|memory| memory.command_sequence),
        remembered_sequence,
        "turning an expression off writes no new remembered choice"
    );
    assert_eq!(
        client.unrecorded_user_expression(Some(repeat_sequence - 1)),
        None,
        "the reader has nothing new to persist from an off request"
    );

    // Once the fade the clip itself declares has run there is nothing left to
    // apply, which is the whole point of the behaviour: otherwise the only way
    // back to the model's own face is a model switch. The fade itself is a
    // renderer concern and is covered by the clearing tests beside it.
    clock.set(Duration::from_secs(2));
    let settled = client
        .send(RuntimeCommand::Tick)
        .expect("expression fade runs on");
    let settled = client
        .wait_for_command(settled, TIMEOUT)
        .expect("fade published");
    assert_eq!(settled.active_expression, None);

    // Asking for a different expression is still a first request, not an off.
    let other = ExpressionId::new("live2d_expression2.exp3.json").expect("expression id");
    let other_sequence = client
        .send(RuntimeCommand::SetExpression(other.clone()))
        .expect("choose a different expression");
    let switched_face = client
        .wait_for_command(other_sequence, TIMEOUT)
        .expect("different expression active");
    assert_eq!(
        switched_face
            .active_expression
            .as_ref()
            .map(|active| &active.expression),
        Some(&other)
    );
    assert_eq!(
        switched_face
            .user_expression_memory
            .as_ref()
            .map(|memory| memory.expression.clone()),
        Some(other),
        "a different expression is a choice the user made, and is remembered as one"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

/// The automatic pick is not a repeat of the user's face.
///
/// The idle scheduler plays expressions through the renderer rather than through
/// the command queue, which is what keeps the toggle out of its path: with the
/// toggle on, a scheduled expression still applies, and the user's own choice
/// still governs what a repeat does.
#[test]
fn the_toggle_does_not_reach_the_automatic_expression_picker() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    let settings_sequence = client
        .send(RuntimeCommand::SetModelSettings(ModelSettings {
            toggle_repeated_expression: true,
            ..ModelSettings::default()
        }))
        .expect("model settings accepted");
    client
        .wait_for_command(settings_sequence, TIMEOUT)
        .expect("model settings published");
    let random_sequence = client
        .send(RuntimeCommand::SetRandomBehaviorSettings(
            RandomBehaviorSettings {
                mode: RandomBehaviorMode::Expressions,
                interval_seconds: 1,
            },
        ))
        .expect("random behavior setting accepted");
    client
        .wait_for_command(random_sequence, TIMEOUT)
        .expect("random behavior setting published");

    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("model activation accepted");
    let frame = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &frame);

    clock.set(Duration::from_secs(1));
    let tick = client
        .send(RuntimeCommand::Tick)
        .expect("random tick accepted");
    let mut snapshot = client
        .wait_for_command(tick, TIMEOUT)
        .expect("random behavior tick published");
    let deadline = Instant::now() + TIMEOUT;
    while snapshot.active_expression.is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
        snapshot = client.snapshot();
    }
    assert!(
        snapshot.active_expression.is_some(),
        "an automatic pick is not a repeat, so the toggle must not turn it back off"
    );

    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

/// The remembered expression a model is restored with is a first request.
///
/// Model activation clears the expression in effect before any deferred command
/// runs, so the restore can never read as a repeat — otherwise a restored face
/// would immediately switch itself off again.
#[test]
fn a_restored_expression_is_never_read_as_a_repeat() {
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let toggle_sequence = client
        .send(RuntimeCommand::SetModelSettings(ModelSettings {
            toggle_repeated_expression: true,
            ..ModelSettings::default()
        }))
        .expect("toggle accepted");
    client
        .wait_for_command(toggle_sequence, TIMEOUT)
        .expect("toggle published");

    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    report_model_prepared(&client, &consumer, &candidate);
    let chosen = ExpressionId::new("live2d_expression1.exp3.json").expect("expression id");
    let first_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("choose expression");
    client
        .wait_for_command(first_sequence, TIMEOUT)
        .expect("chosen expression active");

    // The application restores the remembered expression after switching back to
    // a model. Wait for the commit before sending that follow-up command so this
    // assertion observes the cleared model state rather than a later snapshot.
    let switch_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "keyboard",
        ))))
        .expect("second activation command");
    let switched = wait_for_prepared_model(&client, &consumer, switch_sequence);
    let restored = report_model_prepared(&client, &consumer, &switched);
    assert_eq!(restored.active_expression, None);
    let restore_sequence = client
        .send(RuntimeCommand::SetExpression(chosen.clone()))
        .expect("restore the remembered expression");
    let applied = client
        .wait_for_command(restore_sequence, TIMEOUT)
        .expect("restored expression applied");
    assert_eq!(
        applied
            .active_expression
            .as_ref()
            .map(|active| &active.expression),
        Some(&chosen),
        "the restored face is applied rather than taken straight back off"
    );

    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn overlapping_motions_keep_independent_identity_priority_audio_and_fades() {
    let (_catalog, model) = preset_model_with_motion_fade_out(1.0);
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_audio_and_clock(
        true,
        true,
        16,
        MotionAudioClient::unavailable(),
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation = client
        .send(RuntimeCommand::ActivateModel(Arc::new(model)))
        .expect("activate");
    let candidate = wait_for_prepared_model(&client, &consumer, activation);
    report_model_prepared(&client, &consumer, &candidate);
    let send = |command| {
        let sequence = client.send(command).expect("send command");
        client
            .wait_for_command(sequence, TIMEOUT)
            .expect("command result")
    };
    send(RuntimeCommand::SetModelSettings(ModelSettings {
        allow_motion_overlap: true,
        ..ModelSettings::default()
    }));
    let first = MotionId::new("CAT_motion", 0).expect("first motion");
    let second = MotionId::new("CAT_motion", 1).expect("second motion");
    let first_started = send(RuntimeCommand::StartMotion {
        motion: first.clone(),
        priority: MotionPriority::Force,
    });
    clock.set(Duration::from_millis(100));
    let both = send(RuntimeCommand::StartMotion {
        motion: second.clone(),
        priority: MotionPriority::Normal,
    });
    assert_eq!(
        both.active_motions.len(),
        2,
        "different layers do not compete for priority"
    );
    assert_eq!(both.active_motions[0], first_started.active_motions[0]);
    assert_eq!(
        both.active_motion.as_ref().map(|active| &active.motion),
        Some(&second)
    );
    let audio_before_repeat = both.motion_audio.rejected_after_shutdown;
    let repeated = send(RuntimeCommand::StartMotion {
        motion: first.clone(),
        priority: MotionPriority::Force,
    });
    assert_eq!(repeated.active_motions, both.active_motions);
    assert_eq!(
        repeated.motion_audio.rejected_after_shutdown,
        audio_before_repeat
    );
    let invalid = send(RuntimeCommand::PreviewMotion(
        MotionId::new("missing", 0).expect("missing motion"),
    ));
    assert!(invalid.last_command_failure.is_some());
    assert_eq!(invalid.active_motions, both.active_motions);
    assert_eq!(
        invalid.motion_audio.rejected_after_shutdown,
        audio_before_repeat
    );
    let previewed = send(RuntimeCommand::PreviewMotion(second.clone()));
    assert_eq!(previewed.active_motions[0], both.active_motions[0]);
    assert_ne!(
        previewed.active_motions[1].command_sequence,
        both.active_motions[1].command_sequence
    );
    clock.set(Duration::from_millis(500));
    let stopping = send(RuntimeCommand::StopMotion(first.clone()));
    assert!(stopping.active_motions[0].stop_command_sequence.is_some());
    assert_eq!(stopping.active_motions[1], previewed.active_motions[1]);
    clock.set(Duration::from_millis(700));
    let repeated_stop = send(RuntimeCommand::StopMotion(first.clone()));
    assert_eq!(repeated_stop.active_motions, stopping.active_motions);
    assert_eq!(
        repeated_stop.motion_audio.rejected_after_shutdown,
        stopping.motion_audio.rejected_after_shutdown
    );
    clock.set(Duration::from_millis(1500));
    let deadline = Instant::now() + TIMEOUT;
    while client.snapshot().active_motions.len() != 1 {
        assert!(
            Instant::now() < deadline,
            "only the stopped layer must finish its fade"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        client.snapshot().active_motions[0],
        previewed.active_motions[1]
    );
    send(RuntimeCommand::StartMotion {
        motion: first.clone(),
        priority: MotionPriority::Normal,
    });
    let single = send(RuntimeCommand::SetModelSettings(ModelSettings::default()));
    assert_eq!(single.active_motions.len(), 1);
    assert_eq!(single.active_motions[0].motion, first);
    let ignored = send(RuntimeCommand::StartMotion {
        motion: second,
        priority: MotionPriority::Idle,
    });
    assert_eq!(ignored.active_motions, single.active_motions);
    let stopped = owner.shutdown(TIMEOUT).expect("shutdown");
    assert!(stopped.active_motions.is_empty());
}
