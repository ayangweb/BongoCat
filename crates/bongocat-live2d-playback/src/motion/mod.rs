//! A motion: what it is, what it asks the host to do, and what it looks like at
//! one instant.

use crate::{PlaybackError, PlaybackErrorCode};
use serde::Deserialize;
use std::time::Duration;

const TIME_TOLERANCE: f32 = 0.000_001;
const BEZIER_ITERATIONS: usize = 18;
const MODEL_EYE_BLINK_ID: &str = "EyeBlink";
const MODEL_LIP_SYNC_ID: &str = "LipSync";
const MODEL_OPACITY_ID: &str = "Opacity";
const MAX_USER_DATA_OCCURRENCES_PER_EVALUATION: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MotionCurveTarget {
    Model,
    Parameter,
    PartOpacity,
}

mod clip;
mod event;
mod parse_clip;
mod raw;
mod sample;
mod segment;
#[cfg(test)]
mod tests;
mod validate;
mod weight;

pub(crate) use raw::*;
pub(crate) use segment::*;
pub(crate) use validate::*;
pub(crate) use weight::*;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use event::{MotionUserDataEvaluation, MotionUserDataEvent, MotionUserDataOccurrence};
pub use sample::{
    MotionEvaluation, MotionModelSample, MotionParameterSample, MotionPartOpacitySample,
};

#[derive(Clone, Debug)]
pub struct MotionClip {
    pub(crate) duration_seconds: f32,
    pub(crate) looping: bool,
    pub(crate) fade_in_seconds: f32,
    pub(crate) fade_out_seconds: f32,
    pub(crate) curves: Vec<MotionCurve>,
    pub(crate) user_data: Vec<MotionUserDataEvent>,
}

#[derive(Clone, Debug)]
pub(crate) struct MotionCurve {
    target: MotionCurveTarget,
    pub(crate) id: String,
    pub(crate) fade_in_seconds: Option<f32>,
    pub(crate) fade_out_seconds: Option<f32>,
    pub(crate) initial: MotionPoint,
    pub(crate) segments: Vec<MotionSegment>,
}

impl MotionCurve {
    pub(crate) fn parse(
        raw: RawCurve,
        duration: f32,
    ) -> Result<(Self, usize, usize), PlaybackError> {
        if raw.id.trim().is_empty() {
            return invalid("curve Id must not be blank");
        }
        if let Some(value) = raw.fade_in_seconds {
            validate_fade(value, "curve fade in")?;
        }
        if let Some(value) = raw.fade_out_seconds {
            validate_fade(value, "curve fade out")?;
        }
        if raw.segments.len() < 2 || raw.segments.iter().any(|value| !value.is_finite()) {
            return invalid("curve Segments must start with a finite time/value point");
        }
        let initial = MotionPoint {
            time: raw.segments[0],
            value: raw.segments[1],
        };
        validate_time(initial.time, 0.0, duration, "initial point")?;
        let mut previous = initial;
        let mut index = 2usize;
        let mut segments = Vec::new();
        let mut point_count = 1usize;
        while index < raw.segments.len() {
            let code = raw.segments[index];
            if code.fract() != 0.0 {
                return invalid(format!("segment code at index {index} is not an integer"));
            }
            let (segment, width, added_points) = match code as i32 {
                0 | 2 | 3 => {
                    require_width(&raw.segments, index, 3)?;
                    let end = MotionPoint {
                        time: raw.segments[index + 1],
                        value: raw.segments[index + 2],
                    };
                    validate_time(end.time, previous.time, duration, "segment end")?;
                    let segment = match code as i32 {
                        0 => MotionSegment::Linear { end },
                        2 => MotionSegment::Stepped { end },
                        3 => MotionSegment::InverseStepped { end },
                        _ => unreachable!(),
                    };
                    (segment, 3, 1)
                }
                1 => {
                    require_width(&raw.segments, index, 7)?;
                    let control1 = MotionPoint {
                        time: raw.segments[index + 1],
                        value: raw.segments[index + 2],
                    };
                    let control2 = MotionPoint {
                        time: raw.segments[index + 3],
                        value: raw.segments[index + 4],
                    };
                    let end = MotionPoint {
                        time: raw.segments[index + 5],
                        value: raw.segments[index + 6],
                    };
                    validate_time(end.time, previous.time, duration, "Bezier end")?;
                    validate_time(control1.time, previous.time, end.time, "Bezier control 1")?;
                    validate_time(control2.time, previous.time, end.time, "Bezier control 2")?;
                    (
                        MotionSegment::Bezier {
                            control1,
                            control2,
                            end,
                        },
                        7,
                        3,
                    )
                }
                value => return invalid(format!("segment code {value} is unsupported")),
            };
            previous = segment.end();
            segments.push(segment);
            point_count += added_points;
            index += width;
        }
        let segment_count = segments.len();
        Ok((
            Self {
                target: match raw.target {
                    RawTarget::Model => MotionCurveTarget::Model,
                    RawTarget::Parameter => MotionCurveTarget::Parameter,
                    RawTarget::PartOpacity => MotionCurveTarget::PartOpacity,
                },
                id: raw.id,
                fade_in_seconds: raw.fade_in_seconds,
                fade_out_seconds: raw.fade_out_seconds,
                initial,
                segments,
            },
            segment_count,
            point_count,
        ))
    }

    pub(crate) fn evaluate(&self, time: f32) -> f32 {
        let mut start = self.initial;
        for segment in &self.segments {
            if time <= segment.end().time + TIME_TOLERANCE {
                return segment.evaluate(start, time);
            }
            start = segment.end();
        }
        start.value
    }

    pub(crate) fn weight(
        &self,
        elapsed: f32,
        duration: f32,
        looping: bool,
        default_fade_in: f32,
        default_fade_out: f32,
    ) -> f32 {
        let fade_in = self.fade_in_seconds.unwrap_or(default_fade_in);
        let fade_out = self.fade_out_seconds.unwrap_or(default_fade_out);
        let in_weight = fade_weight(elapsed, fade_in);
        let out_weight = if looping {
            1.0
        } else {
            fade_weight(duration - elapsed, fade_out)
        };
        (in_weight * out_weight).clamp(0.0, 1.0)
    }
}
