use crate::{Live2dError, Live2dErrorCode};
use bongocat_model::CommittedModel;
use serde::Deserialize;
use std::{fs, time::Duration};

const TIME_TOLERANCE: f32 = 0.000_001;
const BEZIER_ITERATIONS: usize = 18;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionCurveTarget {
    Model,
    Parameter,
    PartOpacity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionParameterSample {
    pub id: String,
    pub value: f32,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvaluation {
    pub local_time: Duration,
    pub finished: bool,
    pub parameters: Vec<MotionParameterSample>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionApplyStatus {
    pub finished: bool,
    pub applied_parameter_count: usize,
}

#[derive(Clone, Debug)]
pub struct MotionClip {
    duration_seconds: f32,
    looping: bool,
    fade_in_seconds: f32,
    fade_out_seconds: f32,
    curves: Vec<MotionCurve>,
}

#[derive(Clone, Debug)]
struct MotionCurve {
    target: MotionCurveTarget,
    id: String,
    fade_in_seconds: Option<f32>,
    fade_out_seconds: Option<f32>,
    initial: MotionPoint,
    segments: Vec<MotionSegment>,
}

#[derive(Clone, Copy, Debug)]
struct MotionPoint {
    time: f32,
    value: f32,
}

#[derive(Clone, Copy, Debug)]
enum MotionSegment {
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMotion {
    #[serde(rename = "Version")]
    version: u32,
    #[serde(rename = "Meta")]
    meta: RawMeta,
    #[serde(rename = "Curves")]
    curves: Vec<RawCurve>,
    #[serde(rename = "UserData", default)]
    user_data: Vec<RawUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMeta {
    #[serde(rename = "Duration")]
    duration: f32,
    #[serde(rename = "Fps")]
    fps: f32,
    #[serde(rename = "Loop")]
    looping: bool,
    #[serde(rename = "AreBeziersRestricted")]
    _are_beziers_restricted: bool,
    #[serde(rename = "CurveCount")]
    curve_count: usize,
    #[serde(rename = "TotalSegmentCount")]
    total_segment_count: usize,
    #[serde(rename = "TotalPointCount")]
    total_point_count: usize,
    #[serde(rename = "UserDataCount")]
    user_data_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    total_user_data_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCurve {
    #[serde(rename = "Target")]
    target: RawTarget,
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Segments")]
    segments: Vec<f32>,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum RawTarget {
    Model,
    Parameter,
    PartOpacity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUserData {
    #[serde(rename = "Time")]
    time: f32,
    #[serde(rename = "Value")]
    value: String,
}

impl MotionClip {
    pub fn load(
        model: &CommittedModel,
        group_name: &str,
        motion_index: usize,
    ) -> Result<Self, Live2dError> {
        let group = model
            .index()
            .motion_groups
            .iter()
            .find(|group| group.name == group_name)
            .ok_or_else(|| {
                Live2dError::new(
                    Live2dErrorCode::MotionNotFound,
                    format!("motion group {group_name:?} does not exist"),
                )
            })?;
        let resource = group.motions.get(motion_index).ok_or_else(|| {
            Live2dError::new(
                Live2dErrorCode::MotionNotFound,
                format!("motion {group_name}[{motion_index}] does not exist"),
            )
        })?;
        let path = model.root().join(&resource.file);
        let bytes = fs::read(&path).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::ResourceIo,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        Self::from_slice(
            &bytes,
            resource.fade_in_seconds.map_or(1.0, |value| value.get()),
            resource.fade_out_seconds.map_or(1.0, |value| value.get()),
        )
        .map_err(|mut error| {
            error.detail = format!("{}: {}", path.display(), error.detail);
            error
        })
    }

    pub fn from_slice(
        bytes: &[u8],
        fade_in_seconds: f32,
        fade_out_seconds: f32,
    ) -> Result<Self, Live2dError> {
        validate_fade(fade_in_seconds, "motion fade in")?;
        validate_fade(fade_out_seconds, "motion fade out")?;
        let raw: RawMotion = serde_json::from_slice(bytes).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::MotionInvalid,
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
                return Err(Live2dError::new(
                    Live2dErrorCode::MotionInvalid,
                    "UserData.Time is outside the motion duration",
                ));
            }
            total.checked_add(entry.value.len()).ok_or_else(|| {
                Live2dError::new(Live2dErrorCode::MotionInvalid, "UserData size overflowed")
            })
        })?;
        if user_data_size != raw.meta.total_user_data_size {
            return invalid("Meta.TotalUserDataSize does not match UserData");
        }

        let mut total_segments = 0usize;
        let mut total_points = 0usize;
        let curves = raw
            .curves
            .into_iter()
            .map(|curve| {
                let (curve, segments, points) = MotionCurve::parse(curve, raw.meta.duration)?;
                total_segments = total_segments.checked_add(segments).ok_or_else(|| {
                    Live2dError::new(Live2dErrorCode::MotionInvalid, "segment count overflowed")
                })?;
                total_points = total_points.checked_add(points).ok_or_else(|| {
                    Live2dError::new(Live2dErrorCode::MotionInvalid, "point count overflowed")
                })?;
                Ok(curve)
            })
            .collect::<Result<Vec<_>, Live2dError>>()?;
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
        })
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.duration_seconds)
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn fade_out_duration(&self) -> Duration {
        Duration::from_secs_f32(self.fade_out_seconds)
    }

    pub fn fade_out_weight(&self, elapsed: Duration) -> f32 {
        1.0 - fade_weight(elapsed.as_secs_f32(), self.fade_out_seconds)
    }

    pub fn evaluate(&self, elapsed: Duration) -> MotionEvaluation {
        let elapsed_seconds = elapsed.as_secs_f32();
        let finished = !self.looping && elapsed_seconds >= self.duration_seconds;
        let local_seconds = if self.looping && self.duration_seconds > 0.0 {
            elapsed_seconds.rem_euclid(self.duration_seconds)
        } else {
            elapsed_seconds.min(self.duration_seconds)
        };
        let parameters = self
            .curves
            .iter()
            .filter(|curve| curve.target == MotionCurveTarget::Parameter)
            .map(|curve| MotionParameterSample {
                id: curve.id.clone(),
                value: curve.evaluate(local_seconds),
                weight: curve.weight(
                    elapsed_seconds,
                    self.duration_seconds,
                    self.looping,
                    self.fade_in_seconds,
                    self.fade_out_seconds,
                ),
            })
            .collect();
        MotionEvaluation {
            local_time: Duration::from_secs_f32(local_seconds),
            finished,
            parameters,
        }
    }
}

impl MotionCurve {
    fn parse(raw: RawCurve, duration: f32) -> Result<(Self, usize, usize), Live2dError> {
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

    fn evaluate(&self, time: f32) -> f32 {
        let mut start = self.initial;
        for segment in &self.segments {
            if time <= segment.end().time + TIME_TOLERANCE {
                return segment.evaluate(start, time);
            }
            start = segment.end();
        }
        start.value
    }

    fn weight(
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

impl MotionSegment {
    fn end(self) -> MotionPoint {
        match self {
            Self::Linear { end }
            | Self::Bezier { end, .. }
            | Self::Stepped { end }
            | Self::InverseStepped { end } => end,
        }
    }

    fn evaluate(self, start: MotionPoint, time: f32) -> f32 {
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

fn solve_bezier_time(
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

fn cubic(start: f32, control1: f32, control2: f32, end: f32, time: f32) -> f32 {
    let inverse = 1.0 - time;
    inverse * inverse * inverse * start
        + 3.0 * inverse * inverse * time * control1
        + 3.0 * inverse * time * time * control2
        + time * time * time * end
}

fn normalized_time(start: f32, end: f32, time: f32) -> f32 {
    if end <= start + f32::EPSILON {
        1.0
    } else {
        ((time - start) / (end - start)).clamp(0.0, 1.0)
    }
}

fn fade_weight(remaining: f32, fade_seconds: f32) -> f32 {
    if fade_seconds <= 0.0 {
        return 1.0;
    }
    let progress = (remaining / fade_seconds).clamp(0.0, 1.0);
    0.5 - 0.5 * (progress * std::f32::consts::PI).cos()
}

fn validate_time(time: f32, minimum: f32, maximum: f32, label: &str) -> Result<(), Live2dError> {
    if !time.is_finite() || time + TIME_TOLERANCE < minimum || time > maximum + TIME_TOLERANCE {
        return invalid(format!(
            "{label} time {time} is outside [{minimum}, {maximum}]"
        ));
    }
    Ok(())
}

fn validate_fade(value: f32, label: &str) -> Result<(), Live2dError> {
    if !value.is_finite() || value < 0.0 {
        return invalid(format!("{label} must be finite and non-negative"));
    }
    Ok(())
}

fn require_width(values: &[f32], index: usize, width: usize) -> Result<(), Live2dError> {
    if values.len().saturating_sub(index) < width {
        return invalid(format!("segment at index {index} is truncated"));
    }
    Ok(())
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, Live2dError> {
    Err(Live2dError::new(Live2dErrorCode::MotionInvalid, detail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    fn preset_model(id: &str) -> CommittedModel {
        PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model")
    }

    #[test]
    fn all_preset_motions_parse_and_loop() {
        for model_id in ["standard", "keyboard", "gamepad"] {
            let model = preset_model(model_id);
            for group in &model.index().motion_groups {
                for index in 0..group.motions.len() {
                    let clip = MotionClip::load(&model, &group.name, index).expect("motion clip");
                    assert!(clip.is_looping());
                    assert!(
                        !clip
                            .evaluate(clip.duration() * 3 + Duration::from_millis(20))
                            .finished
                    );
                    assert_eq!(clip.evaluate(Duration::ZERO).parameters.len(), 2);
                }
            }
        }
    }

    #[test]
    fn evaluates_all_segment_kinds_and_natural_completion() {
        let json = br#"{
          "Version":3,
          "Meta":{"Duration":4.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":false,
            "CurveCount":1,"TotalSegmentCount":4,"TotalPointCount":7,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[{"Target":"Parameter","Id":"ParamTest","Segments":[
            0,0, 0,1,1, 1,1.25,1,1.75,3,2,4, 2,3,7, 3,4,9
          ]}]
        }"#;
        let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("synthetic motion");
        assert!(
            (clip.evaluate(Duration::from_millis(500)).parameters[0].value - 0.5).abs() < 0.001
        );
        let bezier = clip.evaluate(Duration::from_millis(1500)).parameters[0].value;
        assert!((1.0..4.0).contains(&bezier));
        assert_eq!(
            clip.evaluate(Duration::from_millis(2500)).parameters[0].value,
            4.0
        );
        assert_eq!(
            clip.evaluate(Duration::from_millis(3500)).parameters[0].value,
            9.0
        );
        assert!(clip.evaluate(Duration::from_secs(4)).finished);
    }

    #[test]
    fn applies_sine_fade_and_wraps_loop_time() {
        let model = preset_model("standard");
        let clip = MotionClip::load(&model, "CAT_motion", 0).expect("preset motion");
        let at_start = clip.evaluate(Duration::ZERO);
        assert_eq!(at_start.parameters[0].weight, 1.0);
        let wrapped = clip.evaluate(clip.duration() + Duration::from_millis(100));
        let initial = clip.evaluate(Duration::from_millis(100));
        assert!((wrapped.parameters[0].value - initial.parameters[0].value).abs() < 0.001);
    }

    #[test]
    fn explicit_fade_out_preserves_the_first_frame_and_reaches_zero() {
        let json = br#"{
          "Version":3,
          "Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
            "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":2,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,1,0,2,1]}]
        }"#;
        let clip = MotionClip::from_slice(json, 0.0, 1.0).expect("synthetic motion");

        assert_eq!(clip.fade_out_duration(), Duration::from_secs(1));
        assert_eq!(clip.fade_out_weight(Duration::ZERO), 1.0);
        assert!((clip.fade_out_weight(Duration::from_millis(500)) - 0.5).abs() < 0.0001);
        assert_eq!(clip.fade_out_weight(Duration::from_secs(1)), 0.0);
        assert_eq!(clip.fade_out_weight(Duration::from_secs(2)), 0.0);
    }

    #[test]
    fn rejects_bad_meta_and_truncated_segments() {
        let bad_count = br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
            "CurveCount":2,"TotalSegmentCount":1,"TotalPointCount":2,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,0,0,1,1]}]
        }"#;
        assert_eq!(
            MotionClip::from_slice(bad_count, 0.0, 0.0)
                .expect_err("bad count")
                .code,
            Live2dErrorCode::MotionInvalid
        );

        let truncated = br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
            "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":4,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,0,1,0.2,0.3]}]
        }"#;
        assert_eq!(
            MotionClip::from_slice(truncated, 0.0, 0.0)
                .expect_err("truncated segment")
                .code,
            Live2dErrorCode::MotionInvalid
        );
    }
}
