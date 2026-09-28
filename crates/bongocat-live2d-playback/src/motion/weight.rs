//! The fade and the weight a curve is scaled by.
//!
//! A fading motion's weight is a sine over the remaining time rather than a
//! linear ramp, because a linear fade reads as a machine switching the model off
//! and a sine reads as a motion ending. The fade's length is validated against
//! the clip's own duration at parse time, so a fade longer than the clip cannot
//! produce a weight that never reaches zero.

pub(crate) fn fade_weight(remaining: f32, fade_seconds: f32) -> f32 {
    if fade_seconds <= 0.0 {
        return 1.0;
    }
    let progress = (remaining / fade_seconds).clamp(0.0, 1.0);
    0.5 - 0.5 * (progress * std::f32::consts::PI).cos()
}

pub(crate) fn motion_weight(
    elapsed: f32,
    duration: f32,
    looping: bool,
    fade_in_seconds: f32,
    fade_out_seconds: f32,
) -> f32 {
    let fade_in = fade_weight(elapsed, fade_in_seconds);
    let fade_out = if looping {
        1.0
    } else {
        fade_weight(duration - elapsed, fade_out_seconds)
    };
    (fade_in * fade_out).clamp(0.0, 1.0)
}
