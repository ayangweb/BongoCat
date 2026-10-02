//! The legacy `config.json`.
//!
//! It is a key table, not a configuration: each mode maps a control code to an
//! index into its hand images, and the keyboard list is split across two
//! sections that continue where the first stopped. Reading it in the legacy's
//! own shape is what lets one mode reuse the other's key caps.

use super::*;

/// The legacy root config, reduced to the sections the conversion consumes.
///
/// Every field is optional and every list defaults to empty: the legacy
/// application writes this file from a global settings object, so a source that
/// only ever ran in one mode may well omit the other sections, and an unknown
/// key must not make an otherwise usable model unreadable.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct LegacyConfig {
    #[serde(default)]
    pub(crate) standard: Option<LegacySection>,
    #[serde(default)]
    pub(crate) keyboard: Option<LegacySection>,
    #[serde(default)]
    pub(crate) gamepad: Option<LegacySection>,
}

/// One mode's section. `standard` binds `hand` to `keyboard`; the other modes
/// split the same pairing into `lefthand` and `righthand`.
///
/// The `keyboard` list is deliberately not part of this type. It repeats the
/// virtual keys of the hand lists, and the legacy application pairs the two by
/// position, so a conversion that walks the hand lists in order reaches the same
/// key caps without a second table. Only the *folder* matters, and only to
/// decide whether the mode draws its key caps as a separate layer at all.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct LegacySection {
    #[serde(default)]
    pub(crate) hand: Vec<Vec<i64>>,
    #[serde(default)]
    pub(crate) lefthand: Vec<Vec<i64>>,
    #[serde(default)]
    pub(crate) righthand: Vec<Vec<i64>>,
}

impl LegacyConfig {
    pub(crate) const fn section(&self, mode: MverInputMode) -> Option<&LegacySection> {
        match mode {
            MverInputMode::Standard => self.standard.as_ref(),
            MverInputMode::Keyboard => self.keyboard.as_ref(),
            MverInputMode::Gamepad => self.gamepad.as_ref(),
        }
    }
}

impl LegacySection {
    /// Expand the section into the output images it asks for.
    ///
    /// The legacy pairing is positional: entry `i` of a hand list belongs with
    /// entry `i` of the keyboard list, and the two split sections of the
    /// keyboard and gamepad modes share one keyboard list, with the right hand
    /// continuing where the left hand stopped. An entry with no control code is
    /// not a binding and is skipped, mirroring how the legacy application reads
    /// the same table.
    pub(crate) fn bindings(&self, mode: MverInputMode) -> Vec<LegacyBinding> {
        let mut bindings = Vec::new();
        match mode {
            MverInputMode::Standard => {
                for (index, entry) in self.hand.iter().enumerate() {
                    if let Some(control_code) = first_control_code(entry) {
                        bindings.push(LegacyBinding {
                            output_directory: OUTPUT_LEFT_KEYS,
                            hand_directory: LEGACY_HAND_DIRECTORY,
                            hand_index: index,
                            keyboard_index: index,
                            control_code,
                        });
                    }
                }
            }
            MverInputMode::Keyboard | MverInputMode::Gamepad => {
                for (index, entry) in self.lefthand.iter().enumerate() {
                    if let Some(control_code) = first_control_code(entry) {
                        bindings.push(LegacyBinding {
                            output_directory: OUTPUT_LEFT_KEYS,
                            hand_directory: LEGACY_LEFT_HAND_DIRECTORY,
                            hand_index: index,
                            keyboard_index: index,
                            control_code,
                        });
                    }
                }
                let right_hand_offset = self.lefthand.len();
                for (index, entry) in self.righthand.iter().enumerate() {
                    if let Some(control_code) = first_control_code(entry) {
                        bindings.push(LegacyBinding {
                            output_directory: OUTPUT_RIGHT_KEYS,
                            hand_directory: LEGACY_RIGHT_HAND_DIRECTORY,
                            hand_index: index,
                            keyboard_index: right_hand_offset + index,
                            control_code,
                        });
                    }
                }
            }
        }
        bindings
    }
}

pub(crate) fn first_control_code(entry: &[i64]) -> Option<i64> {
    entry.first().copied()
}

/// Parse the legacy key table, tolerating the comments its own reader tolerates.
///
/// The legacy application reads this file with JsonCpp's `CharReaderBuilder`, and
/// that builder's default settings set `allowComments`, so a model author
/// documenting their key table with `//` or `/* */` annotations ships a file the
/// format accepts. A strict reader refuses it, and because detection is
/// speculative a refusal never surfaces as an error: the folder is simply taken
/// for an ordinary package and reported as an invalid one, with nothing to say
/// the key table was readable all along. Reading what the format's own reader
/// accepts is what keeps a commented table convertible.
///
/// Anything that still does not parse is reported as "not a legacy config"
/// rather than as an error, exactly as before: the ordinary package import is
/// the path that reports a real diagnostic.
pub(crate) fn parse_legacy_config(bytes: &[u8]) -> Option<LegacyConfig> {
    let text = std::str::from_utf8(bytes).ok()?;
    serde_json::from_str(&without_json_comments(text)).ok()
}

/// Drop `//` and `/* */` comments that are not inside a string literal.
///
/// A comment is removed but its line breaks are kept, so a config that still
/// does not parse fails at roughly the line its author wrote. A `/` inside a
/// string is data — a URL, a file path — and is copied through untouched, which
/// is what the string-literal scan below is for.
pub(crate) fn without_json_comments(text: &str) -> String {
    let mut characters = text.chars().peekable();
    let mut output = String::with_capacity(text.len());
    while let Some(character) = characters.next() {
        match character {
            // A string runs to its closing quote: an escaped quote does not end
            // it, so `\"` cannot be mistaken for the end of the literal.
            '"' => {
                output.push(character);
                let mut escaped = false;
                for character in characters.by_ref() {
                    output.push(character);
                    match character {
                        '\\' => escaped = !escaped,
                        '"' if !escaped => break,
                        _ => escaped = false,
                    }
                }
            }
            '/' if characters.peek() == Some(&'/') => {
                characters.next();
                for character in characters.by_ref() {
                    if character == '\n' || character == '\r' {
                        output.push(character);
                        break;
                    }
                }
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                let mut previous = '\0';
                for character in characters.by_ref() {
                    if character == '\n' || character == '\r' {
                        output.push(character);
                    }
                    if previous == '*' && character == '/' {
                        break;
                    }
                    previous = character;
                }
            }
            character => output.push(character),
        }
    }
    output
}

/// One output key image the legacy table asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyBinding {
    /// The `resources` subdirectory the composed image belongs in.
    pub(crate) output_directory: &'static str,
    /// The legacy hand-image directory inside the mode's resource folder.
    pub(crate) hand_directory: &'static str,
    pub(crate) hand_index: usize,
    pub(crate) keyboard_index: usize,
    /// A Windows virtual key for the pointer modes and an XInput button index
    /// for the gamepad mode; [`legacy_key_name`] knows which space applies.
    pub(crate) control_code: i64,
}
