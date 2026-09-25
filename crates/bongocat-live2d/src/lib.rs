use bongocat_live2d_render::{RenderResourceError, prepare_render_resources};
use bongocat_model::CommittedModel;
use bongocat_render::{RenderResources, RenderSnapshot, TextureAsset};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    sync::Arc,
    time::Duration,
};

use bongocat_live2d_playback::{
    ExpressionClip, ExpressionLayer, MotionClip, PlaybackError, PlaybackErrorCode,
    evaluate_expression_parameter,
};

mod core;
mod core_log;
mod physics;
mod sys;

pub use core_log::{CoreLogError, CoreLogHandle, CoreLogReporter, CoreLogStats};

pub const CUBISM_SDK_RELEASE: &str = "5-r.5";
pub const CUBISM_CORE_VERSION: u32 = 0x0600_0001;
pub const CUBISM_LATEST_MOC_VERSION: u32 = 6;
const MAX_MODEL_EFFECT_TARGETS: usize = 64;
// These are the fixed Cubism Framework breath parameters used by the
// Bongo-Cat-Mver reference. They are intentionally independent of model3
// groups: compatible models use the conventional IDs even when their model3
// omits a Breath group.
const REFERENCE_BREATH_TARGETS: [(&str, f32, f32, f32); 5] = [
    ("ParamAngleX", 0.0, 15.0, 6.5345),
    ("ParamAngleY", 0.0, 8.0, 3.5345),
    ("ParamAngleZ", 0.0, 10.0, 5.5345),
    ("ParamBodyAngleX", 0.0, 4.0, 15.5345),
    ("ParamBreath", 0.5, 0.5, 3.2345),
];
const AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT: f32 = 0.5;
const GENERIC_BREATH_PERIOD: f32 = 4.0;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
pub enum ProductParameter {
    AngleX,
    AngleY,
    AngleZ,
    EyeBallX,
    EyeBallY,
    LeftHandDown,
    RightHandDown,
    MouseX,
    MouseY,
    MouseLeftDown,
    MouseRightDown,
    StickLeftDown,
    StickRightDown,
    StickShowLeftHand,
    StickShowRightHand,
    StickLeftX,
    StickLeftY,
    StickRightX,
    StickRightY,
}

impl ProductParameter {
    pub const ALL: [Self; Self::COUNT] = [
        Self::AngleX,
        Self::AngleY,
        Self::AngleZ,
        Self::EyeBallX,
        Self::EyeBallY,
        Self::LeftHandDown,
        Self::RightHandDown,
        Self::MouseX,
        Self::MouseY,
        Self::MouseLeftDown,
        Self::MouseRightDown,
        Self::StickLeftDown,
        Self::StickRightDown,
        Self::StickShowLeftHand,
        Self::StickShowRightHand,
        Self::StickLeftX,
        Self::StickLeftY,
        Self::StickRightX,
        Self::StickRightY,
    ];
    pub(crate) const COUNT: usize = 19;

