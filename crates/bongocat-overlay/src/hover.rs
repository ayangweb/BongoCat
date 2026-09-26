//! Pointer hover hide for the product overlay window.
//!
//! The legacy application hid its main window while the pointer rested on it:
//! the window content faded out, pointer events started passing through, and
//! both were restored once the pointer left. This module holds the portable
//! part of that behaviour so the Windows and macOS sessions apply exactly the
//! same edge detection, delay, and fade, and so the rules can be tested without
//! a GPU or a native window.
//!
//! Platform adapters own everything that cannot be portable: reading the live
//! window box, converting the cursor into the same coordinate space, and
//! applying the resulting alpha and pass-through to the native window.

use std::time::Duration;

use crate::OverlayWindowBounds;

/// Legacy `transition-opacity-300`: the overlay faded in and out over 300ms.
///
/// The first version interpolates linearly instead of reproducing the CSS
/// `ease` curve, because the legacy timing function was never a documented part
/// of the behaviour. The fade is frame-rate independent because it is a
/// function of absolute elapsed time rather than of accumulated frame steps.
pub(crate) const HOVER_FADE_DURATION: Duration = Duration::from_millis(300);

/// Whether a pointer position lies inside an overlay window box.
///
/// The box is inclusive of its left and top edges and exclusive of its right
/// and bottom edges, which keeps the two shared edges from belonging to
/// neighbouring rectangles. The legacy implementation tested the same window
/// box, so the transparent corners of a rounded overlay still count as inside
/// and a pointer resting on a rounded corner still hides the window.
pub(crate) fn pointer_inside_window(bounds: OverlayWindowBounds, x: f64, y: f64) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    let left = f64::from(bounds.x);
    let top = f64::from(bounds.y);
    x >= left
        && x < left + f64::from(bounds.width)
        && y >= top
        && y < top + f64::from(bounds.height)
}

/// One observation of the overlay window's hover state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointerHoverObservation {
    /// Whether hover hide is configured and the pointer pipeline can be
    /// trusted. When this is `false` the overlay is always shown.
    pub(crate) enabled: bool,
    /// How long the pointer must stay inside before the hide starts.
    pub(crate) delay: Duration,
    /// Whether the last known pointer position was inside the window box.
    pub(crate) pointer_inside: bool,
    /// Monotonic time of this observation.
    pub(crate) now: Duration,
}

/// Pointer-driven hide state for one overlay window.
///
/// The state machine only reacts to transitions between "inside" and "outside",
/// like the legacy implementation: entering starts a delay timer, leaving
/// cancels it and restores the window, and staying put does nothing. A pointer
/// that enters and leaves before the delay elapses never hides the window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PointerHoverHide {
    inside: bool,
    hide_at: Option<Duration>,
    hidden: bool,
    visible: f64,
    target: f64,
    fade_from: f64,
    fade_at: Option<Duration>,
}

impl Default for PointerHoverHide {
    fn default() -> Self {
        Self {
            inside: false,
            hide_at: None,
            hidden: false,
            visible: 1.0,
            target: 1.0,
            fade_from: 1.0,
            fade_at: None,
        }
    }
}

impl PointerHoverHide {
    /// Feed one observation and return the alpha multiplier for the window.
    ///
    /// The result is `1` while the overlay is fully shown and `0` once the
    /// hover hide has finished fading it out, with the intermediate values
    /// spread over [`HOVER_FADE_DURATION`]. The frame that starts a fade keeps
    /// the previous alpha, so the fade always lasts its full duration.
    ///
    /// Pass-through is reported separately by [`Self::hidden`] because the
    /// legacy implementation switched pointer routing when the hide started
    /// rather than when the fade finished.
    pub(crate) fn observe(&mut self, observation: PointerHoverObservation) -> f64 {
        let PointerHoverObservation {
            enabled,
            delay,
            pointer_inside,
            now,
        } = observation;
        let inside = enabled && pointer_inside;
        if !enabled {
            // Turning the setting off must restore the overlay rather than
            // leaving a pending or completed hide behind.
            self.inside = false;
            self.hide_at = None;
            self.hidden = false;
        } else if inside != self.inside {
            self.inside = inside;
            self.hide_at = if inside {
                Some(now.checked_add(delay).unwrap_or(now))
            } else {
                None
            };
            if !inside {
                self.hidden = false;
            }
        }
        if inside && !self.hidden && self.hide_at.is_some_and(|deadline| now >= deadline) {
            self.hidden = true;
            self.hide_at = None;
        }
        self.advance(if self.hidden { 0.0 } else { 1.0 }, now)
    }

