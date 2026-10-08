//! Where the overlay lands, and how a logical size becomes a physical one.

use super::*;

#[test]
fn enumerated_displays_are_non_empty_and_cover_their_own_center() {
    let screens = screen_bounds_all();
    assert!(!screens.is_empty(), "the desktop has at least one display");
    for screen in &screens {
        assert!(screen.width >= 128 && screen.height >= 128);
        let center = OverlayWindowBounds::new(
            screen.x + (screen.width / 2) as i32 - 32,
            screen.y + (screen.height / 2) as i32 - 32,
            64,
            64,
        );
        assert!(bounds_inside_screens(&screens, center));
    }
    // A box starting one display-width past the right-most display cannot
    // intersect any of them.
    let right_most = screens
        .iter()
        .max_by_key(|screen| screen.x + screen.width as i32)
        .expect("at least one display");
    let off_desktop = OverlayWindowBounds::new(
        right_most.x + right_most.width as i32 + 1_000,
        right_most.y,
        350,
        350,
    );
    assert!(!bounds_inside_screens(&screens, off_desktop));
}

#[test]
fn window_sizing_uses_one_canvas_ratio_at_every_scale_and_dpi() {
    for (width, height) in [
        (700.0, 400.0),
        (612.0, 354.0),
        (350.0, 700.0),
        (350.0, 350.0),
        (1400.0, 350.0),
    ] {
        let sizing = WindowSizing::new(CanvasInfo {
            width,
            height,
            origin_x: width / 2.0,
            origin_y: height / 2.0,
            pixels_per_unit: 350.0,
        })
        .unwrap();
        for dpi in [96, 120, 144, 192] {
            for scale in 25..=400 {
                let (w, h) = sizing.dimensions_for_scale(dpi, scale);
                assert!(w >= 64 && h >= 64);
                let expected_height =
                    (f64::from(w) * f64::from(height) / f64::from(width)).ceil() as u32;
                assert_eq!(
                    h, expected_height,
                    "canvas {width}x{height}, DPI {dpi}, scale {scale}"
                );
                let actual = OverlayWindowBounds::new(100, 100, w, h);
                assert_eq!(
                    sizing.normalize(actual),
                    actual,
                    "normalization is idempotent"
                );
                let percent = sizing.scale_percent_for_width(dpi, w).unwrap();
                let acknowledged = sizing.dimensions_for_scale(dpi, percent);
                assert!(
                    w.abs_diff(acknowledged.0) <= 4,
                    "integer percentage rounding stays bounded"
                );
                if (w, h) == sizing.dimensions_for_scale(dpi, 25) {
                    assert_eq!(percent, 25);
                }
            }
        }
    }
    assert!(
        WindowSizing::new(CanvasInfo {
            width: 0.0,
            height: 350.0,
            origin_x: 0.0,
            origin_y: 0.0,
            pixels_per_unit: 350.0,
        })
        .is_none()
    );
}
