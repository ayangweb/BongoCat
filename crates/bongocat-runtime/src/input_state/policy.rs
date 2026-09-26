//! What the runtime believes is held, and how a missing release is found.
//!
//! The problem this exists for is issue #47: a key that goes down and whose
//! release is never delivered leaves the cat holding a key forever. The answer is
//! not a timeout on the animation — it is to keep, for every held control, when it
//! went down and from which source, and to reconcile that set against the events
//! arriving. A control whose source stops reporting is released; a control that
//! nothing ever reported is never invented, because synthesising a press is how a
//! reconciliation turns into a stuck key of the other kind.

use super::*;

impl ReconciliationPolicy {
    pub const fn missing_confirmations(self) -> u8 {
        self.missing_confirmations
    }
}

impl Default for ReconciliationPolicy {
    fn default() -> Self {
        Self {
            missing_confirmations: DEFAULT_MISSING_CONFIRMATIONS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PressedRecord {
    pub(crate) source: InputSource,
    pub(crate) pressed_at: MonotonicMillis,
    pub(crate) last_reconciled_at: Option<MonotonicMillis>,
    pub(crate) runtime_observed_at: Duration,
}
