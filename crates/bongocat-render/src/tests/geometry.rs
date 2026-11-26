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

/// The reference model of issue #1129: a 1000x1000 legacy window, a 1972x2000
/// canvas and 986x1000 key images. The canvas is 986 window pixels wide inside
/// that window, so it starts 7 pixels to the right of the key image the legacy
/// application draws at the window's corner.
#[test]
fn a_legacy_key_image_is_offset_by_the_legacy_window_margin() {
    assert_eq!(legacy_key_frame_margin(986, 1000), 7);

    let canvas = CanvasInfo {
        width: 1972.0,
        height: 2000.0,
        origin_x: 986.0,
        origin_y: 1000.0,
        pixels_per_unit: 2000.0,
    };
    let bounds = ModelBounds::from_canvas(canvas);
    let overlay = legacy_key_overlay_bounds(bounds, 986, 1000);
    // Seven key-image pixels are fourteen canvas pixels, and the canvas is
    // 0.986 model units wide over 1972 canvas pixels.
    let shift = bounds.min_x - overlay.min_x;
    assert!((shift - 7.0 * 2.0 / 2000.0).abs() < 1e-6, "{shift}");
    assert_eq!(overlay.width(), bounds.width());
    assert_eq!(overlay.height(), bounds.height());
    assert_eq!(overlay.min_y, bounds.min_y);
    assert_eq!(overlay.max_y, bounds.max_y);
}

/// A key image at least as wide as it is tall already sits in a legacy window
/// that hugs the canvas, so it keeps the canvas quad. Every model the product
/// ships is in this case, and every one of them must stay exactly where it is.
#[test]
fn a_key_image_that_needs_no_margin_keeps_the_canvas_quad() {
    for (width, height) in [(612, 354), (1200, 1040), (986, 986), (1000, 986)] {
        assert_eq!(
            legacy_key_frame_margin(width, height),
            0,
            "{width}x{height}"
        );
        let bounds = ModelBounds::from_canvas(CanvasInfo {
            width: 1200.0,
            height: 1040.0,
            origin_x: 600.0,
            origin_y: 520.0,
            pixels_per_unit: 1040.0,
        });
        assert_eq!(
            legacy_key_overlay_bounds(bounds, width, height),
            bounds,
            "{width}x{height}"
        );
    }
    // A degenerate image has no margin to apply, and dividing by its width
    // would not be finite.
    assert_eq!(legacy_key_frame_margin(0, 1000), 0);
}

#[test]
fn a_quad_names_both_triangles_with_one_uv_orientation() {
    let bounds = ModelBounds {
        min_x: 1.0,
        max_x: 3.0,
        min_y: -2.0,
        max_y: 2.0,
    };
    let vertices = quad_vertices(bounds);
    assert_eq!(vertices[0].position, [1.0, -2.0]);
    assert_eq!(vertices[1].position, [3.0, -2.0]);
    assert_eq!(vertices[2].position, [3.0, 2.0]);
    assert_eq!(vertices[3].position, [1.0, 2.0]);
    assert_eq!(vertices[0].uv, [0.0, 0.0]);
    assert_eq!(vertices[1].uv, [1.0, 0.0]);
    assert_eq!(vertices[2].uv, [1.0, 1.0]);
    assert_eq!(vertices[3].uv, [0.0, 1.0]);
}
