//! Coalescing an overlay drag into one placement write.
//!
//! A resize drag reports continuously and the configuration is not a hot path, so
//! only the latest value is sent, after the pointer has been still long enough. A
//! shutdown flushes whatever is pending rather than dropping it.

use super::*;

pub(crate) const OVERLAY_PLACEMENT_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Default)]
pub(crate) struct OverlayPlacementDebouncer {
    last_sent_at: Option<Instant>,
    pending: Option<OverlayWindowBounds>,
}

impl OverlayPlacementDebouncer {
    pub(crate) fn observe(
        &mut self,
        bounds: OverlayWindowBounds,
        now: Instant,
    ) -> Option<OverlayWindowBounds> {
        self.pending = Some(bounds);
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= OVERLAY_PLACEMENT_DEBOUNCE)
        {
            self.last_sent_at = Some(now);
            self.pending
        } else {
            None
        }
    }

    pub(crate) fn mark_sent(&mut self, bounds: OverlayWindowBounds) {
        if self.pending == Some(bounds) {
            self.pending = None;
        }
    }

    pub(crate) fn flush(&mut self, now: Instant) -> Option<OverlayWindowBounds> {
        let pending = self.pending;
        if pending.is_some() {
            self.last_sent_at = Some(now);
        }
        pending
    }
}
