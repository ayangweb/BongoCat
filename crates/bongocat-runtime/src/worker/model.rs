//! Preparing and committing a model, and the feedback that says which of the two
//! happened.
//!
//! A preparation is deferred to the first frame the overlay frame source actually
//! draws, so the runtime keeps at most one unresolved activation. That is why
//! `begin_model_activation` and `process_model_commit_feedback` are a pair rather
//! than two independent steps.

use super::super::transport::{SnapshotCell, sequence_reached};
use super::GamepadAxisValues;
use super::audio::{
    activate_model_audio, model_audio_paths, prepare_model_audio, stop_motion_audio,
};
use super::input::compose_model_input;
use super::renderer::evaluate_renderer;
use crate::*;

pub(crate) struct PendingModelActivation {
    pub(crate) token: ModelCommitToken,
    pub(crate) model: Arc<CommittedModel>,
    pub(crate) input_bindings: Option<Arc<InputBindings>>,
    pub(crate) renderer_prepared: bool,
    pub(crate) audio_prepare_sequence: Option<u64>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_model_activation(
    sequence: u64,
    committed: Arc<CommittedModel>,
    proposed_bindings: Option<Arc<InputBindings>>,
    renderer: Option<&mut RuntimeRenderer>,
    input_state: &InputState,
    input_bindings: &mut InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
    model_settings: ModelSettings,
    active_model: &mut Option<Arc<CommittedModel>>,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    pending_model: &mut Option<PendingModelActivation>,
    motion_audio: &MotionAudioClient,
    motion_audio_enabled: bool,
    snapshot: &SnapshotCell,
) {
    let activation_bindings = proposed_bindings.as_deref().unwrap_or(input_bindings);
    let model_input = compose_model_input(
        input_state,
        activation_bindings,
        normalized_cursor,
        gamepad_axis_values,
        gamepad_axis_settings,
        model_settings,
    );
    let Some(renderer) = renderer else {
        if let Some(bindings) = proposed_bindings {
            *input_bindings = Arc::unwrap_or_clone(bindings);
        }
        let model_snapshot = committed.snapshot();
        let model_origin = committed.origin();
        *active_model = Some(committed);
        *active_motion = None;
        *active_expression = None;
        stop_motion_audio(motion_audio, MotionAudioStopReason::ModelSwitched);
        publish(snapshot, |current| {
            current.state = RuntimeState::Ready;
            current.active_model = Some(model_snapshot);
            current.active_model_origin = Some(model_origin);
            current.active_motion = None;
            current.active_expression = None;
            current.motion_events.last_event = None;
            current.model_input = model_input;
            current.render_error = None;
            current.last_command_failure = None;
            current.last_command_sequence = Some(sequence);
        });
        return;
    };
    match renderer.prepare(sequence, &committed, model_input) {
        Ok(token) => {
            let model_snapshot = committed.snapshot();
            let audio_prepare_sequence = motion_audio_enabled
                .then(|| prepare_model_audio(motion_audio, &committed))
                .flatten();
            *pending_model = Some(PendingModelActivation {
                token,
                model: committed,
                input_bindings: proposed_bindings,
                renderer_prepared: false,
                audio_prepare_sequence,
            });
            publish(snapshot, |current| {
                current.pending_model = Some(PendingModelSnapshot {
                    token,
                    model: model_snapshot,
                });
                current.last_command_failure = None;
            });
        }
        Err(code) => publish(snapshot, |current| {
            current.last_command_failure = Some(RuntimeCommandFailure { sequence, code });
            current.last_command_sequence = Some(sequence);
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn process_model_commit_feedback(
    renderer: Option<&mut RuntimeRenderer>,
    pending_model: &mut Option<PendingModelActivation>,
    input_state: &InputState,
    input_bindings: &mut InputBindings,
    normalized_cursor: NormalizedCursorPosition,
    gamepad_axis_values: &GamepadAxisValues,
    gamepad_axis_settings: GamepadAxisSettings,
    model_settings: ModelSettings,
    active_model: &mut Option<Arc<CommittedModel>>,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    motion_audio: &MotionAudioClient,
    next_motion_event_sequence: &mut u64,
    random_behavior_scheduler: &mut RandomBehaviorScheduler,
    overlay_visible: bool,
    snapshot: &SnapshotCell,
    now: Duration,
) {
    let Some(renderer) = renderer else {
        return;
    };
    let Some(pending) = pending_model.as_mut() else {
        if renderer.take_model_commit_feedback().is_some() {
            renderer.record_stale_model_commit_feedback();
        }
        return;
    };
    if !pending.renderer_prepared {
        let Some(feedback) = renderer.take_model_commit_feedback() else {
            return;
        };
        if pending.token != feedback.token {
            renderer.record_stale_model_commit_feedback();
            return;
        }
        match feedback.outcome {
            ModelCommitOutcome::Prepared => pending.renderer_prepared = true,
            ModelCommitOutcome::Rejected(ModelCommitErrorCode::ResourcePreparationFailed)
                if renderer.reject(feedback.token) =>
            {
                let pending = pending_model.take().expect("checked pending model");
                let model_input = compose_model_input(
                    input_state,
                    input_bindings,
                    normalized_cursor,
                    gamepad_axis_values,
                    gamepad_axis_settings,
                    model_settings,
                );
                publish(snapshot, |current| {
                    current.pending_model = None;
                    current.model_input = model_input;
                    current.last_command_failure = Some(RuntimeCommandFailure {
                        sequence: feedback.token.command_sequence,
                        code: RuntimeRenderErrorCode::GpuPreparationFailed,
                    });
                    current.last_command_sequence = Some(feedback.token.command_sequence);
                });
                drop(pending);
                if overlay_visible {
                    evaluate_renderer(
                        Some(renderer),
                        model_input,
                        snapshot,
                        now,
                        active_motion,
                        next_motion_event_sequence,
                    );
                }
                return;
            }
            _ => {
                renderer.record_stale_model_commit_feedback();
                return;
            }
        }
    }
    if pending.audio_prepare_sequence.is_some_and(|sequence| {
        !motion_audio
            .diagnostics()
            .last_processed_sequence
            .is_some_and(|processed| sequence_reached(processed, sequence))
    }) {
        return;
    }
    let pending = pending_model.take().expect("checked pending model");
    if renderer.commit(pending.token) {
        let command_sequence = pending.token.command_sequence;
        let audio_paths = model_audio_paths(&pending.model);
        if let Some(bindings) = pending.input_bindings {
            *input_bindings = Arc::unwrap_or_clone(bindings);
        }
        let model_input = compose_model_input(
            input_state,
            input_bindings,
            normalized_cursor,
            gamepad_axis_values,
            gamepad_axis_settings,
            model_settings,
        );
        let model_snapshot = pending.model.snapshot();
        let model_origin = pending.model.origin();
        activate_model_audio(motion_audio, audio_paths);
        *active_model = Some(pending.model);
        *active_motion = None;
        *active_expression = None;
        random_behavior_scheduler.reset(now);
        stop_motion_audio(motion_audio, MotionAudioStopReason::ModelSwitched);
        publish(snapshot, |current| {
            current.state = RuntimeState::Ready;
            current.active_model = Some(model_snapshot);
            current.active_model_origin = Some(model_origin);
            current.pending_model = None;
            current.active_motion = None;
            current.active_expression = None;
            current.motion_events.last_event = None;
            current.model_input = model_input;
            current.render_error = None;
            current.last_command_failure = None;
            current.last_command_sequence = Some(command_sequence);
        });
        if overlay_visible {
            evaluate_renderer(
                Some(renderer),
                model_input,
                snapshot,
                now,
                active_motion,
                next_motion_event_sequence,
            );
        }
    }
}
