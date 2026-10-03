//! Hide the overlay after a stretch of user inactivity.
//!
//! The request behind this feature is deliberately simple: while the user is
//! not touching the mouse, keyboard or gamepad, fade the cat out so a watched
//! desktop stays clear; the first input that arrives fades it back. This
//! module holds the portable part of that behaviour so both platform sessions
//! apply exactly the same edge detection and fade, and so the rules can be
//! tested without a GPU or a native window.
//!
//! The overlay keeps presenting frames while hidden, just like the pointer
//! hover hide: the window, the frame loop and the runtime state are never
//! torn down, only the alpha and the pointer routing change.

use std::time::Duration;

use bongocat_runtime::MonotonicMillis;

/// Same 300ms window as the pointer hover hide: the first version copies the
/// legacy `transition-opacity-300` without reproducing the CSS `ease` curve.
const IDLE_FADE_DURATION: Duration = Duration::from_millis(300);

/// One observation of the input activity the idle hide works from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IdleObservation {
    /// Whether idle hide is configured and the input pipeline can be
    /// trusted. When this is `false` the overlay is always shown.
    pub(crate) enabled: bool,
    /// How long no fresh input may arrive before the hide starts.
    pub(crate) delay: Duration,
    /// Latest applied input event sequence, from the runtime snapshot.
    pub(crate) input_sequence: Option<u64>,
    /// Timestamp of the latest cursor sample, if one has ever arrived.
    pub(crate) cursor_at: Option<MonotonicMillis>,
    /// Published gamepad axis sample counter.
    pub(crate) gamepad_axis_published: u64,
    /// Monotonic time of this observation.
    pub(crate) now: Duration,
}

/// Inactivity-driven hide state for one overlay window.
///
/// "Activity" is any change in the input counters since the previous
/// observation: a new keyboard/mouse/gamepad event sequence, a new cursor
/// sample timestamp, or a new published gamepad axis sample all reset the
/// idle clock. A mouse held still or a pressed-but-unmoving key does not,
/// so only genuine new input wakes the overlay back up.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IdleHide {
    prev_input_sequence: Option<u64>,
    prev_cursor_at: Option<MonotonicMillis>,
    prev_gamepad_axis_published: u64,
    last_activity: Option<Duration>,
    hidden: bool,
    visible: f64,
    target: f64,
    fade_from: f64,
    fade_at: Option<Duration>,
}

impl Default for IdleHide {
    fn default() -> Self {
        Self {
            prev_input_sequence: None,
            prev_cursor_at: None,
            prev_gamepad_axis_published: 0,
            last_activity: None,
            hidden: false,
            visible: 1.0,
            target: 1.0,
            fade_from: 1.0,
            fade_at: None,
        }
    }
}

impl IdleHide {
    /// Feed one observation and return the alpha multiplier for the window.
    ///
    /// The result is `1` while the overlay is fully shown and `0` once the
    /// idle hide has finished fading it out, with intermediate values spread
    /// over [`IDLE_FADE_DURATION`]. The frame that starts a fade keeps the
    /// previous alpha, so the fade always lasts its full duration.
    ///
    /// Fresh input restores pointer routing at once rather than after the
    /// fade, matching the hover hide: the cause of the hide just went away,
    /// so the overlay must never strand clicks underneath itself.
    pub(crate) fn observe(&mut self, observation: IdleObservation) -> f64 {
        let IdleObservation {
            enabled,
            delay,
            input_sequence,
            cursor_at,
            gamepad_axis_published,
            now,
        } = observation;
        if !enabled {
            // Turning the setting off must restore the overlay rather than
            // leaving a pending or completed hide behind, and it must not
            // let a stale "last activity" hide the window the next time the
            // feature is switched on.
            self.prev_input_sequence = None;
            self.prev_cursor_at = None;
            self.prev_gamepad_axis_published = 0;
            self.last_activity = None;
            self.hidden = false;
        } else {
            let activity = input_sequence != self.prev_input_sequence
                || cursor_at != self.prev_cursor_at
                || gamepad_axis_published != self.prev_gamepad_axis_published;
            self.prev_input_sequence = input_sequence;
            self.prev_cursor_at = cursor_at;
            self.prev_gamepad_axis_published = gamepad_axis_published;
            if activity || self.last_activity.is_none() {
                // The first observation of a freshly enabled timer also
                // starts the clock, so enabling the feature never hides the
                // overlay on its first tick.
                self.last_activity = Some(now);
                self.hidden = false;
            } else if !self.hidden
                && self
                    .last_activity
                    .is_some_and(|last| now.saturating_sub(last) >= delay)
            {
                self.hidden = true;
            }
        }
        self.advance(if self.hidden { 0.0 } else { 1.0 }, now)
    }

    /// Whether the overlay is idle-hidden and must route pointer events through.
    pub(crate) const fn hidden(&self) -> bool {
        self.hidden
    }

    /// The alpha multiplier currently in effect, before the configured opacity.
    ///
    /// Native windows are created with the configured opacity, so a window that
    /// replaces an idle-hidden one has to be corrected before it is drawn or
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
        let progress = ((now - started).as_secs_f64() / IDLE_FADE_DURATION.as_secs_f64()).min(1.0);
        self.visible = self.fade_from + (self.target - self.fade_from) * progress;
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(
        enabled: bool,
        delay_ms: u64,
        input_sequence: Option<u64>,
        cursor_at: Option<u64>,
        axis_published: u64,
        now_ms: u64,
    ) -> IdleObservation {
        IdleObservation {
            enabled,
            delay: Duration::from_millis(delay_ms),
            input_sequence,
            cursor_at: cursor_at.map(MonotonicMillis::new),
            gamepad_axis_published: axis_published,
            now: Duration::from_millis(now_ms),
        }
    }

