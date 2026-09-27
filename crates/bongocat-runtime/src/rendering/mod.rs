//! The renderer's own half of the runtime: what it draws, what it plays, and the
//! order one frame is evaluated in.

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

mod automatic;
mod error;
mod evaluate;
mod expression;
mod lifecycle;
mod model;
mod model_input;
mod motion;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use automatic::*;
pub(crate) use error::*;
pub(crate) use model_input::*;
pub(crate) use state::*;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the runtime names are listed here rather than left to a glob.
