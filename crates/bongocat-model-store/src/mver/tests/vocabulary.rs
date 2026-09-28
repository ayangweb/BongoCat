//! A control code becomes a name the runtime can look up.

use super::*;

#[test]
fn keys_this_product_cannot_draw_are_left_out_instead_of_misnamed() {
    let root = tempdir().expect("root");
    // `1` is the left mouse button and `0x1234` is not a key at all; the
    // legacy table allows both, and neither has a BongoCat overlay.
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[1],[4660],[65]],"keyboard":[[1],[4660],[65]]}"#,
        )],
        true,
    );
    for index in 0..3 {
        write(
            root.path(),
            &format!("img/standard/keyboard/{index}.png"),
            &flat([0, 0, 255, 255]),
        );
        write(
            root.path(),
            &format!("img/standard/hand/{index}.png"),
            &flat([255, 0, 0, 255]),
        );
    }

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(mode.slots.len(), 1, "only KeyA has an overlay");
    assert_eq!(mode.slots[0].reference, "left-keys/KeyA.png");
}

#[test]
fn legacy_control_codes_cover_both_name_spaces() {
    use MverInputMode::{Gamepad, Keyboard, Standard};
    for (mode, code, expected) in [
        (Standard, 0x08, "Backspace"),
        (Standard, 0x10, "Shift"),
        (Standard, 0x25, "LeftArrow"),
        (Standard, 0x41, "KeyA"),
        (Standard, 0x5A, "KeyZ"),
        (Standard, 0x30, "Num0"),
        (Standard, 0x5B, "MetaLeft"),
        (Standard, 0x70, "F1"),
        (Standard, 0x7B, "F12"),
        (Standard, 0x7C, "F13"),
        (Standard, 0x87, "F24"),
        (Standard, 0xC0, "BackQuote"),
        (Standard, 0xBF, "Slash"),
        (Standard, 0xDC, "BackSlash"),
        (Keyboard, 0x52, "KeyR"),
        // The side-independent Alt code keeps the legacy table's own name:
        // `legacy_key_names` is what turns it into the product's two sides.
        (Standard, 0x12, "Alt"),
        (Standard, 0xA4, "AltLeft"),
        (Keyboard, 0xA5, "AltRight"),
        // The shared Enter code keeps the legacy table's own name, updated
        // to the product spelling: `legacy_key_names` is what turns it into
        // the product's two Enter keys.
        (Standard, 0x0D, "Enter"),
        // The gamepad mode addresses buttons, not virtual keys: index 13 is
        // the D-pad down button, while virtual key 0x0D is the Enter keys.
        // The whole XInput order is walked separately, so a handful of
        // spot checks is all this table needs.
        (Gamepad, 13, "DpadDown"),
        (Gamepad, 14, "DpadLeft"),
        (Gamepad, 4, "LeftShoulder"),
        (Gamepad, 6, "LeftTrigger"),
        (Gamepad, 0, "South"),
        (Gamepad, 9, "Start"),
    ] {
        assert_eq!(
            legacy_key_name(mode, code),
            Some(expected),
            "{mode:?} control code {code:#x}"
        );
    }
    assert_eq!(
        legacy_key_name(Standard, 1),
        None,
        "mouse buttons have no overlay"
    );
    assert_eq!(legacy_key_name(Standard, 0x1234), None);
    assert_eq!(legacy_key_name(Gamepad, 16), None);
}

/// Every function key has its own converted name, and the globe key has none.
///
/// `VK_F13` … `VK_F24` are `124` … `135` — API-defined constants of the
/// `windows` crate's `KeyboardAndMouse` module, not a guessed hardware table
/// — and the conversion maps each to its own image, so an F13 binding draws
/// `F13.png` and never the shared `Fn.png`. The reference converter's picker
/// stopped at F12 (`BongoCat-Converter/src/utils/keyMap.ts` numbers 112 …
/// 123), so no model authored with it carries these codes; a hand-written
/// key table can. The globe key has no Windows virtual key at all — the Fn
/// key is handled by the keyboard firmware — so the conversion can never
/// emit `Globe.png` or its pre-rename spelling.
#[test]
fn every_function_key_has_its_own_converted_name() {
    let converted: Vec<&str> = (0x70..=0x87).filter_map(legacy_virtual_key_name).collect();
    let expected: Vec<String> = (1..=24).map(|index| format!("F{index}")).collect();
    assert_eq!(
        converted, expected,
        "F1 … F24 are contiguous in the legacy virtual-key space"
    );

    let names = legacy_keyboard_key_image_names();
    for absent in ["Fn", "Globe", "Function"] {
        assert!(
            !names.contains(&absent),
            "{absent} is not a conversion output: {names:?}"
        );
    }
}

