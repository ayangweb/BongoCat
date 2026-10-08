//! The worker thread: the only place runtime state is mutated.
//!
//! One receive, one command, one published revision. The loop stays in one
//! function on purpose — the ordering between draining reliable input, expiring
//! the keyboard fallback, reconciling pressed state and evaluating the renderer is
//! the behaviour, and it is only checkable when those steps stay adjacent. The
//! steps themselves live in this directory's submodules.

use super::transport::{
    CommandEnvelope, CommandSequenceDisposition, CommandSequenceTracker, CommandTransportCounters,
    SnapshotCell, WorkerCommand, publish,
};
use super::*;
use crate::owner::ShutdownSignal;
use crate::pacing::{record_work_budget, runtime_frame_interval, runtime_tick_work_budget};

pub(crate) mod audio;
pub(crate) mod axes;
pub(crate) mod input;
pub(crate) mod model;
pub(crate) mod random_behavior;
pub(crate) mod renderer;

use audio::{motion_audio_path, prepare_model_audio, stop_motion_audio};
use axes::GamepadAxisValues;
use input::{compose_model_input, consume_cursor, consume_gamepad_axes};
use model::{begin_model_activation, process_model_commit_feedback};
use random_behavior::maybe_trigger_random_behavior;
use renderer::{evaluate_renderer, start_motion};

pub(crate) struct RuntimeWorkerBootstrap {
    pub(crate) snapshot: Arc<SnapshotCell>,
    pub(crate) cursor_producer: CursorProducer,
    pub(crate) gamepad_axis_producer: GamepadAxisProducer,
    pub(crate) initial_overlay_visible: bool,
    pub(crate) initial_motion_audio_enabled: bool,
    pub(crate) renderer: Option<RuntimeRenderBootstrap>,
    pub(crate) motion_audio: MotionAudioClient,
    pub(crate) clock: Arc<dyn MonotonicClock>,
    pub(crate) command_transport: Arc<CommandTransportCounters>,
    pub(crate) shutdown: Arc<ShutdownSignal>,
    pub(crate) panic_after_stopped: bool,
    pub(crate) shutdown_delay: Duration,
}

