//! Preparing, committing and failing a model activation.

use super::*;

#[test]
fn runtime_owns_the_committed_installed_model() {
    let data = tempdir().expect("data root");
    let store = ModelStore::new(
        data.path().join("models"),
        data.path().join("locks/models.writer.lock"),
        ModelPackageLimits::default(),
    )
    .expect("model store");
    let committed = CommittedModel::from(
        store
            .import(
                ModelId::parse("unicode").expect("model id"),
                repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
            )
            .expect("installed model"),
    );
    let owner = RuntimeOwner::start(true, 4);
    let client = owner.client();
    let ready = client
        .wait_for_revision(1, TIMEOUT)
        .expect("ready snapshot");
    client
        .send(RuntimeCommand::ActivateModel(Arc::new(committed)))
        .expect("model command");
    let activated = client
        .wait_for_revision(ready.revision + 1, TIMEOUT)
        .expect("model snapshot");
    assert_eq!(
        activated
            .active_model
            .as_ref()
            .expect("active model")
            .id
            .as_str(),
        "unicode"
    );
    owner.shutdown(TIMEOUT).expect("clean shutdown");
}

#[test]
fn runtime_worker_owns_model_evaluation_and_render_publication() {
    let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let bindings = InputBindings::new(BTreeMap::from([(PhysicalKey::KEY_A, HandSide::Left)]));
    let binding_sequence = client
        .send(RuntimeCommand::SetInputBindings(Arc::new(bindings)))
        .expect("bindings command");
    client
        .wait_for_command(binding_sequence, TIMEOUT)
        .expect("bindings applied");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let initial = wait_for_prepared_model(&client, &consumer, activation_sequence);
    assert!(client.snapshot().active_model.is_none());
    let activated = report_model_prepared(&client, &consumer, &initial);
    assert_eq!(activated.state, RuntimeState::Ready);
    assert_eq!(activated.last_command_failure, None);
    assert_eq!(
        activated
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    let committed_baseline = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == initial.model_generation
            && frame.frame_number > initial.frame_number
    });
    let input = owner.input_producer();
    let down_sequence = input
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        })
        .expect("key down");
    client
        .wait_for_input_sequence(down_sequence, TIMEOUT)
        .expect("key down applied");
    let pressed = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == initial.model_generation
            && frame.transport_sequence > committed_baseline.transport_sequence
            && frame.snapshot != committed_baseline.snapshot
    });
    assert_ne!(pressed.snapshot, committed_baseline.snapshot);

    let stopped = owner.shutdown(TIMEOUT).expect("runtime shutdown");
    assert_eq!(stopped.state, RuntimeState::Stopped);
    while consumer.take_latest().is_some() {}
    let diagnostics = consumer.diagnostics();
    assert_eq!(diagnostics.pending, 0);
    assert_eq!(
        diagnostics.published,
        diagnostics.coalesced.saturating_add(diagnostics.consumed)
    );
}

