//! Display geometry, shared by both platform backends.
//!
//! The two input services each report the connected displays in the same shape
//! so the placement rules — which display a window sits on, whether it is
//! visible at all — are written once instead of per platform.

/// One display's frame in the virtual desktop coordinate space.
///
/// `display_id` is the platform's own identifier and is only used to tell two
/// otherwise identical frames apart; the geometry is what every consumer acts
/// on, and it may be negative or larger than the primary display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayBounds {
    pub display_id: Option<u32>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DisplayBounds {
    /// Whether a window box overlaps this display at all.
    ///
    /// Edge-touching counts as not visible, which is what keeps a window the
    /// user has dragged onto a second monitor from being clamped back onto the
    /// first one.
    pub(crate) fn intersects_window(self, x: f32, y: f32, width: f32, height: f32) -> bool {
        x < self.x + self.width
            && x + width > self.x
            && y < self.y + self.height
            && y + height > self.y
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayBounds;

    #[test]
    fn window_visibility_handles_negative_and_edge_touching_displays() {
        let secondary = DisplayBounds {
            display_id: Some(1),
            x: -1920.0,
            y: -240.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert!(secondary.intersects_window(-1200.0, 100.0, 800.0, 600.0));
        assert!(secondary.intersects_window(-10.0, 100.0, 800.0, 600.0));
        assert!(!secondary.intersects_window(0.0, 100.0, 800.0, 600.0));
        assert!(!secondary.intersects_window(-1200.0, 840.0, 800.0, 600.0));
    }
}
