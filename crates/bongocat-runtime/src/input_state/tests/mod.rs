//! The pressed-set tests, split by what they cover.
//!
//! The reconciliation tests are the ones that matter: issue #47 is a release that
//! never arrives, and every one of them is a way that release can be missed.

use super::*;

const A: InputControl = InputControl::Key(PhysicalKey::KEY_A);

const CTRL: InputControl = InputControl::Key(PhysicalKey::LEFT_CONTROL);

const ALT: InputControl = InputControl::Key(PhysicalKey::LEFT_ALT);

fn edge(sequence: u64, at: u64, control: InputControl, edge: InputEdge) -> SequencedInputEvent {
    SequencedInputEvent {
        sequence,
        event: InputEvent::Edge {
            control,
            edge,
            source: InputSource::Capture,
            at: MonotonicMillis::new(at),
        },
    }
}

mod fallback;
mod reconcile;
mod reset;
mod snapshot_of;
