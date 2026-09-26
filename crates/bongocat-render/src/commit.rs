//! The handshake that swaps one model for another mid-frame.
//!
//! A model switch cannot happen between a drawable being read and being drawn, so
//! the renderer publishes a commit token with the frame and the runtime refuses a
//! switch that would invalidate it. The sequence number is what makes a stale
//! frame identifiable: it has to increase within a model and across a change of
//! one, or a frame from the previous model would look current.

use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModelCommitToken {
    pub command_sequence: u64,
    pub model_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCommitErrorCode {
    ResourcePreparationFailed,
}

impl ModelCommitErrorCode {
    pub const ALL: [Self; 1] = [Self::ResourcePreparationFailed];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourcePreparationFailed => "model_commit_resource_preparation_failed",
        }
    }
}

impl fmt::Display for ModelCommitErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCommitOutcome {
    Prepared,
    Rejected(ModelCommitErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelCommitFeedback {
    pub token: ModelCommitToken,
    pub outcome: ModelCommitOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    pub transport_sequence: u64,
    pub model_generation: u64,
    pub frame_number: u64,
    pub model_commit: Option<ModelCommitToken>,
    pub resources: Arc<RenderResources>,
    pub snapshot: Arc<RenderSnapshot>,
}
