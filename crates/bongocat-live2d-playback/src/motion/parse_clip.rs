//! Turning a raw motion into a clip that can be trusted.
//!
//! This is the only place the raw form is read. Everything it refuses — a meta
//! that does not describe a playable clip, a segment that is truncated, a fade
//! longer than the clip — is refused here, so a clip that exists is a clip that
//! can be evaluated.

use super::*;

impl MotionClip {
    pub fn from_slice(
        bytes: &[u8],
        fade_in_seconds: f32,
        fade_out_seconds: f32,
    ) -> Result<Self, PlaybackError> {
        validate_fade(fade_in_seconds, "motion fade in")?;
        validate_fade(fade_out_seconds, "motion fade out")?;
        let raw: RawMotion = serde_json::from_slice(bytes).map_err(|error| {
            PlaybackError::new(
                PlaybackErrorCode::MotionInvalid,
                format!("motion3 JSON is invalid: {error}"),
            )
        })?;
        if raw.version != 3 {
            return invalid(format!("motion3 version {} is not supported", raw.version));
        }
        if !raw.meta.duration.is_finite() || raw.meta.duration < 0.0 {
            return invalid("Meta.Duration must be finite and non-negative");
        }
        if !raw.meta.fps.is_finite() || raw.meta.fps <= 0.0 {
            return invalid("Meta.Fps must be finite and positive");
        }
        if raw.meta.curve_count != raw.curves.len() {
            return invalid("Meta.CurveCount does not match Curves");
        }
        if raw.meta.user_data_count != raw.user_data.len() {
            return invalid("Meta.UserDataCount does not match UserData");
        }
        let user_data_size = raw.user_data.iter().try_fold(0usize, |total, entry| {
            if !entry.time.is_finite()
                || entry.time < 0.0
                || entry.time > raw.meta.duration + TIME_TOLERANCE
            {
                return Err(PlaybackError::new(
                    PlaybackErrorCode::MotionInvalid,
                    "UserData.Time is outside the motion duration",
                ));
            }
            total.checked_add(entry.value.len()).ok_or_else(|| {
                PlaybackError::new(PlaybackErrorCode::MotionInvalid, "UserData size overflowed")
            })
        })?;
        if user_data_size != raw.meta.total_user_data_size {
            return invalid("Meta.TotalUserDataSize does not match UserData");
        }
        let user_data = raw
            .user_data
            .into_iter()
            .map(|entry| {
                let local_time = Duration::try_from_secs_f32(entry.time).map_err(|_| {
                    PlaybackError::new(
                        PlaybackErrorCode::MotionInvalid,
                        "UserData.Time cannot be represented by the runtime clock",
                    )
                })?;
                Ok(MotionUserDataEvent {
                    local_time,
                    value: entry.value,
                })
            })
            .collect::<Result<Vec<_>, PlaybackError>>()?;

        let mut total_segments = 0usize;
        let mut total_points = 0usize;
        let curves = raw
            .curves
            .into_iter()
            .map(|curve| {
                let (curve, segments, points) = MotionCurve::parse(curve, raw.meta.duration)?;
                total_segments = total_segments.checked_add(segments).ok_or_else(|| {
                    PlaybackError::new(PlaybackErrorCode::MotionInvalid, "segment count overflowed")
                })?;
                total_points = total_points.checked_add(points).ok_or_else(|| {
                    PlaybackError::new(PlaybackErrorCode::MotionInvalid, "point count overflowed")
                })?;
                Ok(curve)
            })
            .collect::<Result<Vec<_>, PlaybackError>>()?;
        if total_segments != raw.meta.total_segment_count
            || total_points != raw.meta.total_point_count
        {
            return invalid("Meta segment or point totals do not match Curves");
        }
        Ok(Self {
            duration_seconds: raw.meta.duration,
            looping: raw.meta.looping,
            fade_in_seconds,
            fade_out_seconds,
            curves,
            user_data,
        })
    }
}
