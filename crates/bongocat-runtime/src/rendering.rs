use crate::{ModelInputSnapshot, ModelSettings, MotionId, RuntimeRenderErrorCode};
use bongocat_live2d_render::resolve_key_overlays;
use bongocat_model::CommittedModel;
use bongocat_render::{
    ModelCommitFeedback, ModelCommitToken, RenderConsumer, RenderProducer, latest_render_channel,
};

use bongocat_live2d::{Live2dError, Live2dModel, ParameterUpdate, ProductParameter};
use bongocat_live2d_playback::{ExpressionClip, ExpressionLayer, MotionClip};
use bongocat_render::RenderFrame;
use std::sync::Arc;
use std::time::Duration;

const BLINK_PERIOD: Duration = Duration::from_secs(5);
const BLINK_CLOSED_DURATION: Duration = Duration::from_millis(180);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderEvaluation {
    pub(crate) rendered: bool,
    pub(crate) motion_finished: bool,
    pub(crate) motion_user_data: Vec<RenderMotionUserDataOccurrence>,
    pub(crate) skipped_motion_user_data: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderMotionUserDataOccurrence {
    pub(crate) cycle: u64,
    pub(crate) local_time: Duration,
    pub(crate) value: String,
}

pub(crate) struct RuntimeRenderer {
    producer: RenderProducer,
    model_settings: ModelSettings,
    next_model_generation: u64,
    next_transport_sequence: u64,
    active: Option<ActiveRenderModel>,
    pending: Option<ActiveRenderModel>,
}

pub(crate) struct RuntimeRenderBootstrap {
    producer: RenderProducer,
}

struct ActiveRenderModel {
    model: Live2dModel,
    resources: Arc<bongocat_render::RenderResources>,
    model_generation: u64,
    next_frame_number: u64,
    last_evaluated_at: Option<Duration>,
    motion: Option<MotionPlayback>,
    expressions: Vec<ExpressionPlayback>,
}

struct MotionPlayback {
    clip: MotionClip,
    looping: bool,
    started_at: Duration,
    completed: bool,
    fade_out_started_at: Option<Duration>,
    last_event_elapsed: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MotionStopStatus {
    Fading,
    Finished,
}

struct ExpressionPlayback {
    clip: ExpressionClip,
    started_at: Duration,
    fade_in_completed: bool,
    fade_out_started_at: Option<Duration>,
}

impl RuntimeRenderer {
    pub(crate) fn channel() -> (RuntimeRenderBootstrap, RenderConsumer) {
        let (producer, consumer) = latest_render_channel();
        (RuntimeRenderBootstrap { producer }, consumer)
    }

    pub(crate) fn start(bootstrap: RuntimeRenderBootstrap) -> Self {
        Self {
            producer: bootstrap.producer,
            model_settings: ModelSettings::default(),
            next_model_generation: 0,
            next_transport_sequence: 0,
            active: None,
            pending: None,
        }
    }

    pub(crate) fn set_model_settings(&mut self, settings: ModelSettings) {
        self.model_settings = settings;
    }

    pub(crate) fn prepare(
        &mut self,
        command_sequence: u64,
        committed: &CommittedModel,
        input: ModelInputSnapshot,
    ) -> Result<ModelCommitToken, RuntimeRenderErrorCode> {
        debug_assert!(self.pending.is_none());
        let mut model = Live2dModel::load(committed)
            .map_err(|error| map_live2d_error(error, RuntimeRenderErrorCode::ModelLoadFailed))?;
        let resources = model.render_resources();
        apply_model_input(&mut model, input, self.model_settings)?;
        let mut snapshot = model.update_and_snapshot().map_err(|error| {
            map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed)
        })?;
        snapshot.active_keys = resolve_key_overlays(&resources, input.key_presses);
        snapshot.mirror_horizontal = self.model_settings.mirror;
        let model_generation = self.next_model_generation;
        let token = ModelCommitToken {
            command_sequence,
            model_generation,
        };
        let frame = RenderFrame {
            transport_sequence: self.next_transport_sequence,
            model_generation,
            frame_number: 0,
            model_commit: Some(token),
            resources: Arc::clone(&resources),
            snapshot: Arc::new(snapshot),
        };
        self.producer
            .publish(frame)
            .map_err(|_| RuntimeRenderErrorCode::TransportClosed)?;
        self.next_model_generation = self.next_model_generation.wrapping_add(1);
        self.next_transport_sequence = self.next_transport_sequence.wrapping_add(1);
        self.pending = Some(ActiveRenderModel {
            model,
            resources,
            model_generation,
            next_frame_number: 1,
            last_evaluated_at: None,
            motion: None,
            expressions: Vec::new(),
        });
        Ok(token)
    }

    pub(crate) fn commit(&mut self, token: ModelCommitToken) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        if pending.model_generation != token.model_generation {
            self.pending = Some(pending);
            return false;
        }
        self.active = Some(pending);
        true
    }

    pub(crate) fn reject(&mut self, token: ModelCommitToken) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.model_generation == token.model_generation)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn take_model_commit_feedback(&self) -> Option<ModelCommitFeedback> {
        self.producer.take_model_commit_feedback()
    }

    pub(crate) fn record_stale_model_commit_feedback(&self) {
        self.producer.record_stale_model_commit_feedback();
    }

    pub(crate) fn start_motion(
        &mut self,
        motion: &MotionId,
        now: Duration,
        looping: bool,
    ) -> Result<(), RuntimeRenderErrorCode> {
        let active = self
            .active
            .as_mut()
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)?;
        let clip = active
            .model
            .motion_clip(motion.group(), motion.index())
            .cloned()
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)?;
        active.motion = Some(MotionPlayback {
            clip,
            looping,
            started_at: now,
            completed: false,
            fade_out_started_at: None,
            last_event_elapsed: None,
        });
        Ok(())
    }

    /// Confirms that the active model has the already-prepared clip before an
    /// audio worker is allowed to make the motion externally observable.
    pub(crate) fn validate_motion(&self, motion: &MotionId) -> Result<(), RuntimeRenderErrorCode> {
        self.active
            .as_ref()
            .and_then(|active| active.model.motion_clip(motion.group(), motion.index()))
            .map(|_| ())
            .ok_or(RuntimeRenderErrorCode::MotionLoadFailed)
    }

    /// A completed one-shot keeps contributing its terminal sample, but it no
    /// longer reserves priority. Derive completion from the injected clock as
    /// well as the last delivered frame so a hidden or sleeping overlay cannot
    /// swallow a command sent after the clip duration. An explicit stop in
    /// progress is still stopping, not settled, until its fade removes the layer.
    pub(crate) fn motion_is_settled(&self, now: Duration) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.motion.as_ref().is_some_and(|playback| {
                let completed = playback.completed
                    || (!playback.looping
                        && now.saturating_sub(playback.started_at) >= playback.clip.duration());
                completed && playback.fade_out_started_at.is_none()
            })
        })
    }

    pub(crate) fn stop_motion(&mut self, now: Duration) -> MotionStopStatus {
        if let Some(active) = &mut self.active
            && let Some(playback) = &mut active.motion
        {
            if playback.clip.fade_out_duration().is_zero() {
                active.motion = None;
                return MotionStopStatus::Finished;
            }
            playback.fade_out_started_at.get_or_insert(now);
            return MotionStopStatus::Fading;
        }
        MotionStopStatus::Finished
    }

    pub(crate) fn set_expression(
        &mut self,
        expression: &crate::ExpressionId,
        now: Duration,
    ) -> Result<(), RuntimeRenderErrorCode> {
        let active = self
            .active
            .as_mut()
            .ok_or(RuntimeRenderErrorCode::ExpressionLoadFailed)?;
        let clip = active
            .model
            .expression_clip(expression.name())
            .cloned()
            .ok_or(RuntimeRenderErrorCode::ExpressionLoadFailed)?;
        if active.expressions.len() > 1 {
            let previous = active
                .expressions
                .pop()
                .expect("expression stack has a newest layer");
            active.expressions.clear();
            active.expressions.push(previous);
        }
        for playback in &mut active.expressions {
            playback.fade_out_started_at = Some(now);
        }
        active.expressions.push(ExpressionPlayback {
            clip,
            started_at: now,
            fade_in_completed: false,
            fade_out_started_at: None,
        });
        debug_assert!(active.expressions.len() <= 2);
        Ok(())
    }

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

    pub(crate) fn close(&self) {
        self.producer.close();
    }
}

