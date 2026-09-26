//! A check the window asked for and has not seen an answer to.
//!
//! A user who presses Check twice should not see the first result arrive and
//! overwrite the second, so the window records what it asked and only accepts an
//! answer for that revision. A check is only asked for when one would actually
//! start: asking while a published result is on screen would replace a version
//! the user has not looked at yet with the same version again.

use super::*;

/// A check this view asked for, and the published revision it asked from.
///
/// The worker is the only writer of the published state, and it publishes `Checking`
/// when it takes the command. The command is a message to another thread, so from the
/// request until that publish the published state still describes the *previous*
/// check. Rendering that is what made a window opened for a new check show the last
/// result for a moment before its progress bar appeared.
///
/// The revision the request was made at is what makes the locally rendered phase
/// safe rather than a guess. A worker that has not answered has not moved the
/// revision, so the stale phase is not adopted; a worker that answers answers with a
/// revision that differs, so the answer always wins. A build that cannot update is
/// excluded before this is ever set — see [`asks_for_a_new_check`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PendingCheck {
    pub(crate) asked_at: Option<u64>,
}

impl PendingCheck {
    /// Whether a check has been asked for and not answered yet.
    pub(crate) fn is_pending(&self) -> bool {
        self.asked_at.is_some()
    }

    /// Render the check this view asked for instead of the last one's result.
    pub(crate) fn begin(&mut self, snapshot: &mut UpdateSnapshot) {
        self.asked_at = Some(snapshot.revision);
        snapshot.phase = UpdatePhase::Checking;
    }

    /// Whether a published revision is the worker answering.
    pub(crate) fn answers(&self, revision: u64) -> bool {
        self.asked_at != Some(revision)
    }

    /// The worker answered; what is published is the current fact again.
    pub(crate) fn settle(&mut self) {
        self.asked_at = None;
    }
}

/// Whether a check asked for now would start one.
///
/// Pure because it is a decision about two facts rather than about a window. It is
/// what keeps a second request from being sent while one is outstanding, and it is
/// what keeps a build that cannot update out of a progress bar that would never
/// resolve: such a build's worker republishes the phase it already had, which advances
/// no revision and therefore would never answer.
pub(crate) const fn asks_for_a_new_check(phase: &UpdatePhase, pending: bool) -> bool {
    !pending && !phase.is_busy() && !matches!(phase, UpdatePhase::Unavailable { .. })
}
