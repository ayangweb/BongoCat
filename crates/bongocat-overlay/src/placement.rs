//! Overlay placement constraint: keep the model window fully on a display.
//!
//! The constraint this module implements is deliberately weaker than the
//! legacy `keep_inside_work_area` behaviour that preceded it:
//!
//! * The region a window is kept inside is the **union of the connected
//!   displays' frames**, not the work area of one display. A model window may
//!   therefore sit over a taskbar, a Dock, or a menu bar, and it keeps the
//!   negative coordinates of a display placed left of or above the primary one.
//! * A window that straddles two adjacent displays is left alone as long as
//!   every part of it still lands on a display. Only a window that actually
//!   leaves the desktop is moved.
//! * Leaving the desktop is corrected **after a delay**, not on the next frame.
//!   The countdown starts when the window is observed at rest outside the
//!   displays and restarts on every observed movement, so dragging the model
//!   from one display to another is never interrupted by the correction.
//!
//! Platform adapters own everything that cannot be portable: enumerating the
//! displays and reading/applying the live window box.

use std::time::Duration;

use crate::{OverlayScreenBounds, OverlayWindowBounds};

/// How long the overlay must stay put outside the displays before it is moved
/// back. The countdown restarts whenever the window box changes, so this is
/// measured from the end of a drag rather than from the moment the window first
/// crossed the desktop edge.
///
/// One second is long enough to cover the pauses inside a deliberate
/// multi-display drag (the window is usually released mid-crossing, and the
/// user often regrabs it immediately) and short enough that a window dragged
/// off the desktop does not stay lost. Native move loops keep the frame source
/// blocked for the whole drag: with the system drag loop on Windows and the
/// AppKit window drag on macOS the session only observes the settled box after
/// the pointer is released, so the delay is always a post-drag delay.
pub(crate) const PLACEMENT_SETTLE_DELAY: Duration = Duration::from_millis(1_000);

/// How long an inspection of the window's placement stays valid.
///
/// A settled window whose box is already known to be on a display does not need
/// to be re-checked on every frame, which matters because enumerating displays
/// allocates on macOS and calls into the window manager on Windows. The
/// inspection is nevertheless revalidated regularly so that a display change
/// under a stationary window (unplugging an external display, a resolution
/// change) is still noticed and corrected without any platform notification.
///
/// This is deliberately shorter than [`PLACEMENT_SETTLE_DELAY`], so a pending
/// correction is re-evaluated against the current displays at least once before
/// its deadline fires: plugging a display in mid-countdown cancels the
/// correction instead of moving a window that has become valid again.
pub(crate) const PLACEMENT_INSPECTION_INTERVAL: Duration = Duration::from_millis(500);