fn apply_automatic_effects(
    model: &mut Live2dModel,
    now: Duration,
) -> Result<(), RuntimeRenderErrorCode> {
    let (breath_time, blink) = automatic_effect_values(now);
    model
        .apply_automatic_effects(breath_time, blink)
        .map_err(|error| map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed))?;
    Ok(())
}

fn automatic_effect_values(now: Duration) -> (Duration, f32) {
    let blink_phase = now.as_secs_f64() % BLINK_PERIOD.as_secs_f64();
    let blink = if blink_phase < BLINK_CLOSED_DURATION.as_secs_f64() {
        -1.0
    } else {
        0.0
    };
    (now, blink)
}

fn apply_model_input(
    model: &mut Live2dModel,
    input: ModelInputSnapshot,
    settings: ModelSettings,
) -> Result<(), RuntimeRenderErrorCode> {
    let (pointer_x, pointer_y, pointer_z) = if settings.ignore_pointer {
        (0.0, 0.0, 0.0)
    } else {
        let horizontal_sign = if settings.mirror_pointer_tracking {
            -1.0
        } else {
            1.0
        };
        (
            input.pointer_x * horizontal_sign,
            input.pointer_y,
            input.pointer_z * horizontal_sign,
        )
    };
    for (parameter, value) in [
        (ProductParameter::MouseX, pointer_x),
        (ProductParameter::MouseY, pointer_y),
        (ProductParameter::AngleX, pointer_x),
        (ProductParameter::AngleY, pointer_y),
        (ProductParameter::AngleZ, pointer_z),
        (ProductParameter::EyeBallX, pointer_x),
        (ProductParameter::EyeBallY, pointer_y),
        (
            ProductParameter::LeftHandDown,
            f32::from(input.left_hand_down),
        ),
        (
            ProductParameter::RightHandDown,
            f32::from(input.right_hand_down),
        ),
        (
            ProductParameter::MouseLeftDown,
            f32::from(input.mouse_left_down),
        ),
        (
            ProductParameter::MouseRightDown,
            f32::from(input.mouse_right_down),
        ),
        (
            ProductParameter::StickLeftDown,
            f32::from(input.stick_left_down),
        ),
        (
            ProductParameter::StickRightDown,
            f32::from(input.stick_right_down),
        ),
        (ProductParameter::StickLeftX, input.stick_left_x),
        (ProductParameter::StickLeftY, input.stick_left_y),
        (ProductParameter::StickRightX, input.stick_right_x),
        (ProductParameter::StickRightY, input.stick_right_y),
    ] {
        match model.set_normalized_parameter(parameter, value) {
            Ok(ParameterUpdate::Applied { .. } | ParameterUpdate::Unsupported) => {}
            Err(error) => {
                return Err(map_live2d_error(
                    error,
                    RuntimeRenderErrorCode::ModelEvaluationFailed,
                ));
            }
        }
    }
    Ok(())
}

