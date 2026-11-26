//! Whether a presented frame is actually a picture, and what to do when it is not
//! yet.
//!
//! A GPU that has just come up can present a frame with no pixels in it. The
//! smoke check is what tells that apart from a model that really is blank, and
//! the backoff is what stops a machine in that state from reporting an error
//! every single frame.

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FramePixelStatistics {
    pub sampled_pixels: usize,
    pub transparent_pixels: usize,
    pub translucent_pixels: usize,
    pub opaque_pixels: usize,
    pub distinct_visible_colors: usize,
}

/// Validate the portable portion of a renderer readback smoke test.
///
/// Backends sample a fixed grid from a completed BGRA/RGBA drawable and pass
/// the bytes here. Alpha is channel four in both layouts, while the first
/// three channels are treated only as an unordered color tuple. The checks
/// establish that the drawable holds a picture rather than one flat surface —
/// untouched (transparent) background, drawn model pixels, and more than one
/// visible color; they do not claim cross-backend pixels are identical.
///
/// A sample count of semi-transparent pixels is deliberately *not* required.
/// Anti-aliasing is a property of the artwork and of where the fixed grid
/// happens to land on it, not of a correct frame: a model whose art has hard
/// alpha edges and is drawn small in a large canvas can composite to fully
/// transparent or fully opaque coverage at every sampled point. Requiring a
/// translucent sample therefore fails a model every renderer can draw — and
/// because this check gates a model commit, the user saw it as "the selected
/// model could not be activated". A blank or single-color surface is still
/// rejected by the three checks below.
pub(crate) fn validate_frame_smoke(
    pixels: impl IntoIterator<Item = [u8; 4]>,
) -> Result<FramePixelStatistics, &'static str> {
    let mut statistics = FramePixelStatistics::default();
    let mut visible_colors = BTreeSet::new();
    for pixel in pixels {
        statistics.sampled_pixels = statistics.sampled_pixels.saturating_add(1);
        match pixel[3] {
            0 => statistics.transparent_pixels = statistics.transparent_pixels.saturating_add(1),
            u8::MAX => {
                statistics.opaque_pixels = statistics.opaque_pixels.saturating_add(1);
                visible_colors.insert([pixel[0], pixel[1], pixel[2]]);
            }
            _ => {
                statistics.translucent_pixels = statistics.translucent_pixels.saturating_add(1);
                visible_colors.insert([pixel[0], pixel[1], pixel[2]]);
            }
        }
    }
    statistics.distinct_visible_colors = visible_colors.len();

    if statistics.sampled_pixels == 0 {
        return Err("renderer readback contained no samples");
    }
    if statistics.transparent_pixels == 0 {
        return Err("renderer readback found no transparent overlay pixels");
    }
    if statistics.opaque_pixels + statistics.translucent_pixels == 0 {
        return Err("renderer readback found no model pixels");
    }
    if statistics.distinct_visible_colors < 2 {
        return Err("renderer readback found insufficient visible color variation");
    }
    Ok(statistics)
}

pub(crate) const FRAME_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(100);

pub(crate) const FRAME_RETRY_MAXIMUM_DELAY: Duration = Duration::from_secs(1);

/// Bounded retry cadence for temporary presentation failures.
///
/// The frame source owns the actual timer. This state only converts repeated
/// temporary failures into a deterministic delay, so a hidden compositor never
/// turns into a busy loop or a stream of renderer errors.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FrameRetryBackoff {
    pub(crate) consecutive_failures: u8,
}

impl FrameRetryBackoff {
    pub(crate) fn register_temporary_failure(&mut self) -> Duration {
        let exponent = self.consecutive_failures.min(4);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        FRAME_RETRY_INITIAL_DELAY
            .checked_mul(1_u32 << exponent)
            .unwrap_or(FRAME_RETRY_MAXIMUM_DELAY)
            .min(FRAME_RETRY_MAXIMUM_DELAY)
    }

    pub(crate) fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }
}