/// The conversion emits the product's spelling for every key image, so the
/// image it installs is the one the runtime looks up.
///
/// This is the conversion's half of the contract; `bongocat-live2d` owns the
/// other half (it can see both tables and asserts every name here resolves to
/// a key). `Backslash` had drifted from the product's `BackSlash` for exactly
/// this reason, and a converted backslash image was unreachable from the day
/// the feature shipped (ADR-0050).
#[test]
fn the_conversion_emits_the_product_spelling_for_every_key_image() {
    let names = legacy_keyboard_key_image_names();
    for (present, absent) in [
        ("BackSlash", "Backslash"),
        ("Backspace", "BackSpace"),
        ("Enter", "Return"),
        ("AltLeft", "Alt"),
        ("AltRight", "Alt"),
    ] {
        assert!(names.contains(&present), "{present} missing: {names:?}");
        assert!(!names.contains(&absent), "{absent} emitted: {names:?}");
    }
    // The two keys the legacy chart gives to a whole family keep their family
    // name: the runtime resolves `Shift`/`Control` for both sides (ADR-0038
    // decision 4).
    assert!(names.contains(&"Shift"));
    assert!(names.contains(&"Control"));
    assert!(names.contains(&"KpEnter"));
    assert!(names.contains(&"KeyA"));
    // No name is a path or contains whitespace: they are resource stems.
    for name in &names {
        assert!(
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()),
            "{name:?} is not a resource stem"
        );
    }
}

/// Left and right Alt may never collapse into one name again. The legacy
/// chart numbers both Alt keys `18`, so that one code has to reach both
/// sides; a table that does name a side keeps it. The two Enter keys are
/// ambiguous in the same way (`13` for both) and expand to their own two
/// product names.
#[test]
fn alt_control_codes_never_collapse_left_and_right() {
    use MverInputMode::{Gamepad, Keyboard, Standard};
    assert_eq!(
        legacy_key_names(Standard, 0x12),
        vec!["AltLeft", "AltRight"],
        "the shared VK_MENU code must reach both Alt keys"
    );
    assert_eq!(
        legacy_key_names(Keyboard, 0x12),
        vec!["AltLeft", "AltRight"]
    );
    assert_eq!(legacy_key_names(Standard, 0xA4), vec!["AltLeft"]);
    assert_eq!(legacy_key_names(Standard, 0xA5), vec!["AltRight"]);
    // The shared Enter code reaches both Enter keys, each under its own
    // product name; the runtime resolves whichever key was pressed.
    assert_eq!(
        legacy_key_names(Standard, 0x0D),
        vec!["Enter", "KpEnter"],
        "the shared VK_RETURN code must reach both Enter keys"
    );
    assert_eq!(legacy_key_names(Keyboard, 0x0D), vec!["Enter", "KpEnter"]);
    // Every other code still names exactly one image.
    assert_eq!(legacy_key_names(Standard, 0x41), vec!["KeyA"]);
    assert_eq!(legacy_key_names(Gamepad, 14), vec!["DpadLeft"]);
    assert!(legacy_key_names(Standard, 1).is_empty());
    assert!(
        legacy_key_names(Gamepad, 0x12).is_empty(),
        "gamepad control codes are button indexes, not virtual keys"
    );
    assert_eq!(
        legacy_key_names(Gamepad, 0x0D),
        vec!["DpadDown"],
        "gamepad control code 13 is the D-pad down button, not a virtual key"
    );
}

/// The whole XInput button order, walked index by index.
///
/// This is the table the reported bug turned on. Six of the sixteen entries
/// used to name the wrong button — the two menu buttons and the two stick
/// clicks were read off the D-pad and the stick rows, and the two D-pad
/// horizontal directions were read off the menu row — so a converted gamepad
/// model installed the right artwork under a name no button press resolves.
/// The expected column is the physical layout every XInput device reports,
/// and the names are the product's own (`GamepadButton::key_image_name`).
#[test]
fn every_xinput_button_index_names_the_button_that_index_reports() {
    use MverInputMode::Gamepad;
    let expected = [
        "South",
        "East",
        "West",
        "North",
        "LeftShoulder",
        "RightShoulder",
        "LeftTrigger",
        "RightTrigger",
        "Select",
        "Start",
        "LeftStick",
        "RightStick",
        "DpadUp",
        "DpadDown",
        "DpadLeft",
        "DpadRight",
    ];
    assert_eq!(
        legacy_gamepad_key_image_names(),
        expected,
        "XInput button order: face, shoulders, triggers, menu, sticks, D-pad"
    );
    for (index, name) in expected.iter().enumerate() {
        assert_eq!(
            legacy_key_name(Gamepad, index as i64),
            Some(*name),
            "XInput button {index}"
        );
    }
    // And every name is a real product button name, so the conversion can
    // only emit an image the renderer is able to resolve.
    let product = bongocat_render::GamepadButton::ALL
        .iter()
        .map(|button| button.key_image_name())
        .collect::<BTreeSet<_>>();
    for name in legacy_gamepad_key_image_names() {
        assert!(product.contains(name), "{name} is not a button name");
    }
}
