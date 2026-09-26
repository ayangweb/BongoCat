//! The form a motion is written in, before it is trusted.
//!
//! This is the parsed model3.json, kept separate from the clip because it is not
//! the same thing: a raw motion can be truncated, name a segment it does not
//! define, or carry a fade longer than the clip itself. All three are refused at
//! the boundary rather than producing a clip that evaluates to nothing.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotion {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "Meta")]
    pub(crate) meta: RawMeta,
    #[serde(rename = "Curves")]
    pub(crate) curves: Vec<RawCurve>,
    #[serde(rename = "UserData", default)]
    pub(crate) user_data: Vec<RawUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMeta {
    #[serde(rename = "Duration")]
    pub(crate) duration: f32,
    #[serde(rename = "Fps")]
    pub(crate) fps: f32,
    #[serde(rename = "Loop")]
    pub(crate) looping: bool,
    #[serde(rename = "AreBeziersRestricted")]
    pub(crate) _are_beziers_restricted: bool,
    #[serde(rename = "CurveCount")]
    pub(crate) curve_count: usize,
    #[serde(rename = "TotalSegmentCount")]
    pub(crate) total_segment_count: usize,
    #[serde(rename = "TotalPointCount")]
    pub(crate) total_point_count: usize,
    #[serde(rename = "UserDataCount")]
    pub(crate) user_data_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    pub(crate) total_user_data_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCurve {
    #[serde(rename = "Target")]
    pub(crate) target: RawTarget,
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Segments")]
    pub(crate) segments: Vec<f32>,
    #[serde(rename = "FadeInTime", default)]
    pub(crate) fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    pub(crate) fade_out_seconds: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) enum RawTarget {
    Model,
    Parameter,
    PartOpacity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawUserData {
    #[serde(rename = "Time")]
    pub(crate) time: f32,
    #[serde(rename = "Value")]
    pub(crate) value: String,
}
