//! The window is sized from the model, and a switch does not resize twice.

use super::*;

#[test]
fn corner_radius_uniform_clamps_to_the_full_ellipse() {
    assert_eq!(
        corner_radius_uniform(0, 350.0, 200.0),
        [0.0, 350.0, 200.0, 0.0]
    );
    assert_eq!(
        corner_radius_uniform(25, 350.0, 200.0),
        [0.25, 350.0, 200.0, 0.0]
    );
    // The legacy percentage ceiling: 50% already inscribes the full
    // ellipse, so larger values must not shrink the visible window further.
    assert_eq!(
        corner_radius_uniform(50, 350.0, 200.0),
        [0.5, 350.0, 200.0, 0.0]
    );
    assert_eq!(
        corner_radius_uniform(100, 350.0, 200.0),
        corner_radius_uniform(50, 350.0, 200.0)
    );
}

#[test]
fn default_overlay_height_follows_the_model_aspect_ratio() {
    let landscape = CanvasInfo {
        width: 700.0,
        height: 400.0,
        origin_x: 350.0,
        origin_y: 200.0,
        pixels_per_unit: 400.0,
    };
    let portrait = CanvasInfo {
        width: 350.0,
        height: 700.0,
        origin_x: 175.0,
        origin_y: 350.0,
        pixels_per_unit: 350.0,
    };

    assert_eq!(default_overlay_window_dimensions(landscape), (350, 200));
    assert_eq!(default_overlay_window_dimensions(portrait), (350, 700));
}

#[test]
fn standard_model_switch_matches_the_initial_window_height() {
    let standard = CanvasInfo {
        width: 612.0,
        height: 354.0,
        origin_x: 306.0,
        origin_y: 177.0,
        pixels_per_unit: 354.0,
    };
    let current = OverlayWindowBounds::new(0, 0, 350, 203);

    assert_eq!(default_overlay_window_dimensions(standard), (350, 203));
    assert_eq!(
        model_switch_window_bounds(current, standard),
        OverlayWindowBounds::new(0, 0, 350, 203)
    );

    let (scaled_width, scaled_height) = model_window_dimensions(standard, 125);
    assert_eq!(
        model_switch_window_bounds(
            OverlayWindowBounds::new(0, 0, scaled_width, scaled_height),
            standard,
        ),
        OverlayWindowBounds::new(0, 0, scaled_width, scaled_height)
    );
}

#[test]
fn model_switch_keeps_width_and_recomputes_height_at_the_live_scale() {
    let landscape = CanvasInfo {
        width: 700.0,
        height: 400.0,
        origin_x: 350.0,
        origin_y: 200.0,
        pixels_per_unit: 400.0,
    };
    let portrait = CanvasInfo {
        width: 350.0,
        height: 700.0,
        origin_x: 175.0,
        origin_y: 350.0,
        pixels_per_unit: 350.0,
    };
    let current = OverlayWindowBounds::new(-240, 180, 700, 123);

    assert_eq!(
        model_switch_window_bounds(current, landscape),
        OverlayWindowBounds::new(-240, 180, 700, 400)
    );
    assert_eq!(
        model_switch_window_bounds(current, portrait),
        OverlayWindowBounds::new(-240, 180, 700, 1_400)
    );
}

#[test]
fn model_switch_height_uses_the_existing_width_instead_of_rescaling_twice() {
    let canvas = CanvasInfo {
        width: 350.0,
        height: 700.0,
        origin_x: 175.0,
        origin_y: 350.0,
        pixels_per_unit: 350.0,
    };
    // 50% of the 350px base width is 175px. The new height must be 350px,
    // not another 50% applied to an already scaled width.
    assert_eq!(
        model_switch_window_bounds(OverlayWindowBounds::new(0, 0, 175, 1), canvas),
        OverlayWindowBounds::new(0, 0, 175, 350)
    );

    // The result remains within the same bounds contract as a native window.
    let extreme = CanvasInfo {
        width: 100_000.0,
        height: 1.0,
        ..canvas
    };
    assert_eq!(
        model_switch_window_bounds(OverlayWindowBounds::new(0, 0, 64, 1), extreme),
        OverlayWindowBounds::new(0, 0, 64, 64)
    );
}
