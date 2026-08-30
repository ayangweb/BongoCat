#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use bongocat_model::CommittedModel;
use bongocat_render::{RenderResources, RenderSnapshot, TextureAsset, TextureId};
use std::{collections::BTreeMap, fmt, sync::Arc};

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
    MotionParameterSample, MotionPartOpacitySample,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod core;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod sys;

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
        write!(formatter, "{:?}: {}", self.code, self.detail)
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
        if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                "motion received an invalid playback weight",
            ));
        }
        let evaluation = motion.evaluate(elapsed);

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
                        .set_parameter_by_id(&sample.id, sample.value, 1.0)?,
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn expression_layers_apply_add_multiply_and_overwrite_to_core_parameters() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("native/resources/models"),
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
    fn part_opacity_motion_curves_use_the_framework_parameter_sink() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("native/resources/models"),
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
                {"Target":"PartOpacity","Id":"ParamAngleX","Segments":[0,0,0,1,20]},
                {"Target":"PartOpacity","Id":"MissingPartSink","Segments":[0,0,0,1,1]}
              ]
            }"#,
            0.0,
            1.0,
        )
        .expect("part opacity motion");

        model
            .restore_parameter_defaults()
            .expect("restore parameter defaults");
        let status = model
            .apply_motion_with_weight(&clip, std::time::Duration::from_millis(500), 0.0)
            .expect("apply part opacity motion");
        assert_eq!(status.applied_parameter_count, 0);
        assert_eq!(status.applied_part_opacity_count, 1);
        assert_eq!(
            model
                .parameter_value(ProductParameter::AngleX)
                .expect("angle parameter"),
            Some(10.0)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn model_motion_curves_apply_eye_blink_lip_sync_and_render_opacity() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use std::path::Path;

        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let committed = PresetModelCatalog::open(
            repository_root.join("native/resources/models"),
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
}