pub(crate) fn run_worker(receiver: Receiver<CommandEnvelope>, bootstrap: RuntimeWorkerBootstrap) {
    let RuntimeWorkerBootstrap {
        snapshot,
        cursor_producer,
        gamepad_axis_producer,
        initial_overlay_visible,
        initial_motion_audio_enabled,
        renderer,
        motion_audio,
        clock,
        command_transport,
        shutdown,
        panic_after_stopped,
        shutdown_delay,
    } = bootstrap;
    let mut renderer = renderer.map(RuntimeRenderer::start);
    let mut active_model = None;
    let mut active_motions = Vec::new();
    let mut active_expression = None;
    let mut input_state = InputState::default();
    let mut input_bindings = InputBindings::default();
    let mut cursor_smoother = CursorSmoother::default();
    let mut normalized_cursor = NormalizedCursorPosition::default();
    let mut gamepad_axis_values = GamepadAxisValues::default();
    let mut gamepad_axis_settings = GamepadAxisSettings::default();
    let mut overlay_visible = initial_overlay_visible;
    let mut maximum_fps = DEFAULT_MAXIMUM_FPS;
    let mut model_settings = ModelSettings::default();
    let mut random_behavior_scheduler = RandomBehaviorScheduler::new(system_seed());
    let mut next_automatic_event_sequence = AUTOMATIC_SEQUENCE_START;
    let mut motion_audio_enabled = initial_motion_audio_enabled;
    let mut pending_model = None;
    let mut deferred_commands = VecDeque::new();
    let mut next_motion_event_sequence = 0u64;
    let mut command_sequences = CommandSequenceTracker::default();
    let mut frame_pacer = FramePacer::new(
        Instant::now(),
        runtime_frame_interval(maximum_fps, overlay_visible),
    );
    publish(&snapshot, |current| current.state = RuntimeState::Ready);
    loop {
        process_model_commit_feedback(
            renderer.as_mut(),
            &mut pending_model,
            &input_state,
            &mut input_bindings,
            normalized_cursor,
            &gamepad_axis_values,
            gamepad_axis_settings,
            model_settings,
            &mut active_model,
            &mut active_motions,
            &mut active_expression,
            &motion_audio,
            &mut next_motion_event_sequence,
            &mut random_behavior_scheduler,
            overlay_visible,
            &snapshot,
            clock.now(),
        );
        if pending_model.is_none() {
            maybe_trigger_random_behavior(
                renderer.as_mut(),
                active_model.as_deref(),
                &mut active_motions,
                &mut active_expression,
                &mut random_behavior_scheduler,
                &mut next_automatic_event_sequence,
                &motion_audio,
                motion_audio_enabled,
                &snapshot,
                clock.now(),
                &shutdown,
            );
        }
        let frame_interval = runtime_frame_interval(maximum_fps, overlay_visible);
        let received = if let Some(sequence) = shutdown.sequence() {
            if pending_model.is_some() {
                // A model commit may be waiting for an overlay acknowledgement. Do not
                // requeue deferred work forever during shutdown; the active model remains
                // valid and the shutdown command must be allowed to release its resources.
                deferred_commands.clear();
                Ok(CommandEnvelope {
                    sequence,
                    command: WorkerCommand::Shutdown,
                })
            } else if let Some(envelope) = deferred_commands.pop_front() {
                Ok(envelope)
            } else {
                match receiver.try_recv() {
                    Ok(envelope) => Ok(envelope),
                    Err(TryRecvError::Empty) => Ok(CommandEnvelope {
                        sequence,
                        command: WorkerCommand::Shutdown,
                    }),
                    Err(TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
                }
            }
        } else if pending_model.is_none() {
            // The wait is measured against the frame deadline rather than started
            // once the previous frame finished, so evaluation keeps the configured
            // `maximum_fps` instead of drifting one frame cost lower every frame.
            deferred_commands.pop_front().map_or_else(
                || receiver.recv_timeout(frame_pacer.wait(Instant::now(), frame_interval)),
                Ok,
            )
        } else {
            receiver.recv_timeout(frame_pacer.wait(Instant::now(), frame_interval))
        };
        // Wall-clock measurement is diagnostics only; product state continues to use
        // the injected monotonic clock above and inside the command handlers.
        let work_started = Instant::now();
        match received {
            Ok(envelope) => {
                consume_cursor(
                    &cursor_producer,
                    &snapshot,
                    &input_state,
                    &input_bindings,
                    &gamepad_axis_values,
                    gamepad_axis_settings,
                    &mut cursor_smoother,
                    &mut normalized_cursor,
                    model_settings,
                    clock.now(),
                );
                consume_gamepad_axes(
                    &gamepad_axis_producer,
                    &snapshot,
                    &mut gamepad_axis_values,
                    &input_state,
                    &input_bindings,
                    normalized_cursor,
                    gamepad_axis_settings,
                    model_settings,
                );
                if pending_model.is_some()
                    && !matches!(envelope.command, WorkerCommand::Shutdown)
                    && !matches!(
                        envelope.command,
                        WorkerCommand::Product(RuntimeCommand::ApplyInput(_))
                    )
                {
                    let sequence = envelope.sequence;
                    deferred_commands.push_back(envelope);
                    command_sequences.defer(sequence);
                    continue;
                }
                match command_sequences.observe(envelope.sequence) {
                    CommandSequenceDisposition::Gap { missing } => {
                        command_transport.sequence_gap(missing);
                    }
                    CommandSequenceDisposition::Duplicate => {
                        command_transport.duplicate_sequence();
                        continue;
                    }
                    CommandSequenceDisposition::OutOfOrder => {
                        command_transport.out_of_order_sequence();
                        continue;
                    }
                    CommandSequenceDisposition::First
                    | CommandSequenceDisposition::InOrder
                    | CommandSequenceDisposition::Deferred => {}
                }
                let sequence = envelope.sequence;
                let mut evaluate_after_command = true;
                match envelope.command {
                    WorkerCommand::Product(RuntimeCommand::Tick) => {
                        publish(&snapshot, |current| {
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetOverlayVisible(visible)) => {
                        overlay_visible = visible;
                        publish(&snapshot, |current| {
                            current.overlay_visible = visible;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetOverlaySettings(settings)) => {
                        if !settings.is_valid() {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::OverlaySettingsInvalid,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        } else {
                            publish(&snapshot, |current| {
                                current.overlay_settings = settings;
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::SetMaximumFps(value)) => {
                        if !maximum_fps_is_valid(value) {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::MaximumFpsInvalid,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        } else {
                            maximum_fps = value;
                            publish(&snapshot, |current| {
                                current.maximum_fps = value;
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::SetRandomBehaviorSettings(settings)) => {
                        if !settings.is_valid() {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::RandomBehaviorSettingsInvalid,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        } else {
                            random_behavior_scheduler.set_settings(settings, clock.now());
                            publish(&snapshot, |current| {
                                current.random_behavior_settings = settings;
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::SetModelSettings(settings)) => {
                        if !settings.allow_motion_overlap
                            && let Some(latest) = active_motions.pop()
                        {
                            active_motions.clear();
                            active_motions.push(latest);
                        }
                        model_settings = settings;
                        if let Some(renderer) = &mut renderer {
                            renderer.set_model_settings(settings);
                        }
                        publish(&snapshot, |current| {
                            current.model_settings = settings;
                            current.active_motions = active_motions.clone();
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                                model_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetMotionAudioEnabled(enabled)) => {
                        let enabling_audio = !motion_audio_enabled && enabled;
                        let disabling_audio = motion_audio_enabled && !enabled;
                        motion_audio_enabled = enabled;
                        if enabling_audio {
                            // A muted activation deliberately skips `Prepare`. Queue it
                            // before publishing the enabled snapshot so any later motion's
                            // `Play` follows the cache preparation in the audio FIFO.
                            if let Some(model) = active_model.as_deref() {
                                let _ = prepare_model_audio(&motion_audio, model);
                            }
                        } else if disabling_audio {
                            stop_motion_audio(&motion_audio, MotionAudioStopReason::Disabled);
                        }
                        publish(&snapshot, |current| {
                            current.motion_audio_enabled = enabled;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetInputBindings(bindings)) => {
                        input_bindings = Arc::unwrap_or_clone(bindings);
                        publish(&snapshot, |current| {
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                                model_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetGamepadAxisSettings(settings)) => {
                        gamepad_axis_settings = settings;
                        publish(&snapshot, |current| {
                            current.gamepad_axis_settings = settings;
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                                model_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetCursorSettings(settings)) => {
                        // Nothing in the worker consumes this: the pointer is
                        // captured on the platform thread, and the overlay
                        // session reads the value back out of the snapshot and
                        // pushes it down. Publishing it here is what makes the
                        // runtime the single source of truth for the setting
                        // rather than a second copy the overlay would have to
                        // fetch from the configuration.
                        publish(&snapshot, |current| {
                            current.cursor_settings = settings;
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::ResetInput(reason)) => {
                        input_state.force_reset(reason);
                        gamepad_axis_values.clear();
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                                model_settings,
                            );
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        })
                    }
                    WorkerCommand::Product(RuntimeCommand::ApplyInput(envelope)) => {
                        let envelope = Arc::unwrap_or_clone(envelope);
                        let input_reset = matches!(envelope.event, InputEvent::Reset { .. });
                        let connected = match &envelope.event {
                            InputEvent::GamepadConnected { connection, at } => {
                                Some((*connection, *at))
                            }
                            _ => None,
                        };
                        let disconnected = match &envelope.event {
                            InputEvent::GamepadDisconnected { connection, .. } => Some(*connection),
                            _ => None,
                        };
                        let disposition = input_state.apply(envelope);
                        if input_reset {
                            gamepad_axis_values.clear();
                        } else if let Some(connection) = disconnected {
                            gamepad_axis_values.clear_connection(connection);
                        }
                        if let Some((connection, at)) = connected
                            && matches!(
                                disposition,
                                InputDisposition::Applied
                                    | InputDisposition::AppliedAfterSequenceGap { .. }
                            )
                        {
                            gamepad_axis_values.activate_connection(connection, at);
                        }
                        let activation_pending = pending_model.is_some();
                        publish(&snapshot, |current| {
                            current.input = input_state.snapshot();
                            current.model_input = compose_model_input(
                                &input_state,
                                &input_bindings,
                                normalized_cursor,
                                &gamepad_axis_values,
                                gamepad_axis_settings,
                                model_settings,
                            );
                            current.last_command_failure = None;
                            if !activation_pending {
                                current.last_command_sequence = Some(sequence);
                            }
                        });
                        evaluate_after_command = !activation_pending;
                    }
                    WorkerCommand::Product(RuntimeCommand::ActivateModel(committed)) => {
                        evaluate_after_command = false;
                        begin_model_activation(
                            sequence,
                            committed,
                            None,
                            renderer.as_mut(),
                            &input_state,
                            &mut input_bindings,
                            normalized_cursor,
                            &gamepad_axis_values,
                            gamepad_axis_settings,
                            model_settings,
                            &mut active_model,
                            &mut active_motions,
                            &mut active_expression,
                            &mut pending_model,
                            &motion_audio,
                            motion_audio_enabled,
                            &snapshot,
                        );
                    }
                    WorkerCommand::Product(RuntimeCommand::ActivateModelWithBindings {
                        model,
                        input_bindings: proposed_bindings,
                    }) => {
                        evaluate_after_command = false;
                        begin_model_activation(
                            sequence,
                            model,
                            Some(proposed_bindings),
                            renderer.as_mut(),
                            &input_state,
                            &mut input_bindings,
                            normalized_cursor,
                            &gamepad_axis_values,
                            gamepad_axis_settings,
                            model_settings,
                            &mut active_model,
                            &mut active_motions,
                            &mut active_expression,
                            &mut pending_model,
                            &motion_audio,
                            motion_audio_enabled,
                            &snapshot,
                        );
                    }
                    WorkerCommand::Product(
                        command @ (RuntimeCommand::StartMotion { .. }
                        | RuntimeCommand::PreviewMotion(_)),
                    ) => {
                        // Both trigger sources play a single cycle. A shortcut
                        // press must produce one visible run and then hold the
                        // clip's final evaluated pose; the clip's own
                        // `Meta.Loop` describes how the asset was authored, not
                        // how the product drives it, so honoring it here would
                        // keep the cat animating forever.
                        let repeat_is_idempotent =
                            matches!(command, RuntimeCommand::StartMotion { .. });
                        let (motion, priority, looping) = match command {
                            RuntimeCommand::StartMotion { motion, priority } => {
                                (motion, priority, false)
                            }
                            RuntimeCommand::PreviewMotion(motion) => {
                                (motion, MotionPriority::Force, false)
                            }
                            _ => unreachable!("matched only motion start commands"),
                        };
                        // A repeat of the run that is already in flight is a
                        // no-op for a product trigger: the equal-priority rule
                        // of the R5 motion queue ignores the request while the
                        // current motion is unfinished, so key repeat or a
                        // press burst neither restarts the clip nor replays its
                        // audio. Once the one-shot run has completed, its final
                        // pose remains visible but no longer reserves priority;
                        // the next request may replace it. A preview stays a
                        // direct UI action and restarts on every request.
                        let now = clock.now();
                        let matching = active_motions.iter().find(|active| active.motion == motion);
                        let duplicate = repeat_is_idempotent
                            && matching.is_some_and(|active| {
                                active.priority == priority
                                    && active.stop_command_sequence.is_none()
                                    && renderer.as_ref().is_some_and(|renderer| {
                                        !renderer.motion_is_settled(&active.motion, now)
                                    })
                            });
                        let candidate = if model_settings.allow_motion_overlap {
                            matching
                        } else {
                            active_motions.last()
                        };
                        let current_priority = candidate
                            .filter(|active| {
                                renderer.as_ref().is_some_and(|renderer| {
                                    !renderer.motion_is_settled(&active.motion, now)
                                })
                            })
                            .map(|active| active.priority);
                        let can_replace =
                            current_priority.is_none_or(|current| priority >= current);
                        if duplicate || !can_replace {
                            publish(&snapshot, |current| {
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        } else {
                            let motion_is_valid = renderer.as_ref().map_or(
                                Err(RuntimeRenderErrorCode::MotionLoadFailed),
                                |renderer| renderer.validate_motion(&motion),
                            );
                            match motion_is_valid {
                                Err(code) => publish(&snapshot, |current| {
                                    current.last_command_failure =
                                        Some(RuntimeCommandFailure { sequence, code });
                                    current.last_command_sequence = Some(sequence);
                                }),
                                Ok(()) if motion_audio_enabled => {
                                    if let Some(path) =
                                        motion_audio_path(active_model.as_deref(), &motion)
                                    {
                                        let _ =
                                            motion_audio.try_publish_with_sequence(|sequence| {
                                                MotionAudioCommand::Play {
                                                    sequence,
                                                    path,
                                                    volume: MotionAudioVolume::FULL,
                                                }
                                            });
                                        start_motion(
                                            &mut renderer,
                                            motion,
                                            priority,
                                            looping,
                                            sequence,
                                            &mut active_motions,
                                            &snapshot,
                                            now,
                                        );
                                    } else {
                                        stop_motion_audio(
                                            &motion_audio,
                                            MotionAudioStopReason::MotionReplaced,
                                        );
                                        start_motion(
                                            &mut renderer,
                                            motion,
                                            priority,
                                            looping,
                                            sequence,
                                            &mut active_motions,
                                            &snapshot,
                                            now,
                                        );
                                    }
                                }
                                Ok(()) => {
                                    start_motion(
                                        &mut renderer,
                                        motion,
                                        priority,
                                        looping,
                                        sequence,
                                        &mut active_motions,
                                        &snapshot,
                                        now,
                                    );
                                }
                            }
                        }
                    }
                    WorkerCommand::Product(RuntimeCommand::StopMotion(motion)) => {
                        let matching = active_motions
                            .iter()
                            .position(|active| active.motion == motion);
                        if let Some(index) = matching
                            && active_motions[index].stop_command_sequence.is_none()
                        {
                            stop_motion_audio(&motion_audio, MotionAudioStopReason::MotionStopped);
                            let stop_status = renderer
                                .as_mut()
                                .map_or(MotionStopStatus::Finished, |renderer| {
                                    renderer.stop_motion(&motion, clock.now())
                                });
                            if stop_status == MotionStopStatus::Fading {
                                active_motions[index].stop_command_sequence = Some(sequence);
                            } else {
                                active_motions.remove(index);
                            }
                        }
                        publish(&snapshot, |current| {
                            current.active_motions = active_motions.clone();
                            current.last_command_failure = None;
                            current.last_command_sequence = Some(sequence);
                        });
                    }
                    WorkerCommand::Product(RuntimeCommand::SetExpression(expression)) => {
                        // Both trigger sources land here — the settings window's
                        // preview button and a shortcut — so this is the one
                        // place that can tell a repeat of the expression already
                        // in effect from a first request for it. Turning that
                        // repeat off is a configured behaviour rather than a
                        // property of the model, and the model that is showing an
                        // expression is the model the request is about: model
                        // activation clears `active_expression` before any
                        // deferred command runs, so a remembered expression
                        // restored onto a fresh model can never read as a repeat.
                        let repeats_the_active_expression = model_settings
                            .toggle_repeated_expression
                            && active_expression
                                .as_ref()
                                .is_some_and(|active| active.expression == expression);
                        if repeats_the_active_expression {
                            if let Some(renderer) = &mut renderer {
                                renderer.clear_expression(clock.now());
                            }
                            // The user is undoing a choice, so this deliberately
                            // writes no `user_expression_memory`: remembering an
                            // expression that is no longer on screen would
                            // restore the face the user just turned off.
                            active_expression = None;
                            publish(&snapshot, |current| {
                                current.active_expression = None;
                                current.last_command_failure = None;
                                current.last_command_sequence = Some(sequence);
                            });
                        } else if let Some(renderer) = &mut renderer {
                            match renderer.set_expression(&expression, clock.now()) {
                                Ok(()) => {
                                    let active = ActiveExpressionSnapshot {
                                        expression: expression.clone(),
                                        command_sequence: sequence,
                                    };
                                    active_expression = Some(active.clone());
                                    // A command is the user asking for a face, so
                                    // this is the one path that records the choice
                                    // for that model. The idle scheduler plays
                                    // expressions through the renderer directly and
                                    // never lands here, which is what keeps an
                                    // automatic pick out of the remembered set.
                                    let memory =
                                        active_model.as_ref().map(|model| UserExpressionMemory {
                                            model: model.id().clone(),
                                            model_origin: model.origin(),
                                            expression,
                                            command_sequence: sequence,
                                        });
                                    publish(&snapshot, |current| {
                                        current.active_expression = Some(active);
                                        if let Some(memory) = memory {
                                            current.user_expression_memory = Some(memory);
                                        }
                                        current.last_command_failure = None;
                                        current.last_command_sequence = Some(sequence);
                                    });
                                }
                                Err(code) => publish(&snapshot, |current| {
                                    current.last_command_failure =
                                        Some(RuntimeCommandFailure { sequence, code });
                                    current.last_command_sequence = Some(sequence);
                                }),
                            }
                        } else {
                            publish(&snapshot, |current| {
                                current.last_command_failure = Some(RuntimeCommandFailure {
                                    sequence,
                                    code: RuntimeRenderErrorCode::ExpressionLoadFailed,
                                });
                                current.last_command_sequence = Some(sequence);
                            });
                        }
                    }
                    WorkerCommand::Shutdown => {
                        if !shutdown_delay.is_zero() {
                            thread::sleep(shutdown_delay);
                        }
                        stop_motion_audio(&motion_audio, MotionAudioStopReason::Shutdown);
                        publish(&snapshot, |current| {
                            current.state = RuntimeState::Stopping;
                            current.pending_model = None;
                            current.active_motions.clear();
                            current.active_expression = None;
                            current.last_command_sequence = Some(sequence);
                        });
                        if let Some(renderer) = &renderer {
                            renderer.close();
                        }
                        publish(&snapshot, |current| current.state = RuntimeState::Stopped);
                        if panic_after_stopped {
                            panic!("runtime worker panic injection");
                        }
                        drop(active_model);
                        return;
                    }
                }
                if evaluate_after_command && overlay_visible && pending_model.is_none() {
                    evaluate_renderer(
                        renderer.as_mut(),
                        compose_model_input(
                            &input_state,
                            &input_bindings,
                            normalized_cursor,
                            &gamepad_axis_values,
                            gamepad_axis_settings,
                            model_settings,
                        ),
                        &snapshot,
                        clock.now(),
                        &mut active_motions,
                        &mut next_motion_event_sequence,
                    );
                    // A command may produce a frame before its slot is due, which
                    // keeps input latency below one interval. Such a frame leaves
                    // the slot pending, so the periodic cadence is unaffected.
                    frame_pacer.frame_produced(Instant::now(), frame_interval);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                consume_cursor(
                    &cursor_producer,
                    &snapshot,
                    &input_state,
                    &input_bindings,
                    &gamepad_axis_values,
                    gamepad_axis_settings,
                    &mut cursor_smoother,
                    &mut normalized_cursor,
                    model_settings,
                    clock.now(),
                );
                consume_gamepad_axes(
                    &gamepad_axis_producer,
                    &snapshot,
                    &mut gamepad_axis_values,
                    &input_state,
                    &input_bindings,
                    normalized_cursor,
                    gamepad_axis_settings,
                    model_settings,
                );
                process_model_commit_feedback(
                    renderer.as_mut(),
                    &mut pending_model,
                    &input_state,
                    &mut input_bindings,
                    normalized_cursor,
                    &gamepad_axis_values,
                    gamepad_axis_settings,
                    model_settings,
                    &mut active_model,
                    &mut active_motions,
                    &mut active_expression,
                    &motion_audio,
                    &mut next_motion_event_sequence,
                    &mut random_behavior_scheduler,
                    overlay_visible,
                    &snapshot,
                    clock.now(),
                );
                if pending_model.is_none() {
                    maybe_trigger_random_behavior(
                        renderer.as_mut(),
                        active_model.as_deref(),
                        &mut active_motions,
                        &mut active_expression,
                        &mut random_behavior_scheduler,
                        &mut next_automatic_event_sequence,
                        &motion_audio,
                        motion_audio_enabled,
                        &snapshot,
                        clock.now(),
                        &shutdown,
                    );
                }
                if overlay_visible && pending_model.is_none() {
                    evaluate_renderer(
                        renderer.as_mut(),
                        compose_model_input(
                            &input_state,
                            &input_bindings,
                            normalized_cursor,
                            &gamepad_axis_values,
                            gamepad_axis_settings,
                            model_settings,
                        ),
                        &snapshot,
                        clock.now(),
                        &mut active_motions,
                        &mut next_motion_event_sequence,
                    );
                }
                // The slot elapsed even when a hidden overlay produced no frame,
                // so the grid advances here unconditionally: leaving the deadline
                // in the past would turn the throttle into a busy loop.
                frame_pacer.frame_produced(Instant::now(), frame_interval);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let elapsed = work_started.elapsed();
        let budget = runtime_tick_work_budget(maximum_fps);
        let mut work_diagnostics = RuntimeWorkDiagnostics::default();
        record_work_budget(&mut work_diagnostics, elapsed, budget);
        if work_diagnostics.budget_exceeded > 0 {
            publish(&snapshot, |current| {
                current.work.budget_exceeded = current
                    .work
                    .budget_exceeded
                    .saturating_add(work_diagnostics.budget_exceeded);
                current.work.last_over_budget_ms = work_diagnostics.last_over_budget_ms;
            });
        }
    }
    consume_cursor(
        &cursor_producer,
        &snapshot,
        &input_state,
        &input_bindings,
        &gamepad_axis_values,
        gamepad_axis_settings,
        &mut cursor_smoother,
        &mut normalized_cursor,
        model_settings,
        clock.now(),
    );
    consume_gamepad_axes(
        &gamepad_axis_producer,
        &snapshot,
        &mut gamepad_axis_values,
        &input_state,
        &input_bindings,
        normalized_cursor,
        gamepad_axis_settings,
        model_settings,
    );
    gamepad_axis_values.clear();
    gamepad_axis_producer.stop();
    if let Some(renderer) = &renderer {
        renderer.close();
    }
    stop_motion_audio(&motion_audio, MotionAudioStopReason::Shutdown);
    publish(&snapshot, |current| current.state = RuntimeState::Stopped);
    if panic_after_stopped {
        panic!("runtime worker panic injection");
    }
}
