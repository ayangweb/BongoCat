//! What a key press is, and which of them are held.
//!
//! A press is a usage code and a side, because the product draws a different
//! image for the left and the right of some keys and the same for others. The set
//! is bounded and deduplicated: a device that reports the same key twice is a
//! device that would otherwise make the key look stuck, and an unbounded set is a
//! device that could grow memory without limit.

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KeySide {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyPress {
    pub key: KeyIdentity,
    pub side: KeySide,
}

/// What a press is: a keyboard key or a gamepad button.
///
/// The two input families are drawn by the same overlay layer and live in the
/// same two per-hand image directories, but they are not the same kind of
/// control and must not be folded into one numeric space. A gamepad button has
/// no HID Keyboard/Keypad usage, so before this type existed a gamepad press
/// could not be expressed at all and the whole family reached the renderer as a
/// paw movement with no key image. The identity is therefore tagged and typed:
/// the renderer resolves a name from it, and there is no encoding in which one
/// family's identity can be mistaken for the other's.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KeyIdentity {
    /// A keyboard key, addressed by its HID usage. The Apple Fn / globe key
    /// folds Apple's vendor page into the same `u16`; see [`GLOBE_KEY_USAGE`].
    Keyboard(u16),
    /// A gamepad button, in the product's own sixteen-button vocabulary.
    Gamepad(GamepadButton),
}

impl Default for KeyIdentity {
    /// The zeroed keyboard usage, which is not a key any adapter reports. Only
    /// the empty slots of a [`KeyPressSet`] ever hold it.
    fn default() -> Self {
        Self::Keyboard(0)
    }
}

impl KeyPress {
    pub const fn keyboard(hid_usage: u16, side: KeySide) -> Self {
        Self {
            key: KeyIdentity::Keyboard(hid_usage),
            side,
        }
    }

    pub const fn gamepad(button: GamepadButton, side: KeySide) -> Self {
        Self {
            key: KeyIdentity::Gamepad(button),
            side,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyPressSet {
    pub(crate) entries: [KeyPress; 64],
    pub(crate) len: u8,
}

impl Default for KeyPressSet {
    fn default() -> Self {
        Self {
            entries: [KeyPress::default(); 64],
            len: 0,
        }
    }
}

impl KeyPressSet {
    pub fn push(&mut self, press: KeyPress) {
        if self.entries[..usize::from(self.len)].contains(&press) {
            return;
        }
        if usize::from(self.len) < self.entries.len() {
            self.entries[usize::from(self.len)] = press;
            self.len += 1;
        }
    }

    pub fn iter(self) -> impl Iterator<Item = KeyPress> {
        self.entries.into_iter().take(usize::from(self.len))
    }
}

/// HID Keyboard/Keypad (page 0x07) function keys, i.e. F1 … F24.
///
/// The set is two ranges rather than one because the HID page puts PrintScreen
/// (`0x46`) through PageDown (`0x4e`), the four arrows, and the whole keypad
/// block between F12 (`0x45`) and F13 (`0x68`). Treating the family as
/// contiguous — or as an open-ended "F followed by a number" — would name
/// PrintScreen `F13` and every keypad key after it.
pub const FUNCTION_KEY_USAGES: [(u16, u16); 2] = [(0x3a, 0x45), (0x68, 0x73)];

/// Names of [`FUNCTION_KEY_USAGES`] in HID order: `F1` … `F12`, `F13` … `F24`.
///
/// These are model asset ids, so they must match what the loader derives from a
/// key image's file stem (`F13.png` → `F13`).
pub const FUNCTION_KEY_NAMES: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

/// Position of `hid_usage` inside [`FUNCTION_KEY_NAMES`], `None` for every
/// other key.
///
/// Derived from [`FUNCTION_KEY_USAGES`] so the key-image resolver and the
/// runtime's hand assignment cannot disagree about what a function key is.
pub const fn function_key_index(hid_usage: u16) -> Option<usize> {
    let mut index = 0;
    let mut range = 0;
    while range < FUNCTION_KEY_USAGES.len() {
        let (first, last) = FUNCTION_KEY_USAGES[range];
        if hid_usage >= first && hid_usage <= last {
            return Some(index + (hid_usage - first) as usize);
        }
        index += (last - first + 1) as usize;
        range += 1;
    }
    None
}

/// The model asset name a function key is drawn with, e.g. `F13`.
pub const fn function_key_name(hid_usage: u16) -> Option<&'static str> {
    match function_key_index(hid_usage) {
        Some(index) => Some(FUNCTION_KEY_NAMES[index]),
        None => None,
    }
}
