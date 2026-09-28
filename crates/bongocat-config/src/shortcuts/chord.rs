//! A chord as the user types it, and the closed vocabulary it can be built from.
//!
//! The key table is a closed list rather than a parse, and a name the list does
//! not carry is refused rather than guessed. That is the whole point: a chord
//! that silently resolved to the wrong key would fire when the user pressed
//! something else, which is worse than one that does not work at all and says
//! so.

/// A platform-neutral keyboard chord used to validate persisted shortcut
/// bindings before a platform adapter attempts to capture or register them.
/// The key is kept as a canonical token because mapping it to a physical key
/// is platform-specific and belongs outside the configuration crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutChord {
    pub(crate) modifiers: ShortcutModifiers,
    pub(crate) key: ShortcutKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutKey {
    pub(crate) canonical: String,
    pub(crate) hid_usage: u16,
}

impl ShortcutKey {
    pub fn parse(value: &str) -> Result<Self, ShortcutParseError> {
        let value = value.trim();
        if value.is_empty()
            || !value.is_ascii()
            || value.chars().any(char::is_whitespace)
            || value.chars().any(|character| !character.is_ascii_graphic())
        {
            return Err(ShortcutParseError::InvalidKey);
        }

        if value.len() == 1 {
            let byte = value.as_bytes()[0];
            if byte.is_ascii_alphabetic() {
                let upper = byte.to_ascii_uppercase();
                return Ok(Self {
                    canonical: char::from(upper).to_string(),
                    hid_usage: 0x04 + u16::from(upper - b'A'),
                });
            }
            if byte.is_ascii_digit() {
                let hid_usage = if byte == b'0' {
                    0x27
                } else {
                    0x1e + u16::from(byte - b'1')
                };
                return Ok(Self {
                    canonical: char::from(byte).to_string(),
                    hid_usage,
                });
            }
        }

        if value.len() == 4 && value[..3].eq_ignore_ascii_case("key") {
            return Self::parse(&value[3..]);
        }
        if value.len() == 6 && value[..5].eq_ignore_ascii_case("digit") {
            return Self::parse(&value[5..]);
        }

        let (canonical, hid_usage) = NAMED_SHORTCUT_KEYS
            .iter()
            .find_map(|(alias, canonical, usage)| {
                value
                    .eq_ignore_ascii_case(alias)
                    .then_some((*canonical, *usage))
            })
            .ok_or(ShortcutParseError::InvalidKey)?;
        Ok(Self {
            canonical: canonical.to_owned(),
            hid_usage,
        })
    }

    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    pub const fn hid_usage(&self) -> u16 {
        self.hid_usage
    }
}