/// Whether every part of `bounds` lands on one of `screens`.
///
/// Displays are axis-aligned rectangles in one shared coordinate space, and
/// they are allowed to overlap or to be arranged in an L, so "on a display" is
/// a question about the union rather than about a single rectangle. The test
/// slices the window at every display edge that falls inside it: within one
/// slice the set of displays covering the slice is constant, and the slice is
/// covered exactly when the window's vertical span fits inside the merged
/// vertical spans of those displays.
pub(crate) fn bounds_inside_screens(
    screens: &[OverlayScreenBounds],
    bounds: OverlayWindowBounds,
) -> bool {
    let (left, top, right, bottom) = window_edges(bounds);
    if screens.is_empty() || right <= left || bottom <= top {
        // No display information is not the same as "on a display"; the caller
        // then finds no correction either, so the window is left where it is.
        return false;
    }

    let mut cuts = vec![left, right];
    for screen in screens {
        let (screen_left, _, screen_right, _) = screen_edges(*screen);
        for edge in [screen_left, screen_right] {
            if edge > left && edge < right {
                cuts.push(edge);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut spans = Vec::with_capacity(screens.len());
    for slice in cuts.windows(2) {
        let (slice_left, slice_right) = (slice[0], slice[1]);
        spans.clear();
        for screen in screens {
            let (screen_left, screen_top, screen_right, screen_bottom) = screen_edges(*screen);
            // Every display edge inside the window is a cut, so a display either
            // covers a slice completely or does not touch it at all.
            if screen_left <= slice_left && screen_right >= slice_right {
                spans.push((screen_top, screen_bottom));
            }
        }
        if !spans_cover(&mut spans, (top, bottom)) {
            return false;
        }
    }
    true
}

/// The box `bounds` should be moved to so that it lands fully on a display.
///
/// The target display is the one with the largest intersection with the window,
/// with ties broken by the nearest center; a window that intersects no display
/// at all targets the nearest one. This matches the display selection the two
/// platform sessions used before the constraint became portable, where Windows
/// asked for `MonitorFromRect(.., MONITOR_DEFAULTTONEAREST)` and macOS picked
/// the largest overlap or, failing that, the smallest center distance.
///
/// The window size is never changed, so a window larger than its display is
/// pinned to the display's origin instead. Callers must check
/// [`bounds_inside_screens`] first: a window that already sits on the desktop
/// can still be moved by this function if it straddles two displays, which is
/// exactly the placement the constraint allows.
pub(crate) fn correction_for_screens(
    screens: &[OverlayScreenBounds],
    bounds: OverlayWindowBounds,
) -> Option<OverlayWindowBounds> {
    nearest_screen(screens, bounds).map(|screen| bounds.clamp_to(screen))
}

/// The display a window belongs to: largest overlap, then nearest center.
fn nearest_screen(
    screens: &[OverlayScreenBounds],
    bounds: OverlayWindowBounds,
) -> Option<OverlayScreenBounds> {
    let (left, top, right, bottom) = window_edges(bounds);
    let center_x = left.saturating_add(right) / 2;
    let center_y = top.saturating_add(bottom) / 2;
    let mut best: Option<(OverlayScreenBounds, i64, i64)> = None;
    for screen in screens {
        let (screen_left, screen_top, screen_right, screen_bottom) = screen_edges(*screen);
        let overlap_x = right.min(screen_right) - left.max(screen_left);
        let overlap_y = bottom.min(screen_bottom) - top.max(screen_top);
        let overlap = overlap_x.max(0).saturating_mul(overlap_y.max(0));
        let screen_center_x = screen_left.saturating_add(screen_right) / 2;
        let screen_center_y = screen_top.saturating_add(screen_bottom) / 2;
        let distance = center_x
            .saturating_sub(screen_center_x)
            .saturating_mul(center_x.saturating_sub(screen_center_x))
            .saturating_add(
                center_y
                    .saturating_sub(screen_center_y)
                    .saturating_mul(center_y.saturating_sub(screen_center_y)),
            );
        let better = match best {
            None => true,
            Some((_, best_overlap, best_distance)) => {
                overlap > best_overlap || (overlap == best_overlap && distance < best_distance)
            }
        };
        if better {
            best = Some((*screen, overlap, distance));
        }
    }
    best.map(|(screen, _, _)| screen)
}

/// Whether the merged `spans` cover `target` without a gap.
fn spans_cover(spans: &mut [(i64, i64)], target: (i64, i64)) -> bool {
    spans.sort_unstable();
    let mut covered_until = target.0;
    for (start, end) in spans.iter().copied() {
        if end <= covered_until {
            continue;
        }
        if start > covered_until {
            // Sorted by start, so nothing later can bridge the gap.
            return false;
        }
        covered_until = end;
        if covered_until >= target.1 {
            return true;
        }
    }
    covered_until >= target.1
}

/// The window box as `(left, top, right, bottom)` in an unbounded integer space,
/// so display coordinates near the `i32` limits cannot overflow.
fn window_edges(bounds: OverlayWindowBounds) -> (i64, i64, i64, i64) {
    let left = i64::from(bounds.x);
    let top = i64::from(bounds.y);
    (
        left,
        top,
        left + i64::from(bounds.width),
        top + i64::from(bounds.height),
    )
}

fn screen_edges(screen: OverlayScreenBounds) -> (i64, i64, i64, i64) {
    let left = i64::from(screen.x);
    let top = i64::from(screen.y);
    (
        left,
        top,
        left + i64::from(screen.width),
        top + i64::from(screen.height),
    )
}

/// One inspection of a settled window box.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InspectedPlacement {
    bounds: OverlayWindowBounds,
    at: Duration,
    /// The box the window must be moved to, or `None` when the placement is
    /// already acceptable (or cannot be improved, as for a window larger than
    /// the display it belongs to).
    correction: Option<OverlayWindowBounds>,
}

/// The delayed placement correction for one overlay window.
///
/// `observe` is fed the live window box, a monotonic clock, and a way to read
/// the current displays. It returns a corrected box only once an unacceptable
/// placement has survived [`PLACEMENT_SETTLE_DELAY`] without the window moving.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OverlayPlacementConstraint {
    inspected: Option<InspectedPlacement>,
    due_at: Option<Duration>,
}

impl OverlayPlacementConstraint {
    /// Feed one observation of the overlay window placement.
    pub(crate) fn observe(
        &mut self,
        bounds: OverlayWindowBounds,
        now: Duration,
        screens: impl FnOnce() -> Vec<OverlayScreenBounds>,
    ) -> Option<OverlayWindowBounds> {
        let reusable = self
            .inspected
            .filter(|inspected| {
                inspected.bounds == bounds
                    && now.saturating_sub(inspected.at) < PLACEMENT_INSPECTION_INTERVAL
            })
            .is_some();
        if !reusable {
            // A different box means the window moved — or that this is the first
            // observation after the constraint was enabled. Both restart the
            // countdown, so a drag in progress is never corrected and turning the
            // setting on never moves a window that the user placed while it was
            // off.
            if self.inspected.map(|inspected| inspected.bounds) != Some(bounds) {
                self.due_at = None;
            }
            let screens = screens();
            let correction = (!bounds_inside_screens(&screens, bounds))
                .then(|| correction_for_screens(&screens, bounds))
                .flatten()
                .filter(|correction| *correction != bounds);
            self.inspected = Some(InspectedPlacement {
                bounds,
                at: now,
                correction,
            });
        }

        let Some(correction) = self.inspected.and_then(|inspected| inspected.correction) else {
            // Nothing to correct: drop any countdown an earlier inspection armed, so
            // a placement that became valid again cannot leave a stale deadline
            // behind that would skip the delay if it turns invalid again.
            self.due_at = None;
            return None;
        };
        let due_at = *self
            .due_at
            .get_or_insert_with(|| now.checked_add(PLACEMENT_SETTLE_DELAY).unwrap_or(now));
        if now < due_at {
            return None;
        }
        // The correction is applied by the session; re-inspect the new box next.
        self.due_at = None;
        self.inspected = None;
        Some(correction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY_MS: u64 = 1_000;
    const INSPECTION_MS: u64 = 500;

    fn screen(x: i32, y: i32, width: u32, height: u32) -> OverlayScreenBounds {
        OverlayScreenBounds {
            x,
            y,
            width,
            height,
        }
    }

    /// Two 1920x1080 displays side by side, the left one holding the origin.
    fn side_by_side() -> Vec<OverlayScreenBounds> {
        vec![screen(-1_920, 0, 1_920, 1_080), screen(0, 0, 1_920, 1_080)]
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn a_window_on_one_display_is_inside() {
        let screens = side_by_side();
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(100, 100, 350, 350)
        ));
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(0, 0, 1_920, 1_080)
        ));
        // A window that exactly covers both displays is still covered by them.
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(-1_920, 0, 3_840, 1_080)
        ));
    }

    #[test]
    fn a_window_straddling_two_displays_is_inside() {
        let screens = side_by_side();
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(-200, 300, 350, 350)
        ));
        // Straddling the seam while sitting over the bottom taskbar strip of both
        // displays is allowed: the displays, not their work areas, are the limit.
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(-100, 1_040, 200, 40)
        ));
    }

    #[test]
    fn a_window_off_the_desktop_is_not_inside() {
        let screens = side_by_side();
        // Past the left edge of the left display.
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(-2_000, 100, 350, 350)
        ));
        // Past the right edge of the right display.
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_800, 100, 350, 350)
        ));
        // Below the bottom edge.
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(100, 1_000, 350, 350)
        ));
        // Above the top edge.
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(100, -100, 350, 350)
        ));
        // No display information at all is not a covered placement.
        assert!(!bounds_inside_screens(
            &[],
            OverlayWindowBounds::new(100, 100, 350, 350)
        ));
    }

    #[test]
    fn a_window_in_the_notch_of_an_l_shaped_desktop_is_not_inside() {
        // A tall display on the left and a short one on the right, top aligned:
        // the space under the short display is desktop-free even though both of
        // the window's horizontal slices touch a display.
        let screens = vec![screen(0, 0, 1_920, 1_080), screen(1_920, 0, 1_920, 400)];
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_700, 600, 400, 300)
        ));
        // The same window inside the tall display alone is covered.
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_000, 600, 400, 300)
        ));
        // And it is covered again once it fits under the short display's height.
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_900, 50, 400, 300)
        ));
    }

    #[test]
    fn overlapping_displays_cover_their_shared_regions() {
        let screens = vec![screen(0, 0, 1_920, 1_080), screen(1_600, 400, 1_920, 1_080)];
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_700, 900, 300, 300)
        ));
        assert!(!bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(1_000, 1_200, 300, 300)
        ));
    }

    #[test]
    fn the_correction_targets_the_display_with_the_largest_overlap() {
        let screens = side_by_side();
        // Straddling the seam but leaning left: the left display owns it, so the
        // box is clamped into it (which is what the caller asks for only when the
        // box is not covered by the displays as a whole).
        assert_eq!(
            correction_for_screens(&screens, OverlayWindowBounds::new(-300, 500, 350, 350)),
            Some(OverlayWindowBounds::new(-350, 500, 350, 350))
        );
        // Leaning right.
        assert_eq!(
            correction_for_screens(&screens, OverlayWindowBounds::new(1_800, 100, 350, 350)),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
        // The union test is what keeps a straddling box in place; the clamp on
        // its own would move it into the display with the larger overlap.
        assert!(bounds_inside_screens(
            &screens,
            OverlayWindowBounds::new(-300, 500, 350, 350)
        ));
    }

    #[test]
    fn a_window_touching_no_display_targets_the_nearest_one() {
        let screens = side_by_side();
        // Fully to the right of the desktop, closer to the right display.
        assert_eq!(
            correction_for_screens(&screens, OverlayWindowBounds::new(4_000, 100, 350, 350)),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
        // Fully to the left, closer to the left display.
        assert_eq!(
            correction_for_screens(&screens, OverlayWindowBounds::new(-4_000, 100, 350, 350)),
            Some(OverlayWindowBounds::new(-1_920, 100, 350, 350))
        );
        assert_eq!(
            correction_for_screens(&[], OverlayWindowBounds::new(0, 0, 350, 350)),
            None
        );
    }

    #[test]
    fn a_window_larger_than_its_display_is_pinned_without_resizing() {
        let screens = vec![screen(0, 0, 1_280, 720)];
        let bounds = OverlayWindowBounds::new(400, 300, 2_000, 1_500);
        assert_eq!(
            correction_for_screens(&screens, bounds),
            Some(OverlayWindowBounds::new(0, 0, 2_000, 1_500))
        );
        // Already pinned to the display origin: nothing left to improve, so the
        // constraint must not keep issuing no-op corrections.
        let mut constraint = OverlayPlacementConstraint::default();
        let pinned = OverlayWindowBounds::new(0, 0, 2_000, 1_500);
        assert_eq!(constraint.observe(pinned, ms(0), || screens.clone()), None);
        assert_eq!(
            constraint.observe(pinned, ms(DELAY_MS * 4), || screens.clone()),
            None
        );
    }

    #[test]
    fn a_settled_window_off_the_desktop_is_corrected_only_after_the_delay() {
        let screens = side_by_side();
        let outside = OverlayWindowBounds::new(1_800, 100, 350, 350);
        let mut constraint = OverlayPlacementConstraint::default();

        assert_eq!(constraint.observe(outside, ms(0), || screens.clone()), None);
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS - 1), || screens.clone()),
            None,
            "the deadline is exclusive"
        );
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS), || screens.clone()),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
        // The corrected box is already on a display, so nothing follows it.
        let corrected = OverlayWindowBounds::new(1_570, 100, 350, 350);
        assert_eq!(
            constraint.observe(corrected, ms(DELAY_MS + 16), || screens.clone()),
            None
        );
    }

    #[test]
    fn every_observed_movement_restarts_the_countdown() {
        let screens = side_by_side();
        let mut constraint = OverlayPlacementConstraint::default();
        // The window is dragged right across the seam. Both boxes in this series
        // sit partly or fully past the desktop edge, so without the per-movement
        // restart the first one would already be due while the drag continues.
        for (step, x) in [1_400_i32, 1_600, 1_800].into_iter().enumerate() {
            let bounds = OverlayWindowBounds::new(x, 100, 350, 350);
            let observed_at = ms(step as u64 * 300);
            assert_eq!(
                constraint.observe(bounds, observed_at, || screens.clone()),
                None
            );
            assert_eq!(
                constraint.observe(bounds, ms(step as u64 * 300 + 200), || screens.clone()),
                None
            );
        }
        // Released at `x = 1800` and last observed at `600ms`: still off the
        // desktop, and still untouched until a full second of rest has passed.
        let released = OverlayWindowBounds::new(1_800, 100, 350, 350);
        assert_eq!(
            constraint.observe(released, ms(1_599), || screens.clone()),
            None,
            "the countdown runs from the last observed movement"
        );
        assert_eq!(
            constraint.observe(released, ms(1_600), || screens.clone()),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
    }

    #[test]
    fn a_window_that_returns_to_the_desktop_is_never_corrected() {
        let screens = side_by_side();
        let mut constraint = OverlayPlacementConstraint::default();
        assert_eq!(
            constraint.observe(
                OverlayWindowBounds::new(1_800, 100, 350, 350),
                ms(0),
                || screens.clone()
            ),
            None
        );
        // The user keeps dragging and releases fully on the second display.
        let settled = OverlayWindowBounds::new(1_400, 100, 350, 350);
        assert_eq!(
            constraint.observe(settled, ms(100), || screens.clone()),
            None
        );
        assert_eq!(
            constraint.observe(settled, ms(100 + DELAY_MS * 10), || screens.clone()),
            None,
            "a window that is back on the desktop must stay where the user put it"
        );
    }

    #[test]
    fn the_first_observation_after_enabling_starts_a_fresh_countdown() {
        let screens = side_by_side();
        let mut constraint = OverlayPlacementConstraint::default();
        // Nothing is armed while the setting is off, so the first observation
        // after it is enabled must not correct the window immediately.
        assert_eq!(
            constraint.observe(
                OverlayWindowBounds::new(1_800, 100, 350, 350),
                ms(0),
                || screens.clone()
            ),
            None
        );
        assert_eq!(
            constraint.observe(
                OverlayWindowBounds::new(1_800, 100, 350, 350),
                ms(999),
                || screens.clone()
            ),
            None
        );
        assert!(
            constraint
                .observe(
                    OverlayWindowBounds::new(1_800, 100, 350, 350),
                    ms(1_000),
                    || screens.clone()
                )
                .is_some()
        );
    }

    #[test]
    fn a_settled_inspection_is_reused_and_revalidated() {
        use std::cell::Cell;

        let screens = side_by_side();
        let bounds = OverlayWindowBounds::new(100, 100, 350, 350);
        let queries = Cell::new(0_u32);
        let mut constraint = OverlayPlacementConstraint::default();
        // A frame loop at 60fps with a settled window must not query the
        // displays on every frame: the first frame inspects, the rest reuse it.
        for frame in 0..=29 {
            assert_eq!(
                constraint.observe(bounds, ms(frame * 16), || {
                    queries.set(queries.get() + 1);
                    screens.clone()
                }),
                None
            );
        }
        assert_eq!(queries.get(), 1);
        assert_eq!(
            constraint.observe(bounds, ms(INSPECTION_MS - 6), || {
                queries.set(queries.get() + 1);
                screens.clone()
            }),
            None
        );
        assert_eq!(queries.get(), 1);
        // ...but the inspection must be revalidated once it expires, so a display
        // change under a stationary window is still noticed.
        assert_eq!(
            constraint.observe(bounds, ms(INSPECTION_MS), || {
                queries.set(queries.get() + 1);
                screens.clone()
            }),
            None
        );
        assert_eq!(queries.get(), 2);
    }

    #[test]
    fn a_revalidated_inspection_keeps_an_armed_countdown() {
        let screens = side_by_side();
        let outside = OverlayWindowBounds::new(1_800, 100, 350, 350);
        let mut constraint = OverlayPlacementConstraint::default();
        assert_eq!(constraint.observe(outside, ms(0), || screens.clone()), None);
        // The revalidation of an unchanged box must not restart the countdown.
        assert_eq!(
            constraint.observe(outside, ms(INSPECTION_MS + 100), || screens.clone()),
            None
        );
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS), || screens.clone()),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
    }

    #[test]
    fn a_display_plugged_in_during_the_countdown_cancels_the_correction() {
        let screens = side_by_side();
        let mut extended = side_by_side();
        extended.push(screen(1_800, 0, 1_920, 1_080));
        let outside = OverlayWindowBounds::new(1_800, 100, 350, 350);
        assert!(!bounds_inside_screens(&screens, outside));
        assert!(bounds_inside_screens(&extended, outside));

        // Control: with the displays unchanged the countdown fires.
        let mut control = OverlayPlacementConstraint::default();
        assert_eq!(control.observe(outside, ms(0), || screens.clone()), None);
        assert!(
            control
                .observe(outside, ms(DELAY_MS), || screens.clone())
                .is_some()
        );

        // The revalidation inside the countdown sees the new display, so the
        // window that is valid again is left where it is.
        let mut constraint = OverlayPlacementConstraint::default();
        assert_eq!(constraint.observe(outside, ms(0), || screens.clone()), None);
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS - 400), || extended.clone()),
            None
        );
        // That cancellation also drops the countdown: if the extra display goes
        // away again while the window stays put, a fresh second of rest is
        // required instead of the stale deadline firing at once.
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS + 100), || screens.clone()),
            None,
            "a cancelled countdown must not be reused"
        );
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS * 2), || screens.clone()),
            None
        );
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS * 2 + 100), || screens.clone()),
            Some(OverlayWindowBounds::new(1_570, 100, 350, 350))
        );
        assert_eq!(
            constraint.observe(outside, ms(DELAY_MS * 5), || extended),
            None
        );
    }
}