    pub const fn id(self) -> &'static str {
        match self {
            Self::AngleX => "ParamAngleX",
            Self::AngleY => "ParamAngleY",
            Self::AngleZ => "ParamAngleZ",
            Self::EyeBallX => "ParamEyeBallX",
            Self::EyeBallY => "ParamEyeBallY",
            Self::LeftHandDown => "CatParamLeftHandDown",
            Self::RightHandDown => "CatParamRightHandDown",
            Self::MouseX => "ParamMouseX",
            Self::MouseY => "ParamMouseY",
            Self::MouseLeftDown => "ParamMouseLeftDown",
            Self::MouseRightDown => "ParamMouseRightDown",
            Self::StickLeftDown => "CatParamStickLeftDown",
            Self::StickRightDown => "CatParamStickRightDown",
            Self::StickShowLeftHand => "CatParamStickShowLeftHand",
            Self::StickShowRightHand => "CatParamStickShowRightHand",
            Self::StickLeftX => "CatParamStickLX",
            Self::StickLeftY => "CatParamStickLY",
            Self::StickRightX => "CatParamStickRX",
            Self::StickRightY => "CatParamStickRY",
        }
    }

    pub(crate) const fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterRange {
    pub minimum: f32,
    pub maximum: f32,
    pub default: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterUpdate {
    Unsupported,
    Applied { value: f32, clamped: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Live2dErrorCode {
    CoreVersionMismatch,
    EmptyMoc,
    InvalidCoreArray,
    InvalidCoreValue,
    MocConsistencyFailed,
    MocReviveFailed,
    ModelInitializeFailed,
    ModelMemoryInvalid,
    ResourceIo,
    TextureIndexInvalid,
    ParameterValueInvalid,
    MotionInvalid,
    MotionNotFound,
    ExpressionInvalid,
    ExpressionNotFound,
    UnsupportedBlendMode,
}

impl Live2dErrorCode {
    pub const ALL: [Self; 16] = [
        Self::CoreVersionMismatch,
        Self::EmptyMoc,
        Self::InvalidCoreArray,
        Self::InvalidCoreValue,
        Self::MocConsistencyFailed,
        Self::MocReviveFailed,
        Self::ModelInitializeFailed,
        Self::ModelMemoryInvalid,
        Self::ResourceIo,
        Self::TextureIndexInvalid,
        Self::ParameterValueInvalid,
        Self::MotionInvalid,
        Self::MotionNotFound,
        Self::ExpressionInvalid,
        Self::ExpressionNotFound,
        Self::UnsupportedBlendMode,
    ];

    /// Stable, path-free identifier for diagnostics and typed callers.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CoreVersionMismatch => "core_version_mismatch",
            Self::EmptyMoc => "empty_moc",
            Self::InvalidCoreArray => "invalid_core_array",
            Self::InvalidCoreValue => "invalid_core_value",
            Self::MocConsistencyFailed => "moc_consistency_failed",
            Self::MocReviveFailed => "moc_revive_failed",
            Self::ModelInitializeFailed => "model_initialize_failed",
            Self::ModelMemoryInvalid => "model_memory_invalid",
            Self::ResourceIo => "resource_io",
            Self::TextureIndexInvalid => "texture_index_invalid",
            Self::ParameterValueInvalid => "parameter_value_invalid",
            Self::MotionInvalid => "motion_invalid",
            Self::MotionNotFound => "motion_not_found",
            Self::ExpressionInvalid => "expression_invalid",
            Self::ExpressionNotFound => "expression_not_found",
            Self::UnsupportedBlendMode => "unsupported_blend_mode",
        }
    }
}

impl fmt::Display for Live2dErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct Live2dError {
    pub code: Live2dErrorCode,
    pub detail: String,
}

impl Live2dError {
    pub(crate) fn new(code: Live2dErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl From<PlaybackError> for Live2dError {
    fn from(error: PlaybackError) -> Self {
        let code = match error.code {
            PlaybackErrorCode::ExpressionInvalid => Live2dErrorCode::ExpressionInvalid,
            PlaybackErrorCode::MotionInvalid => Live2dErrorCode::MotionInvalid,
        };
        Self::new(code, error.detail)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionApplyStatus {
    pub finished: bool,
    pub applied_parameter_count: usize,
    pub applied_part_opacity_count: usize,
    pub applied_eye_blink_count: usize,
    pub applied_lip_sync_count: usize,
    pub model_opacity_applied: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExpressionApplyStatus {
    pub applied_parameter_count: usize,
}

pub struct Live2dModel {
    resources: Arc<RenderResources>,
    motions: BTreeMap<String, Vec<MotionClip>>,
    expressions: BTreeMap<String, ExpressionClip>,
    breath_parameter_ids: Vec<String>,
    eye_blink_parameter_ids: Vec<String>,
    lip_sync_parameter_ids: Vec<String>,
    physics: Option<physics::PhysicsRuntime>,
    model_opacity: f32,
    core: core::CoreModel,
}

fn load_motion_clip(
    model: &CommittedModel,
    group_name: &str,
    motion_index: usize,
) -> Result<MotionClip, Live2dError> {
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
    MotionClip::from_slice(
        &bytes,
        resource.fade_in_seconds.map_or(1.0, |value| value.get()),
        resource.fade_out_seconds.map_or(1.0, |value| value.get()),
    )
    .map_err(|error: PlaybackError| {
        let mut error: Live2dError = error.into();
        error.detail = format!("{}: {}", path.display(), error.detail);
        error
    })
}

fn load_expression_clip(model: &CommittedModel, name: &str) -> Result<ExpressionClip, Live2dError> {
    let resource = model
        .index()
        .expressions
        .iter()
        .find(|resource| resource.name == name)
        .ok_or_else(|| {
            Live2dError::new(
                Live2dErrorCode::ExpressionNotFound,
                format!("expression {name:?} is not declared by model3"),
            )
        })?;
    let path = model.root().join(&resource.file);
    let bytes = fs::read(&path).map_err(|error| {
        Live2dError::new(
            Live2dErrorCode::ResourceIo,
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    ExpressionClip::from_slice(&bytes).map_err(|error: PlaybackError| {
        let mut error: Live2dError = error.into();
        error.detail = format!("{}: {}", path.display(), error.detail);
        error
    })
}

impl Live2dModel {
    pub fn load(model: &CommittedModel) -> Result<Self, Live2dError> {
        let resources = Arc::new(prepare_render_resources(model).map_err(
            |error: RenderResourceError| {
                Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string())
            },
        )?);
        let motions = model
            .index()
            .motion_groups
            .iter()
            .map(|group| {
                let clips = group
                    .motions
                    .iter()
                    .enumerate()
                    .map(|(index, _)| load_motion_clip(model, &group.name, index))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<_, Live2dError>((group.name.clone(), clips))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut expressions = BTreeMap::new();
        for resource in &model.index().expressions {
            let name = resource.name.clone();
            let clip = load_expression_clip(model, &name)?;
            if expressions.insert(name.clone(), clip).is_some() {
                return Err(Live2dError::new(
                    Live2dErrorCode::ExpressionInvalid,
                    format!("model3 declares expression name {name:?} more than once"),
                ));
            }
        }

        let breath_parameter_ids = parameter_group_ids(model, "Breath");
        let eye_blink_parameter_ids = parameter_group_ids(model, "EyeBlink");
        let lip_sync_parameter_ids = parameter_group_ids(model, "LipSync");
        let physics = model
            .physics_definition()
            .map_err(|error| Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string()))?
            .map(physics::PhysicsRuntime::new);
        let moc_path = model.root().join(&model.index().moc);
        let core = core::CoreModel::load(&moc_path)?;
        core.validate_texture_indices(resources.textures.len())?;
        Ok(Self {
            resources,
            motions,
            expressions,
            breath_parameter_ids,
            eye_blink_parameter_ids,
            lip_sync_parameter_ids,
            physics,
            model_opacity: 1.0,
            core,
        })
    }

    pub fn texture_assets(&self) -> &[TextureAsset] {
        &self.resources.textures
    }

    pub fn render_resources(&self) -> Arc<RenderResources> {
        Arc::clone(&self.resources)
    }

    pub fn part_opacity_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        self.core.part_opacity_by_id(id)
    }

    pub fn motion_clip(&self, group: &str, index: usize) -> Option<&MotionClip> {
        self.motions
            .get(group)
            .and_then(|motions| motions.get(index))
    }

    pub fn expression_clip(&self, name: &str) -> Option<&ExpressionClip> {
        self.expressions.get(name)
    }

    pub fn parameter_range(&self, parameter: ProductParameter) -> Option<ParameterRange> {
        self.core.parameter_range(parameter)
    }

    pub fn parameter_value(&self, parameter: ProductParameter) -> Result<Option<f32>, Live2dError> {
        self.core.parameter_value(parameter)
    }

    /// Read a model-declared parameter by its stable Core identifier.
    /// Unknown IDs return `None` so optional effect groups remain portable.
    pub fn parameter_value_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        self.core.parameter_value_by_id(id)
    }

    pub fn set_parameter(
        &mut self,
        parameter: ProductParameter,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        self.core.set_parameter(parameter, value)
    }

    pub fn set_normalized_parameter(
        &mut self,
        parameter: ProductParameter,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{} received a non-finite normalized value", parameter.id()),
            ));
        }
        let Some(range) = self.parameter_range(parameter) else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let normalized = value.clamp(-1.0, 1.0);
        let mapped = if normalized >= 0.0 {
            range.default + (range.maximum - range.default) * normalized
        } else {
            range.default + (range.default - range.minimum) * normalized
        };
        self.set_parameter(parameter, mapped)
    }

    /// Set a model-declared parameter using the same normalized [-1, 1] input
    /// contract as product parameters. Unknown IDs are intentionally ignored
    /// so optional model effects remain portable across presets.
    pub fn set_normalized_parameter_by_id(
        &mut self,
        id: &str,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received a non-finite normalized value"),
            ));
        }
        let Some(range) = self.core.parameter_range_by_id(id) else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let normalized = value.clamp(-1.0, 1.0);
        let mapped = if normalized >= 0.0 {
            range.default + (range.maximum - range.default) * normalized
        } else {
            range.default + (range.default - range.minimum) * normalized
        };
        self.core.set_parameter_by_id(id, mapped, 1.0)
    }

    /// Apply the reference automatic effects at an injected monotonic time.
    ///
    /// The fixed targets match Bongo-Cat-Mver's Cubism Framework breath
    /// configuration. They are applied after product input, as in the
    /// reference, so the conventional angle parameters are not overwritten by
    /// the neutral input snapshot. The `0.5` contribution is additive, matching
    /// `CubismBreath`'s `AddParameterValue`; blending toward the breath target
    /// would incorrectly cancel a strong mouse-driven angle. A model's first
    /// explicit Breath group may contribute additional IDs; the fixed targets
    /// are not duplicated.
    pub fn apply_automatic_effects(
        &mut self,
        elapsed: Duration,
        eye_blink: f32,
    ) -> Result<usize, Live2dError> {
        let seconds = elapsed.as_secs_f64();
        let mut applied = 0;
        for (id, offset, peak, cycle) in REFERENCE_BREATH_TARGETS {
            let target = offset as f64
                + peak as f64 * (std::f64::consts::TAU * seconds / cycle as f64).sin();
            if matches!(
                self.add_parameter_by_id_with_weight(
                    id,
                    target as f32,
                    AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT
                )?,
                ParameterUpdate::Applied { .. }
            ) {
                applied += 1;
            }
        }

        // The optional model3 Breath group keeps the existing model-range
        // blend contract; only the five fixed Mver targets above use the
        // reference additive contribution.
        let generic_phase = ((std::f64::consts::TAU * seconds / GENERIC_BREATH_PERIOD as f64).sin()
            + 1.0) as f32
            * 0.5;
        for id in self.breath_parameter_ids.clone() {
            if REFERENCE_BREATH_TARGETS
                .iter()
                .any(|(fixed, ..)| *fixed == id)
            {
                continue;
            }
            if matches!(
                self.apply_automatic_breath(&id, generic_phase)?,
                ParameterUpdate::Applied { .. }
            ) {
                applied += 1;
            }
        }
        let eye_blink_ids = self.eye_blink_parameter_ids.clone();
        for id in eye_blink_ids {
            if matches!(
                self.set_normalized_parameter_by_id(&id, eye_blink)?,
                ParameterUpdate::Applied { .. }
            ) {
                applied += 1;
            }
        }
        Ok(applied)
    }

    fn add_parameter_by_id_with_weight(
        &mut self,
        id: &str,
        value: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received a non-finite automatic value"),
            ));
        }
        self.core.add_parameter_by_id(id, value, weight)
    }

    fn set_parameter_by_id_with_weight(
        &mut self,
        id: &str,
        value: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received a non-finite automatic value"),
            ));
        }
        self.core.set_parameter_by_id(id, value, weight)
    }

    fn apply_automatic_breath(
        &mut self,
        id: &str,
        phase: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !phase.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received a non-finite automatic phase"),
            ));
        }
        let Some(range) = self.core.parameter_range_by_id(id) else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let phase = phase.clamp(0.0, 1.0);
        let target = range.minimum + (range.maximum - range.minimum) * phase;
        self.set_parameter_by_id_with_weight(id, target, AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT)
    }

    pub fn apply_physics(&mut self, delta: Duration) -> Result<usize, Live2dError> {
        let Some(physics) = self.physics.as_mut() else {
            return Ok(0);
        };
        physics.evaluate(delta, &mut self.core)
    }

    pub fn reset_physics(&mut self) {
        if let Some(physics) = self.physics.as_mut() {
            physics.reset();
        }
    }

    pub fn apply_motion(
        &mut self,
        motion: &MotionClip,
        elapsed: std::time::Duration,
    ) -> Result<MotionApplyStatus, Live2dError> {
        self.apply_motion_with_weight(motion, elapsed, 1.0)
    }

    pub fn apply_motion_with_weight(
        &mut self,
        motion: &MotionClip,
        elapsed: std::time::Duration,
        weight: f32,
    ) -> Result<MotionApplyStatus, Live2dError> {
        self.apply_motion_with_weight_and_looping(motion, elapsed, weight, motion.is_looping())
    }

    pub fn apply_motion_once_with_weight(
        &mut self,
        motion: &MotionClip,
        elapsed: std::time::Duration,
        weight: f32,
    ) -> Result<MotionApplyStatus, Live2dError> {
        self.apply_motion_with_weight_and_looping(motion, elapsed, weight, false)
    }

    fn apply_motion_with_weight_and_looping(
        &mut self,
        motion: &MotionClip,
        elapsed: std::time::Duration,
        weight: f32,
        looping: bool,
    ) -> Result<MotionApplyStatus, Live2dError> {
        if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                "motion received an invalid playback weight",
            ));
        }
        let evaluation = if looping {
            motion.evaluate(elapsed)
        } else {
            motion.evaluate_once(elapsed)
        };

        let model_opacity_applied = if let Some(opacity) = evaluation.model.opacity {
            self.model_opacity = opacity.clamp(0.0, 1.0);
            true
        } else {
            false
        };

        let (applied_parameter_count, applied_eye_blink_count, applied_lip_sync_count) = {
            let mut applied_parameters = 0;
            let mut applied_eye_blink = 0;
            let mut applied_lip_sync = 0;
            let mut eye_blink_curves = vec![false; self.eye_blink_parameter_ids.len()];
            let mut lip_sync_curves = vec![false; self.lip_sync_parameter_ids.len()];
            for sample in &evaluation.parameters {
                let mut value = sample.value;
                let eye_blink_index = evaluation.model.eye_blink.and_then(|_| {
                    self.eye_blink_parameter_ids
                        .iter()
                        .position(|id| id == &sample.id)
                });
                if let Some(index) = eye_blink_index {
                    value *= evaluation
                        .model
                        .eye_blink
                        .expect("checked EyeBlink model curve");
                    eye_blink_curves[index] = true;
                }
                let lip_sync_index = evaluation.model.lip_sync.and_then(|_| {
                    self.lip_sync_parameter_ids
                        .iter()
                        .position(|id| id == &sample.id)
                });
                if let Some(index) = lip_sync_index {
                    value += evaluation
                        .model
                        .lip_sync
                        .expect("checked LipSync model curve");
                    lip_sync_curves[index] = true;
                }
                if matches!(
                    self.core
                        .set_parameter_by_id(&sample.id, value, sample.weight * weight)?,
                    ParameterUpdate::Applied { .. }
                ) {
                    applied_parameters += 1;
                    applied_eye_blink += usize::from(eye_blink_index.is_some());
                    applied_lip_sync += usize::from(lip_sync_index.is_some());
                }
            }
            let effect_weight = evaluation.model.effect_weight * weight;
            if let Some(eye_blink) = evaluation.model.eye_blink {
                for (index, id) in self.eye_blink_parameter_ids.iter().enumerate() {
                    if eye_blink_curves[index] {
                        continue;
                    }
                    if matches!(
                        self.core
                            .set_parameter_by_id(id, eye_blink, effect_weight)?,
                        ParameterUpdate::Applied { .. }
                    ) {
                        applied_eye_blink += 1;
                    }
                }
            }
            if let Some(lip_sync) = evaluation.model.lip_sync {
                for (index, id) in self.lip_sync_parameter_ids.iter().enumerate() {
                    if lip_sync_curves[index] {
                        continue;
                    }
                    if matches!(
                        self.core.set_parameter_by_id(id, lip_sync, effect_weight)?,
                        ParameterUpdate::Applied { .. }
                    ) {
                        applied_lip_sync += 1;
                    }
                }
            }
            (applied_parameters, applied_eye_blink, applied_lip_sync)
        };

        let applied_part_opacity_count = {
            let mut count = 0;
            for sample in &evaluation.part_opacities {
                if matches!(
                    self.core
                        .set_part_opacity_by_id(&sample.id, sample.value, weight)?,
                    ParameterUpdate::Applied { .. }
                ) {
                    count += 1;
                }
            }
            count
        };

        Ok(MotionApplyStatus {
            finished: evaluation.finished,
            applied_parameter_count,
            applied_part_opacity_count,
            applied_eye_blink_count,
            applied_lip_sync_count,
            model_opacity_applied,
        })
    }

    pub fn apply_expression_layers(
        &mut self,
        layers: &[ExpressionLayer<'_>],
    ) -> Result<ExpressionApplyStatus, Live2dError> {
        for layer in layers {
            if !layer.weight.is_finite() || !(0.0..=1.0).contains(&layer.weight) {
                return Err(Live2dError::new(
                    Live2dErrorCode::ParameterValueInvalid,
                    "expression layer received an invalid weight",
                ));
            }
        }

        let applied_parameter_count = {
            let parameter_ids = layers
                .iter()
                .flat_map(|layer| {
                    layer
                        .clip
                        .parameters()
                        .map(|parameter| parameter.id.as_str())
                })
                .collect::<BTreeSet<_>>();
            let mut applied = 0;
            for id in parameter_ids {
                let Some(current) = self.core.parameter_value_by_id(id)? else {
                    continue;
                };
                let target = evaluate_expression_parameter(id, current, layers);
                if matches!(
                    self.core.set_parameter_by_id(id, target, 1.0)?,
                    ParameterUpdate::Applied { .. }
                ) {
                    applied += 1;
                }
            }
            applied
        };

        Ok(ExpressionApplyStatus {
            applied_parameter_count,
        })
    }

    pub fn restore_parameter_defaults(&mut self) -> Result<(), Live2dError> {
        self.core.restore_parameter_defaults()
    }

    pub fn restore_part_opacity_defaults(&mut self) -> Result<(), Live2dError> {
        self.core.restore_part_opacity_defaults()
    }

    pub fn update_and_snapshot(&mut self) -> Result<RenderSnapshot, Live2dError> {
        let mut snapshot = self.core.update_and_snapshot()?;
        snapshot.model_opacity = self.model_opacity;
        Ok(snapshot)
    }
}

