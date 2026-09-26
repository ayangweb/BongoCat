//! What one legacy control code is called in the product's own vocabulary.
//!
//! The two pointer modes address keys with a Windows virtual key and the gamepad
//! mode with an XInput button index, so the same number means different things
//! per mode. Translating here is what makes a converted model reachable by the
//! runtime instead of being a parallel naming scheme nothing looks up.

use super::*;

/// The BongoCat key-image name for one legacy control code.
///
/// The two pointer modes address keys with a Windows virtual key, and the
/// gamepad mode addresses buttons with an XInput button index. Both are
/// translated here into the vocabulary the product already uses: the names a
/// keyboard key resolves to are exactly the ones `bongocat-live2d-render`
/// derives from its HID usage, and the gamepad names are exactly the ones
/// `GamepadButton::key_image_name` defines, so a converted model is reachable by
/// the runtime instead of being a parallel naming scheme nothing looks up.
///
/// `0x08` is the one name the legacy key table spells differently (`BackSpace`);
/// the product's runtime and its shipped preset models both use `Backspace`, so
/// the product spelling wins. `0x12` keeps the legacy table's own `Alt` name —
/// it is the side-independent code, and [`legacy_key_names`] is what turns it
/// into the names the product actually looks up.
///
/// A code with no name is not an error: it is a control this product has no
/// overlay for (a mouse button, or a key outside the resolved set), and the
/// conversion installs no image for it.
pub(crate) const fn legacy_key_name(
    mode: MverInputMode,
    control_code: i64,
) -> Option<&'static str> {
    match mode {
        MverInputMode::Standard | MverInputMode::Keyboard => legacy_virtual_key_name(control_code),
        MverInputMode::Gamepad => legacy_gamepad_button_name(control_code),
    }
}

/// `VK_MENU`: the legacy keyboard chart's `18`, worn by both Alt keys.
pub(crate) const LEGACY_VK_MENU: i64 = 0x12;

/// `VK_RETURN`: the legacy keyboard chart's `13`, worn by both Enter keys.
pub(crate) const LEGACY_VK_RETURN: i64 = 0x0D;

/// The BongoCat key-image names one legacy control code addresses.
///
/// Almost every code names exactly one image, and callers must treat every name
/// it does produce as an equal destination for the same composed overlay.
///
/// `VK_MENU` (`0x12`) is the one code the legacy key table gives to two physical
/// keys: its keyboard chart numbers *both* Alt keys `18`, and the application
/// reads the code with `GetKeyState`, which reports either key. The code itself
/// therefore never says which side was pressed, and the conversion installs that
/// one overlay for `AltLeft` and `AltRight` rather than collapsing the two keys
/// back into a single shared `Alt` name. A hand-written key table that does name
/// a side is honoured: `VK_LMENU` (`0xA4`) and `VK_RMENU` (`0xA5`) resolve to
/// the exact side, exactly as `bongocat-platform` maps the same two codes for
/// the live Windows input path.
///
/// The other modifier codes (`0x10` Shift, `0x11` Control) are ambiguous in the
/// same way and keep their shared family name, which the runtime resolves for
/// both sides; widening this expansion to them is a separate change because it
/// would move an output the real legacy sample's conversion already records.
///
/// `VK_RETURN` (`0x0D`) is ambiguous in the same family way: the application
/// reads it with `GetKeyState`, which reports the main Enter key and the keypad
/// Enter key alike, and the legacy table numbers both `13`. The product names
/// the two keys distinctly (`Enter` and `KpEnter`), so the conversion installs
/// the one overlay for both names — the runtime then draws it for whichever key
/// was actually pressed.
pub(crate) fn legacy_key_names(mode: MverInputMode, control_code: i64) -> Vec<&'static str> {
    if mode != MverInputMode::Gamepad {
        if control_code == LEGACY_VK_MENU {
            return vec!["AltLeft", "AltRight"];
        }
        if control_code == LEGACY_VK_RETURN {
            return vec!["Enter", "KpEnter"];
        }
    }
    legacy_key_name(mode, control_code).into_iter().collect()
}

