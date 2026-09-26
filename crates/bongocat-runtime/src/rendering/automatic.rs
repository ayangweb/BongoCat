//! The effects the model gets whether or not anything drove it.
//!
//! These are the reference breath and the eye blink, and they are periodic and
//! deterministic: the same elapsed time produces the same values, so a frame that
//! is replayed or compared across a model switch does not differ because a clock
//! was read twice. They are the last thing applied in a frame, after the motion,
//! the expression and the input, so nothing a user did is overwritten by them.

use super::*;

pub(crate) fn apply_automatic_effects(
    model: &mut Live2dModel,
    now: Duration,
) -> Result<(), RuntimeRenderErrorCode> {
    let (breath_time, blink) = automatic_effect_values(now);
    model
        .apply_automatic_effects(breath_time, blink)
        .map_err(|error| map_live2d_error(error, RuntimeRenderErrorCode::ModelEvaluationFailed))?;
    Ok(())
}

pub(crate) fn automatic_effect_values(now: Duration) -> (Duration, f32) {
    let blink_phase = now.as_secs_f64() % BLINK_PERIOD.as_secs_f64();
    let blink = if blink_phase < BLINK_CLOSED_DURATION.as_secs_f64() {
        -1.0
    } else {
        0.0
    };
    (now, blink)
}