pub(crate) const NAMED_SHORTCUT_KEYS: &[(&str, &str, u16)] = &[
    ("-", "-", 0x2d),
    ("Minus", "-", 0x2d),
    ("=", "=", 0x2e),
    ("Equal", "=", 0x2e),
    ("Enter", "Enter", 0x28),
    ("Escape", "Escape", 0x29),
    ("Esc", "Escape", 0x29),
    ("Backspace", "Backspace", 0x2a),
    ("Tab", "Tab", 0x2b),
    ("Space", "Space", 0x2c),
    ("BracketLeft", "BracketLeft", 0x2f),
    ("BracketRight", "BracketRight", 0x30),
    ("Backslash", "Backslash", 0x31),
    ("Semicolon", "Semicolon", 0x33),
    ("Quote", "Quote", 0x34),
    ("Backquote", "Backquote", 0x35),
    ("Comma", "Comma", 0x36),
    ("Period", "Period", 0x37),
    ("Slash", "Slash", 0x38),
    ("CapsLock", "CapsLock", 0x39),
    ("F1", "F1", 0x3a),
    ("F2", "F2", 0x3b),
    ("F3", "F3", 0x3c),
    ("F4", "F4", 0x3d),
    ("F5", "F5", 0x3e),
    ("F6", "F6", 0x3f),
    ("F7", "F7", 0x40),
    ("F8", "F8", 0x41),
    ("F9", "F9", 0x42),
    ("F10", "F10", 0x43),
    ("F11", "F11", 0x44),
    ("F12", "F12", 0x45),
    ("PrintScreen", "PrintScreen", 0x46),
    ("ScrollLock", "ScrollLock", 0x47),
    ("Pause", "Pause", 0x48),
    ("Insert", "Insert", 0x49),
    ("Home", "Home", 0x4a),
    ("PageUp", "PageUp", 0x4b),
    ("Delete", "Delete", 0x4c),
    ("End", "End", 0x4d),
    ("PageDown", "PageDown", 0x4e),
    ("ArrowRight", "ArrowRight", 0x4f),
    ("ArrowLeft", "ArrowLeft", 0x50),
    ("ArrowDown", "ArrowDown", 0x51),
    ("ArrowUp", "ArrowUp", 0x52),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShortcutModifiers(pub(crate) u8);

impl ShortcutModifiers {
    pub const CONTROL: u8 = 1 << 0;
    pub const ALT: u8 = 1 << 1;
    pub const SHIFT: u8 = 1 << 2;
    pub const META: u8 = 1 << 3;

    pub(crate) const VALID_BITS: u8 = Self::CONTROL | Self::ALT | Self::SHIFT | Self::META;

    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::VALID_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ShortcutParseError {
    #[error("shortcut must contain a key")]
    MissingKey,
    #[error("shortcut contains an empty part")]
    EmptyPart,
    #[error("shortcut contains a duplicate modifier")]
    DuplicateModifier,
    #[error("shortcut must contain exactly one key")]
    MultipleKeys,
    #[error("shortcut key is not a supported physical key token")]
    InvalidKey,
}

impl ShortcutChord {
    pub fn parse(value: &str) -> Result<Self, ShortcutParseError> {
        let mut modifiers = 0_u8;
        let mut key = None;
        for part in value.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err(ShortcutParseError::EmptyPart);
            }
            let modifier = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => Some(ShortcutModifiers::CONTROL),
                "alt" | "option" => Some(ShortcutModifiers::ALT),
                "shift" => Some(ShortcutModifiers::SHIFT),
                "meta" | "command" | "cmd" | "win" | "windows" => Some(ShortcutModifiers::META),
                _ => None,
            };
            if let Some(modifier) = modifier {
                if modifiers & modifier != 0 {
                    return Err(ShortcutParseError::DuplicateModifier);
                }
                modifiers |= modifier;
                continue;
            }
            if key.is_some() {
                return Err(ShortcutParseError::MultipleKeys);
            }
            key = Some(ShortcutKey::parse(part)?);
        }
        let key = key.ok_or(ShortcutParseError::MissingKey)?;
        Ok(Self {
            modifiers: ShortcutModifiers(modifiers),
            key,
        })
    }

    pub const fn modifiers(&self) -> ShortcutModifiers {
        self.modifiers
    }

    pub fn key(&self) -> &str {
        self.key.canonical()
    }

    pub const fn key_hid_usage(&self) -> u16 {
        self.key.hid_usage()
    }

    pub fn matches(&self, modifiers: ShortcutModifiers, key: &str) -> bool {
        self.modifiers == modifiers
            && ShortcutKey::parse(key).is_ok_and(|key| key.hid_usage() == self.key.hid_usage())
    }

    pub fn matches_hid_usage(&self, modifiers: ShortcutModifiers, hid_usage: u16) -> bool {
        self.modifiers == modifiers && self.key.hid_usage() == hid_usage
    }

    /// Return a stable representation used for conflict detection and later
    /// platform registration. Modifier aliases and input ordering collapse to
    /// this one form.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        for (bit, name) in [
            (ShortcutModifiers::CONTROL, "Control"),
            (ShortcutModifiers::ALT, "Alt"),
            (ShortcutModifiers::SHIFT, "Shift"),
            (ShortcutModifiers::META, "Meta"),
        ] {
            if self.modifiers.0 & bit != 0 {
                parts.push(name);
            }
        }
        parts.push(self.key.canonical());
        parts.join("+")
    }
}
