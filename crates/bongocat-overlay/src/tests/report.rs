//! Visibility means a frame was presented, not that a window exists.

use super::*;

#[test]
fn overlay_visibility_requires_a_successfully_presented_frame() {
    let mut presentation = OverlayPresentationState::default();
    assert!(presentation.require_presented_frame().is_err());

    presentation.record_presented_frame();
    presentation
        .require_presented_frame()
        .expect("presented overlay may become visible");
}
