//! What a motion asks the host to do while it plays.
//!
//! A motion curve can cross a threshold, and each crossing is an event: the
//! difference between one and the next is what makes a motion a sequence rather
//! than a pose. The occurrences come back in order, do not repeat for a curve
//! that stays on one side, and are bounded — a curve that oscillates around a
//! threshold must not be able to produce an unbounded number of events.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionUserDataEvent {
    pub local_time: Duration,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionUserDataOccurrence {
    pub cycle: u64,
    pub local_time: Duration,
    pub value: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MotionUserDataEvaluation {
    pub occurrences: Vec<MotionUserDataOccurrence>,
    pub skipped_occurrences: u64,
}
