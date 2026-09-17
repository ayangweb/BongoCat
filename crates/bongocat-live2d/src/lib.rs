#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use bongocat_model::CommittedModel;
use bongocat_render::{RenderResources, RenderSnapshot, TextureAsset, TextureId};
use image::ImageReader;
use std::{collections::BTreeMap, fmt, fs, sync::Arc};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::collections::BTreeSet;

mod expression;
pub use expression::{
    ExpressionApplyStatus, ExpressionBlendMode, ExpressionClip, ExpressionLayer,
    ExpressionParameter,
};

mod motion;
pub use motion::{
    MotionApplyStatus, MotionClip, MotionCurveTarget, MotionEvaluation, MotionModelSample,
    MotionParameterSample, MotionPartOpacitySample, MotionUserDataEvaluation, MotionUserDataEvent,
    MotionUserDataOccurrence,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod core;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod core_log;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod sys;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use core_log::{CoreLogError, CoreLogHandle, CoreLogReporter, CoreLogStats};

pub const CUBISM_SDK_RELEASE: &str = "5-r.5";
pub const CUBISM_CORE_VERSION: u32 = 0x0600_0001;
pub const CUBISM_LATEST_MOC_VERSION: u32 = 6;
#[cfg(any(target_os = "macos", target_os = "windows"))]
const MAX_MODEL_EFFECT_TARGETS: usize = 64;

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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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
    PlatformUnsupported,
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
    pub const ALL: [Self; 17] = [
        Self::CoreVersionMismatch,
        Self::EmptyMoc,
        Self::InvalidCoreArray,
        Self::InvalidCoreValue,
        Self::MocConsistencyFailed,
        Self::MocReviveFailed,
        Self::ModelInitializeFailed,
        Self::ModelMemoryInvalid,
        Self::PlatformUnsupported,
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
            Self::PlatformUnsupported => "platform_unsupported",
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

#[derive(Debug)]
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

impl fmt::Display for Live2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Live2dError {}

pub struct Live2dModel {
    resources: Arc<RenderResources>,
    motions: BTreeMap<String, Vec<MotionClip>>,
    expressions: BTreeMap<String, ExpressionClip>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    eye_blink_parameter_ids: Vec<String>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    lip_sync_parameter_ids: Vec<String>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    model_opacity: f32,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    core: core::CoreModel,
}

impl Live2dModel {
    pub fn load(model: &CommittedModel) -> Result<Self, Live2dError> {
        let background = load_background_asset(model.root())?;
        let key_assets = load_key_assets(model.root())?;
        let resources = Arc::new(RenderResources {
            textures: model
                .index()
                .textures
                .iter()
                .enumerate()
                .map(|(index, texture)| TextureAsset {
                    id: TextureId::new(index),
                    path: model.root().join(&texture.file),
                    width: texture.width,
                    height: texture.height,
                })
                .collect::<Vec<_>>(),
            key_assets,
            background,
        });
        let motions = model
            .index()
            .motion_groups
            .iter()
            .map(|group| {
                let clips = group
                    .motions
                    .iter()
                    .enumerate()
                    .map(|(index, _)| MotionClip::load(model, &group.name, index))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((group.name.clone(), clips))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut expressions = BTreeMap::new();
        for resource in &model.index().expressions {
            let name = resource.name.clone();
            let clip = ExpressionClip::load(model, &name)?;
            if expressions.insert(name.clone(), clip).is_some() {
                return Err(Live2dError::new(
                    Live2dErrorCode::ExpressionInvalid,
                    format!("model3 declares expression name {name:?} more than once"),
                ));
            }
        }

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let eye_blink_parameter_ids = parameter_group_ids(model, "EyeBlink");
            let lip_sync_parameter_ids = parameter_group_ids(model, "LipSync");
            let moc_path = model.root().join(&model.index().moc);
            let core = core::CoreModel::load(&moc_path)?;
            core.validate_texture_indices(resources.textures.len())?;
            Ok(Self {
                resources,
                motions,
                expressions,
                eye_blink_parameter_ids,
                lip_sync_parameter_ids,
                model_opacity: 1.0,
                core,
            })
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (resources, motions, expressions);
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn texture_assets(&self) -> &[TextureAsset] {
        &self.resources.textures
    }

    pub fn render_resources(&self) -> Arc<RenderResources> {
        Arc::clone(&self.resources)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.parameter_range(parameter)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = parameter;
            None
        }
    }

    pub fn parameter_value(&self, parameter: ProductParameter) -> Result<Option<f32>, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.parameter_value(parameter)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = parameter;
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    /// Read a model-declared parameter by its stable Core identifier.
    /// Unknown IDs return `None` so optional effect groups remain portable.
    pub fn parameter_value_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.parameter_value_by_id(id)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = id;
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn set_parameter(
        &mut self,
        parameter: ProductParameter,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.set_parameter(parameter, value)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (parameter, value);
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
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
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
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
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = id;
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn apply_automatic_effects(
        &mut self,
        breath: f32,
        eye_blink: f32,
    ) -> Result<usize, Live2dError> {
        let mut applied = usize::from(matches!(
            self.set_normalized_parameter_by_id("ParamBreath", breath)?,
            ParameterUpdate::Applied { .. }
        ));
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

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let model_opacity_applied = if let Some(opacity) = evaluation.model.opacity {
            self.model_opacity = opacity.clamp(0.0, 1.0);
            true
        } else {
            false
        };

        #[cfg(any(target_os = "macos", target_os = "windows"))]
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

        #[cfg(any(target_os = "macos", target_os = "windows"))]
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

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let applied_parameter_count = {
            let _ = (
                &evaluation.model,
                &evaluation.parameters,
                &evaluation.part_opacities,
                weight,
            );
            0
        };

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let applied_part_opacity_count = 0;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let applied_eye_blink_count = 0;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let applied_lip_sync_count = 0;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let model_opacity_applied = false;

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

        #[cfg(any(target_os = "macos", target_os = "windows"))]
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
                let mut overwrite = current;
                let mut additive = 0.0;
                let mut multiply = 1.0;
                for layer in layers {
                    let (next_overwrite, next_additive, next_multiply) =
                        match layer.clip.parameter(id) {
                            Some(parameter) => match parameter.blend {
                                ExpressionBlendMode::Additive => (current, parameter.value, 1.0),
                                ExpressionBlendMode::Multiply => (current, 0.0, parameter.value),
                                ExpressionBlendMode::Overwrite => (parameter.value, 0.0, 1.0),
                            },
                            None => (current, 0.0, 1.0),
                        };
                    overwrite += (next_overwrite - overwrite) * layer.weight;
                    additive += (next_additive - additive) * layer.weight;
                    multiply += (next_multiply - multiply) * layer.weight;
                }
                let target = (overwrite + additive) * multiply;
                if matches!(
                    self.core.set_parameter_by_id(id, target, 1.0)?,
                    ParameterUpdate::Applied { .. }
                ) {
                    applied += 1;
                }
            }
            applied
        };

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let applied_parameter_count = {
            let _ = layers;
            0
        };

        Ok(ExpressionApplyStatus {
            applied_parameter_count,
        })
    }

    pub fn restore_parameter_defaults(&mut self) -> Result<(), Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.restore_parameter_defaults()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn update_and_snapshot(&mut self) -> Result<RenderSnapshot, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let mut snapshot = self.core.update_and_snapshot()?;
            snapshot.model_opacity = self.model_opacity;
            Ok(snapshot)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }
}

pub fn resolve_key_overlays(
    resources: &RenderResources,
    presses: bongocat_render::KeyPressSet,
) -> Vec<bongocat_render::KeyOverlay> {
    let mut selected = [None, None];
    for press in presses.iter() {
        let side_index = match press.side {
            bongocat_render::KeySide::Left => 0,
            bongocat_render::KeySide::Right => 1,
        };
        // Runtime supplies the most recently pressed key for each side. Clear
        // the slot before resolving so an unavailable current key never
        // reuses an older overlay from that side.
        selected[side_index] = None;
        let candidates = key_name_candidates(press.hid_usage);
        let Some(asset) = candidates.iter().find_map(|candidate| {
            resources
                .key_assets
                .iter()
                .find(|asset| asset.side == press.side && asset.name == *candidate)
        }) else {
            continue;
        };
        selected[side_index] = Some(bongocat_render::KeyOverlay {
            asset_id: asset.id,
            side: press.side,
        });
    }
    selected.into_iter().flatten().collect()
}

fn load_key_assets(root: &std::path::Path) -> Result<Vec<bongocat_render::KeyAsset>, Live2dError> {
    let mut assets = Vec::new();
    for (side, directory) in [
        (bongocat_render::KeySide::Left, "left-keys"),
        (bongocat_render::KeySide::Right, "right-keys"),
    ] {
        let path = root.join("resources").join(directory);
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        let mut files = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|file| {
                file.is_file()
                    && file
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            })
            .collect::<Vec<_>>();
        files.sort();
        for file in files {
            let image = ImageReader::open(&file)
                .map_err(|error| Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string()))?
                .decode()
                .map_err(|error| {
                    Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string())
                })?;
            let Some(name) = file.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            assets.push(bongocat_render::KeyAsset {
                id: bongocat_render::KeyAssetId::new(assets.len()),
                side,
                name: name.to_owned(),
                path: file,
                width: image.width(),
                height: image.height(),
            });
        }
    }
    Ok(assets)
}

