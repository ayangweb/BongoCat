//! What one instant of a motion looks like once evaluated.
//!
//! Three shapes rather than one, because they are weighted differently: a
//! parameter sample is already scaled by the curve's weight, a part-opacity
//! sample is not, and a model sample is whatever the motion wanted regardless of
//! both. Merging them would mean every consumer had to know which rule applied,
//! and a part opacity weighted twice is invisible while a model curve not
//! weighted at all is a model that ignores the motion.

use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct MotionParameterSample {
    pub id: String,
    pub value: f32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionPartOpacitySample {
    pub id: String,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotionModelSample {
    pub eye_blink: Option<f32>,
    pub lip_sync: Option<f32>,
    pub opacity: Option<f32>,
    pub effect_weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvaluation {
    pub local_time: Duration,
    pub finished: bool,
    pub model: MotionModelSample,
    pub parameters: Vec<MotionParameterSample>,
    pub part_opacities: Vec<MotionPartOpacitySample>,
}
