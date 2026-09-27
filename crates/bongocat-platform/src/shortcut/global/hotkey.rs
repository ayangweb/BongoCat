//! A configured chord, as the keycode the platform library wants.
//!
//! The mapping is a table rather than a parse, and a key name the table does not
//! carry is refused rather than guessed: a chord that silently registered as
//! the wrong key would be a shortcut that fires when the user presses something
//! else, which is worse than one that does not work.

use super::*;

pub(crate) fn desired_registrations(
    compiled: &CompiledShortcuts,
) -> (Vec<Registration>, Vec<String>) {
    let mut registrations = Vec::new();
    let mut unsupported = Vec::new();
    for shortcut in compiled.iter() {
        match shortcut_hotkey(shortcut.chord()) {
            Ok(hotkey) => registrations.push(Registration {
                hotkey,
                target: shortcut.target().clone(),
            }),
            Err(error) => unsupported.push(format!("{}: {error}", shortcut.chord().canonical())),
        }
    }
    (registrations, unsupported)
}

/// Maps a validated configuration chord onto a `global-hotkey` hotkey.
/// Modifier aliases and key tokens were already normalized by
/// `ShortcutChord::parse`; the canonical token set is a closed vocabulary
/// (single letters, digits and the named keys of `NAMED_SHORTCUT_KEYS`).
pub(crate) fn shortcut_hotkey(chord: &ShortcutChord) -> Result<HotKey, ShortcutHotkeyError> {
    let bits = chord.modifiers().bits();
    let mut modifiers = Modifiers::empty();
    if bits & ShortcutModifiers::CONTROL != 0 {
        modifiers |= Modifiers::CONTROL;
    }
    if bits & ShortcutModifiers::ALT != 0 {
        modifiers |= Modifiers::ALT;
    }
    if bits & ShortcutModifiers::SHIFT != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if bits & ShortcutModifiers::META != 0 {
        modifiers |= Modifiers::META;
    }
    let key = shortcut_code(chord.key())?;
    Ok(HotKey::new(Some(modifiers), key))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, thiserror::Error)]
#[error("the key has no global hotkey mapping")]
pub struct ShortcutHotkeyError;

pub(crate) fn shortcut_code(key: &str) -> Result<Code, ShortcutHotkeyError> {
    let bytes = key.as_bytes();
    if bytes.len() == 1 {
        let byte = bytes[0];
        if byte.is_ascii_uppercase() {
            return code_from_name(&format!("Key{}", byte as char));
        }
        if byte.is_ascii_digit() {
            return code_from_name(&format!("Digit{}", byte as char));
        }
    }
    let named = match key {
        "-" => "Minus",
        "=" => "Equal",
        other => other,
    };
    code_from_name(named)
}

pub(crate) fn code_from_name(name: &str) -> Result<Code, ShortcutHotkeyError> {
    name.parse::<Code>().map_err(|_| ShortcutHotkeyError)
}
