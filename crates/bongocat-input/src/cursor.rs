use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::MonotonicMillis;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorViewport {
    pub origin: CursorPosition,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSample {
    pub position: CursorPosition,
    pub viewport: CursorViewport,
    pub at: MonotonicMillis,
}

/// A relative pointer motion reported by the device since the last sample.
///
/// It is separate from [`CursorPosition`] because the two are different facts:
/// the position is where the pointer is, the delta is how far the device moved
/// this time. An application that captures the pointer can keep the position
/// parked while the delta keeps arriving.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorDelta {
    pub x: f64,
    pub y: f64,
}

/// Runtime-owned pointer capture settings.
///
/// These decide how a pointer position is produced, before the position reaches
/// the smoothing and normalization that every model follows. They are settings
/// rather than platform facts because the same device motion has to be read two
/// different ways depending on what the foreground application does with the
/// pointer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CursorSettings {
    /// Follow relative device motion instead of the absolute cursor position.
    ///
    /// An application that captures the pointer — most full-screen games — keeps
    /// the operating-system cursor parked in one place, so the absolute position
    /// stops moving while the device still reports every movement. With this on,
    /// the position is accumulated from that relative motion instead, so the
    /// model keeps following the pointer in those applications. The trade is
    /// that a pointer moved by anything other than the device itself (a
    /// synthetic warp, a second absolute pointing device) is not followed while
    /// it is on, which is why it is off by default.
    pub force_move: bool,
}

/// A pointer position accumulated from relative device motion.
///
/// The accumulator exists for [`CursorSettings::force_move`]: it is seeded from
/// the absolute cursor so it starts where the user's pointer is, then advances
/// by each reported delta and stays inside the viewport. The first sample and
/// any sample on a different viewport reseed from the absolute position, so a
/// display change cannot carry accumulated motion across coordinate systems.
///
/// It is deterministic and has no clock of its own: a caller supplies the delta,
/// the absolute position and the viewport of one sample, and gets the position
/// to publish.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorMotionAccumulator {
    position: Option<CursorPosition>,
    viewport: Option<CursorViewport>,
}

impl CursorMotionAccumulator {
    /// Forget the accumulated position so the next [`Self::advance`] reseeds.
    ///
    /// Called when the mode is switched or the pointer pipeline is reset: a
    /// stale position from before the change must not be advanced again.
    pub fn reset(&mut self) {
        self.position = None;
        self.viewport = None;
    }

    /// Whether a position has been accumulated for a viewport.
    pub fn is_seeded(&self) -> bool {
        self.position.is_some()
    }

    /// Advance by one relative motion and return the position to publish.
    ///
    /// `absolute` is the viewport's own pointer reading. The first sample and
    /// any sample on a different viewport start from it and do not add `delta`:
    /// that motion is already part of where the pointer is, and adding it would
    /// overshoot by one sample. Every later sample advances by `delta` and is
    /// clamped inside `viewport`. A viewport with no usable area leaves the
    /// position unclamped rather than panicking, because a degenerate reading
    /// must not take the input worker down.
    pub fn advance(
        &mut self,
        delta: CursorDelta,
        absolute: CursorPosition,
        viewport: CursorViewport,
    ) -> CursorPosition {
        let position = match (self.viewport, self.position) {
            (Some(current), Some(position)) if current == viewport => CursorPosition {
                x: clamp_to_span(position.x + delta.x, viewport.origin.x, viewport.width),
                y: clamp_to_span(position.y + delta.y, viewport.origin.y, viewport.height),
            },
            _ => absolute,
        };
        self.position = Some(position);
        self.viewport = Some(viewport);
        position
    }
}

/// The pointer-capture state a platform adapter carries between samples.
///
/// [`CursorSettings::force_move`] makes the published position come from
/// accumulated device motion instead of the absolute cursor. This is the state
/// that has to survive between samples: the mode itself, so a change discards
/// the accumulated position rather than advancing a stale one, and the previous
/// location, which is what tells a position-reporting device from a captured
/// pointer when the platform has no flag for it.
///
/// The adapter supplies one sample at a time and gets back the position to
/// publish, so the policy is written once rather than once per platform.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorForceMoveState {
    active: bool,
    motion: CursorMotionAccumulator,
    last_location: Option<CursorPosition>,
}