fn map_live2d_error(
    _error: Live2dError,
    fallback: RuntimeRenderErrorCode,
) -> RuntimeRenderErrorCode {
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_live2d::Live2dErrorCode;
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    fn preset_model(id: &str) -> CommittedModel {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model")
    }

    #[test]
    fn live2d_errors_map_to_stable_runtime_categories() {
        for code in Live2dErrorCode::ALL {
            let error = Live2dError {
                code,
                detail: String::new(),
            };
            let mapped = map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed);
            assert_eq!(
                mapped,
                RuntimeRenderErrorCode::ModelEvaluationFailed,
                "unexpected mapping for {code}"
            );
        }
    }

    #[test]
    fn automatic_effects_are_periodic_and_deterministic() {
        let start = automatic_effect_values(Duration::ZERO);
        let full_cycle = automatic_effect_values(BLINK_PERIOD);
        assert_eq!(start.0, Duration::ZERO);
        assert_eq!(full_cycle.0, BLINK_PERIOD);
        assert_eq!(start.1, -1.0);
        assert_eq!(automatic_effect_values(BLINK_CLOSED_DURATION).1, 0.0);
        assert_eq!(full_cycle.1, -1.0);
    }

    #[test]
    fn automatic_effects_keep_blink_in_closed_or_open_contract() {
        for millis in (0..=BLINK_PERIOD.as_millis()).step_by(37) {
            let (_, blink) = automatic_effect_values(Duration::from_millis(
                u64::try_from(millis).expect("duration fits u64"),
            ));
            assert!(blink == -1.0 || blink == 0.0);
        }
    }

    #[test]
    fn frame_evaluation_order_is_motion_expression_input_effects_then_core_update() {
        let (bootstrap, _consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        let token = renderer
            .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));

        let motion = MotionClip::from_slice(
            br#"{
              "Version":3,
              "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
                "CurveCount":3,"TotalSegmentCount":3,"TotalPointCount":6,
                "UserDataCount":0,"TotalUserDataSize":0},
              "Curves":[
                {"Target":"Parameter","Id":"ParamEyeLOpen","Segments":[0,0.7,0,1,0.7]},
                {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0.2,0,1,0.2]},
                {"Target":"Parameter","Id":"ParamAngleX","Segments":[0,20,0,1,20]}
              ]
            }"#,
            0.0,
            0.0,
        )
        .expect("motion");
        let expression = ExpressionClip::from_slice(
            br#"{
              "Type":"Live2D Expression","FadeInTime":0.0,"FadeOutTime":0.0,
              "Parameters":[
                {"Id":"ParamEyeROpen","Value":0.7,"Blend":"Overwrite"},
                {"Id":"ParamMouthOpenY","Value":0.8,"Blend":"Overwrite"},
                {"Id":"ParamAngleX","Value":10.0,"Blend":"Overwrite"}
              ]
            }"#,
        )
        .expect("expression");
        let active = renderer.active.as_mut().expect("active model");
        active.motion = Some(MotionPlayback {
            clip: motion,
            looping: true,
            started_at: Duration::ZERO,
            completed: false,
            fade_out_started_at: None,
            last_event_elapsed: None,
        });
        active.expressions.push(ExpressionPlayback {
            clip: expression,
            started_at: Duration::ZERO,
            fade_in_completed: false,
            fade_out_started_at: None,
        });

        renderer
            .evaluate(
                ModelInputSnapshot {
                    pointer_x: -0.5,
                    ..ModelInputSnapshot::default()
                },
                Duration::ZERO,
            )
            .expect("evaluate frame");
        let model = &renderer.active.as_ref().expect("active model").model;
        for (id, expected) in [
            ("ParamEyeLOpen", 0.0),
            ("ParamEyeROpen", 0.0),
            ("ParamMouthOpenY", 0.8),
            ("ParamAngleX", -15.0),
        ] {
            let actual = model
                .parameter_value_by_id(id)
                .expect("parameter value")
                .expect("supported parameter");
            assert!((actual - expected).abs() < 0.0001, "{id}: {actual}");
        }

        renderer
            .evaluate(
                ModelInputSnapshot {
                    pointer_x: -0.5,
                    ..ModelInputSnapshot::default()
                },
                Duration::from_secs(2),
            )
            .expect("expression persistence frame");
        let active = renderer.active.as_ref().expect("active model");
        assert_eq!(active.expressions.len(), 1);
        let mouth = active
            .model
            .parameter_value_by_id("ParamMouthOpenY")
            .expect("parameter value")
            .expect("supported parameter");
        assert!(
            (mouth - 0.8).abs() < 0.0001,
            "the latest expression must remain active after its fade-in completes: {mouth}"
        );
    }

    #[test]
    fn completed_motion_holds_the_post_natural_fade_terminal_evaluation() {
        let (bootstrap, _consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        let token = renderer
            .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));
        let mouth_default = renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .parameter_value_by_id("ParamMouthOpenY")
            .expect("mouth parameter")
            .expect("supported parameter");

        let motion = MotionClip::from_slice(
            br#"{
              "Version":3,
              "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
                "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
                "UserDataCount":0,"TotalUserDataSize":0},
              "Curves":[
                {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0,0,1,1]},
                {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]}
              ]
            }"#,
            0.0,
            1.0,
        )
        .expect("fading motion");
        renderer.active.as_mut().expect("active model").motion = Some(MotionPlayback {
            clip: motion,
            looping: false,
            started_at: Duration::ZERO,
            completed: false,
            fade_out_started_at: None,
            last_event_elapsed: None,
        });

        for now in [Duration::from_secs(2), Duration::from_secs(3)] {
            renderer
                .evaluate(ModelInputSnapshot::default(), now)
                .expect("completed fading motion frame");
            let active = renderer.active.as_ref().expect("active model");
            assert!(
                active
                    .motion
                    .as_ref()
                    .is_some_and(|playback| playback.completed)
            );
            assert_eq!(
                active
                    .model
                    .parameter_value_by_id("ParamMouthOpenY")
                    .expect("mouth parameter")
                    .expect("supported parameter"),
                mouth_default,
                "the held sample includes the resource's completed natural fade"
            );
            assert_eq!(
                active
                    .model
                    .part_opacity_by_id("Part")
                    .expect("part opacity")
                    .expect("supported part"),
                0.25,
                "PartOpacity keeps its independent R5 sink value"
            );
        }
    }

    #[test]
    fn latest_expression_stays_full_weight_after_clock_rollback() {
        let (bootstrap, _consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        let token = renderer
            .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));
        let expression = ExpressionClip::from_slice(
            br#"{
              "Type":"Live2D Expression","FadeInTime":1.0,"FadeOutTime":1.0,
              "Parameters":[
                {"Id":"ParamMouthOpenY","Value":0.8,"Blend":"Overwrite"}
              ]
            }"#,
        )
        .expect("expression");
        renderer
            .active
            .as_mut()
            .expect("active model")
            .expressions
            .push(ExpressionPlayback {
                clip: expression,
                started_at: Duration::ZERO,
                fade_in_completed: false,
                fade_out_started_at: None,
            });

        for (now, expected) in [
            (Duration::from_secs(2), 0.8),
            (Duration::from_millis(500), 0.8),
        ] {
            renderer
                .evaluate(ModelInputSnapshot::default(), now)
                .expect("expression frame");
            let active = renderer.active.as_ref().expect("active model");
            assert!(
                active
                    .expressions
                    .first()
                    .is_some_and(|playback| playback.fade_in_completed)
            );
            let mouth = active
                .model
                .parameter_value_by_id("ParamMouthOpenY")
                .expect("parameter value")
                .expect("supported parameter");
            assert!(
                (mouth - expected).abs() < 0.0001,
                "a completed expression fade-in must not restart after clock rollback: {mouth}"
            );
        }
    }

    #[test]
    fn completed_motion_holds_its_terminal_parameters_until_stopped() {
        let (bootstrap, _consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        let token = renderer
            .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));

        let motion = MotionClip::from_slice(
            br#"{
              "Version":3,
              "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
                "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
                "UserDataCount":1,"TotalUserDataSize":5},
              "Curves":[
                {"Target":"Parameter","Id":"Param","Segments":[0,0,0,1,1]},
                {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]}
              ],
              "UserData":[{"Time":0.0,"Value":"start"}]
            }"#,
            0.0,
            0.0,
        )
        .expect("motion");
        renderer.active.as_mut().expect("active model").motion = Some(MotionPlayback {
            clip: motion,
            looping: false,
            started_at: Duration::ZERO,
            completed: false,
            fade_out_started_at: None,
            last_event_elapsed: None,
        });

        for (now, expected_user_data_events, expected_value, expected_part, settled) in [
            (Duration::ZERO, 1, 0.0, 0.0, false),
            (Duration::from_secs(2), 0, 1.0, 0.25, true),
            (Duration::from_secs(3), 0, 1.0, 0.25, true),
            (Duration::from_secs(1), 0, 1.0, 0.25, true),
        ] {
            let evaluation = renderer
                .evaluate(ModelInputSnapshot::default(), now)
                .expect("completed motion frame");
            assert!(
                !evaluation.motion_finished,
                "natural completion must keep the motion layer current"
            );
            assert_eq!(
                evaluation.motion_user_data.len(),
                expected_user_data_events,
                "one-shot UserData must follow the effective playback mode exactly once"
            );
            assert_eq!(renderer.motion_is_settled(now), settled);
            let active = renderer.active.as_ref().expect("active model");
            let value = active
                .model
                .parameter_value_by_id("Param")
                .expect("parameter value")
                .expect("supported parameter");
            assert!(
                (value - expected_value).abs() < 0.0001,
                "the evaluated motion parameter must be reapplied after defaults at {now:?}: {value}"
            );
            let part_opacity = active
                .model
                .part_opacity_by_id("Part")
                .expect("part opacity")
                .expect("supported part");
            assert!(
                (part_opacity - expected_part).abs() < 0.0001,
                "the completed PartOpacity sample must remain current at {now:?}: {part_opacity}"
            );
        }

        let part_opacity_before_stop = {
            let active = renderer.active.as_ref().expect("active model");
            active
                .model
                .part_opacity_by_id("Part")
                .expect("current part opacity")
                .expect("supported part")
        };
        assert!(part_opacity_before_stop < 1.0);
        assert_eq!(
            renderer.stop_motion(Duration::from_secs(3)),
            MotionStopStatus::Finished
        );
        assert!(
            renderer
                .active
                .as_ref()
                .expect("active model")
                .motion
                .is_none()
        );
        renderer
            .evaluate(ModelInputSnapshot::default(), Duration::from_secs(4))
            .expect("frame after zero-duration stop");
        assert!(
            renderer
                .active
                .as_ref()
                .expect("active model")
                .model
                .part_opacity_by_id("Part")
                .expect("part opacity after stop")
                .expect("supported part")
                > 0.99,
            "stopping a motion must restore Core part opacity before the next layer"
        );
    }

    #[test]
    fn model_settings_control_pointer_tracking_and_render_mirroring() {
        let (bootstrap, consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        renderer.set_model_settings(ModelSettings {
            mirror: true,
            mirror_pointer_tracking: true,
            ignore_pointer: false,
        });
        let token = renderer
            .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));
        let initial = consumer.take_latest().expect("initial frame");
        assert!(initial.snapshot.mirror_horizontal);

        renderer
            .evaluate(
                ModelInputSnapshot {
                    pointer_x: 0.5,
                    pointer_y: 0.25,
                    pointer_z: 0.5,
                    ..ModelInputSnapshot::default()
                },
                Duration::ZERO,
            )
            .expect("mirrored pointer frame");
        let model = &renderer.active.as_ref().expect("active model").model;
        let mirrored_angle = model
            .parameter_value_by_id("ParamAngleX")
            .expect("parameter value")
            .expect("supported parameter");
        assert!(mirrored_angle < 0.0);

        renderer.set_model_settings(ModelSettings {
            mirror: false,
            mirror_pointer_tracking: false,
            ignore_pointer: true,
        });
        renderer
            .evaluate(
                ModelInputSnapshot {
                    pointer_x: -1.0,
                    pointer_y: 1.0,
                    pointer_z: -1.0,
                    ..ModelInputSnapshot::default()
                },
                Duration::from_millis(1),
            )
            .expect("ignored pointer frame");
        let ignored_angle = renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .parameter_value_by_id("ParamAngleX")
            .expect("parameter value")
            .expect("supported parameter");
        let expected_reference_angle =
            0.5 * 15.0 * (std::f64::consts::TAU * 0.001 / 6.5345).sin() as f32;
        assert!(
            (ignored_angle - expected_reference_angle).abs() < 0.0001,
            "ignored pointer must leave only the reference breath: {ignored_angle}"
        );
        let frame = consumer.take_latest().expect("updated frame");
        assert!(!frame.snapshot.mirror_horizontal);
    }
}