fn parameter_group_ids(model: &CommittedModel, name: &str) -> Vec<String> {
    model
        .index()
        .groups
        .iter()
        .find(|group| group.target == "Parameter" && group.name == name)
        .map(|group| {
            group
                .ids
                .iter()
                .take(MAX_MODEL_EFFECT_TARGETS)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_live2d_render::{
        KeyImageInventory, key_name_candidates, load_background_asset, load_key_assets,
        resolve_key_overlays,
    };
    use bongocat_render::{BlendMode, KeySide};
    use std::path::Path;

    #[test]
    fn vertex_layout_is_tightly_packed_for_gpu_upload() {
        assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
        assert_eq!(align_of::<bongocat_render::Vertex>(), 4);
    }

    #[test]
    fn supported_blend_modes_are_explicit() {
        assert_ne!(BlendMode::Normal, BlendMode::Additive);
        assert_ne!(BlendMode::Additive, BlendMode::Multiplicative);
    }

    #[test]
    fn key_overlay_resolution_is_side_scoped_and_resource_strict() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        let asset = |id: usize, side: KeySide, name: &str| KeyAsset {
            id: KeyAssetId::new(id),
            side,
            name: name.to_owned(),
            path: PathBuf::from(format!("{name}.png")),
            width: 612,
            height: 354,
        };
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: vec![
                asset(0, KeySide::Left, "KeyA"),
                asset(1, KeySide::Left, "Fn"),
                asset(2, KeySide::Left, "Shift"),
                asset(3, KeySide::Left, "ShiftLeft"),
                asset(4, KeySide::Right, "UpArrow"),
            ],
            background: None,
        };

        let mut presses = KeyPressSet::default();
        presses.push(KeyPress {
            hid_usage: 0x04,
            side: KeySide::Left,
        });
        assert_eq!(
            resolve_key_overlays(&resources, presses)
                .into_iter()
                .map(|overlay| overlay.asset_id.index())
                .collect::<Vec<_>>(),
            vec![0]
        );

        let mut presses = KeyPressSet::default();
        presses.push(KeyPress {
            hid_usage: 0x3a,
            side: KeySide::Left,
        });
        assert_eq!(
            resolve_key_overlays(&resources, presses)[0]
                .asset_id
                .index(),
            1
        );

        let mut presses = KeyPressSet::default();
        presses.push(KeyPress {
            hid_usage: 0xe1,
            side: KeySide::Left,
        });
        assert_eq!(
            resolve_key_overlays(&resources, presses)[0]
                .asset_id
                .index(),
            3
        );

        let mut presses = KeyPressSet::default();
        presses.push(KeyPress {
            hid_usage: 0x52,
            side: KeySide::Left,
        });
        assert!(resolve_key_overlays(&resources, presses).is_empty());
    }

    /// The globe key and the F1 … F24 fallback are two different keys with two
    /// different images, and neither can reach the other's artwork.
    ///
    /// `Fn` is the shared function-row image the old `rdev` layer derived from
    /// an unsupported `F<number>`, so every model authored against it — the two
    /// shipped keyboard presets included — carries that stem. `Globe` is the
    /// globe key's own name and `Function` is the pre-rename spelling of that
    /// same key. The two candidate lists are disjoint, which is the whole point:
    /// a model shipping only `Fn.png` cannot draw the globe key, and a model
    /// shipping only `Globe.png` cannot draw a function key.
    #[test]
    fn the_globe_key_never_shares_an_image_with_the_function_row() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        assert_eq!(
            key_name_candidates(bongocat_render::GLOBE_KEY_USAGE),
            vec!["Globe", "Function"],
            "the globe key's own name, then its pre-rename spelling"
        );
        for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
            let candidates = key_name_candidates(hid_usage);
            assert!(
                candidates.contains(&"Fn"),
                "0x{hid_usage:02x} keeps the shared function-row fallback"
            );
            assert!(
                !candidates
                    .iter()
                    .any(|name| matches!(*name, "Globe" | "Function")),
                "0x{hid_usage:02x} must not inherit a globe name: {candidates:?}"
            );
        }

        let asset = |name: &str| KeyAsset {
            id: KeyAssetId::new(0),
            side: KeySide::Left,
            name: name.to_owned(),
            path: PathBuf::from(format!("{name}.png")),
            width: 612,
            height: 354,
        };
        let resources = |name: &str| RenderResources {
            textures: Vec::new(),
            key_assets: vec![asset(name)],
            background: None,
        };
        let resolve = |name: &str, hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            resolve_key_overlays(&resources(name), presses)
                .first()
                .map(|overlay| overlay.asset_id.index())
        };

        assert_eq!(
            resolve("Fn", 0x3b),
            Some(0),
            "F2 draws the shared row image"
        );
        assert_eq!(
            resolve("Fn", bongocat_render::GLOBE_KEY_USAGE),
            None,
            "the shared row image is not the globe key's"
        );
        assert_eq!(resolve("Globe", bongocat_render::GLOBE_KEY_USAGE), Some(0));
        assert_eq!(
            resolve("Globe", 0x3b),
            None,
            "the globe image is not a function key's"
        );
        assert_eq!(
            resolve("Function", bongocat_render::GLOBE_KEY_USAGE),
            Some(0),
            "a package that still carries the pre-rename spelling draws"
        );
        assert_eq!(
            resolve("Function", 0x3b),
            None,
            "and it is not a function key's image either"
        );
    }

    #[test]
    fn function_keys_prefer_their_own_image_and_fall_back_to_the_shared_fn_asset() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        let asset = |id: usize, name: &str| KeyAsset {
            id: KeyAssetId::new(id),
            side: KeySide::Left,
            name: name.to_owned(),
            path: PathBuf::from(format!("{name}.png")),
            width: 612,
            height: 354,
        };
        // One model draws F1, F5 and F13 individually next to the shared image;
        // the other ships nothing but the shared image.
        let partially_specific = RenderResources {
            textures: Vec::new(),
            key_assets: vec![
                asset(0, "Fn"),
                asset(1, "F1"),
                asset(2, "F5"),
                asset(3, "F13"),
            ],
            background: None,
        };
        let shared_only = RenderResources {
            textures: Vec::new(),
            key_assets: vec![asset(0, "Fn")],
            background: None,
        };
        let resolve = |resources: &RenderResources, hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            resolve_key_overlays(resources, presses)
                .first()
                .map(|overlay| overlay.asset_id.index())
        };

        assert_eq!(
            resolve(&partially_specific, 0x3a),
            Some(1),
            "F1 has its own"
        );
        assert_eq!(
            resolve(&partially_specific, 0x3e),
            Some(2),
            "F5 has its own"
        );
        assert_eq!(
            resolve(&partially_specific, 0x68),
            Some(3),
            "F13 has its own, past the F12 boundary"
        );
        assert_eq!(resolve(&partially_specific, 0x3b), Some(0), "F2 uses Fn");
        assert_eq!(resolve(&partially_specific, 0x45), Some(0), "F12 uses Fn");
        assert_eq!(resolve(&partially_specific, 0x73), Some(0), "F24 uses Fn");

        for hid_usage in 0x3a..=0x45 {
            assert_eq!(
                resolve(&shared_only, hid_usage),
                Some(0),
                "0x{hid_usage:02x} must fall back to the shared Fn image"
            );
        }
        for hid_usage in 0x68..=0x73 {
            assert_eq!(
                resolve(&shared_only, hid_usage),
                Some(0),
                "0x{hid_usage:02x} must fall back to the shared Fn image"
            );
        }
        // PrintScreen (0x46), Keypad = (0x67) and Execute (0x74) sit next to the
        // two function-key ranges and must not inherit the `Fn` fallback.
        for hid_usage in [0x46, 0x67, 0x74] {
            assert_eq!(
                resolve(&shared_only, hid_usage),
                None,
                "0x{hid_usage:02x} is not a function key"
            );
        }
        assert_eq!(
            resolve(&shared_only, 0x29),
            None,
            "the fallback covers function keys only"
        );
    }

    /// The two Alt keys are distinct physical keys and must resolve to distinct
    /// artwork. Before the bundled models were renamed, both HID codes fell
    /// through to the same `Alt.png`, so pressing right Alt drew the *left*
    /// artwork and the model's own `AltGr.png` was unreachable.
    #[test]
    fn alt_keys_resolve_their_own_image_and_keep_the_legacy_alias() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        // HID usages: `0xe2` is AltLeft, `0xe6` is AltRight.
        assert_eq!(
            key_name_candidates(0xe2),
            vec!["AltLeft", "Alt"],
            "left Alt prefers its own image over the shared family image"
        );
        assert_eq!(
            key_name_candidates(0xe6),
            vec!["AltRight", "AltGr", "Alt"],
            "right Alt must never land on the left artwork while the model still \
             speaks the legacy naming"
        );

        // Every asset sits in `left-keys`: the runtime binds both Alt keys to
        // the left hand, so the side dimension is not what this test varies.
        let resources = |names: &[&str]| RenderResources {
            textures: Vec::new(),
            key_assets: names
                .iter()
                .enumerate()
                .map(|(index, name)| KeyAsset {
                    id: KeyAssetId::new(index),
                    side: KeySide::Left,
                    name: (*name).to_owned(),
                    path: PathBuf::from(format!("{name}.png")),
                    width: 612,
                    height: 354,
                })
                .collect(),
            background: None,
        };
        let resolve = |model: &RenderResources, hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            resolve_key_overlays(model, presses)
                .first()
                .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
        };

        // A renamed (or freshly imported) model: each side draws its own image.
        let renamed = resources(&["AltLeft", "AltRight"]);
        assert_eq!(resolve(&renamed, 0xe2).as_deref(), Some("AltLeft"));
        assert_eq!(resolve(&renamed, 0xe6).as_deref(), Some("AltRight"));

        // A model that predates the rename: `AltGr` is right Alt's old name, and
        // the left key must not fall back to the right artwork.
        let legacy = resources(&["Alt", "AltGr"]);
        assert_eq!(resolve(&legacy, 0xe2).as_deref(), Some("Alt"));
        assert_eq!(resolve(&legacy, 0xe6).as_deref(), Some("AltGr"));

        // A model with a single shared `Alt` image keeps drawing it on both
        // sides, which is the best an ambiguous model can do.
        let shared = resources(&["Alt"]);
        assert_eq!(resolve(&shared, 0xe2).as_deref(), Some("Alt"));
        assert_eq!(resolve(&shared, 0xe6).as_deref(), Some("Alt"));
    }

    /// Main Enter and keypad Enter are distinct physical keys (HID `0x28` and
    /// `0x58`). The main key keeps its pre-rename `Return` name as a legacy
    /// alias, the keypad key prefers a dedicated `KpEnter.png` and falls back
    /// to the main `Enter` artwork when the model did not draw one.
    #[test]
    fn enter_keys_resolve_distinct_names_with_a_keypad_fallback() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        assert_eq!(
            key_name_candidates(0x28),
            vec!["Enter", "Return"],
            "main Enter prefers the canonical name and keeps the legacy alias"
        );
        assert_eq!(
            key_name_candidates(0x58),
            vec!["KpEnter", "Enter"],
            "keypad Enter prefers its own image and falls back to the main artwork"
        );

        // Every asset sits in `left-keys`: the side dimension is not what this
        // test varies.
        let resources = |names: &[&str]| RenderResources {
            textures: Vec::new(),
            key_assets: names
                .iter()
                .enumerate()
                .map(|(index, name)| KeyAsset {
                    id: KeyAssetId::new(index),
                    side: KeySide::Left,
                    name: (*name).to_owned(),
                    path: PathBuf::from(format!("{name}.png")),
                    width: 612,
                    height: 354,
                })
                .collect(),
            background: None,
        };
        let resolve = |model: &RenderResources, hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            resolve_key_overlays(model, presses)
                .first()
                .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
        };

        // A renamed (or freshly imported) model without keypad artwork: the
        // keypad key draws the main Enter image.
        let renamed = resources(&["Enter"]);
        assert_eq!(resolve(&renamed, 0x28).as_deref(), Some("Enter"));
        assert_eq!(resolve(&renamed, 0x58).as_deref(), Some("Enter"));

        // A model that drew both keys uses each artwork for its own key.
        let dedicated = resources(&["Enter", "KpEnter"]);
        assert_eq!(resolve(&dedicated, 0x28).as_deref(), Some("Enter"));
        assert_eq!(resolve(&dedicated, 0x58).as_deref(), Some("KpEnter"));

        // A model that predates the rename: the legacy `Return` image still
        // draws for the main key. The keypad key has no candidate for it —
        // `Return` was never the keypad key's name — so nothing is drawn.
        let legacy = resources(&["Return"]);
        assert_eq!(resolve(&legacy, 0x28).as_deref(), Some("Return"));
        assert_eq!(resolve(&legacy, 0x58), None);

        // The two keys never collapse into one candidate list.
        let none = resources(&[]);
        assert_eq!(resolve(&none, 0x28), None);
        assert_eq!(resolve(&none, 0x58), None);
    }

    /// The bundled keyboard models must speak the renamed vocabulary: no
    /// `Return.png` on disk, the main Enter key resolves to the renamed file,
    /// and the keypad Enter key falls back to the very same artwork because
    /// the presets ship no dedicated `KpEnter.png`.
    #[test]
    fn shipped_keyboard_models_draw_both_enter_keys_from_the_renamed_artwork() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
        use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        for id in ["standard", "keyboard"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let resources = RenderResources {
                textures: Vec::new(),
                key_assets: load_key_assets(model.root()).expect("key assets"),
                background: None,
            };
            assert!(
                !resources
                    .key_assets
                    .iter()
                    .any(|asset| asset.name == "Return"),
                "{id} must not ship the pre-rename `Return` image"
            );

            let resolve = |hid_usage: u16| {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress {
                    hid_usage,
                    side: KeySide::Left,
                });
                let overlays = resolve_key_overlays(&resources, presses);
                let asset = &resources.key_assets[overlays[0].asset_id.index()];
                (asset.name.clone(), asset.path.clone())
            };
            let (main_name, main_path) = resolve(0x28);
            let (keypad_name, keypad_path) = resolve(0x58);
            assert_eq!(main_name, "Enter", "{id} main Enter");
            assert_eq!(keypad_name, "Enter", "{id} keypad Enter falls back");
            assert!(main_path.ends_with("resources/left-keys/Enter.png"));
            assert_eq!(main_path, keypad_path, "{id} shares the Enter artwork");
        }
    }

    /// The whole keypad block carries the `Kp*` vocabulary the Mver conversion
    /// has always emitted, plus `NumLock`. Every keypad key that duplicates a
    /// main keyboard key lists that key's name as its second candidate; the five
    /// keys with no counterpart on the main keyboard keep their exact name and
    /// nothing else, because there is no artwork to fall back to.
    #[test]
    fn keypad_keys_name_themselves_and_fall_back_to_their_main_keyboard_twin() {
        for (hid_usage, expected) in [
            (0x53, vec!["NumLock"]),
            (0x54, vec!["KpDivide", "Slash"]),
            (0x55, vec!["KpMultiply"]),
            (0x56, vec!["KpMinus", "Minus"]),
            (0x57, vec!["KpPlus"]),
            (0x58, vec!["KpEnter", "Enter"]),
            (0x59, vec!["Kp1", "Num1"]),
            (0x5a, vec!["Kp2", "Num2"]),
            (0x5b, vec!["Kp3", "Num3"]),
            (0x5c, vec!["Kp4", "Num4"]),
            (0x5d, vec!["Kp5", "Num5"]),
            (0x5e, vec!["Kp6", "Num6"]),
            (0x5f, vec!["Kp7", "Num7"]),
            (0x60, vec!["Kp8", "Num8"]),
            (0x61, vec!["Kp9", "Num9"]),
            (0x62, vec!["Kp0", "Num0"]),
            (0x63, vec!["KpDecimal", "Dot"]),
        ] {
            assert_eq!(
                key_name_candidates(hid_usage),
                expected,
                "keypad 0x{hid_usage:02x}"
            );
        }
    }

    /// The fallback is what a user actually sees: no model in the repository and
    /// none of the collected community samples draws a dedicated `Kp*.png`. Each
    /// shipped keyboard preset must therefore draw the digit, Enter and Slash
    /// artwork for the keypad keys that duplicate them, and must keep drawing
    /// nothing for the five keys that have no counterpart to borrow from.
    #[test]
    fn shipped_keyboard_models_draw_the_keypad_from_the_main_keyboard_artwork() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
        use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        for id in ["standard", "keyboard"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let resources = RenderResources {
                textures: Vec::new(),
                key_assets: load_key_assets(model.root()).expect("key assets"),
                background: None,
            };
            assert!(
                !resources
                    .key_assets
                    .iter()
                    .any(|asset| asset.name.starts_with("Kp")),
                "{id} ships no keypad artwork, so every keypad key is a fallback"
            );

            let resolve = |hid_usage: u16| {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress {
                    hid_usage,
                    side: KeySide::Left,
                });
                resolve_key_overlays(&resources, presses)
                    .first()
                    .map(|overlay| resources.key_assets[overlay.asset_id.index()].name.clone())
            };

            for (hid_usage, artwork) in [
                (0x54, "Slash"),
                (0x58, "Enter"),
                (0x59, "Num1"),
                (0x5a, "Num2"),
                (0x5b, "Num3"),
                (0x5c, "Num4"),
                (0x5d, "Num5"),
                (0x5e, "Num6"),
                (0x5f, "Num7"),
                (0x60, "Num8"),
                (0x61, "Num9"),
                (0x62, "Num0"),
            ] {
                assert_eq!(
                    resolve(hid_usage).as_deref(),
                    Some(artwork),
                    "{id} keypad 0x{hid_usage:02x} must fall back to {artwork}"
                );
            }

            // Nothing to draw either way: 0x53, 0x55 and 0x57 have no main
            // keyboard counterpart at all, and 0x56 / 0x63 have one (`Minus`,
            // `Dot`) whose artwork the shipped models do not ship.
            for hid_usage in [0x53, 0x55, 0x56, 0x57, 0x63] {
                assert_eq!(
                    resolve(hid_usage),
                    None,
                    "{id} keypad 0x{hid_usage:02x} has no artwork to draw"
                );
            }
        }
    }

    /// Every HID usage the key vocabulary has to cover, written out rather than
    /// derived from the ranges this crate happens to use.
    ///
    /// This is a **superset** of what either platform adapter can produce, and
    /// deliberately so: the vocabulary is a contract with model authors, so a
    /// key is named even when no keyboard can press it. Two usages here are
    /// unreachable on both platforms today — `IntlHash` (`0x32`, macOS gives the
    /// ISO `#` key the same keycode as ANSI `\`) and `F21` … `F24`
    /// (`0x70..=0x73`, no Carbon keycode and no Windows scan code this adapter
    /// maps). Everything else is reachable on at least one platform; each
    /// adapter has its own exhaustive test pinning exactly which
    /// (`bongocat-platform`'s `this_adapter_reports_exactly_the_keycodes_the_platform_defines`
    /// and the Windows scan-code matrix).
    ///
    /// The modifier block sits outside `0x04..=0x65` and outside `0x68..=0x73`,
    /// and the globe key is not on the Keyboard/Keypad page at all, so no range
    /// over that page can be made to include it. Both were dropped once by a
    /// rewrite that only looked at the ranges the implementation used.
    fn adapter_keyboard_usages() -> Vec<u16> {
        (0x04..=0x65)
            .chain([0x67])
            .chain(0x68..=0x73)
            .chain(0xe0..=0xe7)
            .chain([bongocat_render::GLOBE_KEY_USAGE])
            .collect()
    }

    /// The vocabulary has no holes: every key the platform adapters can report
    /// carries at least one candidate name, so a model that ships artwork for it
    /// is honoured without a product change. That set is the main block, keypad
    /// `=` (macOS reports it separately), F13 … F24, the eight modifier usages
    /// `0xe0..=0xe7` and the Apple Fn / globe key; HID `0x66` (Power) is the only
    /// usage in the block neither adapter ever produces, so it stays unnamed.
    #[test]
    fn every_key_the_platform_adapters_can_report_has_a_name() {
        for usage in adapter_keyboard_usages() {
            assert!(
                !key_name_candidates(usage).is_empty(),
                "0x{usage:02x} has no candidate name"
            );
        }
        assert!(
            key_name_candidates(0x66).is_empty(),
            "Power is not a key either adapter maps"
        );
    }

    /// Every image name the Mver conversion can install is a name the runtime
    /// resolves, and resolves *first*.
    ///
    /// Two separate things have to hold, and only checking the first is not
    /// enough. The name must reach a key at all — otherwise the conversion
    /// installs an image nothing can draw, and the key does nothing whatsoever,
    /// because a press without a hand assignment is dropped before the resolver
    /// ever sees it (ADR-0042). And it must be the name the runtime reaches
    /// first, not a legacy alias that happens to save it: `Backslash` was
    /// precisely the second failure, spelled `Backslash` by the conversion while
    /// the product spells it `BackSlash`, so every converted backslash image was
    /// unreachable (ADR-0050). A name that only resolves through an alias would
    /// let that drift back in silently, because the alias keeps the artwork
    /// reachable either way.
    ///
    /// `Shift` and `Control` are the two deliberate exceptions: the legacy chart
    /// gives each of them a single code for both sides, so the conversion emits
    /// the family name and the runtime resolves it for either side (ADR-0038
    /// decision 4). They are asserted to still need the exception, so the list
    /// cannot quietly become dead. The set of conversion outputs comes from
    /// `bongocat-model`, so a new code in the legacy table is covered here
    /// without touching this test.
    #[test]
    fn every_key_image_name_the_conversion_can_install_resolves_to_a_key() {
        const FAMILY_NAMES: [&str; 2] = ["Shift", "Control"];

        let candidates = |usage: u16| key_name_candidates(usage);
        let resolvable: std::collections::BTreeSet<&str> = adapter_keyboard_usages()
            .into_iter()
            .flat_map(key_name_candidates)
            .collect();
        let canonical: std::collections::BTreeSet<&str> = adapter_keyboard_usages()
            .into_iter()
            .filter_map(|usage| candidates(usage).first().copied())
            .collect();

        let outputs = bongocat_model_store::legacy_keyboard_key_image_names();
        let unresolvable: Vec<&str> = outputs
            .iter()
            .copied()
            .filter(|name| !resolvable.contains(name))
            .collect();
        assert!(
            unresolvable.is_empty(),
            "no key resolves these conversion outputs: {unresolvable:?}"
        );
        let alias_only: Vec<&str> = outputs
            .iter()
            .copied()
            .filter(|name| !canonical.contains(name) && !FAMILY_NAMES.contains(name))
            .collect();
        assert!(
            alias_only.is_empty(),
            "these conversion outputs are only reachable through a legacy alias: {alias_only:?}"
        );

        for family in FAMILY_NAMES {
            assert!(
                !canonical.contains(family),
                "{family} has a canonical name now; drop it from the exception list"
            );
        }
    }

    /// A name is a contract with model authors, not a list of the images the
    /// shipped models happen to carry. A model that provides a key image the
    /// presets never shipped must draw it with no product change — and a model
    /// that provides none of them must keep drawing nothing.
    #[test]
    fn a_model_providing_a_named_key_image_draws_it() {
        use bongocat_render::{
            KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources,
        };
        use std::path::PathBuf;

        let resources = |names: &[&str]| RenderResources {
            textures: Vec::new(),
            key_assets: names
                .iter()
                .enumerate()
                .map(|(index, name)| KeyAsset {
                    id: KeyAssetId::new(index),
                    side: KeySide::Left,
                    name: (*name).to_owned(),
                    path: PathBuf::from(format!("{name}.png")),
                    width: 612,
                    height: 354,
                })
                .collect(),
            background: None,
        };
        let resolve = |model: &RenderResources, hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            resolve_key_overlays(model, presses)
                .first()
                .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
        };

        // A model that draws the punctuation block, the navigation cluster and
        // the keypad keys no shipped model ever drew.
        let future = resources(&[
            "Minus",
            "Equal",
            "LeftBracket",
            "RightBracket",
            "BackSlash",
            "IntlHash",
            "SemiColon",
            "Quote",
            "Comma",
            "Dot",
            "PrintScreen",
            "ScrollLock",
            "Pause",
            "Insert",
            "Home",
            "PageUp",
            "Delete",
            "End",
            "PageDown",
            "NumLock",
            "KpMultiply",
            "KpMinus",
            "KpPlus",
            "KpDecimal",
            "IntlBackslash",
            "Apps",
            "KpEqual",
        ]);
        for (hid_usage, name) in [
            (0x2d, "Minus"),
            (0x2e, "Equal"),
            (0x2f, "LeftBracket"),
            (0x30, "RightBracket"),
            (0x31, "BackSlash"),
            (0x32, "IntlHash"),
            (0x33, "SemiColon"),
            (0x34, "Quote"),
            (0x36, "Comma"),
            (0x37, "Dot"),
            (0x46, "PrintScreen"),
            (0x47, "ScrollLock"),
            (0x48, "Pause"),
            (0x49, "Insert"),
            (0x4a, "Home"),
            (0x4b, "PageUp"),
            (0x4c, "Delete"),
            (0x4d, "End"),
            (0x4e, "PageDown"),
            (0x53, "NumLock"),
            (0x55, "KpMultiply"),
            (0x56, "KpMinus"),
            (0x57, "KpPlus"),
            (0x63, "KpDecimal"),
            (0x64, "IntlBackslash"),
            (0x65, "Apps"),
            (0x67, "KpEqual"),
        ] {
            assert_eq!(
                resolve(&future, hid_usage).as_deref(),
                Some(name),
                "0x{hid_usage:02x} must draw {name}.png when a model ships it"
            );
        }

        // The shipped vocabulary draws nothing for them, because it ships none
        // of these images — the reason is the resource, not the name.
        let shipped = resources(&["KeyA", "Num1", "Enter", "Slash"]);
        for hid_usage in [0x37, 0x2d, 0x4c, 0x63] {
            assert_eq!(
                resolve(&shipped, hid_usage),
                None,
                "0x{hid_usage:02x} has no artwork in the shipped vocabulary"
            );
        }
    }

    /// The inventory exists to be asked before the product reacts to a key, so
    /// it has to name exactly the assets the renderer will load: a name the
    /// inventory reports and the loader does not have would move the paw for an
    /// image that can never appear, and the reverse would drop a key that draws
    /// perfectly well.
    #[test]
    fn key_image_inventory_lists_exactly_the_assets_the_renderer_loads() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        for id in ["standard", "keyboard", "gamepad"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let inventory = KeyImageInventory::read(model.root());
            let assets = load_key_assets(model.root()).expect("key assets");
            assert!(
                !assets.is_empty(),
                "{id} ships no key artwork, so the test proves nothing"
            );
            for (side, names) in [
                (KeySide::Left, inventory.names(KeySide::Left)),
                (KeySide::Right, inventory.names(KeySide::Right)),
            ] {
                let loaded = assets
                    .iter()
                    .filter(|asset| asset.side == side)
                    .map(|asset| asset.name.clone())
                    .collect::<BTreeSet<_>>();
                assert_eq!(names, &loaded, "{id} {side:?}");
            }
        }
    }

    /// The rule the product applies before it reacts to any key: a model can
    /// draw a key only when it ships artwork that key resolves to. `standard`
    /// draws the letters, `Delete` and the keypad digits that fall back to the
    /// number row, and cannot draw `.`, PrintScreen, NumLock, keypad `.` or the
    /// arrow cluster — none of which it ships.
    #[test]
    fn a_shipped_model_can_draw_only_the_keys_it_ships_artwork_for() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        let load = |id: &str| {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            KeyImageInventory::read(model.root())
        };

        let standard = load("standard");
        for (hid_usage, drawable, why) in [
            (0x04, true, "KeyA.png"),
            (0x1e, true, "Num1.png"),
            (0x28, true, "Enter.png"),
            (0x3a, true, "Fn.png covers the function row"),
            (0x68, true, "Fn.png covers F13, past the F12 boundary"),
            (0x73, true, "Fn.png covers F24"),
            (0x4c, true, "Delete.png"),
            (0x58, true, "keypad Enter falls back to Enter.png"),
            (0x59, true, "keypad 1 falls back to Num1.png"),
            (0x2d, false, "Minus.png is not shipped"),
            (0x37, false, "Dot.png is not shipped"),
            (0x46, false, "PrintScreen.png is not shipped"),
            (0x53, false, "NumLock.png is not shipped"),
            (0x63, false, "keypad . has no artwork to fall back to"),
            (0x67, false, "keypad = is not shipped"),
            (
                bongocat_render::GLOBE_KEY_USAGE,
                false,
                "Globe.png is not shipped, and Fn.png is not its image",
            ),
        ] {
            assert_eq!(
                standard.can_draw(KeySide::Left, hid_usage),
                drawable,
                "standard 0x{hid_usage:02x}: {why}"
            );
        }
        // The shared function-row image is the only function-key artwork the
        // keyboard presets ship, so the whole row is drawable from it — F13 …
        // F24 included. `can_draw` is what gates the binding
        // (`bongocat-app::input_bindings_for_model`), so a model that ships
        // `Fn.png` alone still binds all 24 keys and still draws whichever one a
        // keyboard reports. F21 … F24 are named and bound but unreachable on both
        // platforms today, which is exactly why the fallback has to hold for the
        // whole range: the vocabulary promises them whether or not hardware can
        // press them.
        //
        // Both keyboard presets are checked because both ship `Fn.png` and
        // neither ships a per-key `F<number>.png`: the fallback is the contract,
        // not a property of `standard`.
        let keyboard = load("keyboard");
        for (id, inventory) in [("standard", &standard), ("keyboard", &keyboard)] {
            for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
                assert!(
                    inventory.can_draw(KeySide::Left, hid_usage),
                    "{id} 0x{hid_usage:02x} must be drawable from Fn.png"
                );
            }
        }
        // The arrow cluster is the other hand's artwork, and `standard` ships no
        // `right-keys` directory at all.
        assert!(
            !standard.can_draw(KeySide::Left, 0x52),
            "standard left UpArrow"
        );
        assert!(
            !standard.can_draw(KeySide::Right, 0x52),
            "standard right UpArrow"
        );

        assert!(keyboard.can_draw(KeySide::Right, 0x52), "keyboard UpArrow");
        assert!(
            !keyboard.can_draw(KeySide::Left, 0x52),
            "keyboard left UpArrow"
        );
        assert!(!keyboard.can_draw(KeySide::Left, 0x37), "keyboard Dot.png");

        let gamepad = load("gamepad");
        assert!(
            !gamepad.can_draw(KeySide::Left, 0x04),
            "the gamepad model ships no keyboard artwork"
        );
    }

    /// The shipped contract: the bundled keyboard models must expose both Alt
    /// keys, with different artwork, under the names the resolver asks for.
    #[test]
    fn shipped_keyboard_models_draw_both_alt_keys_with_their_own_artwork() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
        use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        for id in ["standard", "keyboard"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let resources = RenderResources {
                textures: Vec::new(),
                key_assets: load_key_assets(model.root()).expect("key assets"),
                background: None,
            };
            for legacy in ["Alt", "AltGr"] {
                assert!(
                    !resources
                        .key_assets
                        .iter()
                        .any(|asset| asset.name == legacy),
                    "{id} must not ship the pre-rename `{legacy}` image"
                );
            }

            let resolve = |hid_usage: u16| {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress {
                    hid_usage,
                    side: KeySide::Left,
                });
                let overlays = resolve_key_overlays(&resources, presses);
                let asset = &resources.key_assets[overlays[0].asset_id.index()];
                (asset.name.clone(), asset.path.clone())
            };
            let (left_name, left_path) = resolve(0xe2);
            let (right_name, right_path) = resolve(0xe6);
            assert_eq!(left_name, "AltLeft", "{id} left Alt");
            assert_eq!(right_name, "AltRight", "{id} right Alt");
            assert!(left_path.ends_with("resources/left-keys/AltLeft.png"));
            assert!(right_path.ends_with("resources/left-keys/AltRight.png"));
            assert_ne!(
                fs::read(&left_path).expect("left Alt artwork"),
                fs::read(&right_path).expect("right Alt artwork"),
                "{id} must draw a different image for each Alt key"
            );
        }
    }

    #[test]
    fn shipped_keyboard_models_draw_every_function_key_with_the_shipped_fn_image() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
        use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog =
            PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
        // The gamepad model is driven by gamepad buttons and ships no `Fn.png`,
        // so only the two keyboard-vocabulary models are covered here.
        for id in ["standard", "keyboard"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let resources = RenderResources {
                textures: Vec::new(),
                key_assets: load_key_assets(model.root()).expect("key assets"),
                background: None,
            };
            assert!(
                !resources
                    .key_assets
                    .iter()
                    .any(|asset| asset.name.starts_with('F') && asset.name != "Fn"),
                "{id} must not ship a dedicated function-key image yet"
            );
            for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
                let mut presses = KeyPressSet::default();
                presses.push(KeyPress {
                    hid_usage,
                    side: KeySide::Left,
                });
                let overlays = resolve_key_overlays(&resources, presses);
                let asset = &resources.key_assets[overlays[0].asset_id.index()];
                assert_eq!(asset.name, "Fn", "{id} 0x{hid_usage:02x}");
                assert!(
                    asset.path.ends_with("resources/left-keys/Fn.png"),
                    "{id} 0x{hid_usage:02x} resolved to {}",
                    asset.path.display()
                );
            }
        }
    }

    /// The other half of the contract: when the model *does* ship a dedicated
    /// image, the loader must pick it up from disk and the resolver must prefer
    /// it over the shared one.
    #[test]
    fn a_model_shipping_a_dedicated_function_key_image_uses_it() {
        use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};
        use tempfile::tempdir;

        let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/models/standard/resources/left-keys");
        let root = tempdir().expect("root");
        let left_keys = root.path().join("resources/left-keys");
        fs::create_dir_all(&left_keys).expect("left keys directory");
        // The shipped standard model has no dedicated function-key image, so its
        // `Fn.png` bytes stand in for one: this test asserts which file wins, not
        // what the file contains.
        let shared = fs::read(shipped.join("Fn.png")).expect("shipped Fn.png");
        for name in ["Fn", "F13"] {
            fs::write(left_keys.join(format!("{name}.png")), &shared)
                .unwrap_or_else(|error| panic!("write {name}.png: {error}"));
        }

        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: load_key_assets(root.path()).expect("key assets"),
            background: None,
        };
        let resolve = |hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress {
                hid_usage,
                side: KeySide::Left,
            });
            let overlays = resolve_key_overlays(&resources, presses);
            resources.key_assets[overlays[0].asset_id.index()]
                .path
                .clone()
        };

        assert!(
            resolve(0x68).ends_with("left-keys/F13.png"),
            "F13 has its own"
        );
        assert!(resolve(0x69).ends_with("left-keys/Fn.png"), "F14 uses Fn");
        assert!(resolve(0x3a).ends_with("left-keys/Fn.png"), "F1 uses Fn");
        assert!(resolve(0x73).ends_with("left-keys/Fn.png"), "F24 uses Fn");
    }

    #[test]
    fn preset_models_expose_valid_background_assets() {
        use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default())
            .expect("preset model catalog");
        for id in ["standard", "keyboard", "gamepad"] {
            let model = catalog
                .load(&bongocat_model::ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let background = load_background_asset(model.root())
                .expect("background can be decoded")
                .expect("preset background");
            assert_eq!(background.width, 612);
            assert_eq!(background.height, 354);
        }
    }

    #[test]
    fn product_parameter_ids_are_stable_and_unique() {
        let mut ids = ProductParameter::ALL
            .iter()
            .map(|parameter| parameter.id())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ProductParameter::ALL.len());
        assert_eq!(ProductParameter::LeftHandDown.id(), "CatParamLeftHandDown");
        assert_eq!(ProductParameter::StickRightY.id(), "CatParamStickRY");
    }

    #[test]
    fn live2d_error_codes_are_stable_and_unique() {
        let mut codes = Live2dErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| !code.is_empty()));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Live2dErrorCode::ALL.len());
        assert_eq!(Live2dErrorCode::MotionInvalid.to_string(), "motion_invalid");
        let error = Live2dError::new(Live2dErrorCode::ResourceIo, "/private/model.moc3");
        assert!(error.to_string().starts_with("resource_io: "));
    }

    #[test]
    fn preset_motion_and_expression_resources_load_through_live2d_adapter() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        let catalog = PresetModelCatalog::open(root, ModelPackageLimits::default())
            .expect("preset model catalog");
        for id in ["standard", "keyboard", "gamepad"] {
            let committed = catalog
                .load(&ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let model = Live2dModel::load(&committed).expect("Live2D model");
            assert!(model.motion_clip("CAT_motion", 0).is_some());
            assert!(
                model
                    .expression_clip("live2d_expression0.exp3.json")
                    .is_some()
            );
        }
    }

    #[test]
    fn expression_layers_apply_add_multiply_and_overwrite_to_core_parameters() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");

        let clip = ExpressionClip::from_slice(
            br#"{
              "Type":"Live2D Expression",
              "FadeInTime":0,
              "Parameters":[
                {"Id":"ParamAngleX","Value":10,"Blend":"Add"},
                {"Id":"ParamEyeLOpen","Value":0.5,"Blend":"Multiply"},
                {"Id":"ParamAngleY","Value":-15,"Blend":"Overwrite"}
              ]
            }"#,
        )
        .expect("expression clip");
        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        let eye_default = model
            .core
            .parameter_value_by_id("ParamEyeLOpen")
            .expect("eye parameter")
            .expect("supported eye parameter");
        let applied = model
            .apply_expression_layers(&[ExpressionLayer {
                clip: &clip,
                weight: 1.0,
            }])
            .expect("apply expression");
        assert_eq!(applied.applied_parameter_count, 3);
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamAngleX")
                .expect("angle x"),
            Some(10.0)
        );
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamAngleY")
                .expect("angle y"),
            Some(-15.0)
        );
        let eye = model
            .core
            .parameter_value_by_id("ParamEyeLOpen")
            .expect("eye parameter")
            .expect("supported eye parameter");
        assert!((eye - eye_default * 0.5).abs() < 0.0001);
    }

    #[test]
    fn part_opacity_motion_curves_use_the_core_part_sink() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        let clip = MotionClip::from_slice(
            br#"{
              "Version":3,
              "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
                "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
                "UserDataCount":0,"TotalUserDataSize":0},
              "Curves":[
                {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]},
                {"Target":"PartOpacity","Id":"MissingPartSink","Segments":[0,0,0,1,1]}
              ]
            }"#,
            0.0,
            1.0,
        )
        .expect("part opacity motion");

        let parameter_before = model
            .parameter_value(ProductParameter::AngleX)
            .expect("angle parameter");
        let part_opacity_before = model
            .part_opacity_by_id("Part")
            .expect("initial part opacity");
        let status = model
            .apply_motion_with_weight(&clip, std::time::Duration::from_millis(500), 1.0)
            .expect("apply part opacity motion");
        assert_eq!(status.applied_parameter_count, 0);
        assert_eq!(status.applied_part_opacity_count, 1);
        assert_eq!(
            model.part_opacity_by_id("Part").expect("part opacity"),
            Some(0.125)
        );
        assert_eq!(
            model
                .parameter_value(ProductParameter::AngleX)
                .expect("angle parameter"),
            parameter_before
        );

        model
            .restore_part_opacity_defaults()
            .expect("restore part opacity defaults");
        assert_eq!(
            model.part_opacity_by_id("Part").expect("part opacity"),
            part_opacity_before
        );
    }

    #[test]
    fn model_motion_curves_apply_eye_blink_lip_sync_and_render_opacity() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        assert_eq!(
            model.eye_blink_parameter_ids,
            ["ParamEyeLOpen", "ParamEyeROpen"]
        );
        assert!(model.lip_sync_parameter_ids.is_empty());
        model
            .lip_sync_parameter_ids
            .push("ParamMouthOpenY".to_owned());
        let clip = MotionClip::from_slice(
            br#"{
              "Version":3,
              "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
                "CurveCount":5,"TotalSegmentCount":5,"TotalPointCount":10,
                "UserDataCount":0,"TotalUserDataSize":0},
              "Curves":[
                {"Target":"Model","Id":"EyeBlink","Segments":[0,0.5,0,1,0.5]},
                {"Target":"Model","Id":"LipSync","Segments":[0,0.2,0,1,0.2]},
                {"Target":"Model","Id":"Opacity","Segments":[0,0.4,0,1,0.4]},
                {"Target":"Parameter","Id":"ParamEyeLOpen","Segments":[0,0.8,0,1,0.8]},
                {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0.3,0,1,0.3]}
              ]
            }"#,
            0.0,
            0.0,
        )
        .expect("model effect motion");

        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        let status = model
            .apply_motion(&clip, std::time::Duration::from_millis(500))
            .expect("apply model curves");
        assert_eq!(status.applied_parameter_count, 2);
        assert_eq!(status.applied_eye_blink_count, 2);
        assert_eq!(status.applied_lip_sync_count, 1);
        assert!(status.model_opacity_applied);
        for (id, expected) in [
            ("ParamEyeLOpen", 0.4),
            ("ParamEyeROpen", 0.5),
            ("ParamMouthOpenY", 0.5),
        ] {
            let actual = model
                .core
                .parameter_value_by_id(id)
                .expect("parameter value")
                .expect("supported parameter");
            assert!((actual - expected).abs() < 0.0001, "{id}: {actual}");
        }
        let snapshot = model.update_and_snapshot().expect("render snapshot");
        assert!((snapshot.model_opacity - 0.4).abs() < 0.0001);

        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        let snapshot = model.update_and_snapshot().expect("next render snapshot");
        assert!((snapshot.model_opacity - 0.4).abs() < 0.0001);
    }

    #[test]
    fn automatic_effects_match_reference_breath_and_eye_blink() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        assert_eq!(model.breath_parameter_ids, ["ParamBreath"]);

        model
            .set_parameter(ProductParameter::AngleX, -15.0)
            .expect("product angle input");
        model
            .apply_automatic_effects(std::time::Duration::ZERO, 0.0)
            .expect("additive reference breath");
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamAngleX")
                .expect("angle value")
                .expect("supported angle parameter"),
            -15.0,
            "reference breath must add to, not blend away, mouse input"
        );

        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        let breath_range = model
            .core
            .parameter_range_by_id("ParamBreath")
            .expect("breath range");
        let applied = model
            .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
            .expect("automatic effects");
        assert_eq!(applied, 7);
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamEyeLOpen")
                .expect("left eye"),
            Some(0.0)
        );
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamEyeROpen")
                .expect("right eye"),
            Some(0.0)
        );
        let breath = model
            .core
            .parameter_value_by_id("ParamBreath")
            .expect("breath")
            .expect("supported breath parameter");
        assert!(
            breath > breath_range.default && breath < breath_range.maximum,
            "reference breath must stay within its authored range: {breath}"
        );
    }

    #[test]
    fn reference_breath_does_not_require_a_model_group() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        let breath_default = model
            .core
            .parameter_value_by_id("ParamBreath")
            .expect("breath value")
            .expect("supported breath parameter");
        let breath_range = model
            .core
            .parameter_range_by_id("ParamBreath")
            .expect("breath range");
        model.breath_parameter_ids.clear();

        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        assert_eq!(
            model
                .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
                .expect("automatic effects without Breath"),
            7,
            "the fixed reference targets remain active without a model3 Breath group"
        );
        let breath = model
            .core
            .parameter_value_by_id("ParamBreath")
            .expect("breath value")
            .expect("supported breath parameter");
        assert_ne!(
            breath, breath_default,
            "the conventional reference target should not be ignored"
        );

        for step in 0..=40 {
            model
                .restore_parameter_defaults()
                .expect("restore parameter defaults");
            model
                .apply_automatic_effects(std::time::Duration::from_millis(step * 100), 1.0)
                .expect("reference automatic effects");
            let breath = model
                .core
                .parameter_value_by_id("ParamBreath")
                .expect("breath value")
                .expect("supported breath parameter");
            assert!(breath.is_finite(), "reference step {step}: {breath}");
            let lower = breath_range.default
                + (breath_range.minimum - breath_range.default)
                    * AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT;
            let upper = breath_range.default
                + (breath_range.maximum - breath_range.default)
                    * AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT;
            assert!(
                (lower..=upper).contains(&breath),
                "reference step {step}: {breath} escaped [{lower}, {upper}]"
            );
        }
    }

    #[test]
    fn declared_physics_drives_a_parameter_without_a_model_motion() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use serde_json::json;
        use std::fs;
        use std::path::Path;

        fn copy_tree(source: &Path, destination: &Path) {
            fs::create_dir_all(destination).expect("destination directory");
            for entry in fs::read_dir(source).expect("source directory") {
                let entry = entry.expect("source entry");
                let target = destination.join(entry.file_name());
                if entry.file_type().expect("entry type").is_dir() {
                    copy_tree(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), target).expect("copied model file");
                }
            }
        }

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let package = tempfile::tempdir().expect("temporary package");
        let source = repository_root.join("resources/models/standard");
        let catalog_root = package.path().join("catalog");
        let model_root = catalog_root.join("physics");
        copy_tree(&source, &model_root);
        let model_path = model_root.join("cat.model3.json");
        let mut model_json: serde_json::Value =
            serde_json::from_slice(&fs::read(&model_path).expect("model3")).expect("model3 JSON");
        model_json["FileReferences"]["Physics"] = json!("cat.physics3.json");
        fs::write(
            &model_path,
            serde_json::to_vec_pretty(&model_json).expect("model3 JSON serialization"),
        )
        .expect("updated model3");
        fs::write(
            model_root.join("cat.physics3.json"),
            r#"{
              "Version":3,
              "Meta":{
                "PhysicsSettingCount":1,"TotalInputCount":1,"TotalOutputCount":1,"VertexCount":2,"Fps":60,
                "EffectiveForces":{"Gravity":{"X":0,"Y":-1},"Wind":{"X":0,"Y":0}},
                "PhysicsDictionary":[{"Id":"PhysicsSetting1","Name":"test"}]
              },
              "PhysicsSettings":[{
                "Id":"PhysicsSetting1",
                "Input":[{"Source":{"Target":"Parameter","Id":"ParamAngleX"},"Weight":100,"Type":"X","Reflect":false}],
                "Output":[{"Destination":{"Target":"Parameter","Id":"ParamAngleY"},"VertexIndex":1,"Scale":1,"Weight":100,"Type":"X","Reflect":false}],
                "Vertices":[
                  {"Position":{"X":0,"Y":0},"Mobility":1,"Delay":0,"Acceleration":0,"Radius":0},
                  {"Position":{"X":0,"Y":10},"Mobility":1,"Delay":0,"Acceleration":0,"Radius":10}
                ],
                "Normalization":{"Position":{"Minimum":-10,"Default":0,"Maximum":10},"Angle":{"Minimum":-10,"Default":0,"Maximum":10}}
              }]
            }"#,
        )
        .expect("physics fixture");

        let committed = PresetModelCatalog::open(&catalog_root, ModelPackageLimits::default())
            .expect("catalog")
            .load(&ModelId::parse("physics").expect("model id"))
            .expect("committed model");
        let mut model = Live2dModel::load(&committed).expect("Live2D model");
        model
            .set_parameter(ProductParameter::AngleX, 30.0)
            .expect("input");
        model
            .apply_physics(std::time::Duration::from_millis(100))
            .expect("physics evaluation");
        let output = model
            .parameter_value_by_id("ParamAngleY")
            .expect("physics value")
            .expect("physics parameter");
        assert!(output.abs() > 0.1, "physics output: {output}");
    }

    #[test]
    fn all_preset_models_expose_the_automatic_effect_parameter_contract() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let catalog = PresetModelCatalog::open(
            repository_root.join("resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog");
        for id in ["standard", "keyboard", "gamepad"] {
            let committed = catalog
                .load(&ModelId::parse(id).expect("model id"))
                .expect("preset model");
            let mut model = Live2dModel::load(&committed).expect("Live2D model");
            assert_eq!(
                model.eye_blink_parameter_ids,
                ["ParamEyeLOpen", "ParamEyeROpen"],
                "{id} EyeBlink group"
            );
            assert_eq!(
                model.breath_parameter_ids,
                ["ParamBreath"],
                "{id} Breath group"
            );
            let breath_range = model
                .core
                .parameter_range_by_id("ParamBreath")
                .unwrap_or_else(|| panic!("{id} breath parameter"));
            model
                .restore_parameter_defaults()
                .expect("restore parameter defaults");
            assert_eq!(
                model
                    .apply_automatic_effects(std::time::Duration::from_secs(1), -1.0)
                    .expect("automatic effects"),
                7,
                "{id} automatic effect count"
            );
            let breath = model
                .core
                .parameter_value_by_id("ParamBreath")
                .expect("breath value")
                .expect("supported breath parameter");
            assert!(
                breath > breath_range.default && breath < breath_range.maximum,
                "{id} reference breath stayed within its authored range: {breath}"
            );
        }
    }
}