/// Asset names a pressed key can be drawn with, most specific first.
///
/// A model may ship one image per key or a single shared image for a whole key
/// family. The HID function keys F1 … F24 therefore resolve to their own
/// `F1.png` … `F24.png` when the model provides one and fall back to the shared
/// `Fn.png` otherwise; every other key only ever has an exact name plus, for the
/// four modifier pairs, the shared side-independent asset (`Control`, `Shift`,
/// `Alt`, `Meta`). A name that the model does not provide is skipped, so an
/// incomplete model simply draws nothing for that key.
///
/// `AltGr` is the one legacy name in this table. BongoCat models written before
/// the import normalizer existed ship the right Alt artwork as
/// `AltGr.png` — the name the old `rdev`-based input layer used — and a package
/// that reaches the model store without passing through that normalizer (an
/// install predating it, or a model directory placed by hand) still has to draw
/// the right artwork instead of the left one. It is deliberately right-Alt-only:
/// `Alt.png` stays the shared family image, exactly as it was before.
fn key_name_candidates(hid_usage: u16) -> Vec<&'static str> {
    let function_key = bongocat_render::function_key_name(hid_usage);
    let exact = match hid_usage {
        0x04..=0x1d => Some(KEY_LETTERS[usize::from(hid_usage - 0x04)]),
        0x1e..=0x27 => Some(KEY_NUMBERS[usize::from(hid_usage - 0x1e)]),
        0x28 => Some("Return"),
        0x29 => Some("Escape"),
        0x2a => Some("Backspace"),
        0x2b => Some("Tab"),
        0x2c => Some("Space"),
        0x35 => Some("BackQuote"),
        0x38 => Some("Slash"),
        0x39 => Some("CapsLock"),
        0x4f => Some("RightArrow"),
        0x50 => Some("LeftArrow"),
        0x51 => Some("DownArrow"),
        0x52 => Some("UpArrow"),
        0xe0 => Some("ControlLeft"),
        0xe1 => Some("ShiftLeft"),
        0xe2 => Some("AltLeft"),
        0xe3 => Some("MetaLeft"),
        0xe4 => Some("ControlRight"),
        0xe5 => Some("ShiftRight"),
        0xe6 => Some("AltRight"),
        0xe7 => Some("MetaRight"),
        // Function keys are the only named keys left, and the whole HID range is
        // covered by one arithmetic lookup instead of 24 arms here.
        _ => function_key,
    };
    let mut candidates = Vec::with_capacity(2);
    if let Some(exact) = exact {
        candidates.push(exact);
    }
    if function_key.is_some() {
        // The model's shared function-key image, and always the last candidate:
        // a dedicated `F1.png` … `F24.png` wins, every function key the model
        // did not draw individually lands on `Fn.png`.
        candidates.push("Fn");
    }
    match hid_usage {
        0xe0 | 0xe4 => candidates.push("Control"),
        0xe1 | 0xe5 => candidates.push("Shift"),
        0xe2 => candidates.push("Alt"),
        0xe6 => {
            // Right Alt keeps its pre-rename name as an alias between the exact
            // `AltRight` and the shared `Alt`: a legacy model draws its own
            // right artwork when it has one, and the family image otherwise.
            candidates.push("AltGr");
            candidates.push("Alt");
        }
        0xe3 | 0xe7 => candidates.push("Meta"),
        _ => {}
    }
    candidates
}

