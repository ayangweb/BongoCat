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

    // The idle hide is applied inside the running tick for the same reason.
    next = current;
    next.hide_on_idle = true;
    assert!(!current.requires_window_recreation(next));

    next = current;
    next.hide_on_idle_delay_ms = 10_000;
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

/// Recalling the modifier is read inside the tick, like the hover hide, so that
/// a window replacement never has to happen to start and stop giving the pointer
/// back.
#[test]
fn recalling_the_hold_modifier_never_replaces_the_window() {
    let current = OverlaySessionOptions::default();
    let mut next = current;
    next.hold_modifier_to_interact = Some(ModifierKey::RightShift);
    assert!(!current.requires_window_recreation(next));
}

/// The hold answers one question about the current frame, and it answers it for
/// the physical key rather than the family.
///
/// The overlay reads this on every tick to decide both whether the window passes
/// pointer events through and whether the hover hide may run, so a match against
/// the wrong key would either never give the pointer back or give it back from a
/// key the user is not holding.
#[test]
fn the_hold_modifier_answers_for_the_configured_physical_key_only() {
    let held = |modifier| {
        let mut pressed = PressedModifiers::NONE;
        pressed.insert(modifier);
        pressed
    };
    let configured = OverlaySessionOptions {
        hold_modifier_to_interact: Some(ModifierKey::LeftShift),
        ..OverlaySessionOptions::default()
    };
    assert!(!configured.hold_modifier_pressed(PressedModifiers::NONE));
    assert!(!configured.hold_modifier_pressed(held(ModifierKey::RightShift)));
    assert!(!configured.hold_modifier_pressed(held(ModifierKey::LeftControl)));
    assert!(
        configured.hold_modifier_pressed(held(ModifierKey::LeftShift)),
        "the configured key is held"
    );

    let right = OverlaySessionOptions {
        hold_modifier_to_interact: Some(ModifierKey::RightShift),
        ..OverlaySessionOptions::default()
    };
    assert!(!right.hold_modifier_pressed(held(ModifierKey::LeftShift)));
    assert!(right.hold_modifier_pressed(held(ModifierKey::RightShift)));

    // Unconfigured is not "held by everything": with no key chosen the overlay
    // keeps behaving exactly as configured, which is what the shipped default has
    // to mean.
    let unconfigured = OverlaySessionOptions::default();
    assert!(!unconfigured.hold_modifier_pressed(PressedModifiers::NONE));
    for modifier in ModifierKey::ALL {
        assert!(
            !unconfigured.hold_modifier_pressed(held(modifier)),
            "{modifier:?} must not suspend anything when no modifier is configured"
        );
    }

    // The runtime settings carry the same key through unchanged, so what the page
    // recorded is what the overlay watches for.
    let projected = OverlaySessionOptions::default().with_runtime_settings(OverlaySettings {
        hold_modifier_to_interact: Some(ModifierKey::RightMeta),
        ..OverlaySettings::default()
    });
    assert_eq!(
        projected.hold_modifier_to_interact,
        Some(ModifierKey::RightMeta)
    );
    assert!(projected.hold_modifier_pressed(held(ModifierKey::RightMeta)));
    assert!(!projected.hold_modifier_pressed(held(ModifierKey::LeftMeta)));
}
