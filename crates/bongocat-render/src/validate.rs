//! The check that stands between a snapshot and a GPU upload.
//!
//! Every rule here is one a backend would otherwise have to assume: an index in
//! range, a count that matches, a drawable with no triangles allowed, a mask that
//! names a texture. The point is that the check happens once, on the platform-
//! neutral side, so the two backends cannot disagree about what a valid frame is
//! and neither has to repeat the work.

use super::*;

/// Platform-neutral validation required before a renderer allocates GPU resources.
///
/// Both native backends consume the same immutable snapshot. Keeping its basic
/// resource and geometry invariants here prevents a malformed model generation
/// from being accepted by one backend and rejected by the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RenderSnapshotValidationError {
    #[error("{}", Self::InvalidModelOpacity.message())]
    InvalidModelOpacity,
    #[error("{}", Self::DuplicateTextureId.message())]
    DuplicateTextureId,
    #[error("{}", Self::DuplicateDrawableId.message())]
    DuplicateDrawableId,
    #[error("{}", Self::MissingDrawableTexture.message())]
    MissingDrawableTexture,
    #[error("{}", Self::MissingMaskSource.message())]
    MissingMaskSource,
    #[error("{}", Self::EmptyDrawableGeometry.message())]
    EmptyDrawableGeometry,
    #[error("{}", Self::DrawableIndexOutOfRange.message())]
    DrawableIndexOutOfRange,
    #[error("{}", Self::NonFiniteVertex.message())]
    NonFiniteVertex,
    #[error("{}", Self::InvalidDrawableOpacity.message())]
    InvalidDrawableOpacity,
    #[error("{}", Self::NonFiniteBlendColor.message())]
    NonFiniteBlendColor,
}

impl RenderSnapshotValidationError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidModelOpacity => "model opacity is outside [0, 1]",
            Self::DuplicateTextureId => "texture resource ids are not unique",
            Self::DuplicateDrawableId => "drawable resource ids are not unique",
            Self::MissingDrawableTexture => "drawable references a missing texture",
            Self::MissingMaskSource => "drawable references a missing mask source",
            Self::EmptyDrawableGeometry => "drawable geometry is empty",
            Self::DrawableIndexOutOfRange => "drawable triangle index is out of range",
            Self::NonFiniteVertex => "drawable vertex contains a non-finite value",
            Self::InvalidDrawableOpacity => "drawable opacity is outside [0, 1]",
            Self::NonFiniteBlendColor => "drawable blend color contains a non-finite value",
        }
    }
}

pub fn validate_render_snapshot(
    resources: &RenderResources,
    snapshot: &RenderSnapshot,
) -> Result<(), RenderSnapshotValidationError> {
    if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
        return Err(RenderSnapshotValidationError::InvalidModelOpacity);
    }

    let texture_ids = resources
        .textures
        .iter()
        .map(|texture| texture.id)
        .collect::<BTreeSet<_>>();
    if texture_ids.len() != resources.textures.len() {
        return Err(RenderSnapshotValidationError::DuplicateTextureId);
    }

    let drawable_ids = snapshot
        .drawables
        .iter()
        .map(|drawable| drawable.id)
        .collect::<BTreeSet<_>>();
    if drawable_ids.len() != snapshot.drawables.len() {
        return Err(RenderSnapshotValidationError::DuplicateDrawableId);
    }

    for drawable in &snapshot.drawables {
        if !texture_ids.contains(&drawable.texture_id) {
            return Err(RenderSnapshotValidationError::MissingDrawableTexture);
        }
        if drawable
            .masks
            .iter()
            .any(|mask| !drawable_ids.contains(mask))
        {
            return Err(RenderSnapshotValidationError::MissingMaskSource);
        }
        // A drawable the Core reports with no triangles is a normal, if
        // uncommon, shape: it draws nothing, but other drawables may still name
        // it as a mask source, and the mask buffer it contributes to is then
        // simply left at its cleared value. Third-party models do ship such
        // drawables — an authoring tool that deletes every triangle of a part
        // without deleting the part leaves one behind — so rejecting the whole
        // model over it would fail a model every renderer can draw. Only a
        // triangle list that addresses no vertices at all is malformed, because
        // no backend can bind it.
        if drawable.vertices.is_empty() && !drawable.indices.is_empty() {
            return Err(RenderSnapshotValidationError::EmptyDrawableGeometry);
        }
        if drawable
            .indices
            .iter()
            .any(|index| usize::from(*index) >= drawable.vertices.len())
        {
            return Err(RenderSnapshotValidationError::DrawableIndexOutOfRange);
        }
        if drawable.vertices.iter().any(|vertex| {
            vertex
                .position
                .into_iter()
                .chain(vertex.uv)
                .any(|value| !value.is_finite())
        }) {
            return Err(RenderSnapshotValidationError::NonFiniteVertex);
        }
        if !drawable.opacity.is_finite() || !(0.0..=1.0).contains(&drawable.opacity) {
            return Err(RenderSnapshotValidationError::InvalidDrawableOpacity);
        }
        if drawable
            .multiply_color
            .into_iter()
            .chain(drawable.screen_color)
            .any(|value| !value.is_finite())
        {
            return Err(RenderSnapshotValidationError::NonFiniteBlendColor);
        }
    }

    Ok(())
}