    /// Whether the overlay is hiding and must route pointer events through.
    pub(crate) const fn hidden(&self) -> bool {
        self.hidden
    }

    /// The alpha multiplier currently in effect, before the configured opacity.
    ///
    /// Native windows are created with the configured opacity, so a window that
    /// replaces a hover-hidden one has to be corrected before it is drawn or
    /// shown.
    pub(crate) const fn visible(&self) -> f64 {
        self.visible
    }

    fn advance(&mut self, target: f64, now: Duration) -> f64 {
        if target != self.target {
            self.target = target;
            self.fade_from = self.visible;
            self.fade_at = Some(now);
            return self.visible;
        }
        let Some(started) = self.fade_at else {
            self.fade_at = Some(now);
            return self.visible;
        };
        if now <= started {
            return self.visible;
        }
        let progress = ((now - started).as_secs_f64() / HOVER_FADE_DURATION.as_secs_f64()).min(1.0);
        self.visible = self.fade_from + (self.target - self.fade_from) * progress;
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: OverlayWindowBounds = OverlayWindowBounds::new(100, 200, 350, 200);

    fn observation(
        enabled: bool,
        delay_ms: u64,
        inside: bool,
        now_ms: u64,
    ) -> PointerHoverObservation {
        PointerHoverObservation {
            enabled,
            delay: Duration::from_millis(delay_ms),
            pointer_inside: inside,
            now: Duration::from_millis(now_ms),
        }
    }

    #[test]
    fn pointer_hit_testing_uses_a_half_open_window_box() {
        assert!(pointer_inside_window(BOUNDS, 100.0, 200.0));
        assert!(pointer_inside_window(BOUNDS, 449.0, 399.0));
        assert!(!pointer_inside_window(BOUNDS, 450.0, 300.0));
        assert!(!pointer_inside_window(BOUNDS, 300.0, 400.0));
        assert!(!pointer_inside_window(BOUNDS, 99.0, 300.0));
        assert!(!pointer_inside_window(BOUNDS, 300.0, 199.0));
        // Negative origins are ordinary multi-display coordinates, not errors.
        let left_of_primary = OverlayWindowBounds::new(-1_920, -120, 400, 300);
        assert!(pointer_inside_window(left_of_primary, -1_800.0, -100.0));
        assert!(!pointer_inside_window(left_of_primary, -1_920.0, 200.0));
        assert!(!pointer_inside_window(BOUNDS, f64::NAN, 300.0));
    }

    #[test]
    fn hovering_for_the_delay_hides_and_leaving_restores() {
        let mut hide = PointerHoverHide::default();
        assert_eq!(hide.observe(observation(true, 0, false, 0)), 1.0);
        assert!(!hide.hidden());

        assert_eq!(hide.observe(observation(true, 0, true, 100)), 1.0);
        assert!(hide.hidden(), "a zero delay hides on the entering frame");
        assert_eq!(hide.observe(observation(true, 0, true, 250)), 0.5);
        assert_eq!(hide.observe(observation(true, 0, true, 400)), 0.0);
        assert_eq!(hide.observe(observation(true, 0, true, 1_000)), 0.0);

        assert_eq!(hide.observe(observation(true, 0, false, 1_100)), 0.0);
        assert!(!hide.hidden(), "leaving restores pointer routing at once");
        assert_eq!(hide.observe(observation(true, 0, false, 1_250)), 0.5);
        assert_eq!(hide.observe(observation(true, 0, false, 1_400)), 1.0);
        assert_eq!(hide.observe(observation(true, 0, false, 2_000)), 1.0);
    }

    #[test]
    fn a_delay_postpones_the_hide_and_leaving_cancels_it() {
        let mut hide = PointerHoverHide::default();
        hide.observe(observation(true, 500, false, 0));

        assert_eq!(hide.observe(observation(true, 500, true, 10)), 1.0);
        assert!(!hide.hidden());
        assert_eq!(hide.observe(observation(true, 500, true, 509)), 1.0);
        assert!(!hide.hidden(), "the deadline is exclusive");
        assert_eq!(hide.observe(observation(true, 500, true, 510)), 1.0);
        assert!(hide.hidden());

        // A pointer that leaves before the deadline never hides the window, and
        // a later hover must start a fresh delay rather than reusing the old one.
        let mut cancelled = PointerHoverHide::default();
        cancelled.observe(observation(true, 500, false, 0));
        cancelled.observe(observation(true, 500, true, 10));
        assert_eq!(cancelled.observe(observation(true, 500, false, 200)), 1.0);
        assert!(!cancelled.hidden());
        cancelled.observe(observation(true, 500, true, 300));
        assert_eq!(cancelled.observe(observation(true, 500, true, 700)), 1.0);
        assert!(!cancelled.hidden(), "the second hover restarts the delay");
        cancelled.observe(observation(true, 500, true, 810));
        assert!(cancelled.hidden());
    }

    #[test]
    fn disabling_the_setting_restores_the_overlay_through_the_fade() {
        let mut hide = PointerHoverHide::default();
        hide.observe(observation(true, 0, false, 0));
        hide.observe(observation(true, 0, true, 10));
        assert_eq!(hide.observe(observation(true, 0, true, 320)), 0.0);

        assert_eq!(hide.observe(observation(false, 0, true, 330)), 0.0);
        assert!(!hide.hidden());
        assert_eq!(hide.observe(observation(false, 0, true, 480)), 0.5);
        assert_eq!(hide.observe(observation(false, 0, true, 630)), 1.0);
        assert_eq!(hide.observe(observation(false, 0, true, 1_000)), 1.0);
    }

    #[test]
    fn the_fade_is_a_function_of_elapsed_time_and_never_overshoots() {
        // The same elapsed time must produce the same alpha no matter how many
        // frames it was sampled over.
        let mut at_60_fps = PointerHoverHide::default();
        let mut at_240_fps = PointerHoverHide::default();
        at_60_fps.observe(observation(true, 0, false, 0));
        at_240_fps.observe(observation(true, 0, false, 0));
        at_60_fps.observe(observation(true, 0, true, 1));
        at_240_fps.observe(observation(true, 0, true, 1));
        for frame in 1..=37 {
            at_60_fps.observe(observation(true, 0, true, 1 + frame * 4));
            at_240_fps.observe(observation(true, 0, true, 1 + frame));
        }
        let sixty = at_60_fps.observe(observation(true, 0, true, 151));
        let two_forty = at_240_fps.observe(observation(true, 0, true, 151));
        assert_eq!(sixty, 0.5);
        assert_eq!(two_forty, sixty);

        // A single long stall must clamp at the target instead of wrapping past it.
        let mut stalled = PointerHoverHide::default();
        stalled.observe(observation(true, 0, false, 0));
        stalled.observe(observation(true, 0, true, 1));
        assert_eq!(stalled.observe(observation(true, 0, true, 60_000)), 0.0);
        assert_eq!(stalled.observe(observation(true, 0, false, 60_001)), 0.0);
        assert_eq!(stalled.observe(observation(true, 0, false, 60_301)), 1.0);
    }

    #[test]
    fn the_first_observation_establishes_the_fade_clock_without_moving_alpha() {
        let mut hide = PointerHoverHide::default();
        assert_eq!(hide.observe(observation(true, 0, true, 5_000)), 1.0);
        assert!(hide.hidden(), "a zero delay hides on the entering frame");
        assert_eq!(hide.observe(observation(true, 0, true, 5_150)), 0.5);
    }
}
