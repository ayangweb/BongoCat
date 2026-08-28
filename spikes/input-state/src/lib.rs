#![forbid(unsafe_code)]

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InputKey(pub u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Down(InputKey),
    Up(InputKey),
    /// Snapshot of keys currently held by the operating system.
    Reconcile(BTreeSet<InputKey>),
    /// Clears all local state after a session/device/desktop transition.
    Reset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInputEvent {
    pub sequence: u64,
    pub event: InputEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceDisposition {
    Applied,
    AppliedAfterGap { missing: u64 },
    Duplicate,
    OutOfOrder,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputCounters {
    pub captured_down: u64,
    pub captured_up: u64,
    pub reconciled_release: u64,
    pub duplicate_down: u64,
    pub reset: u64,
    pub sequence_gaps: u64,
    pub missing_sequences: u64,
    pub duplicate_sequences: u64,
    pub out_of_order_sequences: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PressedState {
    pressed: BTreeSet<InputKey>,
    counters: InputCounters,
    last_sequence: Option<u64>,
}

impl PressedState {
    pub fn pressed(&self) -> &BTreeSet<InputKey> {
        &self.pressed
    }

    pub fn counters(&self) -> InputCounters {
        self.counters
    }

    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    pub fn apply(&mut self, event: InputEvent) {
        self.apply_event(event);
    }

    pub fn apply_sequenced(&mut self, envelope: SequencedInputEvent) -> SequenceDisposition {
        if let Some(last_sequence) = self.last_sequence {
            if envelope.sequence == last_sequence {
                self.counters.duplicate_sequences += 1;
                return SequenceDisposition::Duplicate;
            }
            if envelope.sequence < last_sequence {
                self.counters.out_of_order_sequences += 1;
                return SequenceDisposition::OutOfOrder;
            }
            let missing = envelope.sequence - last_sequence - 1;
            self.last_sequence = Some(envelope.sequence);
            if missing > 0 {
                self.counters.sequence_gaps += 1;
                self.counters.missing_sequences += missing;
                // Unknown edges may have changed the OS state. Reset before
                // applying the first known event after a gap.
                self.apply_event(InputEvent::Reset);
                self.apply_event(envelope.event);
                return SequenceDisposition::AppliedAfterGap { missing };
            }
        } else {
            self.last_sequence = Some(envelope.sequence);
        }
        self.apply_event(envelope.event);
        SequenceDisposition::Applied
    }

    fn apply_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Down(key) => {
                if !self.pressed.insert(key) {
                    self.counters.duplicate_down += 1;
                } else {
                    self.counters.captured_down += 1;
                }
            }
            InputEvent::Up(key) => {
                self.counters.captured_up += 1;
                self.pressed.remove(&key);
            }
            InputEvent::Reconcile(snapshot) => {
                let before = self.pressed.len();
                self.pressed.retain(|key| snapshot.contains(key));
                self.counters.reconciled_release +=
                    (before.saturating_sub(self.pressed.len())) as u64;
            }
            InputEvent::Reset => {
                self.pressed.clear();
                self.counters.reset += 1;
            }
        }
    }

    pub fn is_pressed(&self, key: InputKey) -> bool {
        self.pressed.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: InputKey = InputKey(29);
    const ALT: InputKey = InputKey(56);
    const A: InputKey = InputKey(30);

    fn snapshot(keys: impl IntoIterator<Item = InputKey>) -> BTreeSet<InputKey> {
        keys.into_iter().collect()
    }

    #[test]
    fn issue_47_lost_release_is_reconciled() {
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(CTRL));
        state.apply(InputEvent::Down(ALT));
        state.apply(InputEvent::Down(A));
        // The A-up edge is lost by the platform callback.
        state.apply(InputEvent::Up(ALT));
        state.apply(InputEvent::Up(CTRL));
        state.apply(InputEvent::Reconcile(snapshot([])));
        assert!(state.pressed().is_empty());
        assert_eq!(state.counters().reconciled_release, 1);
    }

    #[test]
    fn reconciliation_does_not_clear_keys_still_held() {
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(CTRL));
        state.apply(InputEvent::Down(A));
        state.apply(InputEvent::Reconcile(snapshot([CTRL])));
        assert!(state.is_pressed(CTRL));
        assert!(!state.is_pressed(A));
    }

    #[test]
    fn duplicate_down_is_visible_and_does_not_duplicate_state() {
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(A));
        state.apply(InputEvent::Down(A));
        assert_eq!(state.pressed().len(), 1);
        assert_eq!(state.counters().duplicate_down, 1);
        assert_eq!(state.counters().captured_down, 1);
    }

    #[test]
    fn reset_clears_everything_after_session_transition() {
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(CTRL));
        state.apply(InputEvent::Down(ALT));
        state.apply(InputEvent::Reset);
        assert!(state.pressed().is_empty());
        assert_eq!(state.counters().reset, 1);
    }

    #[test]
    fn sequence_gap_resets_before_applying_current_event() {
        let mut state = PressedState::default();
        assert_eq!(
            state.apply_sequenced(SequencedInputEvent {
                sequence: 1,
                event: InputEvent::Down(CTRL),
            }),
            SequenceDisposition::Applied
        );
        assert_eq!(
            state.apply_sequenced(SequencedInputEvent {
                sequence: 3,
                event: InputEvent::Down(A),
            }),
            SequenceDisposition::AppliedAfterGap { missing: 1 }
        );
        assert!(!state.is_pressed(CTRL));
        assert!(state.is_pressed(A));
        assert_eq!(state.counters().sequence_gaps, 1);
        assert_eq!(state.counters().missing_sequences, 1);
        assert_eq!(state.counters().reset, 1);
        assert_eq!(state.last_sequence(), Some(3));
    }

    #[test]
    fn duplicate_and_out_of_order_sequences_are_ignored_and_counted() {
        let mut state = PressedState::default();
        state.apply_sequenced(SequencedInputEvent {
            sequence: 4,
            event: InputEvent::Down(A),
        });
        assert_eq!(
            state.apply_sequenced(SequencedInputEvent {
                sequence: 4,
                event: InputEvent::Up(A),
            }),
            SequenceDisposition::Duplicate
        );
        assert!(state.is_pressed(A));
        assert_eq!(
            state.apply_sequenced(SequencedInputEvent {
                sequence: 2,
                event: InputEvent::Up(A),
            }),
            SequenceDisposition::OutOfOrder
        );
        assert!(state.is_pressed(A));
        assert_eq!(state.counters().duplicate_sequences, 1);
        assert_eq!(state.counters().out_of_order_sequences, 1);
    }
}
