//! The options the renderer can actually honour.

use super::*;

#[test]
fn product_options_accept_config_boundaries() {
    for options in [
        OverlaySessionOptions {
            scale_percent: 25,
            opacity_percent: 1,
            corner_radius_percent: 0,
            maximum_fps: 15,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            scale_percent: 400,
            opacity_percent: 100,
            corner_radius_percent: 50,
            hide_on_pointer_hover: true,
            hide_on_pointer_hover_delay_ms: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS,
            maximum_fps: 240,
            ..OverlaySessionOptions::default()
        },
    ] {
        validate_product_options(options).expect("valid product options");
    }
}

#[test]
fn product_options_reject_values_outside_config_boundaries() {
    for options in [
        OverlaySessionOptions {
            scale_percent: 24,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            scale_percent: 401,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            opacity_percent: 0,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            corner_radius_percent: 51,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            hide_on_pointer_hover_delay_ms: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS + 1,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            maximum_fps: 14,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            maximum_fps: 241,
            ..OverlaySessionOptions::default()
        },
    ] {
        assert!(validate_product_options(options).is_err());
    }
}
