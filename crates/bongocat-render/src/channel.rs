//! The bounded channel between the runtime's renderer and the overlay.
//!
//! The channel carries the latest frame and coalesces: a producer that is faster
//! than the consumer overwrites rather than queues, because a frame that arrives
//! late is a frame nobody looks at. The one thing that is never coalesced is a
//! pending model commit — discarding it would mean a switch that silently did not
//! happen — and the one thing that is never overwritten is the commit feedback the
//! runtime is waiting for.

use super::*;

#[derive(Default)]
pub(crate) struct LatestFrameState {
    pub(crate) pending: Option<RenderFrame>,
    // Model commit frames are control-plane messages. Keep them reliable even
    // when the data-plane latest frame is replaced by a faster producer.
    pub(crate) pending_model_commit: Option<RenderFrame>,
    pub(crate) last_transport_sequence: Option<u64>,
    pub(crate) feedback: Option<ModelCommitFeedback>,
    pub(crate) closed: bool,
    pub(crate) diagnostics: RenderTransportDiagnostics,
}

#[derive(Default)]
pub(crate) struct LatestFrameSlot {
    pub(crate) state: Mutex<LatestFrameState>,
}

pub struct RenderProducer {
    pub(crate) slot: Arc<LatestFrameSlot>,
}

impl RenderProducer {
    pub fn publish(&self, frame: RenderFrame) -> Result<(), RenderPublishError> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.diagnostics.rejected_after_close =
                state.diagnostics.rejected_after_close.saturating_add(1);
            return Err(RenderPublishError::Closed(frame));
        }
        if state
            .last_transport_sequence
            .is_some_and(|previous| frame.transport_sequence <= previous)
        {
            state.diagnostics.non_monotonic = state.diagnostics.non_monotonic.saturating_add(1);
            return Err(RenderPublishError::NonMonotonic(frame));
        }
        state.last_transport_sequence = Some(frame.transport_sequence);
        state.diagnostics.published = state.diagnostics.published.saturating_add(1);
        if frame.model_commit.is_some() {
            if state.pending_model_commit.replace(frame).is_some() {
                state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
            }
        } else if state.pending.replace(frame).is_some() {
            state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
        }
        Ok(())
    }

    pub fn close(&self) {
        self.slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
    }

    pub fn diagnostics(&self) -> RenderTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn take_model_commit_feedback(&self) -> Option<ModelCommitFeedback> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let feedback = state.feedback.take();
        if feedback.is_some() {
            state.diagnostics.feedback_consumed =
                state.diagnostics.feedback_consumed.saturating_add(1);
        }
        feedback
    }

    pub fn record_stale_model_commit_feedback(&self) {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.diagnostics.feedback_stale = state.diagnostics.feedback_stale.saturating_add(1);
    }
}

pub struct RenderConsumer {
    pub(crate) slot: Arc<LatestFrameSlot>,
}

impl RenderConsumer {
    /// Consume only a reliable model commit frame, leaving current-generation
    /// coalesced data available for the next visible render pass.
    pub fn take_model_commit(&self) -> Option<RenderFrame> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = state.pending_model_commit.take();
        if let Some(frame) = frame.as_ref() {
            if state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.model_generation < frame.model_generation)
            {
                state.pending = None;
                state.diagnostics.coalesced = state.diagnostics.coalesced.saturating_add(1);
            }
            state.diagnostics.consumed = state.diagnostics.consumed.saturating_add(1);
        }
        frame
    }

    pub fn take_latest(&self) -> Option<RenderFrame> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frame = state
            .pending_model_commit
            .take()
            .or_else(|| state.pending.take());
        if frame.is_some() {
            state.diagnostics.consumed = state.diagnostics.consumed.saturating_add(1);
        }
        frame
    }

    pub fn diagnostics(&self) -> RenderTransportDiagnostics {
        self.slot.diagnostics()
    }

    pub fn report_model_commit(
        &self,
        feedback: ModelCommitFeedback,
    ) -> Result<(), ModelCommitFeedbackError> {
        let mut state = self
            .slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            state.diagnostics.feedback_rejected_after_close = state
                .diagnostics
                .feedback_rejected_after_close
                .saturating_add(1);
            return Err(ModelCommitFeedbackError::Closed(feedback));
        }
        if state.feedback.is_some() {
            state.diagnostics.feedback_occupied =
                state.diagnostics.feedback_occupied.saturating_add(1);
            return Err(ModelCommitFeedbackError::Occupied(feedback));
        }
        state.feedback = Some(feedback);
        state.diagnostics.feedback_reported = state.diagnostics.feedback_reported.saturating_add(1);
        Ok(())
    }
}

impl LatestFrameSlot {
    pub(crate) fn diagnostics(&self) -> RenderTransportDiagnostics {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RenderTransportDiagnostics {
            pending: u64::from(state.pending.is_some())
                .saturating_add(u64::from(state.pending_model_commit.is_some())),
            feedback_pending: u64::from(state.feedback.is_some()),
            ..state.diagnostics
        }
    }
}

pub fn latest_render_channel() -> (RenderProducer, RenderConsumer) {
    let slot = Arc::new(LatestFrameSlot::default());
    (
        RenderProducer {
            slot: Arc::clone(&slot),
        },
        RenderConsumer { slot },
    )
}
