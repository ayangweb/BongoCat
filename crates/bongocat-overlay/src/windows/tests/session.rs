//! The options the renderer can actually honour.

use super::*;

#[test]
fn product_options_reject_values_outside_renderer_boundaries() {
    for options in [
        OverlaySessionOptions {
            scale_percent: 24,
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
        assert!(validate_options(options).is_err());
    }
}
