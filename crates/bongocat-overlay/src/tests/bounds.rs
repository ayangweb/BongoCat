//! A persisted box is honoured only while it still makes sense.

use super::*;

#[test]
fn persisted_overlay_bounds_are_bounded_and_scale_explicitly() {
    let bounds = OverlayWindowBounds::new(-640, 120, 400, 600)
        .validate()
        .expect("valid overlay bounds");
    assert_eq!(
        bounds.rescale(100, 125),
        OverlayWindowBounds::new(-640, 120, 500, 750)
    );
    assert!(OverlayWindowBounds::new(0, 0, 63, 600).validate().is_err());
    assert!(
        OverlayWindowBounds::new(1_000_001, 0, 400, 600)
            .validate()
            .is_err()
    );
}

#[test]
fn overlay_bounds_clamp_to_a_screen_without_changing_size() {
    let screen = OverlayScreenBounds {
        x: -1_920,
        y: 0,
        width: 1_920,
        height: 1_080,
    };
    assert_eq!(
        OverlayWindowBounds::new(-2_100, 900, 400, 300).clamp_to(screen),
        OverlayWindowBounds::new(-1_920, 780, 400, 300)
    );
    // A window larger than the display keeps its size and is pinned to the
    // display origin rather than being pushed off the opposite edge.
    assert_eq!(
        OverlayWindowBounds::new(-1_500, 100, 2_400, 1_200).clamp_to(screen),
        OverlayWindowBounds::new(-1_920, 0, 2_400, 1_200)
    );
}

#[test]
fn presentation_and_geometry_changes_use_in_place_window_transitions() {
    let current = OverlaySessionOptions::default();
    let mut next = current;
    next.always_on_top = false;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.click_through = true;
    assert!(!current.requires_window_recreation(next));

    // Hover hide has to work while the session keeps running, so it must
    // never be routed through a window replacement.
    next = current;
    next.hide_on_pointer_hover = true;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.hide_on_pointer_hover_delay_ms = 1_500;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.opacity_percent = 80;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.scale_percent = 125;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.corner_radius_percent = 25;
    assert!(current.requires_window_recreation(next));

    next = current;
    next.keep_inside_screen = false;
    assert!(current.requires_window_recreation(next));
}
