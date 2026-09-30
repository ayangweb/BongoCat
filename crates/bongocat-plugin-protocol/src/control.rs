//! What a control's name looks like to a person.
//!
//! The wire carries the name the model window's key artwork already uses — `KeyA`,
//! `LeftShift`, `KpEnter` — because that vocabulary is a closed contract with model
//! authors and it is the one place it exists. It is not, however, what is written on a
//! keycap, and a plugin showing a key to somebody cannot invent that spelling without a
//! table of a hundred names that would then have to be kept in step with the artwork's.
//!
//! So the mapping is here, in the protocol, next to the names it maps from. A plugin that
//! wants a different spelling formats [`InputEvent`](crate::InputEvent)'s `control`
//! itself; a plugin that wants the obvious one calls [`control_label`] and gets the same
//! answer every other plugin gets.
//!
//! # The rules, in order
//!
//! 1. A name in the table is that table's entry. The table is where the awkward ones go,
//!    and "awkward" means: two names for one keycap (`LeftShift` and `ShiftLeft` are the
//!    same key to a person), a name that is a category rather than a key (`Modifiers`),
//!    a name that is longer than a keycap (`PageDown`), and the numeric keypad's habit of
//!    prefixing everything with `Kp`.
//! 2. `Key<one character>` is that character, so `KeyA` is `A` and `Digit7` is `7`. A key
//!    whose name starts `Key` and is longer is *not* abbreviated: `Keyboard` is not `K`.
//! 3. Anything else is its own name, with nothing guessed.
//!
//! Rule 3 is the important one. A name this table has never heard of is shown as it
//! arrived, because a plugin showing `XF86AudioRaiseVolume` is showing a real key, and a
//! plugin showing `Key?` because the table did not have an entry is showing a bug.

/// A key's name as a person reads it, or the name itself when the table has no entry.
///
/// Never allocates, and never fails: a name that cannot be shortened is a name that can be
/// shown as it is.
pub fn control_label(name: &str) -> &str {
    if let Some(known) = known(name) {
        return known;
    }
    if let Some(character) = one_character(name) {
        return character;
    }
    name
}

/// The table, which is the whole of the first rule.
fn known(name: &str) -> Option<&'static str> {
    // The keypad digits first, because there are ten of them and they are the one entry
    // that is arithmetic rather than a list — and because `Kp0`..`Kp9` is a shape a table
    // can recognise without a name being written down ten times.
    if let Some(digit) = name
        .strip_prefix("Kp")
        .filter(|rest| rest.len() == 1)
        .and_then(|rest| rest.parse::<u8>().ok())
        && let Some(label) = keypad_label(digit)
    {
        return Some(label);
    }
    Some(match name {
        // Modifiers: two spellings for one keycap, because the artwork has both.
        "LeftShift" | "ShiftLeft" | "RightShift" | "ShiftRight" => "Shift",
        "LeftControl" | "ControlLeft" | "RightControl" | "ControlRight" => "Ctrl",
        "LeftAlt" | "AltLeft" | "RightAlt" | "AltRight" => "Alt",
        "LeftMeta" | "MetaLeft" | "RightMeta" | "MetaRight" => "Super",
        "LeftGUI" | "GuiLeft" | "RightGUI" | "GuiRight" => "Super",
        "Globe" => "Fn",
        "CapsLock" => "Caps",
        "NumLock" => "Num",
        "ScrollLock" => "Scroll",
        "PrintScreen" => "PrtSc",
        // Long names are longer than a keycap.
        "Escape" => "Esc",
        "PageDown" => "PgDn",
        "PageUp" => "PgUp",
        "Backspace" => "Bksp",
        "BackSlash" => "\\",
        "SemiColon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" | "Dot" => ".",
        "Minus" => "-",
        "Equal" => "=",
        "IntlHash" => "#",
        "IntlBackslash" => "|",
        "IntlYen" => "¥",
        "IntlRo" | "IntlKana" => "Kana",
        "IntlConvert" => "Conv",
        "IntlNonConvert" => "Kana",
        "Space" => "Space",
        "Enter" | "Return" => "Enter",
        // Arrows, which have a glyph and are the only keys where one is unambiguous.
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        // The numeric keypad, which spells everything with a prefix.
        "KpEnter" | "NumpadEnter" => "Num Enter",
        "KpPlus" | "NumpadAdd" => "Num +",
        "KpMinus" | "NumpadSubtract" => "Num −",
        "KpAsterisk" | "NumpadMultiply" => "Num *",
        "KpSlash" | "NumpadDivide" => "Num /",
        "KpDot" | "NumpadDecimal" => "Num .",
        "KpEqual" | "NumpadEqual" => "Num =",
        "KpComma" | "NumpadComma" => "Num ,",
        // Media keys, which say what they do.
        "AudioVolumeMute" => "Mute",
        "AudioVolumeDown" => "Vol −",
        "AudioVolumeUp" => "Vol +",
        "AudioPlay" => "Play",
        "AudioPause" => "Pause",
        "AudioStop" => "Stop",
        "AudioNext" => "Next",
        "AudioPrev" => "Prev",
        // A category rather than a key, and the one place a name that is not a key at all
        // reaches a plugin. Shown as itself because a person who is told "Modifiers" has
        // been told the truth.
        "Modifiers" => "Modifiers",
        // Mouse buttons, from the names the protocol puts on the wire.
        "left" => "LMB",
        "right" => "RMB",
        "middle" => "MMB",
        "back" => "Back",
        "forward" => "Fwd",
        _ => return None,
    })
}