const KEY_LETTERS: [&str; 26] = [
    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL",
    "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX",
    "KeyY", "KeyZ",
];
const KEY_NUMBERS: [&str; 10] = [
    "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9", "Num0",
];

fn load_background_asset(
    root: &std::path::Path,
) -> Result<Option<bongocat_render::BackgroundAsset>, Live2dError> {
    let path = root.join("resources/background.png");
    if !path.is_file() {
        return Ok(None);
    }
    let image = ImageReader::open(&path)
        .map_err(|error| Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string()))?
        .decode()
        .map_err(|error| Live2dError::new(Live2dErrorCode::ResourceIo, error.to_string()))?;
    Ok(Some(bongocat_render::BackgroundAsset {
        path,
        width: image.width(),
        height: image.height(),
    }))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
    use bongocat_render::BlendMode;

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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn automatic_effects_use_declared_group_and_optional_breath_parameter() {
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
        let applied = model
            .apply_automatic_effects(1.0, -1.0)
            .expect("automatic effects");
        assert_eq!(applied, 3);
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
        assert_eq!(
            model
                .core
                .parameter_value_by_id("ParamBreath")
                .expect("breath"),
            Some(1.0)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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
            assert!(
                model.core.parameter_range_by_id("ParamBreath").is_some(),
                "{id} breath parameter"
            );
            model
                .restore_parameter_defaults()
                .expect("restore parameter defaults");
            assert_eq!(
                model
                    .apply_automatic_effects(1.0, -1.0)
                    .expect("automatic effects"),
                3,
                "{id} automatic effect count"
            );
        }
    }
}
