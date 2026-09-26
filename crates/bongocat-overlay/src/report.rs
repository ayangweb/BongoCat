//! What one frame of the overlay achieved.
//!
//! A frame is only reported as visible once something was actually presented:
//! the window existing is not the same as the user seeing the cat, and a
//! diagnostics report that conflated them would say the overlay works on a
//! machine where the GPU silently dropped every drawable.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductOverlayReport {
    pub frames_presented: u64,
    /// Whether the overlay window was fully on a display when the session
    /// finished, or the constraint was off. A window that is still waiting out
    /// its settle delay after a drag is reported as `false`.
    pub placement_fully_visible: bool,
    pub dynamic_snapshots: u64,
    pub model_commit_rejections: u64,
    pub input_start_error: Option<PlatformInputError>,
    pub input_diagnostics: Option<PlatformInputDiagnostics>,
    pub render_diagnostics: RenderTransportDiagnostics,
    pub model_generation: u64,
    pub drawable_count: usize,
    pub masked_drawable_count: usize,
    pub texture_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayTickOutcome {
    Presented,
    Hidden,
    Deferred(Duration),
}

impl OverlayTickOutcome {
    pub const fn retry_after(self) -> Option<Duration> {
        match self {
            Self::Deferred(delay) => Some(delay),
            Self::Presented | Self::Hidden => None,
        }
    }
}
