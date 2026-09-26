//! The effects are periodic, deterministic, and applied last.

use super::*;

#[test]
fn automatic_effects_are_periodic_and_deterministic() {
    let start = automatic_effect_values(Duration::ZERO);
    let full_cycle = automatic_effect_values(BLINK_PERIOD);
    assert_eq!(start.0, Duration::ZERO);
    assert_eq!(full_cycle.0, BLINK_PERIOD);
    assert_eq!(start.1, -1.0);
    assert_eq!(automatic_effect_values(BLINK_CLOSED_DURATION).1, 0.0);
    assert_eq!(full_cycle.1, -1.0);
}

#[test]
fn automatic_effects_keep_blink_in_closed_or_open_contract() {
    for millis in (0..=BLINK_PERIOD.as_millis()).step_by(37) {
        let (_, blink) = automatic_effect_values(Duration::from_millis(
            u64::try_from(millis).expect("duration fits u64"),
        ));
        assert!(blink == -1.0 || blink == 0.0);
    }
}
