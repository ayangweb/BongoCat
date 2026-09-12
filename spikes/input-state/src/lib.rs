#![forbid(unsafe_code)]

use std::collections::BTreeSet;

pub const DEFAULT_RECONCILIATION_INTERVAL_MS: u64 = 250;
pub const DEFAULT_MISSING_CONFIRMATIONS: u8 = 2;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconciliationPolicy {
    interval_ms: u64,
    missing_confirmations: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationPolicyError {
    ZeroInterval,
    ZeroConfirmations,
}

impl ReconciliationPolicy {
    pub const fn new(
        interval_ms: u64,
        missing_confirmations: u8,
    ) -> Result<Self, ReconciliationPolicyError> {
        if interval_ms == 0 {
            return Err(ReconciliationPolicyError::ZeroInterval);
        }
        if missing_confirmations == 0 {
            return Err(ReconciliationPolicyError::ZeroConfirmations);
        }
        Ok(Self {
            interval_ms,
            missing_confirmations,
        })
    }

    pub const fn interval_ms(self) -> u64 {
        self.interval_ms
    }

    pub const fn missing_confirmations(self) -> u8 {
        self.missing_confirmations
    }
}

impl Default for ReconciliationPolicy {
    fn default() -> Self {
        Self {
            interval_ms: DEFAULT_RECONCILIATION_INTERVAL_MS,
            missing_confirmations: DEFAULT_MISSING_CONFIRMATIONS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationClockError {
    NonMonotonic { previous_ms: u64, received_ms: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationSchedule {
    Due,
    NotDue { remaining_ms: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconciliationScheduler {
    policy: ReconciliationPolicy,
    last_run_ms: Option<u64>,
}

impl ReconciliationScheduler {
    pub const fn new(policy: ReconciliationPolicy) -> Self {
        Self {
            policy,
            last_run_ms: None,
        }
    }

    pub const fn policy(self) -> ReconciliationPolicy {
        self.policy
    }

    pub const fn last_run_ms(self) -> Option<u64> {
        self.last_run_ms
    }

    pub fn poll(
        &mut self,
        now_ms: u64,
    ) -> Result<ReconciliationSchedule, ReconciliationClockError> {
        let Some(previous_ms) = self.last_run_ms else {
            self.last_run_ms = Some(now_ms);
            return Ok(ReconciliationSchedule::Due);
        };
        if now_ms < previous_ms {
            return Err(ReconciliationClockError::NonMonotonic {
                previous_ms,
                received_ms: now_ms,
            });
        }
        let elapsed_ms = now_ms - previous_ms;
        if elapsed_ms >= self.policy.interval_ms() {
            self.last_run_ms = Some(now_ms);
            Ok(ReconciliationSchedule::Due)
        } else {
            Ok(ReconciliationSchedule::NotDue {
                remaining_ms: self.policy.interval_ms() - elapsed_ms,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub checked: usize,
    pub released: usize,
    pub still_pressed: usize,
    pub pending_confirmations: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PressedState {
    pressed: BTreeSet<InputKey>,
    missing_confirmations: std::collections::BTreeMap<InputKey, u8>,
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
                self.missing_confirmations.remove(&key);
                if !self.pressed.insert(key) {
                    self.counters.duplicate_down += 1;
                } else {
                    self.counters.captured_down += 1;
                }
            }
            InputEvent::Up(key) => {
                self.counters.captured_up += 1;
                self.pressed.remove(&key);
                self.missing_confirmations.remove(&key);
            }
            InputEvent::Reconcile(snapshot) => {
                let before = self.pressed.len();
                self.pressed.retain(|key| snapshot.contains(key));
                self.counters.reconciled_release +=
                    (before.saturating_sub(self.pressed.len())) as u64;
                self.missing_confirmations.clear();
            }
            InputEvent::Reset => {
                self.pressed.clear();
                self.missing_confirmations.clear();
                self.counters.reset += 1;
            }
        }
    }

    pub fn is_pressed(&self, key: InputKey) -> bool {
        self.pressed.contains(&key)
    }

    pub fn reconcile_with_policy(
        &mut self,
        snapshot: &BTreeSet<InputKey>,
        policy: ReconciliationPolicy,
    ) -> ReconciliationReport {
        let mut released = 0usize;
        let pressed_before = self.pressed.len();
        let threshold = policy.missing_confirmations();
        let pressed_keys = self.pressed.iter().copied().collect::<Vec<_>>();
        for key in pressed_keys {
            if snapshot.contains(&key) {
                self.missing_confirmations.remove(&key);
                continue;
            }
            let confirmations = self.missing_confirmations.entry(key).or_insert(0);
            *confirmations = confirmations.saturating_add(1);
            if *confirmations >= threshold {
                self.pressed.remove(&key);
                self.missing_confirmations.remove(&key);
                released += 1;
            }
        }
        self.counters.reconciled_release += released as u64;
        ReconciliationReport {
            checked: pressed_before,
            released,
            still_pressed: self.pressed.len(),
            pending_confirmations: self.missing_confirmations.len(),
        }
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

    #[test]
    fn scheduler_runs_immediately_then_only_after_interval() {
        let policy = ReconciliationPolicy::new(250, 2).unwrap();
        let mut scheduler = ReconciliationScheduler::new(policy);
        assert_eq!(scheduler.poll(1_000).unwrap(), ReconciliationSchedule::Due);
        assert_eq!(
            scheduler.poll(1_249).unwrap(),
            ReconciliationSchedule::NotDue { remaining_ms: 1 }
        );
        assert_eq!(scheduler.poll(1_250).unwrap(), ReconciliationSchedule::Due);
        assert_eq!(scheduler.last_run_ms(), Some(1_250));
    }

    #[test]
    fn reconciliation_policy_rejects_zero_interval_or_confirmation_threshold() {
        assert_eq!(
            ReconciliationPolicy::new(0, 2),
            Err(ReconciliationPolicyError::ZeroInterval)
        );
        assert_eq!(
            ReconciliationPolicy::new(250, 0),
            Err(ReconciliationPolicyError::ZeroConfirmations)
        );
    }

    #[test]
    fn scheduler_rejects_clock_rollback_without_moving_last_run() {
        let mut scheduler = ReconciliationScheduler::new(ReconciliationPolicy::default());
        scheduler.poll(500).unwrap();
        assert_eq!(
            scheduler.poll(499),
            Err(ReconciliationClockError::NonMonotonic {
                previous_ms: 500,
                received_ms: 499,
            })
        );
        assert_eq!(scheduler.last_run_ms(), Some(500));
    }

    #[test]
    fn transient_missing_snapshot_does_not_release_pressed_key() {
        let policy = ReconciliationPolicy::new(100, 2).unwrap();
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(A));
        let empty = snapshot([]);
        let first = state.reconcile_with_policy(&empty, policy);
        assert_eq!(first.released, 0);
        assert_eq!(first.pending_confirmations, 1);
        assert!(state.is_pressed(A));
        let held = snapshot([A]);
        let confirmed = state.reconcile_with_policy(&held, policy);
        assert_eq!(confirmed.pending_confirmations, 0);
        assert!(state.is_pressed(A));
    }

    #[test]
    fn repeated_missing_snapshots_release_key_and_count_reconciliation() {
        let policy = ReconciliationPolicy::default();
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(A));
        let empty = snapshot([]);
        assert_eq!(state.reconcile_with_policy(&empty, policy).released, 0);
        let report = state.reconcile_with_policy(&empty, policy);
        assert_eq!(report.released, 1);
        assert!(state.pressed().is_empty());
        assert_eq!(state.counters().reconciled_release, 1);
    }

    #[test]
    fn reset_discards_pending_false_positive_confirmations() {
        let mut state = PressedState::default();
        state.apply(InputEvent::Down(A));
        state.reconcile_with_policy(&snapshot([]), ReconciliationPolicy::default());
        state.apply(InputEvent::Reset);
        state.apply(InputEvent::Down(A));
        let report = state.reconcile_with_policy(&snapshot([]), ReconciliationPolicy::default());
        assert_eq!(report.released, 0);
        assert!(state.is_pressed(A));
    }
}
