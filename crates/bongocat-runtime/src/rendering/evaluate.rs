//! One frame, in order.
//!
//! The order is the contract: motion, then expression, then input, then the
//! automatic effects, then Cubism's own update. Anything else and a key press
//! would be overwritten by the breath in the same frame, which looks like the key
//! press did nothing.

use super::*;

impl RuntimeRenderer {
    pub(crate) fn evaluate(
        &mut self,
        input: ModelInputSnapshot,
        now: Duration,
    ) -> Result<RenderEvaluation, RuntimeRenderErrorCode> {
        let Some(active) = &mut self.active else {
            return Ok(RenderEvaluation::default());
        };
        active.model.restore_parameter_defaults().map_err(|error| {
            map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
        })?;
        active
            .model
            .restore_part_opacity_defaults()
            .map_err(|error| {
                map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
            })?;
        let mut motion_user_data = Vec::new();
        let mut skipped_motion_user_data = 0;
        let motion_finished = if let Some(playback) = &mut active.motion {
            let elapsed = now.saturating_sub(playback.started_at);
            let elapsed = if playback.completed {
                playback.clip.duration()
            } else if playback.looping {
                elapsed
            } else {
                elapsed.min(playback.clip.duration())
            };
            let fade_out_elapsed = playback
                .fade_out_started_at
                .map(|started_at| now.saturating_sub(started_at));
            let explicit_fade_finished = fade_out_elapsed
                .is_some_and(|elapsed| elapsed >= playback.clip.fade_out_duration());
            let user_data = playback.clip.user_data_events_between_with_looping(
                playback.last_event_elapsed,
                elapsed,
                playback.looping,
            );
            if playback
                .last_event_elapsed
                .is_none_or(|previous| elapsed >= previous)
            {
                playback.last_event_elapsed = Some(elapsed);
            }
            motion_user_data = user_data
                .occurrences
                .into_iter()
                .map(|occurrence| RenderMotionUserDataOccurrence {
                    cycle: occurrence.cycle,
                    local_time: occurrence.local_time,
                    value: occurrence.value,
                })
                .collect();
            skipped_motion_user_data = user_data.skipped_occurrences;
            let weight =
                fade_out_elapsed.map_or(1.0, |elapsed| playback.clip.fade_out_weight(elapsed));
            let status = if playback.looping {
                active
                    .model
                    .apply_motion_with_weight(&playback.clip, elapsed, weight)
            } else {
                active
                    .model
                    .apply_motion_once_with_weight(&playback.clip, elapsed, weight)
            }
            .map_err(|error| {
                map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
            })?;
            if !playback.looping && status.finished {
                playback.completed = true;
            }
            explicit_fade_finished
        } else {
            false
        };
        if motion_finished {
            active.motion = None;
        }
        active.expressions.retain(|playback| {
            playback.fade_out_started_at.is_none_or(|started_at| {
                now.saturating_sub(started_at) < playback.clip.fade_out_duration()
            })
        });
        let expression_layers = active
            .expressions
            .iter_mut()
            .map(|playback| {
                let fade_in_elapsed = now.saturating_sub(playback.started_at);
                let fade_in = if playback.fade_in_completed {
                    1.0
                } else {
                    let weight = playback.clip.fade_in_weight(fade_in_elapsed);
                    if fade_in_elapsed >= playback.clip.fade_in_duration() {
                        playback.fade_in_completed = true;
                    }
                    weight
                };
                let fade_out = playback.fade_out_started_at.map_or(1.0, |started_at| {
                    playback
                        .clip
                        .fade_out_weight(now.saturating_sub(started_at))
                });
                ExpressionLayer {
                    clip: &playback.clip,
                    weight: fade_in * fade_out,
                }
            })
            .collect::<Vec<_>>();
        active
            .model
            .apply_expression_layers(&expression_layers)
            .map_err(|error| {
                map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
            })?;
        let physics_delta = match active.last_evaluated_at {
            Some(previous) if now >= previous => now - previous,
            Some(_) => {
                active.model.reset_physics();
                Duration::ZERO
            }
            None => Duration::ZERO,
        };
        apply_model_input(&mut active.model, input, self.model_settings)?;
        apply_automatic_effects(&mut active.model, now)?;
        active.model.apply_physics(physics_delta).map_err(|error| {
            map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
        })?;
        active.last_evaluated_at = Some(now);
        let mut snapshot = active.model.update_and_snapshot().map_err(|error| {
            map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
        })?;
        snapshot.active_keys = resolve_key_overlays(&active.resources, input.key_presses);
        snapshot.mirror_horizontal = self.model_settings.mirror;
        self.producer
            .publish(RenderFrame {
                transport_sequence: self.next_transport_sequence,
                model_generation: active.model_generation,
                frame_number: active.next_frame_number,
                model_commit: None,
                resources: Arc::clone(&active.resources),
                snapshot: Arc::new(snapshot),
            })
            .map_err(|_| RuntimeRenderErrorCode::TransportClosed)?;
        active.next_frame_number = active.next_frame_number.wrapping_add(1);
        self.next_transport_sequence = self.next_transport_sequence.wrapping_add(1);
        Ok(RenderEvaluation {
            rendered: true,
            motion_finished,
            motion_user_data,
            skipped_motion_user_data,
        })
    }
}
