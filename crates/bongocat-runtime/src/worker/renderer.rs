//! Evaluating the renderer and turning a failure into a health transition.
//!
//! Health degrades once and recovers, rather than flapping per frame: a renderer
//! that failed on one frame is very likely to fail on the next, and a snapshot
//! that flips on every frame would make the settings page unreadable.

use super::super::transport::SnapshotCell;
use crate::rendering::{RenderEvaluation, RuntimeRenderer};
use crate::*;

pub(crate) fn evaluate_renderer(
    renderer: Option<&mut RuntimeRenderer>,
    input: ModelInputSnapshot,
    snapshot: &SnapshotCell,
    now: Duration,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    next_motion_event_sequence: &mut u64,
) {
    let Some(renderer) = renderer else {
        return;
    };
    let event_motion = active_motion.as_ref().map(|active| active.motion.clone());
    match renderer.evaluate(input, now) {
        Ok(RenderEvaluation {
            rendered: false, ..
        }) => {}
        Ok(evaluation) => {
            if !evaluation.motion_user_data.is_empty() || evaluation.skipped_motion_user_data > 0 {
                publish(snapshot, |current| {
                    current.motion_events.skipped = current
                        .motion_events
                        .skipped
                        .saturating_add(evaluation.skipped_motion_user_data);
                    if let Some(motion) = event_motion {
                        for occurrence in evaluation.motion_user_data {
                            let observed = MotionUserDataSnapshot {
                                event_sequence: *next_motion_event_sequence,
                                motion: motion.clone(),
                                cycle: occurrence.cycle,
                                local_time: occurrence.local_time,
                                value: occurrence.value,
                            };
                            *next_motion_event_sequence =
                                next_motion_event_sequence.wrapping_add(1);
                            current.motion_events.emitted =
                                current.motion_events.emitted.saturating_add(1);
                            current.motion_events.last_event = Some(observed);
                        }
                    } else {
                        current.motion_events.skipped = current
                            .motion_events
                            .skipped
                            .saturating_add(evaluation.motion_user_data.len() as u64);
                    }
                });
            }
            if evaluation.motion_finished {
                *active_motion = None;
                publish(snapshot, |current| current.active_motion = None);
            }
            update_renderer_health(snapshot, Ok(()));
        }
        Err(code) => update_renderer_health(snapshot, Err(code)),
    }
}

/// Publish a renderer failure once, then restore the ready state after its
/// first successful evaluation. Repeated identical failures must not create a
/// revision storm while a renderer is recovering.
pub(crate) fn update_renderer_health(
    snapshot: &SnapshotCell,
    evaluation: Result<(), RuntimeRenderErrorCode>,
) {
    let previous_error = snapshot
        .value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .render_error;

    match evaluation {
        Ok(()) if previous_error.is_some() => publish(snapshot, |current| {
            current.state = RuntimeState::Ready;
            current.render_error = None;
        }),
        Ok(()) => {}
        Err(code) if previous_error == Some(code) => {}
        Err(code) => publish(snapshot, |current| {
            current.state = RuntimeState::Degraded;
            current.render_error = Some(code);
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_motion(
    renderer: &mut Option<RuntimeRenderer>,
    motion: MotionId,
    priority: MotionPriority,
    looping: bool,
    sequence: u64,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    snapshot: &SnapshotCell,
    now: Duration,
) {
    let result = renderer
        .as_mut()
        .map_or(Err(RuntimeRenderErrorCode::MotionLoadFailed), |renderer| {
            renderer.start_motion(&motion, now, looping)
        });
    match result {
        Ok(()) => {
            let started = ActiveMotionSnapshot {
                motion,
                priority,
                command_sequence: sequence,
                stop_command_sequence: None,
            };
            *active_motion = Some(started.clone());
            publish(snapshot, |current| {
                current.active_motion = Some(started);
                current.last_command_failure = None;
                current.last_command_sequence = Some(sequence);
            });
        }
        Err(code) => publish(snapshot, |current| {
            current.last_command_failure = Some(RuntimeCommandFailure { sequence, code });
            current.last_command_sequence = Some(sequence);
        }),
    }
}
