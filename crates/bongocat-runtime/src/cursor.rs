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
pub(crate) struct CursorSmoother {
    target: Option<CursorSample>,
    current: Option<CursorPosition>,
    last_updated_at: Option<Duration>,
}

impl CursorSmoother {
    pub(crate) fn set_target(&mut self, sample: CursorSample, now: Duration) {
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

    pub(crate) fn advance(&mut self, now: Duration) -> bool {
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

    pub(crate) fn normalized(&self) -> NormalizedCursorPosition {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorPublishError {
    NonMonotonic(CursorSample),
    RuntimeStopped(CursorSample),
}

impl std::fmt::Display for CursorPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NonMonotonic(_) => "cursor sample time moved backwards",
            Self::RuntimeStopped(_) => "runtime is stopped",
        })
    }
}

impl std::error::Error for CursorPublishError {}

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
    pub(crate) fn new(slot: Arc<CursorSlot>) -> Self {
        Self { slot }
    }

    pub fn publish(&self, sample: CursorSample) -> Result<(), CursorPublishError> {
        self.slot.publish(sample)
    }

    pub fn diagnostics(&self) -> CursorTransportDiagnostics {
        self.slot.diagnostics()
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
