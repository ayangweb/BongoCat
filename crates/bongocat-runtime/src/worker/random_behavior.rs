//! The optional idle behavior, which starts a model behavior on its own interval.
//!
//! It never replaces something the user or a shortcut started: an automatic
//! action is only taken when nothing else owns the motion, so an idle product
//! cannot cut off a press.

use super::super::transport::SnapshotCell;
use super::audio::{motion_audio_path, stop_motion_audio};
use crate::owner::ShutdownSignal;
use crate::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn maybe_trigger_random_behavior(
    renderer: Option<&mut RuntimeRenderer>,
    active_model: Option<&CommittedModel>,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    scheduler: &mut RandomBehaviorScheduler,
    next_automatic_event_sequence: &mut u64,
    motion_audio: &MotionAudioClient,
    motion_audio_enabled: bool,
    snapshot: &SnapshotCell,
    now: Duration,
    shutdown: &ShutdownSignal,
) {
    shutdown.run_if_not_shutdown(|| {
        maybe_trigger_random_behavior_locked(
            renderer,
            active_model,
            active_motion,
            active_expression,
            scheduler,
            next_automatic_event_sequence,
            motion_audio,
            motion_audio_enabled,
            snapshot,
            now,
        );
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn maybe_trigger_random_behavior_locked(
    renderer: Option<&mut RuntimeRenderer>,
    active_model: Option<&CommittedModel>,
    active_motion: &mut Option<ActiveMotionSnapshot>,
    active_expression: &mut Option<ActiveExpressionSnapshot>,
    scheduler: &mut RandomBehaviorScheduler,
    next_automatic_event_sequence: &mut u64,
    motion_audio: &MotionAudioClient,
    motion_audio_enabled: bool,
    snapshot: &SnapshotCell,
    now: Duration,
) {
    let Some(renderer) = renderer else {
        return;
    };
    let Some(model) = active_model else {
        return;
    };
    let Some(behavior) = scheduler.poll(now, || model.snapshot().behaviors) else {
        return;
    };
    let automatic_sequence = *next_automatic_event_sequence;
    *next_automatic_event_sequence = next_automatic_event_sequence.wrapping_sub(1);
    match behavior {
        bongocat_model::ModelBehaviorSnapshot::Motion { group, index } => {
            let Ok(motion) = MotionId::new(group, index) else {
                return;
            };
            let motion_is_settled = renderer.motion_is_settled(now);
            let current_priority = active_motion
                .as_ref()
                .filter(|_| !motion_is_settled)
                .map(|active| active.priority);
            if current_priority.is_some_and(|priority| priority > MotionPriority::Idle) {
                return;
            }
            if renderer.validate_motion(&motion).is_err() {
                return;
            }
            if motion_audio_enabled {
                if let Some(path) = motion_audio_path(Some(model), &motion) {
                    let _ = motion_audio.try_publish_with_sequence(|sequence| {
                        MotionAudioCommand::Play {
                            sequence,
                            path,
                            volume: MotionAudioVolume::FULL,
                        }
                    });
                } else {
                    stop_motion_audio(motion_audio, MotionAudioStopReason::MotionReplaced);
                }
            }
            if renderer.start_motion(&motion, now, false).is_err() {
                return;
            }
            let active = ActiveMotionSnapshot {
                motion,
                priority: MotionPriority::Idle,
                command_sequence: automatic_sequence,
                stop_command_sequence: None,
            };
            *active_motion = Some(active.clone());
            publish(snapshot, |current| current.active_motion = Some(active));
        }
        bongocat_model::ModelBehaviorSnapshot::Expression { name } => {
            let Ok(expression) = ExpressionId::new(name) else {
                return;
            };
            if renderer.set_expression(&expression, now).is_err() {
                return;
            }
            let active = ActiveExpressionSnapshot {
                expression,
                command_sequence: automatic_sequence,
            };
            *active_expression = Some(active.clone());
            publish(snapshot, |current| current.active_expression = Some(active));
        }
    }
}