#[test]
fn reliable_input_bypasses_deferred_commands_during_model_preparation() {
    let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 8);
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let activation_sequence = client
        .send(RuntimeCommand::ActivateModel(Arc::new(preset_model(
            "standard",
        ))))
        .expect("activation command");
    let candidate = wait_for_prepared_model(&client, &consumer, activation_sequence);
    let visibility_sequence = client
        .send(RuntimeCommand::SetOverlayVisible(false))
        .expect("deferred visibility command");
    let input_sequence = owner
        .input_producer()
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        })
        .expect("key down behind deferred command");
    let input_applied = client
        .wait_for_input_sequence(input_sequence, TIMEOUT)
        .expect("reliable input bypasses deferred non-input command");
    assert_eq!(input_applied.input.pressed_key_count, 1);
    assert!(input_applied.overlay_visible);

    report_model_prepared(&client, &consumer, &candidate);
    let visibility_applied = client
        .wait_for_command(visibility_sequence, TIMEOUT)
        .expect("deferred command resumes after model commit");
    assert!(!visibility_applied.overlay_visible);
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[test]
fn cpu_and_gpu_model_failures_preserve_the_active_model_and_bindings() {
    let data = tempdir().expect("data root");
    let store = ModelStore::new(
        data.path().join("models"),
        data.path().join("locks/models.writer.lock"),
        ModelPackageLimits::default(),
    )
    .expect("model store");
    let broken = CommittedModel::from(
        store
            .import(
                ModelId::parse("broken").expect("model id"),
                repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
            )
            .expect("install structurally valid model"),
    );
    let clock = Arc::new(ManualClock::default());
    let (owner, consumer) = RuntimeOwner::start_with_rendering_and_clock(
        true,
        8,
        Arc::clone(&clock) as Arc<dyn MonotonicClock>,
    );
    let client = owner.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let left_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
        PhysicalKey::KEY_A,
        HandSide::Left,
    )])));
    let first_sequence = client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(preset_model("standard")),
            input_bindings: left_bindings,
        })
        .expect("standard activation");
    let first = wait_for_prepared_model(&client, &consumer, first_sequence);
    let first_active = report_model_prepared(&client, &consumer, &first);
    assert_eq!(
        first_active
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert_eq!(first_active.active_model_origin, Some(ModelOrigin::Preset));
    let active_motion_id = MotionId::new("CAT_motion", 0).expect("motion id");
    let motion_sequence = client
        .send(RuntimeCommand::StartMotion {
            motion: active_motion_id.clone(),
            priority: MotionPriority::Normal,
        })
        .expect("motion command");
    let motion_active = client
        .wait_for_command(motion_sequence, TIMEOUT)
        .expect("motion active");
    let expected_motion = motion_active.active_motion.clone();
    assert!(expected_motion.is_some());
    let expression_sequence = client
        .send(RuntimeCommand::SetExpression(
            ExpressionId::new("live2d_expression1.exp3.json").expect("expression id"),
        ))
        .expect("expression command");
    let expression_active = client
        .wait_for_command(expression_sequence, TIMEOUT)
        .expect("expression active");
    let expected_expression = expression_active.active_expression.clone();
    assert!(expected_expression.is_some());

    let right_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
        PhysicalKey::KEY_A,
        HandSide::Right,
    )])));
    let broken_sequence = client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(broken),
            input_bindings: right_bindings,
        })
        .expect("broken activation command");
    let rejected = client
        .wait_for_command(broken_sequence, TIMEOUT)
        .expect("broken activation result");
    assert_eq!(
        rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence: broken_sequence,
            code: RuntimeRenderErrorCode::ModelLoadFailed,
        })
    );
    assert_eq!(rejected.state, RuntimeState::Ready);
    assert_eq!(rejected.active_motion, expected_motion);
    assert_eq!(rejected.active_expression, expected_expression);
    assert_eq!(
        rejected
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert_eq!(rejected.active_model_origin, Some(ModelOrigin::Preset));
    let preserved = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == first.model_generation && frame.frame_number > first.frame_number
    });
    assert_eq!(preserved.model_generation, 0);
    let input_producer = owner.input_producer();
    let down_sequence = input_producer
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(1),
        })
        .expect("key down");
    let input_after_rejection = client
        .wait_for_input_sequence(down_sequence, TIMEOUT)
        .expect("key down applied");
    assert!(input_after_rejection.model_input.left_hand_down);
    assert!(!input_after_rejection.model_input.right_hand_down);

    let gpu_rejected_sequence = client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(preset_model("keyboard")),
            input_bindings: Arc::new(InputBindings::new(BTreeMap::from([(
                PhysicalKey::KEY_A,
                HandSide::Right,
            )]))),
        })
        .expect("GPU-rejected activation");
    let gpu_candidate = wait_for_prepared_model(&client, &consumer, gpu_rejected_sequence);
    assert_eq!(gpu_candidate.model_generation, 1);
    let pending_release_sequence = input_producer
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Up,
            source: InputSource::Capture,
            at: MonotonicMillis::new(2),
        })
        .expect("key up while model commit is pending");
    let released_while_pending = client
        .wait_for_input_sequence(pending_release_sequence, TIMEOUT)
        .expect("pending model does not block key release");
    assert!(!released_while_pending.model_input.left_hand_down);
    assert!(!released_while_pending.model_input.right_hand_down);
    let pending_down_sequence = input_producer
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge: InputEdge::Down,
            source: InputSource::Capture,
            at: MonotonicMillis::new(3),
        })
        .expect("key down while model commit is pending");
    let while_pending = client
        .wait_for_input_sequence(pending_down_sequence, TIMEOUT)
        .expect("pending model does not block key down");
    assert_eq!(
        while_pending
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert!(while_pending.model_input.left_hand_down);
    assert!(!while_pending.model_input.right_hand_down);

    let rejected_token = gpu_candidate.model_commit.expect("candidate token");
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: rejected_token,
            outcome: ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed),
        })
        .expect("reject GPU candidate");
    let gpu_rejected = client
        .wait_for_command(gpu_rejected_sequence, TIMEOUT)
        .expect("GPU rejection applied");
    assert_eq!(
        gpu_rejected.last_command_failure,
        Some(RuntimeCommandFailure {
            sequence: gpu_rejected_sequence,
            code: RuntimeRenderErrorCode::GpuPreparationFailed,
        })
    );
    assert_eq!(
        gpu_rejected
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("standard")
    );
    assert_eq!(gpu_rejected.active_motion, expected_motion);
    assert_eq!(gpu_rejected.active_expression, expected_expression);
    assert!(gpu_rejected.model_input.left_hand_down);
    assert!(!gpu_rejected.model_input.right_hand_down);
    let resumed = wait_for_render_frame(&consumer, |frame| {
        frame.model_generation == first.model_generation
            && frame.transport_sequence > gpu_candidate.transport_sequence
    });
    assert!(resumed.frame_number > first.frame_number);

    let replacement_sequence = client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(preset_model("keyboard")),
            input_bindings: Arc::new(InputBindings::new(BTreeMap::from([(
                PhysicalKey::KEY_A,
                HandSide::Right,
            )]))),
        })
        .expect("replacement activation");
    let replacement = wait_for_prepared_model(&client, &consumer, replacement_sequence);
    assert_eq!(replacement.model_generation, 2);
    let replaced = report_model_prepared(&client, &consumer, &replacement);
    assert_eq!(replaced.last_command_failure, None);
    assert_eq!(
        replaced
            .active_model
            .as_ref()
            .map(|model| model.id.as_str()),
        Some("keyboard")
    );
    assert!(replaced.active_motion.is_none());
    assert!(replaced.active_expression.is_none());
    assert!(replaced.model_input.right_hand_down);
    assert!(!replaced.model_input.left_hand_down);
    owner.shutdown(TIMEOUT).expect("runtime shutdown");
}