impl CursorForceMoveState {
    /// Read the mode, discarding accumulated state when it changed.
    ///
    /// Returns whether accumulation is active, which is the caller's signal to
    /// publish an accumulated position instead of the absolute cursor.
    pub fn sync(&mut self, enabled: bool) -> bool {
        if self.active != enabled {
            self.active = enabled;
            self.reset();
        }
        self.active
    }

    /// Forget the accumulated position; the next [`Self::advance`] reseeds.
    ///
    /// Called on a mode change, on an input reset, and when the platform sees a
    /// device that reports a position rather than motion.
    pub fn reset(&mut self) {
        self.motion.reset();
        self.last_location = None;
    }

    /// Whether a position has been accumulated for a viewport.
    pub fn is_seeded(&self) -> bool {
        self.motion.is_seeded()
    }

    /// Advance by one sample and return the position to publish.
    ///
    /// A device that reports a position moves the location while reporting no
    /// motion; a captured pointer is the opposite, with the location parked
    /// while the motion keeps arriving. The former reseeds from `absolute`,
    /// which is always correct because it is where the pointer is, so an
    /// absolute pointing device cannot freeze the accumulated position.
    pub fn advance(
        &mut self,
        absolute: CursorPosition,
        delta: CursorDelta,
        viewport: CursorViewport,
    ) -> CursorPosition {
        let reports_position = delta == CursorDelta::default()
            && self.last_location.is_some_and(|last| last != absolute);
        if reports_position {
            self.motion.reset();
        }
        self.last_location = Some(absolute);
        self.motion.advance(delta, absolute, viewport)
    }
}

fn clamp_to_span(value: f64, origin: f64, extent: f64) -> f64 {
    let end = origin + extent;
    if !value.is_finite() || !origin.is_finite() || !end.is_finite() || extent <= 0.0 {
        return value;
    }
    value.clamp(origin, end)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorSampleError {
    NonFinite,
    EmptyViewport,
}

impl CursorSample {
    pub fn new(
        position: CursorPosition,
        viewport: CursorViewport,
        at: MonotonicMillis,
    ) -> Result<Self, CursorSampleError> {
        if !position.x.is_finite()
            || !position.y.is_finite()
            || !viewport.origin.x.is_finite()
            || !viewport.origin.y.is_finite()
            || !viewport.width.is_finite()
            || !viewport.height.is_finite()
        {
            return Err(CursorSampleError::NonFinite);
        }
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            return Err(CursorSampleError::EmptyViewport);
        }
        Ok(Self {
            position,
            viewport,
            at,
        })
    }

    pub fn normalized(self) -> NormalizedCursorPosition {
        normalize_position(self.position, self.viewport)
    }
}

fn normalize_position(
    position: CursorPosition,
    viewport: CursorViewport,
) -> NormalizedCursorPosition {
    let x_ratio = (position.x - viewport.origin.x) / viewport.width;
    let y_ratio = (position.y - viewport.origin.y) / viewport.height;
    let x = (1.0 - 2.0 * x_ratio).clamp(-1.0, 1.0) as f32;
    let y = (1.0 - 2.0 * y_ratio).clamp(-1.0, 1.0) as f32;
    NormalizedCursorPosition {
        x,
        y,
        z: (-x * y).clamp(-1.0, 1.0),
    }
}

const CURSOR_DAMPING_DECAY_AT_60_FPS: f64 = 0.75;
const CURSOR_SETTLE_DISTANCE: f64 = 0.5;

#[derive(Default)]
pub struct CursorSmoother {
    target: Option<CursorSample>,
    current: Option<CursorPosition>,
    last_updated_at: Option<Duration>,
}

impl CursorSmoother {
    pub fn set_target(&mut self, sample: CursorSample, now: Duration) {
        if self.target.is_none()
            || self
                .target
                .is_some_and(|target| target.viewport != sample.viewport)
        {
            self.current = Some(sample.position);
            self.target = Some(sample);
            self.last_updated_at = Some(now);
            return;
        }
        self.advance(now);
        self.target = Some(sample);
        self.last_updated_at = Some(now);
    }

