//! A sequence wraps, and stays monotonic while it does.

use super::*;

#[test]
fn sequence_ordering_handles_audio_sequence_wraparound() {
    assert!(sequence_reached(0, u64::MAX));
    assert!(sequence_reached(6, 5));
    assert!(!sequence_reached(5, 6));
    assert!(!sequence_reached(u64::MAX - 1, 1));
}
