//! Reading the clips a model ships.
//!
//! A motion and an expression are separate resources with separate schemas, and
//! a model that ships one without the other is normal rather than broken: a
//! missing clip is `None`, and only a clip that is present and unreadable is an
//! error.

use super::*;

pub(crate) fn load_motion_clip(
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

pub(crate) fn load_expression_clip(
    model: &CommittedModel,
    name: &str,
) -> Result<ExpressionClip, Live2dError> {
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

pub(crate) fn parameter_group_ids(model: &CommittedModel, name: &str) -> Vec<String> {
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
