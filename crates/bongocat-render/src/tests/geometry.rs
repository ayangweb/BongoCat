//! The conversion from Cubism's orientation, done once.

use super::*;

#[test]
fn canvas_bounds_keep_core_origin_orientation() {
    let bounds = ModelBounds::from_canvas(CanvasInfo {
        width: 100.0,
        height: 80.0,
        origin_x: 20.0,
        origin_y: 30.0,
        pixels_per_unit: 10.0,
    });
    assert_eq!(bounds.center(), [3.0, -1.0]);
    assert_eq!(bounds.width(), 10.0);
    assert_eq!(bounds.height(), 8.0);
}
