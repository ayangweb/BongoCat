//! Frame pacing and the injectable clock the runtime measures against.
//!
//! Pacing is deadline-anchored rather than sleep-after-work: waiting one interval
//! after a frame finishes makes the achieved cadence `interval + work`, so the
//! configured maximum is never reached and the shortfall is worst at the high end
//! of the range. A frame that overruns its slot skips the slots it missed rather
//! than producing a catch-up burst.

use crate::{HIDDEN_OVERLAY_FRAME_INTERVAL, RuntimeWorkDiagnostics, maximum_fps_is_valid};
use std::time::{Duration, Instant};

pub fn frame_interval_for_maximum_fps(maximum_fps: u16) -> Option<Duration> {
    maximum_fps_is_valid(maximum_fps).then(|| Duration::from_secs_f64(1.0 / f64::from(maximum_fps)))
}

pub fn frame_interval_for_runtime(maximum_fps: u16, overlay_visible: bool) -> Option<Duration> {
    let visible_interval = frame_interval_for_maximum_fps(maximum_fps)?;
    Some(if overlay_visible {
        visible_interval
    } else {
        HIDDEN_OVERLAY_FRAME_INTERVAL
    })
}

pub(crate) fn runtime_tick_work_budget(maximum_fps: u16) -> Duration {
    frame_interval_for_maximum_fps(maximum_fps)
        .map(|interval| interval / 2)
        .unwrap_or_else(|| Duration::from_millis(8))
}

pub(crate) fn record_work_budget(
    diagnostics: &mut RuntimeWorkDiagnostics,
    elapsed: Duration,
    budget: Duration,
) {
    if elapsed <= budget {
        return;
    }
    diagnostics.budget_exceeded = diagnostics.budget_exceeded.saturating_add(1);
    diagnostics.last_over_budget_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;
}

pub(crate) fn runtime_frame_interval(maximum_fps: u16, overlay_visible: bool) -> Duration {
    frame_interval_for_runtime(maximum_fps, overlay_visible)
        .expect("runtime frame scheduling state is validated before it is stored")
}

/// Deadline-anchored pacing for a frame source.
///
/// Waiting one frame interval *after* a frame is finished makes the achieved
/// cadence `interval + work`, so the overshoot grows with the frame cost: the
/// configured `maximum_fps` is then never reached and the shortfall is worst at
/// the high end of the range. The pacer keeps a fixed grid of deadlines
/// instead, so the wait absorbs the work already spent in the current
/// iteration. A frame that overruns its slot skips the slots it missed rather
/// than producing a catch-up burst, and a schedule change re-anchors the grid
/// so the new interval holds from the next frame.
#[derive(Clone, Copy, Debug)]
pub struct FramePacer {
    interval: Duration,
    deadline: Instant,
}

impl FramePacer {
    /// Starts a grid whose first frame is due one `interval` after `now`.
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            interval,
            deadline: now + interval,
        }
    }

    /// How long the caller may wait before the next frame is due; never
    /// negative, because an already-due frame may not wait at all.
    ///
    /// `interval` is the schedule that currently applies. A value different
    /// from the one the grid was built on — a `maximum_fps` change or the
    /// hidden-overlay throttle — re-anchors the grid before the wait is
    /// measured, so the new cadence takes effect without a restart.
    pub fn wait(&mut self, now: Instant, interval: Duration) -> Duration {
        if interval != self.interval {
            self.interval = interval;
            self.deadline = now + interval;
        }
        self.deadline.saturating_duration_since(now)
    }

    /// Records a frame produced at `now` and advances the grid.
    ///
    /// A frame that a command produced ahead of its slot leaves the slot
    /// pending, so the wait measured for the next frame is unchanged. Once the
    /// slot has elapsed the grid advances by exactly one interval, which is
    /// what keeps the achieved cadence on the configured rate instead of
    /// `interval + work`; a slot that was overrun entirely re-anchors instead
    /// of producing a catch-up burst.
    pub fn frame_produced(&mut self, now: Instant, interval: Duration) {
        if interval != self.interval {
            self.interval = interval;
            self.deadline = now + interval;
            return;
        }
        if now < self.deadline {
            return;
        }
        let next = self.deadline + interval;
        self.deadline = if next > now { next } else { now + interval };
    }
}

pub trait MonotonicClock: Send + Sync + 'static {
    fn now(&self) -> Duration;
}

pub(crate) struct SystemMonotonicClock {
    origin: Instant,
}

impl SystemMonotonicClock {
    pub(crate) fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}
