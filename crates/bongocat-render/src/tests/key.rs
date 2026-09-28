//! A press is a usage and a side, and the set of them is bounded.

use super::*;

#[test]
fn function_key_table_covers_f1_through_f24_and_stops_at_the_gaps() {
    let mut counted = 0;
    for (first, last) in FUNCTION_KEY_USAGES {
        assert!(first <= last);
        counted += usize::from(last - first + 1);
    }
    assert_eq!(
        counted,
        FUNCTION_KEY_NAMES.len(),
        "every usage in the table needs exactly one name"
    );

    assert_eq!(function_key_name(0x3a), Some("F1"));
    assert_eq!(function_key_name(0x45), Some("F12"));
    assert_eq!(function_key_name(0x68), Some("F13"));
    assert_eq!(function_key_name(0x73), Some("F24"));
    assert_eq!(function_key_index(0x73), Some(23));

    // The HID page puts PrintScreen…ArrowUp, the keypad block and Execute
    // between the two ranges, so none of them is a function key.
    for hid_usage in [0x46, 0x48, 0x4c, 0x52, 0x62, 0x67, 0x74, 0x7d] {
        assert_eq!(
            function_key_name(hid_usage),
            None,
            "0x{hid_usage:02x} must not be named as a function key"
        );
    }
}

#[test]
fn key_press_set_deduplicates_and_has_bounded_capacity() {
    let mut presses = KeyPressSet::default();
    let press = KeyPress::keyboard(0x04, KeySide::Left);
    presses.push(press);
    presses.push(press);
    for usage in 0x05..=0x50 {
        presses.push(KeyPress::keyboard(usage, KeySide::Left));
    }
    assert_eq!(presses.iter().count(), 64);
    assert_eq!(presses.iter().filter(|entry| *entry == press).count(), 1);
}

/// A gamepad press and a keyboard press are different identities even when
/// they name the same side, and no gamepad button can be folded into the
/// HID usage space: that is what keeps a button from being resolved as a key.
#[test]
fn a_gamepad_press_is_never_the_same_identity_as_a_key_press() {
    for button in GamepadButton::ALL {
        let press = KeyPress::gamepad(button, KeySide::Left);
        assert_eq!(press.key, KeyIdentity::Gamepad(button));
        assert_ne!(press, KeyPress::keyboard(0x04, KeySide::Left));
    }
    assert_eq!(KeyPress::default().key, KeyIdentity::Keyboard(0));
}
