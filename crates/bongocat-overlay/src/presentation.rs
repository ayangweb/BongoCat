//! What the window looks like between frames.
//!
//! The overlay's background, corner radius and opacity are read from one place so
//! that a change to any of them is applied the same way, whether it came from a
//! settings change or from the model switch that also resized the window.

use super::*;

#[derive(Default)]
pub(crate) struct OverlayPresentationState {
    pub(crate) has_presented_frame: bool,
}

impl OverlayPresentationState {
    pub(crate) fn record_presented_frame(&mut self) {
        self.has_presented_frame = true;
    }

    pub(crate) fn require_presented_frame(&self) -> Result<(), OverlayError> {
        if !self.has_presented_frame {
            return Err(OverlayError::new(
                "overlay cannot become visible before its first presented frame",
            ));
        }
        Ok(())
    }
}
