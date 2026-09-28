//! What the transport says when it cannot carry a frame.
//!
//! Every failure here is one the caller can act on, and none of them is silent: a
//! dropped frame that says nothing is indistinguishable from a model that stopped
//! moving. The diagnostics carry counts so a reader can tell a producer that
//! produced nothing from one whose output was refused.

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderTransportDiagnostics {
    pub published: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub non_monotonic: u64,
    pub rejected_after_close: u64,
    pub pending: u64,
    pub feedback_reported: u64,
    pub feedback_consumed: u64,
    pub feedback_occupied: u64,
    pub feedback_rejected_after_close: u64,
    pub feedback_stale: u64,
    pub feedback_pending: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderPublishError {
    #[error("render frame sequence moved backwards")]
    NonMonotonic(RenderFrame),
    #[error("render transport is closed")]
    Closed(RenderFrame),
}

impl RenderPublishError {
    pub fn into_frame(self) -> RenderFrame {
        match self {
            Self::NonMonotonic(frame) | Self::Closed(frame) => frame,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelCommitFeedbackError {
    #[error("a model commit result is already pending")]
    Occupied(ModelCommitFeedback),
    #[error("render transport is closed")]
    Closed(ModelCommitFeedback),
}

impl ModelCommitFeedbackError {
    pub fn into_feedback(self) -> ModelCommitFeedback {
        match self {
            Self::Occupied(feedback) | Self::Closed(feedback) => feedback,
        }
    }
}
