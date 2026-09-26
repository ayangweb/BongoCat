//! The Live2D adapter: a committed model, and everything the product does to it.
//!
//! `Live2dModel` is the whole surface. It loads a model, exposes its parameters
//! and clips, applies motions, expressions, physics and the automatic effects,
//! and produces the render snapshot. The parts those methods are made of are the
//! modules beside it.

mod automatic;
mod error;
mod load;
mod parameter;
mod status;
#[cfg(test)]
mod tests;

// The crate-private half. `error`, `parameter` and `status` hold only `pub`
// items and are re-exported by name below, so a glob for them would be
// narrowing nothing but saying something.
pub(crate) use automatic::*;
pub(crate) use load::*;

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

// The public surface, one module at a time. These items are already public, so
// the re-exports are the whole of it and no glob is needed to carry a second,
// narrower copy.
pub use error::{Live2dError, Live2dErrorCode};
pub use parameter::{ParameterRange, ParameterUpdate, ProductParameter};
pub use status::{ExpressionApplyStatus, MotionApplyStatus};

pub const CUBISM_SDK_RELEASE: &str = "5-r.5";

pub const CUBISM_CORE_VERSION: u32 = 0x0600_0001;

pub const CUBISM_LATEST_MOC_VERSION: u32 = 6;

pub struct Live2dModel {
    pub(crate) resources: Arc<RenderResources>,
    pub(crate) motions: BTreeMap<String, Vec<MotionClip>>,
    pub(crate) expressions: BTreeMap<String, ExpressionClip>,
    pub(crate) breath_parameter_ids: Vec<String>,
    pub(crate) eye_blink_parameter_ids: Vec<String>,
    pub(crate) lip_sync_parameter_ids: Vec<String>,
    pub(crate) physics: Option<physics::PhysicsRuntime>,
    pub(crate) model_opacity: f32,
    pub(crate) core: core::CoreModel,
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

    pub(crate) fn add_parameter_by_id_with_weight(
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

    pub(crate) fn set_parameter_by_id_with_weight(
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

    pub(crate) fn apply_automatic_breath(
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

    pub(crate) fn apply_motion_with_weight_and_looping(
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
