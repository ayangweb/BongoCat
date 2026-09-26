#![forbid(unsafe_code)]

//! Pure Live2D motion and expression playback contracts.
//!
//! This crate parses and evaluates model-declared motion3/exp3 clips without
//! loading Cubism Core, touching GPU resources, or driving a renderer. The
//! `bongocat-live2d` crate adapts these clips to its Core parameter owner.

mod error;
mod expression;
mod motion;

pub use error::{PlaybackError, PlaybackErrorCode};
pub use expression::{
    ExpressionBlendMode, ExpressionClip, ExpressionLayer, ExpressionParameter,
    evaluate_expression_parameter,
};
pub use motion::{
    MotionClip, MotionEvaluation, MotionModelSample, MotionParameterSample,
    MotionPartOpacitySample, MotionUserDataEvaluation, MotionUserDataEvent,
    MotionUserDataOccurrence,
};
