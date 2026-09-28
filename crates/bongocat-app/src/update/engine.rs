//! What the worker is allowed to ask of the update subsystem.
//!
//! The worker talks to a trait rather than to [`UpdateRuntime`] directly, so a
//! test can supply an engine that is not the network. That is the only reason the
//! trait exists: there is one real implementation, and the second one is a fake
//! that lets a failed check and a half-finished install be exercised without a
//! server.

use super::*;

/// The update pipeline the worker drives.
///
/// The worker's own job is the state machine around the pipeline: which phase is
/// published when, which failures are attributed to which stage, and what happens to
/// the phase the window is rendering. Depending on this trait rather than on
/// [`UpdateRuntime`] directly is what lets that state machine be exercised without a
/// network, a published release or a real install.
pub(crate) trait UpdateEngine: Send + 'static {
    fn unavailability(&self) -> Option<UpdateUnavailability>;
    fn release_page_url(&self, version: &str) -> Option<String>;
    fn check(&self) -> Result<UpdateOutcome, UpdateError>;
    fn install(&self, observe: &dyn Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError>;
}

impl UpdateEngine for UpdateRuntime {
    fn unavailability(&self) -> Option<UpdateUnavailability> {
        UpdateRuntime::unavailability(self)
    }

    fn release_page_url(&self, version: &str) -> Option<String> {
        UpdateRuntime::release_page_url(self, version)
    }

    fn check(&self) -> Result<UpdateOutcome, UpdateError> {
        UpdateRuntime::check(self)
    }

    fn install(&self, observe: &dyn Fn(UpdateEvent)) -> Result<UpdateOutcome, UpdateError> {
        self.install_with_observer(observe)
    }
}
