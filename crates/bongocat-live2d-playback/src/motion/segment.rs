//! Where in a segment a time falls, and what it weighs.
//!
//! A Cubism curve segment is a cubic Bézier in *time* as well as in value, which
//! is why the time is solved before the value is read: reading the value at the
//! wrong time is a motion that is subtly wrong everywhere rather than obviously
//! wrong somewhere. The solve is monotone and clamped, so a segment that starts
//! and ends at the same time cannot produce a division by zero.

use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionPoint {
    pub(crate) time: f32,
    pub(crate) value: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MotionSegment {
    Linear {
        end: MotionPoint,
    },
    Bezier {
        control1: MotionPoint,
        control2: MotionPoint,
        end: MotionPoint,
    },
    Stepped {
        end: MotionPoint,
    },
    InverseStepped {
        end: MotionPoint,
    },
}

impl MotionSegment {
    pub(crate) fn end(self) -> MotionPoint {
        match self {
            Self::Linear { end }
            | Self::Bezier { end, .. }
            | Self::Stepped { end }
            | Self::InverseStepped { end } => end,
        }
    }

    pub(crate) fn evaluate(self, start: MotionPoint, time: f32) -> f32 {
        match self {
            Self::Linear { end } => {
                let progress = normalized_time(start.time, end.time, time);
                start.value + (end.value - start.value) * progress
            }
            Self::Bezier {
                control1,
                control2,
                end,
            } => {
                let progress = solve_bezier_time(start, control1, control2, end, time);
                cubic(
                    start.value,
                    control1.value,
                    control2.value,
                    end.value,
                    progress,
                )
            }
            Self::Stepped { .. } => start.value,
            Self::InverseStepped { end } => end.value,
        }
    }
}

pub(crate) fn solve_bezier_time(
    start: MotionPoint,
    control1: MotionPoint,
    control2: MotionPoint,
    end: MotionPoint,
    time: f32,
) -> f32 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..BEZIER_ITERATIONS {
        let middle = (low + high) * 0.5;
        if cubic(start.time, control1.time, control2.time, end.time, middle) < time {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) * 0.5
}

pub(crate) fn cubic(start: f32, control1: f32, control2: f32, end: f32, time: f32) -> f32 {
    let inverse = 1.0 - time;
    inverse * inverse * inverse * start
        + 3.0 * inverse * inverse * time * control1
        + 3.0 * inverse * time * time * control2
        + time * time * time * end
}

pub(crate) fn normalized_time(start: f32, end: f32, time: f32) -> f32 {
    if end <= start + f32::EPSILON {
        1.0
    } else {
        ((time - start) / (end - start)).clamp(0.0, 1.0)
    }
}
