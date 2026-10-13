//! Fitting a model into the window without distorting it.

use super::*;

#[test]
fn model_transform_preserves_aspect_ratio() {
    let canvas = CanvasInfo {
        width: 2048.0,
        height: 2048.0,
        origin_x: 1024.0,
        origin_y: 1024.0,
        pixels_per_unit: 1024.0,
    };
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, false),
        [1.0, 1.0, -0.0, -0.0]
    );
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 1600.0, 800.0, false),
        [0.5, 1.0, -0.0, -0.0]
    );
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, true),
        [-1.0, 1.0, 0.0, -0.0]
    );
    // A viewport taller than the model's canvas keeps the canvas against the
    // bottom edge: the whole margin is headroom above the canvas, where a
    // model may draw effects of its own.
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 800.0, 1600.0, false),
        [1.0, 0.5, -0.0, -0.5]
    );
}
