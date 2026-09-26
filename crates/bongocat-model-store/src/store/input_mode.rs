//! Which input family a source belongs to.
//!
//! An ordinary package is classified from the key artwork it ships, because that
//! is the only thing in it that says which model it is. A BongoCatMver import
//! does not use this: the legacy section the user picked is authoritative, and
//! the two vocabularies are kept as separate types so each boundary owns its
//! own stable names.

use super::*;

/// The input family resolved from an ordinary model package's key artwork.
///
/// A BongoCatMver import does not use this classifier: its selected legacy
/// section is authoritative. The three values are kept separate from the config
/// and UI protocol enums so each boundary owns its stable type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStoreInputMode {
    Standard,
    Keyboard,
    Gamepad,
}

impl From<MverInputMode> for ModelStoreInputMode {
    fn from(mode: MverInputMode) -> Self {
        match mode {
            MverInputMode::Standard => Self::Standard,
            MverInputMode::Keyboard => Self::Keyboard,
            MverInputMode::Gamepad => Self::Gamepad,
        }
    }
}

impl ModelStoreInputMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Keyboard => "keyboard",
            Self::Gamepad => "gamepad",
        }
    }
}

/// Key-image stems that identify an ordinary package as a gamepad model.
///
/// Both vocabularies are accepted. The canonical half is
/// `GamepadButton::key_image_name`, and a package that still carries the
/// third-party stems the old Tauri input layer produced keeps being recognized:
/// the import normalizer rewrites those stems on the way in
/// (`crate::key_names`), but a directory copied into the store by hand bypasses
/// it, and classifying it as a keyboard model would be a worse answer than
/// recognizing the family while leaving its unreachable artwork alone.
pub(crate) const GAMEPAD_MODE_KEY_IMAGES: &[&str] = &[
    "South",
    "East",
    "West",
    "North",
    "Select",
    "Start",
    "LeftStick",
    "RightStick",
    "LeftShoulder",
    "RightShoulder",
    "LeftTrigger",
    "RightTrigger",
    "DpadUp",
    "DpadDown",
    "DpadLeft",
    "DpadRight",
    "DPadUp",
    "DPadDown",
    "DPadLeft",
    "DPadRight",
    "LeftThumb",
    "RightThumb",
    "LeftTrigger2",
    "RightTrigger2",
];

pub(crate) fn key_image_names(root: &Path, directory: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(entries) = fs::read_dir(root.join("resources").join(directory)) else {
        return names;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|name| name.to_str()) {
            names.insert(name.to_owned());
        }
    }
    names
}

pub(crate) fn classify_input_mode(root: &Path) -> Result<ModelStoreInputMode, ModelStoreError> {
    let left = key_image_names(root, "left-keys");
    let right = key_image_names(root, "right-keys");
    let has_gamepad_image = [(&left, &right)]
        .into_iter()
        .flat_map(|(left, right)| left.iter().chain(right.iter()))
        .any(|name| GAMEPAD_MODE_KEY_IMAGES.contains(&name.as_str()));
    if has_gamepad_image {
        Ok(ModelStoreInputMode::Gamepad)
    } else if !right.is_empty() {
        Ok(ModelStoreInputMode::Keyboard)
    } else if !left.is_empty() {
        Ok(ModelStoreInputMode::Standard)
    } else {
        Err(ModelStoreError::new(
            ModelStoreDiagnostic::InvalidPackage,
            Some("resources/left-keys".to_owned()),
            "an ordinary model package must contain at least one left-keys or right-keys PNG",
        ))
    }
}