/// The single character a name abbreviates to, or `None` when it is not one of those.
///
/// `KeyA` and `Digit7` are the two shapes the HID table produces, and both are a prefix
/// plus exactly one character. `Keyboard` is a prefix plus seven, and a name that is not
/// one of those two shapes is left alone — which is the difference between a table that
/// knows what it is abbreviating and one that guesses.
fn one_character(name: &str) -> Option<&str> {
    // Uppercase for the letter, a digit for the digit — and *not* "any one alphanumeric
    // character". That looser rule turns `Keys` into `s`, which is the guessing this
    // function exists to avoid: the two shapes here are the ones the HID usage table
    // produces for a letter key and a digit key, and nothing else.
    if let Some(letter) = name
        .strip_prefix("Key")
        .filter(|rest| rest.len() == 1)
        .filter(|rest| rest.chars().all(|character| character.is_ascii_uppercase()))
    {
        return Some(letter);
    }
    name.strip_prefix("Digit")
        .filter(|rest| rest.len() == 1)
        .filter(|rest| rest.chars().all(|character| character.is_ascii_digit()))
}

/// The numeric keypad's digits, which are `Kp0`..`Kp9` on the wire and `Num 0`.. on a cap.
pub fn keypad_label(digit: u8) -> Option<&'static str> {
    Some(match digit {
        0 => "Num 0",
        1 => "Num 1",
        2 => "Num 2",
        3 => "Num 3",
        4 => "Num 4",
        5 => "Num 5",
        6 => "Num 6",
        7 => "Num 7",
        8 => "Num 8",
        9 => "Num 9",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_key_is_the_letter_and_a_digit_key_is_the_digit() {
        assert_eq!(control_label("KeyA"), "A");
        assert_eq!(control_label("KeyZ"), "Z");
        assert_eq!(control_label("Digit0"), "0");
        assert_eq!(control_label("Digit9"), "9");
    }

    #[test]
    fn a_name_that_only_starts_with_key_is_not_abbreviated() {
        // The rule that keeps this from being a guess: `Key` plus more than one character
        // is a different key, and `Key?` on a keycap is a bug rather than a label.
        assert_eq!(control_label("Keyboard"), "Keyboard");
        assert_eq!(control_label("Key"), "Key");
        assert_eq!(control_label("Keys"), "Keys");
    }

    #[test]
    fn the_two_spellings_of_a_modifier_are_one_keycap() {
        // The artwork has both, so both arrive, and a person pressing shift has one key.
        for name in ["LeftShift", "ShiftLeft", "RightShift", "ShiftRight"] {
            assert_eq!(control_label(name), "Shift", "{name}");
        }
        for name in ["LeftControl", "ControlLeft", "RightControl", "ControlRight"] {
            assert_eq!(control_label(name), "Ctrl", "{name}");
        }
        for name in ["LeftAlt", "AltLeft", "RightAlt", "AltRight"] {
            assert_eq!(control_label(name), "Alt", "{name}");
        }
        for name in [
            "LeftMeta",
            "MetaLeft",
            "RightMeta",
            "MetaRight",
            "LeftGUI",
            "GuiLeft",
        ] {
            assert_eq!(control_label(name), "Super", "{name}");
        }
    }

    #[test]
    fn a_long_name_is_shortened_to_what_fits_on_a_keycap() {
        assert_eq!(control_label("Escape"), "Esc");
        assert_eq!(control_label("PageDown"), "PgDn");
        assert_eq!(control_label("CapsLock"), "Caps");
        assert_eq!(control_label("PrintScreen"), "PrtSc");
    }

    #[test]
    fn the_keypad_says_it_is_the_keypad() {
        // `KpEnter` shown as `Enter` would be a lie: there are two of them, and the one
        // being pressed matters to whoever is reading the panel.
        assert_eq!(control_label("KpEnter"), "Num Enter");
        assert_eq!(control_label("KpPlus"), "Num +");
        assert_eq!(control_label("KpSlash"), "Num /");
        for digit in 0..=9u8 {
            assert_eq!(
                control_label(&format!("Kp{digit}")),
                keypad_label(digit).expect("a keypad digit"),
                "so a keypad digit and the table agree"
            );
        }
        assert_eq!(keypad_label(10), None);
    }

    #[test]
    fn an_arrow_is_its_glyph() {
        assert_eq!(control_label("ArrowUp"), "↑");
        assert_eq!(control_label("ArrowDown"), "↓");
        assert_eq!(control_label("ArrowLeft"), "←");
        assert_eq!(control_label("ArrowRight"), "→");
    }

    #[test]
    fn a_mouse_button_is_named_as_a_button() {
        assert_eq!(control_label("left"), "LMB");
        assert_eq!(control_label("right"), "RMB");
        assert_eq!(control_label("middle"), "MMB");
    }

    #[test]
    fn a_name_the_table_has_never_seen_is_shown_as_it_arrived() {
        // The rule that matters most: an unknown name is a real key, and shortening it by
        // guesswork would show something that is not on the keyboard.
        assert_eq!(
            control_label("XF86AudioRaiseVolume"),
            "XF86AudioRaiseVolume"
        );
        assert_eq!(control_label("other_7"), "other_7");
        assert_eq!(control_label(""), "");
    }

    #[test]
    fn the_labels_are_all_short_enough_to_be_a_keycap() {
        // Bounded by a panel's own width, and a label longer than a keycap is a label that
        // pushes every other keycap off the row.
        for name in [
            "KeyA",
            "LeftShift",
            "AudioVolumeUp",
            "IntlNonConvert",
            "XF86AudioRaiseVolume",
            "Modifiers",
            "other_7",
        ] {
            assert!(
                control_label(name).chars().count() <= 20,
                "{name} became {:?}",
                control_label(name)
            );
        }
    }
}