    #[test]
    fn no_input_for_the_delay_hides_and_fresh_input_restores() {
        let mut hide = IdleHide::default();
        // The first observation starts the clock without hiding anything.
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), None, 0, 0)),
            1.0
        );
        assert!(!hide.hidden());
        // A mouse move at t=500 resets the clock, so the hide only starts
        // after a full quiet second since that move.
        hide.observe(observation(true, 1_000, Some(0), Some(500), 0, 500));
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), Some(500), 0, 1_499)),
            1.0
        );
        assert!(!hide.hidden(), "the delay deadline is exclusive");
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), Some(500), 0, 1_500)),
            1.0
        );
        assert!(hide.hidden());
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), Some(500), 0, 1_650)),
            0.5
        );
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), Some(500), 0, 1_800)),
            0.0
        );
        // A key event wakes the overlay back up at once.
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(1), Some(500), 0, 1_900)),
            0.0
        );
        assert!(!hide.hidden());
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(1), Some(500), 0, 2_050)),
            0.5
        );
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(1), Some(500), 0, 2_200)),
            1.0
        );
    }

    #[test]
    fn every_input_family_resets_the_idle_clock() {
        // Each case changes exactly one of the three activity counters, so
        // none of them can pass because the other two moved.
        for step in [
            (|observation: &mut IdleObservation| observation.input_sequence = Some(7))
                as fn(&mut IdleObservation),
            (|observation: &mut IdleObservation| {
                observation.cursor_at = Some(MonotonicMillis::new(9));
            }) as fn(&mut IdleObservation),
            (|observation: &mut IdleObservation| {
                observation.gamepad_axis_published = 3;
            }) as fn(&mut IdleObservation),
        ] {
            let mut hide = IdleHide::default();
            hide.observe(observation(true, 1_000, Some(1), None, 0, 0));
            let mut later = observation(true, 1_000, Some(1), None, 0, 900);
            step(&mut later);
            hide.observe(later);
            // The reset means t=1900 is one second after the fresh input at
            // t=900, so t=1899 is still too early and t=1900 hides.
            assert_eq!(
                hide.observe(IdleObservation {
                    now: Duration::from_millis(1_899),
                    ..later
                }),
                1.0
            );
            assert!(!hide.hidden());
            hide.observe(IdleObservation {
                now: Duration::from_millis(1_900),
                ..later
            });
            assert!(hide.hidden());
        }
    }

    #[test]
    fn repeated_observations_without_input_do_not_count_as_activity() {
        let mut hide = IdleHide::default();
        let sample = observation(true, 500, Some(3), Some(100), 4, 0);
        hide.observe(sample);
        for at in [100_u64, 200, 300, 400] {
            hide.observe(IdleObservation {
                now: Duration::from_millis(at),
                ..sample
            });
        }
        let later = hide.observe(IdleObservation {
            now: Duration::from_millis(500),
            ..sample
        });
        assert!(
            hide.hidden(),
            "a still mouse must not reset the idle clock (alpha {later})"
        );
    }

    #[test]
    fn disabling_the_setting_restores_the_overlay_and_resets_the_clock() {
        let mut hide = IdleHide::default();
        // The enabling tick is itself the first activity, so a zero delay
        // still hides on the next one and the fade starts there.
        hide.observe(observation(true, 0, Some(0), None, 0, 0));
        assert_eq!(
            hide.observe(observation(true, 0, Some(0), None, 0, 320)),
            1.0
        );
        assert!(hide.hidden());
        assert_eq!(
            hide.observe(observation(true, 0, Some(0), None, 0, 470)),
            0.5
        );
        assert_eq!(
            hide.observe(observation(true, 0, Some(0), None, 0, 620)),
            0.0
        );

        assert_eq!(
            hide.observe(observation(false, 0, Some(0), None, 0, 630)),
            0.0
        );
        assert!(
            !hide.hidden(),
            "turning the setting off restores routing at once"
        );
        assert_eq!(
            hide.observe(observation(false, 0, Some(0), None, 0, 780)),
            0.5
        );
        assert_eq!(
            hide.observe(observation(false, 0, Some(0), None, 0, 930)),
            1.0
        );
        // Re-enabling starts a fresh delay window instead of hiding at once,
        // even though the last activity was seconds ago.
        assert_eq!(
            hide.observe(observation(true, 1_000, Some(0), None, 0, 1_000)),
            1.0
        );
        assert!(!hide.hidden());
        hide.observe(observation(true, 1_000, Some(0), None, 0, 1_999));
        assert!(!hide.hidden(), "the fresh window is still running");
        hide.observe(observation(true, 1_000, Some(0), None, 0, 2_000));
        assert!(hide.hidden());
    }

    #[test]
    fn a_zero_delay_hides_as_soon_as_input_stops() {
        let mut hide = IdleHide::default();
        assert_eq!(hide.observe(observation(true, 0, Some(0), None, 0, 0)), 1.0);
        assert!(!hide.hidden(), "the enabling tick starts the clock first");
        assert_eq!(
            hide.observe(observation(true, 0, Some(0), None, 0, 10)),
            1.0
        );
        assert!(hide.hidden());
    }
}