    pub fn advance(&mut self, now: Duration) -> bool {
        let (Some(target), Some(current), Some(previous)) =
            (self.target, self.current, self.last_updated_at)
        else {
            return false;
        };
        if now <= previous || current == target.position {
            return false;
        }

        let frames = now.saturating_sub(previous).as_secs_f64() * 60.0;
        let alpha = 1.0 - CURSOR_DAMPING_DECAY_AT_60_FPS.powf(frames);
        let interpolated = CursorPosition {
            x: current.x + (target.position.x - current.x) * alpha,
            y: current.y + (target.position.y - current.y) * alpha,
        };
        let distance =
            (target.position.x - interpolated.x).hypot(target.position.y - interpolated.y);
        self.current = Some(if distance < CURSOR_SETTLE_DISTANCE {
            target.position
        } else {
            interpolated
        });
        self.last_updated_at = Some(now);
        true
    }

    pub fn normalized(&self) -> NormalizedCursorPosition {
        match (self.current, self.target) {
            (Some(current), Some(target)) => normalize_position(current, target.viewport),
            _ => NormalizedCursorPosition::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NormalizedCursorPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorTransportDiagnostics {
    pub published: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub non_monotonic: u64,
    pub rejected_after_stop: u64,
    pub pending: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorSnapshot {
    pub sample: Option<CursorSample>,
    pub transport: CursorTransportDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum CursorPublishError {
    #[error("cursor sample time moved backwards")]
    NonMonotonic(CursorSample),
    #[error("runtime is stopped")]
    RuntimeStopped(CursorSample),
}

#[derive(Default)]
struct CursorSlotState {
    pending: Option<CursorSample>,
    last_published_at: Option<MonotonicMillis>,
    stopped: bool,
    diagnostics: CursorTransportDiagnostics,
}

#[derive(Default)]
pub(crate) struct CursorSlot {
    state: Mutex<CursorSlotState>,
}

impl CursorSlot {
    fn publish(&self, sample: CursorSample) -> Result<(), CursorPublishError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            state.diagnostics.rejected_after_stop =
                state.diagnostics.rejected_after_stop.saturating_add(1);
            return Err(CursorPublishError::RuntimeStopped(sample));
        }
        if state
            .last_published_at
            .is_some_and(|previous| sample.at < previous)
        {
            state.diagnostics.non_monotonic = state.diagnostics.non_monotonic.saturating_add(1);
            return Err(CursorPublishError::NonMonotonic(sample));
        }
        state.last_published_at = Some(sample.at);
        state.diagnostics.published = state.diagnostics.published.saturating_add(1);
        if state.pending.replace(sample).is_some() {
            state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
        }
        Ok(())
    }

    pub(crate) fn take(&self) -> Option<CursorSample> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sample = state.pending.take();
        if sample.is_some() {
            state.diagnostics.consumed = state.diagnostics.consumed.saturating_add(1);
        }
        sample
    }

    pub(crate) fn stop(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stopped = true;
    }

    pub(crate) fn diagnostics(&self) -> CursorTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        CursorTransportDiagnostics {
            pending: u64::from(state.pending.is_some()),
            ..state.diagnostics
        }
    }
}

#[derive(Clone)]
pub struct CursorProducer {
    slot: Arc<CursorSlot>,
}

impl CursorProducer {
    pub fn new() -> Self {
        Self {
            slot: Arc::new(CursorSlot::default()),
        }
    }

    pub fn publish(&self, sample: CursorSample) -> Result<(), CursorPublishError> {
        self.slot.publish(sample)
    }

    pub fn take(&self) -> Option<CursorSample> {
        self.slot.take()
    }

