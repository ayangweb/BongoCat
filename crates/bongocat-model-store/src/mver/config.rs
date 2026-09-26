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