/// Every key-image name the conversion can install, for both keyboard modes.
///
/// The conversion writes the product's own key vocabulary (ADR-0037 §7), so this
/// is the set `bongocat-live2d::key_name_candidates` has to be able to resolve:
/// a name in this list that no candidate list produces is an image the
/// conversion installs and the runtime can never draw. Making the set
/// traversable is what lets a contract test catch that instead of a user —
/// `Backslash` had drifted from the product's `BackSlash` for exactly that
/// reason (ADR-0050).
///
/// Gamepad button names are absent from this list only because it answers the
/// *keyboard* modes; the gamepad mode's own sixteen names are published by
/// [`legacy_gamepad_key_image_names`] and are checked against
/// `GamepadButton::key_image_name` the same way. The globe key is absent for a
/// different reason: the legacy code space has no code for it at all, so the
/// conversion can never emit `Globe.png`.
pub fn legacy_keyboard_key_image_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    for mode in [MverInputMode::Standard, MverInputMode::Keyboard] {
        // A Windows virtual key is a `WORD`; the conversion reads one as `i64`
        // and every code outside that range resolves to `None`.
        for control_code in 0..=0xFF {
            names.extend(legacy_key_names(mode, control_code));
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Every key-image name the gamepad conversion can install, in XInput button
/// index order.
///
/// Traversed for the same reason as [`legacy_keyboard_key_image_names`]: a name
/// the conversion writes that the resolver has no vocabulary for is an image the
/// conversion installs and the runtime can never draw. The check compares these
/// against `GamepadButton::key_image_name`, so the converter cannot drift away
/// from the button vocabulary again.
pub fn legacy_gamepad_key_image_names() -> Vec<&'static str> {
    (0..16).filter_map(legacy_gamepad_button_name).collect()
}

pub(crate) const fn legacy_virtual_key_name(virtual_key: i64) -> Option<&'static str> {
    match virtual_key {
        0x08 => Some("Backspace"),
        0x09 => Some("Tab"),
        0x0D => Some("Enter"),
        0x10 => Some("Shift"),
        0x11 => Some("Control"),
        0x12 => Some("Alt"),
        0x13 => Some("Pause"),
        0x14 => Some("CapsLock"),
        0x1B => Some("Escape"),
        0x20 => Some("Space"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x23 => Some("End"),
        0x24 => Some("Home"),
        0x25 => Some("LeftArrow"),
        0x26 => Some("UpArrow"),
        0x27 => Some("RightArrow"),
        0x28 => Some("DownArrow"),
        0x2C => Some("PrintScreen"),
        0x2D => Some("Insert"),
        0x2E => Some("Delete"),
        0x30 => Some("Num0"),
        0x31 => Some("Num1"),
        0x32 => Some("Num2"),
        0x33 => Some("Num3"),
        0x34 => Some("Num4"),
        0x35 => Some("Num5"),
        0x36 => Some("Num6"),
        0x37 => Some("Num7"),
        0x38 => Some("Num8"),
        0x39 => Some("Num9"),
        0x41 => Some("KeyA"),
        0x42 => Some("KeyB"),
        0x43 => Some("KeyC"),
        0x44 => Some("KeyD"),
        0x45 => Some("KeyE"),
        0x46 => Some("KeyF"),
        0x47 => Some("KeyG"),
        0x48 => Some("KeyH"),
        0x49 => Some("KeyI"),
        0x4A => Some("KeyJ"),
        0x4B => Some("KeyK"),
        0x4C => Some("KeyL"),
        0x4D => Some("KeyM"),
        0x4E => Some("KeyN"),
        0x4F => Some("KeyO"),
        0x50 => Some("KeyP"),
        0x51 => Some("KeyQ"),
        0x52 => Some("KeyR"),
        0x53 => Some("KeyS"),
        0x54 => Some("KeyT"),
        0x55 => Some("KeyU"),
        0x56 => Some("KeyV"),
        0x57 => Some("KeyW"),
        0x58 => Some("KeyX"),
        0x59 => Some("KeyY"),
        0x5A => Some("KeyZ"),
        0x5B => Some("MetaLeft"),
        0x5C => Some("MetaRight"),
        0x5D => Some("Apps"),
        0x60 => Some("Kp0"),
        0x61 => Some("Kp1"),
        0x62 => Some("Kp2"),
        0x63 => Some("Kp3"),
        0x64 => Some("Kp4"),
        0x65 => Some("Kp5"),
        0x66 => Some("Kp6"),
        0x67 => Some("Kp7"),
        0x68 => Some("Kp8"),
        0x69 => Some("Kp9"),
        0x6A => Some("KpMultiply"),
        0x6B => Some("KpPlus"),
        0x6D => Some("KpMinus"),
        0x6E => Some("KpDecimal"),
        0x6F => Some("KpDivide"),
        0x70 => Some("F1"),
        0x71 => Some("F2"),
        0x72 => Some("F3"),
        0x73 => Some("F4"),
        0x74 => Some("F5"),
        0x75 => Some("F6"),
        0x76 => Some("F7"),
        0x77 => Some("F8"),
        0x78 => Some("F9"),
        0x79 => Some("F10"),
        0x7A => Some("F11"),
        0x7B => Some("F12"),
        // `VK_F13` … `VK_F24`. The reference converter's own picker stopped at
        // F12 (`BongoCat-Converter/src/utils/keyMap.ts` numbers 112 … 123), so
        // no model authored with it can carry these codes — but the range is
        // API-defined and contiguous (`windows`'s KeyboardAndMouse module
        // declares 124 … 135), and a hand-written key table can address it.
        // Each code maps to its own image, the same way F1 … F12 do, so an
        // F13 binding draws `F13.png` and never the shared `Fn.png`.
        0x7C => Some("F13"),
        0x7D => Some("F14"),
        0x7E => Some("F15"),
        0x7F => Some("F16"),
        0x80 => Some("F17"),
        0x81 => Some("F18"),
        0x82 => Some("F19"),
        0x83 => Some("F20"),
        0x84 => Some("F21"),
        0x85 => Some("F22"),
        0x86 => Some("F23"),
        0x87 => Some("F24"),
        0x90 => Some("NumLock"),
        0x91 => Some("ScrollLock"),
        // `VK_LMENU` / `VK_RMENU`: the side-specific Alt codes a hand-written
        // key table can name instead of the shared `VK_MENU`.
        0xA4 => Some("AltLeft"),
        0xA5 => Some("AltRight"),
        0xBA => Some("SemiColon"),
        0xBB => Some("Equal"),
        0xBC => Some("Comma"),
        0xBD => Some("Minus"),
        0xBE => Some("Dot"),
        0xBF => Some("Slash"),
        0xC0 => Some("BackQuote"),
        0xDB => Some("LeftBracket"),
        // The legacy chart spells this key `Backslash`; the product's runtime
        // and its shipped presets spell it `BackSlash`. The product spelling
        // wins, the same way `Backspace` beats the chart's `BackSpace` above,
        // so the image this installs is the one the resolver looks up.
        0xDC => Some("BackSlash"),
        0xDD => Some("RightBracket"),
        0xDE => Some("Quote"),
        _ => None,
    }
}

/// The BongoCat key-image name for one legacy gamepad button index.
///
/// The legacy gamepad section addresses buttons with a Windows XInput button
/// index — the standard ordering every XInput device reports: face buttons
/// first, then the two shoulders, the two analog triggers, the two menu buttons,
/// the two stick clicks, then the D-pad in up/down/left/right order. The names
/// are the product's own button names (`GamepadButton::key_image_name`), which
/// the runtime resolves for a press of that button.
///
/// The two pairs this table used to get wrong are the reason it is written out
/// rather than derived: XInput `8`/`9` are the menu buttons and `10`/`11` are
/// the stick clicks — not the sticks and not the D-pad — and `14`/`15` are
/// D-pad left/right — not the menu buttons. The previous table read them from
/// the third-party backend's own control names, which is where a shoulder's
/// `LeftTrigger` and an analog trigger's `LeftTrigger2` came from, and a
/// converted gamepad model therefore installed the right artwork under the wrong
/// button's name.
pub(crate) const fn legacy_gamepad_button_name(button: i64) -> Option<&'static str> {
    match button {
        0 => Some("South"),
        1 => Some("East"),
        2 => Some("West"),
        3 => Some("North"),
        4 => Some("LeftShoulder"),
        5 => Some("RightShoulder"),
        6 => Some("LeftTrigger"),
        7 => Some("RightTrigger"),
        8 => Some("Select"),
        9 => Some("Start"),
        10 => Some("LeftStick"),
        11 => Some("RightStick"),
        12 => Some("DpadUp"),
        13 => Some("DpadDown"),
        14 => Some("DpadLeft"),
        15 => Some("DpadRight"),
        _ => None,
    }
}
