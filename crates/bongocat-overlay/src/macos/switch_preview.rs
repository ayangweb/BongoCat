//! The standalone model preview.
//!
//! `just preview` shows one model full-screen for as long as the user asks,
//! driving the same session and the same renderer the overlay uses, with a
//! synthetic input driver standing in for the keyboard. It is a diagnostic
//! tool, not a product mode, which is why its input table is allowed to be
//! narrower than the runtime's.

use super::*;

pub(crate) const SWITCH_WARMUP_FRAMES: u64 = 30;

pub(crate) const SWITCH_SETTLE_FRAMES: u64 = 30;

pub(crate) const PRESET_MODEL_IDS: [&str; 3] = ["standard", "keyboard", "gamepad"];

pub(crate) fn run_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
    interactive: bool,
    switch_cycles: Option<u32>,
) -> Result<PreviewReport, OverlayError> {
    if interactive && switch_cycles.is_some() {
        return Err(OverlayError::new(
            "interactive input and model-switch probing cannot run together",
        ));
    }
    if switch_cycles == Some(0) {
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
    let committed = Arc::new(
        catalog
            .load(&model_id)
            .map_err(|error| OverlayError::new(error.to_string()))?,
    );
    let switch_models = switch_cycles
        .map(|_| {
            PRESET_MODEL_IDS
                .iter()
                .map(|id| {
                    let id = ModelId::parse(*id)
                        .map_err(|error| OverlayError::new(error.to_string()))?;
                    catalog
                        .load(&id)
                        .map(Arc::new)
                        .map_err(|error| OverlayError::new(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let (runtime, render_consumer) = RuntimeOwner::start_with_rendering(true, 64);
    let runtime_client = runtime.client();
    runtime_client
        .wait_for_revision(1, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview runtime did not become ready"))?;
    let binding_sequence = runtime_client
        .send(RuntimeCommand::SetInputBindings(std::sync::Arc::new(
            preview_input_bindings(model_id.as_str()),
        )))
        .map_err(|error| OverlayError::new(error.to_string()))?;
    runtime_client
        .wait_for_command(binding_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview input bindings were not applied"))?;
    let activation_sequence = runtime_client
        .send(RuntimeCommand::ActivateModel(Arc::clone(&committed)))
        .map_err(|error| OverlayError::new(error.to_string()))?;
    let prepared = runtime_client
        .wait_for_model_preparation(activation_sequence, RUNTIME_TIMEOUT)
        .ok_or_else(|| OverlayError::new("preview model activation was not prepared"))?;
    if let Some(failure) = prepared
        .last_command_failure
        .filter(|failure| failure.sequence == activation_sequence)
    {
        return Err(OverlayError::new(format!(
            "preview model activation failed: {:?}",
            failure.code
        )));
    }
    let initial_frame = render_consumer
        .take_latest()
        .ok_or_else(|| OverlayError::new("runtime did not publish the initial render frame"))?;
    let initial_token = initial_frame
        .model_commit
        .filter(|token| token.command_sequence == activation_sequence)
        .ok_or_else(|| OverlayError::new("initial frame has the wrong model commit token"))?;
    let mut previous_snapshot = std::sync::Arc::clone(&initial_frame.snapshot);
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| OverlayError::new("macOS preview must run on the main thread"))?;
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    application.finishLaunching();
    let mut overlay =
        match NativeOverlay::create(mtm, &initial_frame, OverlaySessionOptions::default(), None) {
            Ok(overlay) => overlay,
            Err(error) => {
                reject_model_commit(&runtime_client, &render_consumer, initial_token)?;
                return Err(error);
            }
        };
    report_model_commit(
        &runtime_client,
        &render_consumer,
        initial_token,
        ModelCommitOutcome::Prepared,
    )?;
    let failed_gpu_prepare_preserved = if switch_cycles.is_some() {
        let mut invalid_resources = initial_frame.resources.as_ref().clone();
        let Some(first_texture) = invalid_resources.textures.first_mut() else {
            return Err(OverlayError::new(
                "model-switch probe requires at least one texture",
            ));
        };
        first_texture.path = model_root.join(".missing-gpu-prepare-texture.png");
        let probe_frame = RenderFrame {
            transport_sequence: initial_frame.transport_sequence.saturating_add(1),
            model_generation: initial_frame.model_generation.saturating_add(1),
            frame_number: 0,
            model_commit: None,
            resources: Arc::new(invalid_resources),
            snapshot: Arc::clone(&initial_frame.snapshot),
        };
        let generation_before = overlay.model_generation;
        if overlay.sync_frame(&probe_frame).is_ok() {
            return Err(OverlayError::new(
                "invalid GPU model preparation unexpectedly succeeded",
            ));
        }
        if overlay.model_generation != generation_before {
            return Err(OverlayError::new(
                "failed GPU model preparation replaced the active generation",
            ));
        }
        true
    } else {
        false
    };
    let mut frame_timing = FrameTimingCollector::new();
    let draw_started = Instant::now();
    overlay.draw(true)?;
    frame_timing.record_draw(draw_started.elapsed());
    overlay.set_visible(true)?;

    let input_producer = runtime.input_producer();
    let cursor_producer = runtime.cursor_producer();
    let gamepad_axis_producer = runtime.gamepad_axis_producer();
    let mut input_driver = PreviewInputDriver::default();
    let input_service = interactive
        .then(|| {
            MacInputService::start(
                input_producer.clone(),
                cursor_producer.clone(),
                gamepad_axis_producer.clone(),
            )
        })
        .transpose()
        .map_err(|error| OverlayError::new(error.to_string()))?;

    let started = Instant::now();
    let mut next_frame = started;
    let mut frames_presented = 1_u64;
    let mut dynamic_snapshots = 0_u64;
    let target_model_switches =
        switch_cycles.map(|cycles| u64::from(cycles).saturating_mul(PRESET_MODEL_IDS.len() as u64));
    let mut model_switch_commands = 0_u64;
    let mut model_switches = 0_u64;
    let mut current_model_index = if switch_cycles.is_some() {
        PRESET_MODEL_IDS
            .iter()
            .position(|id| *id == model_id.as_str())
            .ok_or_else(|| OverlayError::new("initial preset is not in the switch sequence"))?
    } else {
        0
    };
    let mut settle_frames = 0_u64;
    let mut metal_bytes_before = None;
    while overlay.panel.isVisible()
        && target_model_switches.map_or_else(
            || duration.is_zero() || started.elapsed() < duration,
            |target| model_switches < target || settle_frames < SWITCH_SETTLE_FRAMES,
        )
    {
        pump_application_events(&application);
        let elapsed = started.elapsed();
        if !interactive
            && let Some(sequence) = input_driver.update(
                model_id.as_str(),
                elapsed,
                &input_producer,
                &cursor_producer,
            )?
        {
            runtime_client
                .wait_for_input_sequence(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("preview input did not reach the runtime"))?;
        }
        if let (Some(target), Some(models)) = (target_model_switches, switch_models.as_ref())
            && frames_presented >= SWITCH_WARMUP_FRAMES
            && model_switch_commands == model_switches
            && model_switch_commands < target
        {
            metal_bytes_before.get_or_insert_with(|| overlay.current_allocated_size());
            current_model_index = (current_model_index + 1) % models.len();
            let sequence = runtime_client
                .send(RuntimeCommand::ActivateModel(Arc::clone(
                    &models[current_model_index],
                )))
                .map_err(|error| OverlayError::new(error.to_string()))?;
            let prepared = runtime_client
                .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("model switch was not prepared"))?;
            if let Some(failure) = prepared
                .last_command_failure
                .filter(|failure| failure.sequence == sequence)
            {
                return Err(OverlayError::new(format!(
                    "model switch failed: {:?}",
                    failure.code
                )));
            }
            model_switch_commands = model_switch_commands.saturating_add(1);
        }
        let mut gpu_model_switched = false;
        if let Some(frame) = render_consumer.take_latest() {
            if frame.snapshot.as_ref() != previous_snapshot.as_ref() {
                dynamic_snapshots = dynamic_snapshots.saturating_add(1);
            }
            let previous_generation = overlay.model_generation;
            gpu_model_switched = match overlay.sync_frame(&frame) {
                Ok(switched) => switched,
                Err(error) if frame.model_commit.is_some() => {
                    reject_model_commit(
                        &runtime_client,
                        &render_consumer,
                        frame.model_commit.expect("checked model commit token"),
                    )?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            if gpu_model_switched {
                overlay.resize_for_model(frame.snapshot.canvas)?;
                let token = frame
                    .model_commit
                    .ok_or_else(|| OverlayError::new("model switch frame has no commit token"))?;
                report_model_commit(
                    &runtime_client,
                    &render_consumer,
                    token,
                    ModelCommitOutcome::Prepared,
                )?;
                debug_assert_eq!(
                    frame.model_generation,
                    previous_generation.saturating_add(1)
                );
                model_switches = model_switches.saturating_add(1);
            }
            previous_snapshot = frame.snapshot;
        }
        let draw_started = Instant::now();
        overlay.draw(gpu_model_switched)?;
        frame_timing.record_draw(draw_started.elapsed());
        frames_presented += 1;
        if target_model_switches.is_some_and(|target| model_switches == target) {
            settle_frames = settle_frames.saturating_add(1);
        }
        next_frame += FRAME_INTERVAL;
        if let Some(delay) = next_frame.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
        } else {
            frame_timing.record_missed_deadline();
            next_frame = Instant::now();
        }
    }

    if let Some(target) = target_model_switches
        && (model_switch_commands != target || model_switches != target)
    {
        return Err(OverlayError::new(format!(
            "model-switch preview stopped after {model_switches}/{target} committed GPU switches"
        )));
    }
    let metal_bytes_before = metal_bytes_before.unwrap_or_else(|| overlay.current_allocated_size());
    let metal_bytes_after = overlay.current_allocated_size();
    if target_model_switches.is_some() && metal_bytes_after > metal_bytes_before {
        return Err(OverlayError::new(format!(
            "Metal allocation grew from {metal_bytes_before} to {metal_bytes_after} bytes during model switching"
        )));
    }

    let (platform_input_edges, platform_cursor_samples) = if let Some(input_service) = input_service
    {
        let diagnostics = input_service
            .stop()
            .map_err(|error| OverlayError::new(error.to_string()))?;
        (diagnostics.consumed_edges, diagnostics.cursor_consumed)
    } else {
        if let Some(sequence) = input_driver.release_all(started.elapsed(), &input_producer)? {
            runtime_client
                .wait_for_input_sequence(sequence, RUNTIME_TIMEOUT)
                .ok_or_else(|| OverlayError::new("preview releases did not reach the runtime"))?;
        }
        (0, 0)
    };
    let stopped = runtime
        .shutdown(RUNTIME_TIMEOUT)
        .map_err(|error| OverlayError::new(error.to_string()))?;
    while render_consumer.take_latest().is_some() {}
    let render_diagnostics = render_consumer.diagnostics();

    Ok(PreviewReport {
        frames_presented,
        dynamic_snapshots,
        runtime_input_events: stopped.input.transport.enqueued,
        platform_input_edges,
        runtime_cursor_published: stopped.cursor.transport.published,
        runtime_cursor_coalesced: stopped.cursor.transport.coalesced,
        runtime_cursor_consumed: stopped.cursor.transport.consumed,
        platform_cursor_samples,
        render_frames_published: render_diagnostics.published,
        render_frames_coalesced: render_diagnostics.coalesced,
        render_frames_consumed: render_diagnostics.consumed,
        model_switches,
        failed_gpu_prepare_preserved,
        gpu_bytes_before: metal_bytes_before,
        gpu_bytes_after: metal_bytes_after,
        drawable_count: overlay.model.meshes.len(),
        masked_drawable_count: overlay.model.masked_drawable_count,
        texture_count: overlay.model.textures.len(),
        warmup_thread_high_water: None,
        threads_after: None,
        frame_timing: Some(frame_timing.summary()),
    })
}

#[derive(Default)]
pub(crate) struct PreviewInputDriver {
    pub(crate) pressed: BTreeSet<InputControl>,
}

impl PreviewInputDriver {
    pub(crate) fn update(
        &mut self,
        model_id: &str,
        elapsed: Duration,
        producer: &InputProducer,
        cursor: &CursorProducer,
    ) -> Result<Option<u64>, OverlayError> {
        self.publish_cursor(elapsed, cursor)?;
        let step = (elapsed.as_millis() / 600) % 4;
        let mut desired = BTreeSet::new();
        match model_id {
            "standard" => {
                if step < 2 {
                    desired.insert(InputControl::Key(PhysicalKey::KEY_A));
                }
                if step == 0 {
                    desired.insert(InputControl::Mouse(MouseButton::Left));
                } else if step == 1 {
                    desired.insert(InputControl::Mouse(MouseButton::Right));
                }
            }
            "keyboard" | "gamepad" => {
                desired.insert(InputControl::Key(if step < 2 {
                    PhysicalKey::KEY_A
                } else {
                    RIGHT_ARROW
                }));
            }
            _ => {}
        }
        self.apply(desired, elapsed, producer)
    }

    pub(crate) fn release_all(
        &mut self,
        elapsed: Duration,
        producer: &InputProducer,
    ) -> Result<Option<u64>, OverlayError> {
        self.apply(BTreeSet::new(), elapsed, producer)
    }

    pub(crate) fn apply(
        &mut self,
        desired: BTreeSet<InputControl>,
        elapsed: Duration,
        producer: &InputProducer,
    ) -> Result<Option<u64>, OverlayError> {
        let at = MonotonicMillis::new(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
        let releases = self
            .pressed
            .difference(&desired)
            .copied()
            .collect::<Vec<_>>();
        let presses = desired
            .difference(&self.pressed)
            .copied()
            .collect::<Vec<_>>();
        let mut last_sequence = None;
        for (control, edge) in releases
            .into_iter()
            .map(|control| (control, InputEdge::Up))
            .chain(
                presses
                    .into_iter()
                    .map(|control| (control, InputEdge::Down)),
            )
        {
            last_sequence = Some(
                producer
                    .publish(InputEvent::Edge {
                        control,
                        edge,
                        source: InputSource::Capture,
                        at,
                    })
                    .map_err(|error| OverlayError::new(error.to_string()))?,
            );
        }
        self.pressed = desired;
        Ok(last_sequence)
    }

    pub(crate) fn publish_cursor(
        &self,
        elapsed: Duration,
        producer: &CursorProducer,
    ) -> Result<(), OverlayError> {
        let seconds = elapsed.as_secs_f64();
        let x = (seconds * std::f64::consts::TAU / 4.0).sin();
        let y = (seconds * std::f64::consts::TAU / 5.0).cos();
        let sample = CursorSample::new(
            CursorPosition {
                x: 1.0 - x,
                y: 1.0 - y,
            },
            CursorViewport {
                origin: CursorPosition { x: 0.0, y: 0.0 },
                width: 2.0,
                height: 2.0,
            },
            MonotonicMillis::new(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        )
        .map_err(|error| OverlayError::new(format!("invalid preview cursor sample: {error:?}")))?;
        producer
            .publish(sample)
            .map_err(|error| OverlayError::new(error.to_string()))
    }
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
