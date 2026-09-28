//! The private mirror of the schema, as `serde` hands it over.
//!
//! `Model3.json` is written by other tools and is not always well-formed: it
//! spells optional fields as null, numbers as strings, and counts as floats.
//! These types absorb that, so everything above them sees the schema in the
//! specification's own terms and one place owns the tolerance.

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct ModelDefinition {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "FileReferences")]
    pub(crate) files: FileReferences,
    #[serde(rename = "Groups", default)]
    pub(crate) groups: Vec<RawModelGroup>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileReferences {
    #[serde(rename = "Moc")]
    pub(crate) moc: String,
    #[serde(rename = "Textures", default)]
    pub(crate) textures: Vec<String>,
    #[serde(rename = "DisplayInfo", default)]
    pub(crate) display_info: Option<String>,
    #[serde(rename = "Expressions", default)]
    pub(crate) expressions: Vec<RawNamedResource>,
    #[serde(rename = "Motions", default)]
    pub(crate) motions: BTreeMap<String, Vec<RawMotionResource>>,
    #[serde(rename = "Physics", default)]
    pub(crate) physics: Option<String>,
    #[serde(rename = "Pose", default)]
    pub(crate) pose: Option<String>,
    #[serde(rename = "UserData", default)]
    pub(crate) user_data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawNamedResource {
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "File")]
    pub(crate) file: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawMotionResource {
    #[serde(rename = "File")]
    pub(crate) file: String,
    #[serde(rename = "Sound", default)]
    pub(crate) sound: Option<String>,
    #[serde(rename = "FadeInTime", default)]
    pub(crate) fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    pub(crate) fade_out_seconds: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawModelGroup {
    #[serde(rename = "Target")]
    pub(crate) target: String,
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "Ids", default)]
    pub(crate) ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDisplayInfo {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "Parameters", default)]
    pub(crate) parameters: Vec<RawDisplayInfoParameter>,
    #[serde(rename = "ParameterGroups", default)]
    pub(crate) parameter_groups: Vec<RawDisplayInfoParameterGroup>,
    #[serde(rename = "Parts", default)]
    pub(crate) parts: Vec<RawDisplayInfoPart>,
    /// Official Cubism cdi3.json field (Cubism 5 SDK): each inner array binds
    /// parameters that are driven together, e.g. by one pointer axis.
    #[serde(rename = "CombinedParameters", default)]
    pub(crate) combined_parameters: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDisplayInfoParameter {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "GroupId")]
    pub(crate) group_id: String,
    #[serde(rename = "Name")]
    pub(crate) _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDisplayInfoParameterGroup {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "GroupId")]
    pub(crate) _group_id: String,
    #[serde(rename = "Name")]
    pub(crate) _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDisplayInfoPart {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Name")]
    pub(crate) _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawExpressionResource {
    #[serde(rename = "Type")]
    pub(crate) kind: String,
    #[serde(rename = "FadeInTime", default)]
    pub(crate) fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    pub(crate) fade_out_seconds: Option<f32>,
    #[serde(rename = "Parameters")]
    pub(crate) parameters: Vec<RawExpressionParameter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawExpressionParameter {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Value")]
    pub(crate) value: f32,
    #[serde(rename = "Blend", default)]
    pub(crate) blend: Option<RawExpressionBlend>,
}

#[derive(Debug, Deserialize)]
pub(crate) enum RawExpressionBlend {
    Add,
    Multiply,
    Overwrite,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotionResourceFile {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "Meta")]
    pub(crate) meta: RawMotionResourceMeta,
    #[serde(rename = "Curves")]
    pub(crate) curves: Vec<RawMotionResourceCurve>,
    #[serde(rename = "UserData", default)]
    pub(crate) user_data: Vec<RawMotionResourceUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotionResourceMeta {
    #[serde(rename = "Duration")]
    pub(crate) duration: f32,
    #[serde(rename = "Fps")]
    pub(crate) fps: f32,
    #[serde(rename = "Loop")]
    pub(crate) _looping: bool,
    #[serde(rename = "AreBeziersRestricted")]
    pub(crate) _are_beziers_restricted: bool,
    #[serde(rename = "CurveCount")]
    pub(crate) curve_count: usize,
    #[serde(rename = "TotalSegmentCount")]
    pub(crate) _total_segment_count: usize,
    #[serde(rename = "TotalPointCount")]
    pub(crate) _total_point_count: usize,
    #[serde(rename = "UserDataCount")]
    pub(crate) user_data_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    pub(crate) total_user_data_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotionResourceUserData {
    #[serde(rename = "Time")]
    pub(crate) time: f32,
    #[serde(rename = "Value")]
    pub(crate) value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotionResourceCurve {
    #[serde(rename = "Target")]
    pub(crate) target: RawMotionResourceTarget,
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Segments")]
    pub(crate) segments: Vec<f32>,
    #[serde(rename = "FadeInTime", default)]
    pub(crate) fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    pub(crate) fade_out_seconds: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPoseResource {
    #[serde(rename = "Type")]
    pub(crate) kind: String,
    #[serde(rename = "FadeInTime", default)]
    pub(crate) fade_in_seconds: Option<f32>,
    #[serde(rename = "Groups")]
    pub(crate) groups: Vec<Vec<RawPosePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPosePart {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Link", default)]
    pub(crate) links: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsResource {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "Meta")]
    pub(crate) meta: RawPhysicsMeta,
    #[serde(rename = "PhysicsSettings")]
    pub(crate) settings: Vec<RawPhysicsSetting>,
}

pub(crate) fn deserialize_physics_fps<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<f64>::deserialize(deserializer)? {
        Some(fps) => Ok(Some(fps)),
        None => Err(<D::Error as serde::de::Error>::custom(
            "physics3 Meta.Fps must be a number",
        )),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsMeta {
    #[serde(rename = "PhysicsSettingCount")]
    pub(crate) setting_count: usize,
    #[serde(rename = "TotalInputCount")]
    pub(crate) input_count: usize,
    #[serde(rename = "TotalOutputCount")]
    pub(crate) output_count: usize,
    #[serde(rename = "VertexCount")]
    pub(crate) vertex_count: usize,
    /// Legacy SDK exports may omit this field; the runtime then uses the
    /// incoming frame delta instead of inventing a fixed step rate.
    #[serde(default, rename = "Fps", deserialize_with = "deserialize_physics_fps")]
    pub(crate) fps: Option<f64>,
    #[serde(rename = "EffectiveForces")]
    pub(crate) effective_forces: RawPhysicsForces,
    #[serde(rename = "PhysicsDictionary")]
    pub(crate) dictionary: Vec<RawPhysicsDictionaryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsForces {
    #[serde(rename = "Gravity")]
    pub(crate) gravity: RawPhysicsVector,
    #[serde(rename = "Wind")]
    pub(crate) wind: RawPhysicsVector,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsVector {
    #[serde(rename = "X")]
    pub(crate) x: f64,
    #[serde(rename = "Y")]
    pub(crate) y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsDictionaryEntry {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Name")]
    pub(crate) _name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsSetting {
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Input")]
    pub(crate) inputs: Vec<RawPhysicsInput>,
    #[serde(rename = "Output")]
    pub(crate) outputs: Vec<RawPhysicsOutput>,
    #[serde(rename = "Vertices")]
    pub(crate) vertices: Vec<RawPhysicsVertex>,
    #[serde(rename = "Normalization")]
    pub(crate) normalization: RawPhysicsNormalization,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsInput {
    #[serde(rename = "Source")]
    pub(crate) source: RawPhysicsTarget,
    #[serde(rename = "Weight")]
    pub(crate) weight: f64,
    #[serde(rename = "Type")]
    pub(crate) kind: RawPhysicsChannel,
    #[serde(rename = "Reflect")]
    pub(crate) reflect: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsOutput {
    #[serde(rename = "Destination")]
    pub(crate) destination: RawPhysicsTarget,
    #[serde(rename = "VertexIndex")]
    pub(crate) vertex_index: usize,
    #[serde(rename = "Scale")]
    pub(crate) scale: f64,
    #[serde(rename = "Weight")]
    pub(crate) weight: f64,
    #[serde(rename = "Type")]
    pub(crate) kind: RawPhysicsChannel,
    #[serde(rename = "Reflect")]
    pub(crate) reflect: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) enum RawPhysicsChannel {
    X,
    Y,
    Angle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsTarget {
    #[serde(rename = "Target")]
    pub(crate) _target: RawPhysicsTargetKind,
    #[serde(rename = "Id")]
    pub(crate) id: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) enum RawPhysicsTargetKind {
    Parameter,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsNormalization {
    #[serde(rename = "Position")]
    pub(crate) position: RawPhysicsRange,
    #[serde(rename = "Angle")]
    pub(crate) angle: RawPhysicsRange,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsRange {
    #[serde(rename = "Minimum")]
    pub(crate) minimum: f64,
    #[serde(rename = "Default")]
    pub(crate) default: f64,
    #[serde(rename = "Maximum")]
    pub(crate) maximum: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPhysicsVertex {
    #[serde(rename = "Position")]
    pub(crate) position: RawPhysicsVector,
    #[serde(rename = "Mobility")]
    pub(crate) mobility: f64,
    #[serde(rename = "Delay")]
    pub(crate) delay: f64,
    #[serde(rename = "Acceleration")]
    pub(crate) acceleration: f64,
    #[serde(rename = "Radius")]
    pub(crate) radius: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawModelUserDataResource {
    #[serde(rename = "Version")]
    pub(crate) version: u32,
    #[serde(rename = "Meta")]
    pub(crate) meta: RawModelUserDataMeta,
    #[serde(rename = "UserData")]
    pub(crate) entries: Vec<RawModelUserDataEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawModelUserDataMeta {
    #[serde(rename = "UserDataCount")]
    pub(crate) entry_count: usize,
    #[serde(rename = "TotalUserDataSize")]
    pub(crate) total_value_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawModelUserDataEntry {
    #[serde(rename = "Target")]
    pub(crate) target: RawModelUserDataTarget,
    #[serde(rename = "Id")]
    pub(crate) id: String,
    #[serde(rename = "Value")]
    pub(crate) value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RawModelUserDataTarget {
    ArtMesh,
}

#[derive(Debug, Deserialize)]
pub(crate) enum RawMotionResourceTarget {
    Model,
    Parameter,
    PartOpacity,
}
