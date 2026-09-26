//! The pressed-key set the runtime keeps, and the policy that keeps it honest.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use bongocat_input::{
    GamepadButton, GamepadConnection, HandSide, InputBindings, InputControl, InputDiagnostics,
    InputEdge, InputEvent, InputResetReason, InputSource, InputTransportDiagnostics,
    MonotonicMillis, MouseButton, NormalizedCursorPosition, SequencedInputEvent,
};
#[cfg(test)]
use bongocat_input::{GamepadButtonKey, PhysicalKey};
use bongocat_render::{KeyIdentity, KeyPress, KeyPressSet, KeySide};

const DEFAULT_MISSING_CONFIRMATIONS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReconciliationPolicy {
    missing_confirmations: u8,
}

mod apply;
mod policy;
mod reset;
mod snapshot;
mod snapshot_of;
#[cfg(test)]
mod tests;

pub(crate) use policy::*;
pub(crate) use snapshot::*;
// `apply`, `reset` and `snapshot_of` hold nothing but `impl` blocks, so a glob
// over them would re-export no name at all.

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the runtime names are listed here rather than left to a glob.
pub use snapshot::{InputSnapshot, ModelInputSnapshot};

#[derive(Debug, Default)]
pub(crate) struct InputState {
    pub(crate) pressed: BTreeMap<InputControl, PressedRecord>,
    pub(crate) active_gamepads: BTreeSet<GamepadConnection>,
    pub(crate) missing_confirmations: BTreeMap<InputControl, u8>,
    pub(crate) policy: ReconciliationPolicy,
    pub(crate) diagnostics: InputDiagnostics,
    pub(crate) last_sequence: Option<u64>,
    pub(crate) last_timestamp: Option<MonotonicMillis>,
    pub(crate) last_reset_reason: Option<InputResetReason>,
}
