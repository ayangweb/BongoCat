//! Measuring a model switch without showing it.
//!
//! The preview replays the switch a fixed number of times inside a hidden
//! session and reports whether each one stayed inside the handle and thread
//! budget, which is the only way to see a switch's cost before the user pays it
//! interactively.

use super::*;

pub(crate) const PRESET_MODEL_IDS: [&str; 3] = ["standard", "keyboard", "gamepad"];

pub(crate) fn run_model_switch_preview(
    model_id: &str,
    model_root: &Path,
    switch_cycles: u32,
) -> Result<PreviewReport, OverlayError> {
    if switch_cycles == 0 {
        return Err(OverlayError::new(
            "model-switch cycle count must be greater than zero",
        ));
    }
    let model_id =
        ModelId::parse(model_id).map_err(|error| OverlayError::new(error.to_string()))?;
    let preset_root = model_root
        .parent()
        .ok_or_else(|| OverlayError::new("preset model root has no catalog parent"))?;
    let catalog = PresetModelCatalog::open(preset_root, ModelPackageLimits::default())
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let models = PRESET_MODEL_IDS
        .iter()
        .map(|id| {
            let id = ModelId::parse(*id).map_err(|error| OverlayError::new(error.to_string()))?;
            catalog
                .load(&id)
                .map(Arc::new)
                .map_err(|error| OverlayError::new(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut current_model_index = PRESET_MODEL_IDS
        .iter()
        .position(|id| *id == model_id.as_str())
        .ok_or_else(|| OverlayError::new("initial preset is not in the switch sequence"))?;

    let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
    let runtime_client = runtime.client();
    runtime_client
        .wait_for_revision(1, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview runtime did not become ready"))?;
    let input_producer = runtime.input_producer();
    let (initial_token, initial_frame) = prepare_switch_frame(
        &runtime_client,
        &render_consumer,
        Arc::clone(&models[current_model_index]),
        Arc::new(preview_input_bindings(model_id.as_str())),
    )?;

    let com_apartment = ComApartment::initialize()?;
    let mut overlay = match NativeOverlay::create(
        &initial_frame,
        OverlaySessionOptions::default(),
        None,
        None,
        None,
    ) {
        Ok(overlay) => overlay,
        Err(error) => {
            reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
            return Err(error);
        }
    };
    overlay.draw(true)?;
    overlay.set_visible(true)?;
    report_model_commit(
        &runtime_client,
        &render_consumer,
        initial_token,
        ModelCommitOutcome::Prepared,
    )?;

    let mut frames_presented = 1_u64;
    let mut dynamic_snapshots = 0_u64;
    let mut previous_snapshot = Arc::clone(&initial_frame.snapshot);
    let initial_generation = overlay.renderer.model_generation;

    let pressed_sequence = publish_preview_key_edge(&input_producer, InputEdge::Down, 1)?;
    let pressed = runtime_client
        .wait_for_input_sequence(pressed_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview key press did not reach the runtime"))?;
    if !pressed.model_input.left_hand_down || pressed.model_input.right_hand_down {
        return Err(OverlayError::new(
            "initial model bindings did not map the probe key to the left hand",
        ));
    }

    let rejected_model_index = (current_model_index + 1) % models.len();
    let rejected_bindings = Arc::new(InputBindings::new(BTreeMap::from([(
        PhysicalKey::KEY_A,
        HandSide::Right,
    )])));
    let (rejected_token, rejected_frame) = prepare_switch_frame(
        &runtime_client,
        &render_consumer,
        Arc::clone(&models[rejected_model_index]),
        rejected_bindings,
    )?;
    let mut invalid_resources = rejected_frame.resources.as_ref().clone();
    let Some(first_texture) = invalid_resources.textures.first_mut() else {
        return Err(OverlayError::new(
            "model-switch probe requires at least one texture",
        ));
    };
    first_texture.path = model_root.join(".missing-d3d11-prepare-texture.png");
    let invalid_frame = RenderFrame {
        resources: Arc::new(invalid_resources),
        ..rejected_frame
    };
    if overlay.renderer.sync_frame(&invalid_frame).is_ok() {
        return Err(OverlayError::new(
            "invalid D3D11 model preparation unexpectedly succeeded",
        ));
    }
    if overlay.renderer.model_generation != initial_generation {
        return Err(OverlayError::new(
            "failed D3D11 preparation replaced the active GPU generation",
        ));
    }
    reject_model_commit(&runtime_client, &render_consumer, rejected_token)?;
    let rejected = runtime_client.snapshot();
    if rejected.pending_model.is_some()
        || rejected
            .active_model
            .as_ref()
            .is_none_or(|active| active.id != model_id)
        || !rejected.model_input.left_hand_down
        || rejected.model_input.right_hand_down
    {
        return Err(OverlayError::new(
            "GPU rejection did not preserve the active CPU model and input bindings",
        ));
    }
    overlay.draw(true)?;
    frames_presented = frames_presented.saturating_add(1);

    let released_sequence = publish_preview_key_edge(&input_producer, InputEdge::Up, 2)?;
    let released = runtime_client
        .wait_for_input_sequence(released_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview key release did not reach the runtime"))?;
    if released.model_input.left_hand_down || released.model_input.right_hand_down {
        return Err(OverlayError::new(
            "preview key release left a pressed hand after GPU rejection",
        ));
    }

    // Initialize the DXGI memory-query path before sampling the warmup thread
    // high-water mark. Some drivers create a helper thread on the first query.
    let _ = overlay.renderer.current_local_memory_usage()?;
    let switches_per_cycle = models.len() as u64;
    let warmup_cycles = SWITCH_WARMUP_CYCLES.max(u64::from(switch_cycles));
    let warmup_switches = warmup_cycles.saturating_mul(switches_per_cycle);
    let target_switches = u64::from(switch_cycles).saturating_mul(switches_per_cycle);
    let total_target_switches = warmup_switches.saturating_add(target_switches);
    let mut total_switches = 0_u64;
    let mut model_switches = 0_u64;
    let mut gpu_bytes_before = None;
    let mut handles_before = None;
    let mut warmup_thread_high_water = 0_u32;
    while total_switches < total_target_switches {
        pump_window_messages();
        current_model_index = (current_model_index + 1) % models.len();
        let target_id = PRESET_MODEL_IDS[current_model_index];
        let generation_before = overlay.renderer.model_generation;
        let (token, frame) = prepare_switch_frame(
            &runtime_client,
            &render_consumer,
            Arc::clone(&models[current_model_index]),
            Arc::new(preview_input_bindings(target_id)),
        )?;
        if frame.snapshot.as_ref() != previous_snapshot.as_ref() {
            dynamic_snapshots = dynamic_snapshots.saturating_add(1);
        }
        if !overlay.renderer.sync_frame(&frame)? {
            return Err(OverlayError::new(
                "D3D11 renderer did not replace a newer model generation",
            ));
        }
        overlay.resize_for_model(frame.snapshot.canvas)?;
        if overlay.renderer.model_generation <= generation_before {
            return Err(OverlayError::new(
                "D3D11 renderer committed a non-monotonic model generation",
            ));
        }
        overlay.draw(true)?;
        report_model_commit(
            &runtime_client,
            &render_consumer,
            token,
            ModelCommitOutcome::Prepared,
        )?;
        let committed = runtime_client.snapshot();
        if committed.pending_model.is_some()
            || committed
                .active_model
                .as_ref()
                .is_none_or(|active| active.id.as_str() != target_id)
        {
            return Err(OverlayError::new(
                "runtime and D3D11 renderer did not commit the same model",
            ));
        }
        previous_snapshot = frame.snapshot;
        frames_presented = frames_presented.saturating_add(1);
        total_switches = total_switches.saturating_add(1);

        if total_switches <= warmup_switches {
            warmup_thread_high_water = warmup_thread_high_water.max(
                process_thread_count().map_err(windows_error("count warmup process threads"))?,
            );
        } else {
            model_switches = model_switches.saturating_add(1);
        }
        if total_switches == warmup_switches {
            gpu_bytes_before = Some(overlay.renderer.current_local_memory_usage()?);
            handles_before =
                Some(process_handle_count().map_err(windows_error("count process handles"))?);
            warmup_thread_high_water =
                settle_process_threads(warmup_thread_high_water, THREAD_SETTLE_TIMEOUT)?.high_water;
        }
    }

    let gpu_bytes_before = gpu_bytes_before
        .ok_or_else(|| OverlayError::new("model-switch probe did not finish its warmup cycle"))?;
    if warmup_thread_high_water == 0 {
        return Err(OverlayError::new(
            "model-switch probe did not sample thread usage",
        ));
    }
    let handles_before = handles_before
        .ok_or_else(|| OverlayError::new("model-switch probe did not sample handle usage"))?;
    let gpu_bytes_after = overlay.renderer.current_local_memory_usage()?;
    let handles_after =
        process_handle_count().map_err(windows_error("count process handles after switching"))?;
    let threads_after =
        settle_process_threads(warmup_thread_high_water, THREAD_SETTLE_TIMEOUT)?.settled_count;
    if gpu_bytes_after > gpu_bytes_before {
        return Err(OverlayError::new(format!(
            "DXGI local memory usage grew from {gpu_bytes_before} to {gpu_bytes_after} bytes during model switching"
        )));
    }
    if thread_growth_exceeded(warmup_thread_high_water, threads_after) {
        return Err(OverlayError::new(format!(
            "process thread count exceeded the warmup high-water mark {warmup_thread_high_water} plus {THREAD_GROWTH_LIMIT} with {threads_after} threads during model switching"
        )));
    }
    if handles_after > handles_before.saturating_add(HANDLE_GROWTH_LIMIT) {
        return Err(OverlayError::new(format!(
            "process handle count grew from {handles_before} to {handles_after} during model switching"
        )));
    }

    let final_snapshot = runtime_client.snapshot();
    if final_snapshot
        .active_model
        .as_ref()
        .is_none_or(|active| active.id != model_id)
    {
        return Err(OverlayError::new(
            "complete model-switch cycles did not return to the initial model",
        ));
    }
    let drawable_count = overlay.renderer.model.meshes.len();
    let masked_drawable_count = overlay.renderer.model.masked_drawable_count;
    let texture_count = overlay.renderer.model.textures.len();
    let stopped = runtime
        .shutdown(RUNTIME_TIMEOUT)
        .map_err(|error| OverlayError::new(error.to_string()))?;
    while render_consumer.take_latest().is_some() {}
    let render_diagnostics = render_consumer.diagnostics();
    drop(overlay);
    drop(com_apartment);

    Ok(PreviewReport {
        frames_presented,
        dynamic_snapshots,
        runtime_input_events: stopped.input.transport.enqueued,
        platform_input_edges: 0,
        runtime_cursor_published: stopped.cursor.transport.published,
        runtime_cursor_coalesced: stopped.cursor.transport.coalesced,
        runtime_cursor_consumed: stopped.cursor.transport.consumed,
        platform_cursor_samples: 0,
        render_frames_published: render_diagnostics.published,
        render_frames_coalesced: render_diagnostics.coalesced,
        render_frames_consumed: render_diagnostics.consumed,
        model_switches,
        failed_gpu_prepare_preserved: true,
        gpu_bytes_before,
        gpu_bytes_after,
        drawable_count,
        masked_drawable_count,
        texture_count,
        warmup_thread_high_water: Some(warmup_thread_high_water),
        threads_after: Some(threads_after),
        frame_timing: None,
    })
}

pub(crate) fn prepare_switch_frame(
    runtime_client: &RuntimeClient,
    render_consumer: &RenderConsumer,
    model: Arc<CommittedModel>,
    input_bindings: Arc<InputBindings>,
) -> Result<(ModelCommitToken, RenderFrame), OverlayError> {
    let sequence = runtime_client
        .send(RuntimeCommand::ActivateModelWithBindings {
            model,
            input_bindings,
        })
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let prepared = runtime_client
        .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("model switch was not prepared"))?;
    if let Some(failure) = prepared
        .last_command_failure
        .filter(|failure| failure.sequence == sequence)
    {
        return Err(OverlayError::new(format!(
            "model switch failed before GPU preparation: {:?}",
            failure.code
        )));
    }
    let token = prepared
        .pending_model
        .as_ref()
        .filter(|pending| pending.token.command_sequence == sequence)
        .map(|pending| pending.token)
        .ok_or_else(|| OverlayError::new("runtime published the wrong pending model token"))?;
    let frame = render_consumer
        .take_latest()
        .filter(|frame| frame.model_commit == Some(token))
        .ok_or_else(|| OverlayError::new("runtime did not publish the matching model frame"))?;
    Ok((token, frame))
}

pub(crate) fn publish_preview_key_edge(
    producer: &InputProducer,
    edge: InputEdge,
    at_millis: u64,
) -> Result<u64, OverlayError> {
    producer
        .publish(InputEvent::Edge {
            control: InputControl::Key(PhysicalKey::KEY_A),
            edge,
            source: InputSource::Capture,
            at: MonotonicMillis::new(at_millis),
        })
        .map_err(|error| OverlayError::new(error.to_string()))
}

pub(crate) fn preview_input_bindings(model_id: &str) -> InputBindings {
    let mut key_hands = BTreeMap::new();
    if matches!(model_id, "standard" | "keyboard") {
        for usage in 0x04..=0x27 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
        for usage in [
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x35, 0x38, 0x39, 0x4c, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4,
            0xe5, 0xe6, 0xe7,
        ] {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
    } else {
        key_hands.insert(PhysicalKey::KEY_A, HandSide::Left);
    }
    if matches!(model_id, "keyboard" | "gamepad") {
        for usage in 0x4f..=0x52 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Right);
        }
    }
    let gamepad_hands = if model_id == "gamepad" {
        BTreeMap::from([
            (GamepadButton::South, HandSide::Left),
            (GamepadButton::East, HandSide::Right),
        ])
    } else {
        BTreeMap::new()
    };
    InputBindings::with_gamepad_hands(key_hands, gamepad_hands)
}