    pub fn diagnostics(&self) -> CursorTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn stop(&self) {
        self.slot.stop();
    }
}

impl Default for CursorProducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: f64, at: u64) -> CursorSample {
        CursorSample::new(
            CursorPosition { x, y: 40.0 },
            CursorViewport {
                origin: CursorPosition { x: 0.0, y: 0.0 },
                width: 100.0,
                height: 100.0,
            },
            MonotonicMillis::new(at),
        )
        .expect("valid sample")
    }

    #[test]
    fn latest_slot_accounts_for_coalesced_consumed_and_pending_samples() {
        let slot = CursorSlot::default();
        for index in 0..10_000 {
            slot.publish(sample(index as f64, index))
                .expect("sample accepted");
        }
        assert_eq!(slot.take(), Some(sample(9_999.0, 9_999)));
        slot.publish(sample(10_000.0, 10_000))
            .expect("pending sample accepted");
        assert_eq!(
            slot.diagnostics(),
            CursorTransportDiagnostics {
                published: 10_001,
                coalesced: 9_999,
                consumed: 1,
                pending: 1,
                ..CursorTransportDiagnostics::default()
            }
        );
    }

    #[test]
    fn normalization_matches_legacy_display_relative_direction() {
        let normalized = sample(25.0, 0).normalized();
        assert_eq!(normalized.x, 0.5);
        assert!((normalized.y - 0.2).abs() < f32::EPSILON);
        assert!((normalized.z + 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn invalid_geometry_non_monotonic_time_and_stop_are_explicit() {
        assert_eq!(
            CursorSample::new(
                CursorPosition {
                    x: f64::NAN,
                    y: 0.0,
                },
                CursorViewport {
                    origin: CursorPosition { x: 0.0, y: 0.0 },
                    width: 1.0,
                    height: 1.0,
                },
                MonotonicMillis::new(0),
            ),
            Err(CursorSampleError::NonFinite)
        );
        let slot = CursorSlot::default();
        slot.publish(sample(0.0, 2)).expect("first sample");
        assert_eq!(
            slot.publish(sample(0.0, 1)),
            Err(CursorPublishError::NonMonotonic(sample(0.0, 1)))
        );
        slot.stop();
        assert_eq!(
            slot.publish(sample(0.0, 3)),
            Err(CursorPublishError::RuntimeStopped(sample(0.0, 3)))
        );
    }

    #[test]
    fn smoothing_matches_legacy_decay_and_is_frame_rate_independent() {
        let viewport = CursorViewport {
            origin: CursorPosition { x: 0.0, y: 0.0 },
            width: 100.0,
            height: 100.0,
        };
        let start = CursorSample::new(
            CursorPosition { x: 50.0, y: 50.0 },
            viewport,
            MonotonicMillis::new(0),
        )
        .expect("start sample");
        let target = CursorSample::new(
            CursorPosition { x: 0.0, y: 50.0 },
            viewport,
            MonotonicMillis::new(1),
        )
        .expect("target sample");
        let one_frame = Duration::from_secs_f64(1.0 / 60.0);

        let mut full_frame = CursorSmoother::default();
        full_frame.set_target(start, Duration::ZERO);
        full_frame.set_target(target, Duration::ZERO);
        assert!(full_frame.advance(one_frame));

        let mut half_frames = CursorSmoother::default();
        half_frames.set_target(start, Duration::ZERO);
        half_frames.set_target(target, Duration::ZERO);
        assert!(half_frames.advance(one_frame / 2));
        assert!(half_frames.advance(one_frame));

        let expected_x = 0.25;
        assert!((full_frame.normalized().x - expected_x).abs() < 1e-6);
        assert!((half_frames.normalized().x - expected_x).abs() < 1e-6);

        let mut settling = CursorSmoother::default();
        settling.set_target(start, Duration::ZERO);
        let nearby = CursorSample::new(
            CursorPosition { x: 49.0, y: 50.0 },
            viewport,
            MonotonicMillis::new(2),
        )
        .expect("nearby target");
        settling.set_target(nearby, Duration::ZERO);
        assert!(settling.advance(one_frame * 3));
        assert_eq!(settling.normalized(), nearby.normalized());
        assert!(!settling.advance(one_frame * 4));
    }

    #[test]
    fn first_sample_and_viewport_changes_snap_without_cross_display_drift() {
        let mut smoother = CursorSmoother::default();
        smoother.set_target(sample(25.0, 0), Duration::ZERO);
        assert_eq!(smoother.normalized(), sample(25.0, 0).normalized());

        let changed_viewport = CursorSample::new(
            CursorPosition { x: 150.0, y: 40.0 },
            CursorViewport {
                origin: CursorPosition { x: 100.0, y: 0.0 },
                width: 200.0,
                height: 100.0,
            },
            MonotonicMillis::new(1),
        )
        .expect("second display sample");
        smoother.set_target(changed_viewport, Duration::from_millis(1));
        assert_eq!(smoother.normalized(), changed_viewport.normalized());
    }

    fn viewport(origin_x: f64, origin_y: f64, width: f64, height: f64) -> CursorViewport {
        CursorViewport {
            origin: CursorPosition {
                x: origin_x,
                y: origin_y,
            },
            width,
            height,
        }
    }

    #[test]
    fn accumulator_seeds_from_the_absolute_cursor_then_follows_relative_motion() {
        let mut accumulator = CursorMotionAccumulator::default();
        assert!(!accumulator.is_seeded());
        let viewport = viewport(0.0, 0.0, 100.0, 100.0);
        // The seeding sample starts where the absolute cursor is and does not
        // add its own motion: that motion is already part of the position, so
        // adding it would overshoot by one sample.
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 8.0, y: 8.0 },
                CursorPosition { x: 50.0, y: 50.0 },
                viewport,
            ),
            CursorPosition { x: 50.0, y: 50.0 }
        );
        assert!(accumulator.is_seeded());
        // A parked absolute cursor plus motion still moves the accumulated one,
        // which is the whole point of the mode.
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 10.0, y: -4.0 },
                CursorPosition { x: 50.0, y: 50.0 },
                viewport,
            ),
            CursorPosition { x: 60.0, y: 46.0 }
        );
    }

    #[test]
    fn accumulator_clamps_inside_the_viewport_and_saturates_at_the_edge() {
        let mut accumulator = CursorMotionAccumulator::default();
        let viewport = viewport(100.0, 0.0, 100.0, 50.0);
        accumulator.advance(
            CursorDelta { x: 0.0, y: 0.0 },
            CursorPosition { x: 150.0, y: 25.0 },
            viewport,
        );
        // Pushing past the edge saturates rather than banking motion the user
        // would have to spend reversing before the pointer moves again.
        assert_eq!(
            accumulator.advance(
                CursorDelta {
                    x: 1_000.0,
                    y: -1_000.0
                },
                CursorPosition { x: 150.0, y: 25.0 },
                viewport,
            ),
            CursorPosition { x: 200.0, y: 0.0 }
        );
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: -1.0, y: 1.0 },
                CursorPosition { x: 150.0, y: 25.0 },
                viewport,
            ),
            CursorPosition { x: 199.0, y: 1.0 }
        );
    }

    #[test]
    fn accumulator_reseeds_when_the_viewport_changes_or_is_reset() {
        let first = viewport(0.0, 0.0, 100.0, 100.0);
        let second = viewport(100.0, 0.0, 200.0, 100.0);
        let mut accumulator = CursorMotionAccumulator::default();
        accumulator.advance(
            CursorDelta { x: 0.0, y: 0.0 },
            CursorPosition { x: 50.0, y: 50.0 },
            first,
        );
        // A display change must not carry motion across coordinate systems, so
        // the accumulated position snaps to the new viewport's own reading.
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 5.0, y: 5.0 },
                CursorPosition { x: 150.0, y: 40.0 },
                second,
            ),
            CursorPosition { x: 150.0, y: 40.0 }
        );

        accumulator.reset();
        assert!(!accumulator.is_seeded());
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 7.0, y: 0.0 },
                CursorPosition { x: 150.0, y: 40.0 },
                second,
            ),
            CursorPosition { x: 150.0, y: 40.0 }
        );
    }

    #[test]
    fn accumulator_tolerates_a_degenerate_viewport() {
        let mut accumulator = CursorMotionAccumulator::default();
        let empty = viewport(0.0, 0.0, 0.0, 0.0);
        // A viewport with no area must not panic and must not swallow the
        // position it was given.
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 3.0, y: 4.0 },
                CursorPosition { x: 10.0, y: 20.0 },
                empty,
            ),
            CursorPosition { x: 10.0, y: 20.0 }
        );
        assert_eq!(
            accumulator.advance(
                CursorDelta { x: 3.0, y: 4.0 },
                CursorPosition { x: 10.0, y: 20.0 },
                empty,
            ),
            CursorPosition { x: 13.0, y: 24.0 },
            "a degenerate viewport cannot clamp, so later motion is kept as it is"
        );
    }

    #[test]
    fn force_move_state_discards_the_accumulated_position_on_a_mode_change() {
        let viewport = viewport(0.0, 0.0, 100.0, 100.0);
        let mut state = CursorForceMoveState::default();
        assert!(state.sync(true), "the mode is reported as active");
        // The first sample seeds from the absolute cursor and drops its motion.
        assert_eq!(
            state.advance(
                CursorPosition { x: 50.0, y: 50.0 },
                CursorDelta { x: 4.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 50.0, y: 50.0 }
        );
        assert_eq!(
            state.advance(
                CursorPosition { x: 50.0, y: 50.0 },
                CursorDelta { x: 4.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 54.0, y: 50.0 }
        );

        // Turning the mode off and on again starts over: the position from
        // before the change describes a different reading.
        assert!(!state.sync(false), "the mode is reported as inactive");
        assert!(state.sync(true));
        assert_eq!(
            state.advance(
                CursorPosition { x: 80.0, y: 20.0 },
                CursorDelta { x: 4.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 80.0, y: 20.0 }
        );
    }

    #[test]
    fn force_move_state_reseeds_for_a_device_that_reports_a_position() {
        let viewport = viewport(0.0, 0.0, 100.0, 100.0);
        let mut state = CursorForceMoveState::default();
        state.sync(true);
        state.advance(
            CursorPosition { x: 50.0, y: 50.0 },
            CursorDelta { x: 4.0, y: 0.0 },
            viewport,
        );
        assert_eq!(
            state.advance(
                CursorPosition { x: 50.0, y: 50.0 },
                CursorDelta { x: 4.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 54.0, y: 50.0 }
        );
        // An absolute pointing device moves the location while reporting no
        // motion, so the accumulated position reseeds instead of freezing.
        assert_eq!(
            state.advance(
                CursorPosition { x: 70.0, y: 50.0 },
                CursorDelta::default(),
                viewport,
            ),
            CursorPosition { x: 70.0, y: 50.0 }
        );
        assert_eq!(
            state.advance(
                CursorPosition { x: 70.0, y: 50.0 },
                CursorDelta { x: 2.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 72.0, y: 50.0 }
        );
    }

    #[test]
    fn force_move_state_keeps_accumulating_when_a_moving_location_also_reports_motion() {
        // A captured application may recentre the cursor, which moves the
        // location while the device still reports motion. Reseeding there would
        // snap the model to the centre, so motion wins.
        let viewport = viewport(0.0, 0.0, 100.0, 100.0);
        let mut state = CursorForceMoveState::default();
        state.sync(true);
        state.advance(
            CursorPosition { x: 10.0, y: 10.0 },
            CursorDelta { x: 4.0, y: 0.0 },
            viewport,
        );
        assert_eq!(
            state.advance(
                CursorPosition { x: 30.0, y: 10.0 },
                CursorDelta { x: 4.0, y: 0.0 },
                viewport,
            ),
            CursorPosition { x: 14.0, y: 10.0 }
        );
    }

    #[test]
    fn idle_time_before_a_new_target_does_not_skip_smoothing() {
        let mut smoother = CursorSmoother::default();
        smoother.set_target(sample(50.0, 0), Duration::ZERO);
        smoother.set_target(sample(0.0, 1), Duration::from_secs(60));
        assert_eq!(smoother.normalized(), sample(50.0, 0).normalized());

        assert!(smoother.advance(Duration::from_secs(60) + Duration::from_secs_f64(1.0 / 60.0)));
        assert!((smoother.normalized().x - 0.25).abs() < 1e-6);
    }
}
