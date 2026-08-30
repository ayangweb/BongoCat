use crate::{ModelInputSnapshot, MotionId, RuntimeRenderErrorCode};
use bongocat_model::CommittedModel;
use bongocat_render::{
    ModelCommitFeedback, ModelCommitToken, RenderConsumer, RenderProducer, latest_render_channel,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_live2d::{
    ExpressionClip, ExpressionLayer, Live2dModel, MotionClip, ParameterUpdate, ProductParameter,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_render::RenderFrame;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderEvaluation {
    pub(crate) rendered: bool,
    pub(crate) motion_finished: bool,
}

pub(crate) struct RuntimeRenderer {
    producer: RenderProducer,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    next_model_generation: u64,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    next_transport_sequence: u64,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    active: Option<ActiveRenderModel>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pending: Option<ActiveRenderModel>,
}

pub(crate) struct RuntimeRenderBootstrap {
    producer: RenderProducer,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ActiveRenderModel {
    model: Live2dModel,
    resources: Arc<bongocat_render::RenderResources>,
    model_generation: u64,
    next_frame_number: u64,
    motion: Option<MotionPlayback>,
    expressions: Vec<ExpressionPlayback>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct MotionPlayback {
    clip: MotionClip,
    started_at: Duration,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ExpressionPlayback {
    clip: ExpressionClip,
    started_at: Duration,
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
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            next_model_generation: 0,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            next_transport_sequence: 0,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            active: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            pending: None,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        command_sequence: u64,
        committed: &CommittedModel,
        input: ModelInputSnapshot,
    ) -> Result<ModelCommitToken, RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            debug_assert!(self.pending.is_none());
            let mut model = Live2dModel::load(committed)
                .map_err(|_| RuntimeRenderErrorCode::ModelLoadFailed)?;
            apply_model_input(&mut model, input)?;
            let snapshot = model
                .update_and_snapshot()
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
            let resources = model.render_resources();
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
                motion: None,
                expressions: Vec::new(),
            });
            Ok(token)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (command_sequence, committed, input);
            Err(RuntimeRenderErrorCode::PlatformUnsupported)
        }
    }

    pub(crate) fn commit(&mut self, token: ModelCommitToken) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
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

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = token;
            false
        }
    }

    pub(crate) fn reject(&mut self, token: ModelCommitToken) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
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

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = token;
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
    ) -> Result<(), RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
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
                started_at: now,
            });
            Ok(())
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (motion, now);
            Err(RuntimeRenderErrorCode::PlatformUnsupported)
        }
    }

    pub(crate) fn stop_motion(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(active) = &mut self.active {
            active.motion = None;
        }
    }

    pub(crate) fn set_expression(
        &mut self,
        expression: &crate::ExpressionId,
        now: Duration,
    ) -> Result<(), RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
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
                fade_out_started_at: None,
            });
            debug_assert!(active.expressions.len() <= 2);
            Ok(())
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (expression, now);
            Err(RuntimeRenderErrorCode::PlatformUnsupported)
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        input: ModelInputSnapshot,
        now: Duration,
    ) -> Result<RenderEvaluation, RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let Some(active) = &mut self.active else {
                return Ok(RenderEvaluation::default());
            };
            active
                .model
                .restore_parameter_defaults()
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
            let motion_finished = if let Some(playback) = &active.motion {
                active
                    .model
                    .apply_motion(&playback.clip, now.saturating_sub(playback.started_at))
                    .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?
                    .finished
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
                .iter()
                .map(|playback| {
                    let fade_in = playback
                        .clip
                        .fade_in_weight(now.saturating_sub(playback.started_at));
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
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
            apply_model_input(&mut active.model, input)?;
            let snapshot = active
                .model
                .update_and_snapshot()
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
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
            })
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (input, now);
            Ok(RenderEvaluation::default())
        }
    }

    pub(crate) fn close(&self) {
        self.producer.close();
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn apply_model_input(
    model: &mut Live2dModel,
    input: ModelInputSnapshot,
) -> Result<(), RuntimeRenderErrorCode> {
    for (parameter, value) in [
        (ProductParameter::MouseX, input.pointer_x),
        (ProductParameter::MouseY, input.pointer_y),
        (ProductParameter::AngleX, input.pointer_x),
        (ProductParameter::AngleY, input.pointer_y),
        (ProductParameter::AngleZ, input.pointer_z),
        (ProductParameter::EyeBallX, input.pointer_x),
        (ProductParameter::EyeBallY, input.pointer_y),
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
    ] {
        match model.set_normalized_parameter(parameter, value) {
            Ok(ParameterUpdate::Applied { .. } | ParameterUpdate::Unsupported) => {}
            Err(_) => return Err(RuntimeRenderErrorCode::ModelEvaluationFailed),
        }
    }
    Ok(())
}
