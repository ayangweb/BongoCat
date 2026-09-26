//! The `Model3.json` schema a package declares.
//!
//! These are the types a package is written against, in the terms the
//! specification uses. They are read from JSON and handed to the renderer; they
//! never touch the filesystem themselves.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelPackageIndex {
    pub schema_version: u32,
    pub model_version: u32,
    pub entry: String,
    pub moc: String,
    pub textures: Vec<ImageResource>,
    pub display_info: Option<String>,
    pub expressions: Vec<NamedResource>,
    pub motion_groups: Vec<MotionGroup>,
    pub groups: Vec<ModelGroup>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub user_data: Option<String>,
    pub package_file_count: usize,
    pub package_total_bytes: u64,
    pub unreferenced_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsVector {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsRange {
    pub minimum: f64,
    pub default: f64,
    pub maximum: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsChannel {
    X,
    Y,
    Angle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsInput {
    pub parameter_id: String,
    pub weight: f64,
    pub channel: PhysicsChannel,
    pub reflect: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsOutput {
    pub parameter_id: String,
    pub vertex_index: usize,
    pub scale: f64,
    pub weight: f64,
    pub channel: PhysicsChannel,
    pub reflect: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsVertex {
    pub position: PhysicsVector,
    pub mobility: f64,
    pub delay: f64,
    pub acceleration: f64,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsSetting {
    pub inputs: Vec<PhysicsInput>,
    pub outputs: Vec<PhysicsOutput>,
    pub vertices: Vec<PhysicsVertex>,
    pub normalization_position: PhysicsRange,
    pub normalization_angle: PhysicsRange,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsDefinition {
    /// The authored physics step rate. `0.0` means the legacy resource omitted
    /// `Meta.Fps`; the runtime then uses the current frame delta instead of
    /// inventing a fixed rate.
    pub fps: f64,
    pub gravity: PhysicsVector,
    pub wind: PhysicsVector,
    pub settings: Vec<PhysicsSetting>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImageResource {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NamedResource {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MotionGroup {
    pub name: String,
    pub motions: Vec<MotionResource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MotionResource {
    pub file: String,
    pub sound: Option<String>,
    pub fade_in_seconds: Option<FiniteSeconds>,
    pub fade_out_seconds: Option<FiniteSeconds>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelGroup {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FiniteSeconds(pub(crate) f32);

impl Eq for FiniteSeconds {}

impl FiniteSeconds {
    pub const fn get(self) -> f32 {
        self.0
    }
}
