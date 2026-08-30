use crate::{ModelInputSnapshot, RuntimeRenderErrorCode};
use bongocat_model::CommittedModel;
use bongocat_render::{RenderConsumer, RenderProducer, latest_render_channel};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_live2d::{Live2dModel, ParameterUpdate, ProductParameter};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_render::RenderFrame;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::Arc;

pub(crate) struct RuntimeRenderer {
    producer: RenderProducer,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    next_model_generation: u64,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    active: Option<ActiveRenderModel>,
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
            active: None,
        }
    }

    pub(crate) fn activate(
        &mut self,
        committed: &CommittedModel,
        input: ModelInputSnapshot,
    ) -> Result<(), RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let mut model = Live2dModel::load(committed)
                .map_err(|_| RuntimeRenderErrorCode::ModelLoadFailed)?;
            apply_model_input(&mut model, input)?;
            let snapshot = model
                .update_and_snapshot()
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
            let resources = model.render_resources();
            let model_generation = self.next_model_generation;
            let frame = RenderFrame {
                model_generation,
                frame_number: 0,
                resources: Arc::clone(&resources),
                snapshot: Arc::new(snapshot),
            };
            self.producer
                .publish(frame)
                .map_err(|_| RuntimeRenderErrorCode::TransportClosed)?;
            self.next_model_generation = self.next_model_generation.wrapping_add(1);
            self.active = Some(ActiveRenderModel {
                model,
                resources,
                model_generation,
                next_frame_number: 1,
            });
            Ok(())
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (committed, input);
            Err(RuntimeRenderErrorCode::PlatformUnsupported)
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        input: ModelInputSnapshot,
    ) -> Result<bool, RuntimeRenderErrorCode> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let Some(active) = &mut self.active else {
                return Ok(false);
            };
            apply_model_input(&mut active.model, input)?;
            let snapshot = active
                .model
                .update_and_snapshot()
                .map_err(|_| RuntimeRenderErrorCode::ModelEvaluationFailed)?;
            self.producer
                .publish(RenderFrame {
                    model_generation: active.model_generation,
                    frame_number: active.next_frame_number,
                    resources: Arc::clone(&active.resources),
                    snapshot: Arc::new(snapshot),
                })
                .map_err(|_| RuntimeRenderErrorCode::TransportClosed)?;
            active.next_frame_number = active.next_frame_number.wrapping_add(1);
            Ok(true)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = input;
            Ok(false)
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
