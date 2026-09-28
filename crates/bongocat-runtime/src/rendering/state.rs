//! What the renderer is drawing and playing right now.
//!
//! The renderer holds one model, one motion and one expression at a time, because
//! the runtime's model is the only one on screen. A motion that has finished holds
//! its terminal parameters rather than snapping to the model's defaults, because a
//! model that visibly springs back the instant its motion ends is a model a user
//! would describe as glitching.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderMotionUserDataOccurrence {
    pub(crate) cycle: u64,
    pub(crate) local_time: Duration,
    pub(crate) value: String,
}

pub(crate) struct RuntimeRenderer {
    pub(crate) producer: RenderProducer,
    pub(crate) model_settings: ModelSettings,
    pub(crate) next_model_generation: u64,
    pub(crate) next_transport_sequence: u64,
    pub(crate) active: Option<ActiveRenderModel>,
    pub(crate) pending: Option<ActiveRenderModel>,
}

pub(crate) struct RuntimeRenderBootstrap {
    pub(crate) producer: RenderProducer,
}

pub(crate) struct ActiveRenderModel {
    pub(crate) model: Live2dModel,
    pub(crate) resources: Arc<bongocat_render::RenderResources>,
    pub(crate) model_generation: u64,
    pub(crate) next_frame_number: u64,
    pub(crate) last_evaluated_at: Option<Duration>,
    pub(crate) motion: Option<MotionPlayback>,
    pub(crate) expressions: Vec<ExpressionPlayback>,
}

pub(crate) struct MotionPlayback {
    pub(crate) clip: MotionClip,
    pub(crate) looping: bool,
    pub(crate) started_at: Duration,
    pub(crate) completed: bool,
    pub(crate) fade_out_started_at: Option<Duration>,
    pub(crate) last_event_elapsed: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MotionStopStatus {
    Fading,
    Finished,
}

pub(crate) struct ExpressionPlayback {
    pub(crate) clip: ExpressionClip,
    pub(crate) started_at: Duration,
    pub(crate) fade_in_completed: bool,
    pub(crate) fade_out_started_at: Option<Duration>,
}
