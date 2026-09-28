//! Every rule a backend would otherwise have to assume.

use super::*;

#[test]
fn render_snapshot_validation_accepts_complete_drawable_resources() {
    assert_eq!(
        validate_render_snapshot(&validated_resources(), &validated_snapshot()),
        Ok(())
    );
}

/// A drawable the Core reports with vertices but no triangles draws
/// nothing, and a model that ships one is still drawable. The mask buffer
/// it is named by simply keeps the value the backend cleared it to, which
/// is exactly what rendering zero triangles would have left there.
#[test]
fn render_snapshot_validation_accepts_a_drawable_without_triangles() {
    let resources = validated_resources();
    let mut snapshot = validated_snapshot();
    snapshot.drawables[0].indices.clear();
    assert_eq!(validate_render_snapshot(&resources, &snapshot), Ok(()));

    // The malformed neighbour of that shape is the other way round: a
    // triangle list whose vertices are gone, which no backend can bind.
    snapshot.drawables[0].vertices.clear();
    snapshot.drawables[0].indices = vec![0, 1, 2];
    assert_eq!(
        validate_render_snapshot(&resources, &snapshot),
        Err(RenderSnapshotValidationError::EmptyDrawableGeometry)
    );
}

#[test]
fn render_snapshot_validation_rejects_each_shared_gpu_preflight_violation() {
    let resources = validated_resources();
    let snapshot = validated_snapshot();

    let mut invalid_opacity = snapshot.clone();
    invalid_opacity.model_opacity = 1.5;
    assert_eq!(
        validate_render_snapshot(&resources, &invalid_opacity),
        Err(RenderSnapshotValidationError::InvalidModelOpacity)
    );

    let mut duplicate_texture = resources.clone();
    duplicate_texture
        .textures
        .push(duplicate_texture.textures[0].clone());
    assert_eq!(
        validate_render_snapshot(&duplicate_texture, &snapshot),
        Err(RenderSnapshotValidationError::DuplicateTextureId)
    );

    let mut duplicate_drawable = snapshot.clone();
    duplicate_drawable
        .drawables
        .push(duplicate_drawable.drawables[0].clone());
    assert_eq!(
        validate_render_snapshot(&resources, &duplicate_drawable),
        Err(RenderSnapshotValidationError::DuplicateDrawableId)
    );

    let mut missing_texture = snapshot.clone();
    missing_texture.drawables[0].texture_id = TextureId::new(1);
    assert_eq!(
        validate_render_snapshot(&resources, &missing_texture),
        Err(RenderSnapshotValidationError::MissingDrawableTexture)
    );

    let mut missing_mask = snapshot.clone();
    missing_mask.drawables[0].masks.push(DrawableId::new(1));
    assert_eq!(
        validate_render_snapshot(&resources, &missing_mask),
        Err(RenderSnapshotValidationError::MissingMaskSource)
    );

    let mut empty_geometry = snapshot.clone();
    empty_geometry.drawables[0].vertices.clear();
    assert_eq!(
        validate_render_snapshot(&resources, &empty_geometry),
        Err(RenderSnapshotValidationError::EmptyDrawableGeometry)
    );

    let mut out_of_range_index = snapshot.clone();
    out_of_range_index.drawables[0].indices = vec![3];
    assert_eq!(
        validate_render_snapshot(&resources, &out_of_range_index),
        Err(RenderSnapshotValidationError::DrawableIndexOutOfRange)
    );

    let mut non_finite_vertex = snapshot.clone();
    non_finite_vertex.drawables[0].vertices[0].uv[0] = f32::NAN;
    assert_eq!(
        validate_render_snapshot(&resources, &non_finite_vertex),
        Err(RenderSnapshotValidationError::NonFiniteVertex)
    );

    let mut invalid_drawable_opacity = snapshot.clone();
    invalid_drawable_opacity.drawables[0].opacity = -0.1;
    assert_eq!(
        validate_render_snapshot(&resources, &invalid_drawable_opacity),
        Err(RenderSnapshotValidationError::InvalidDrawableOpacity)
    );

    let mut non_finite_color = snapshot;
    non_finite_color.drawables[0].screen_color[3] = f32::INFINITY;
    assert_eq!(
        validate_render_snapshot(&resources, &non_finite_color),
        Err(RenderSnapshotValidationError::NonFiniteBlendColor)
    );
}
