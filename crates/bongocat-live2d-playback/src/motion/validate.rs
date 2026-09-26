//! Refusing a motion that cannot be played.
//!
//! Every one of these is a check at the boundary rather than a clamp later. A
//! fade longer than the clip clamped to the clip's length is a motion that does
//! not do what its author wrote; refused, it is a file the user can be told about.

use super::*;

pub(crate) fn validate_time(
    time: f32,
    minimum: f32,
    maximum: f32,
    label: &str,
) -> Result<(), PlaybackError> {
    if !time.is_finite() || time + TIME_TOLERANCE < minimum || time > maximum + TIME_TOLERANCE {
        return invalid(format!(
            "{label} time {time} is outside [{minimum}, {maximum}]"
        ));
    }
    Ok(())
}

pub(crate) fn validate_fade(value: f32, label: &str) -> Result<(), PlaybackError> {
    if !value.is_finite() || value < 0.0 {
        return invalid(format!("{label} must be finite and non-negative"));
    }
    Ok(())
}

pub(crate) fn require_width(
    values: &[f32],
    index: usize,
    width: usize,
) -> Result<(), PlaybackError> {
    if values.len().saturating_sub(index) < width {
        return invalid(format!("segment at index {index} is truncated"));
    }
    Ok(())
}

pub(crate) fn invalid<T>(detail: impl Into<String>) -> Result<T, PlaybackError> {
    Err(PlaybackError::new(PlaybackErrorCode::MotionInvalid, detail))
}
