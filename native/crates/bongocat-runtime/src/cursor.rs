use std::sync::{Arc, Mutex};

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
        let x_ratio = (self.position.x - self.viewport.origin.x) / self.viewport.width;
        let y_ratio = (self.position.y - self.viewport.origin.y) / self.viewport.height;
        let x = (1.0 - 2.0 * x_ratio).clamp(-1.0, 1.0) as f32;
        let y = (1.0 - 2.0 * y_ratio).clamp(-1.0, 1.0) as f32;
        NormalizedCursorPosition {
            x,
            y,
            z: (-x * y).clamp(-1.0, 1.0),
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
}
