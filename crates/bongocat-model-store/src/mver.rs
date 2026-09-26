//! BongoCatMver model sources.
//!
//! The legacy application (`Bongo-Cat-Mver`) keeps one Live2D model *and* one
//! set of key images per input mode, and pairs the two through a key table in a
//! root `config.json`:
//!
//! ```text
//! <source>/config.json                control code -> hand image index, per mode
//! <source>/img/<mode>/cat_model/      one Live2D model3 package
//! <source>/img/<mode>/hand/*.png      the paw drawn for a key
//! <source>/img/<mode>/keyboard/*.png  the key cap drawn underneath it
//! <source>/img/<mode>/mousebg.png     standard mode background (`bg.png` otherwise)
//! <source>/img/<mode>/cat.png         the mode's cover image
//! ```
//!
//! The two pointer modes address keys with a Windows virtual key and the gamepad
//! mode addresses buttons with an XInput button index, so the same number means
//! different things per mode; [`vocabulary::legacy_key_name`] is the one place
//! that knows which space applies.
//!
//! BongoCat's package format differs in two ways, and both matter:
//!
//! * The Live2D package must sit at the *package root*, not inside a
//!   `cat_model/` subdirectory, because entry discovery only looks at the root.
//! * Key images are one composed image per key, named after the key, under
//!   `resources/left-keys` / `resources/right-keys`. The runtime resolves the
//!   pointer modes' images from an HID usage and the bundled gamepad model names
//!   its own by button, so a legacy control code has to be translated into that
//!   same vocabulary instead of a second, parallel naming scheme.
//!
//! One legacy source therefore describes up to three BongoCat models — one per
//! mode that carries a usable `model3.json` — and each is installed as its own
//! model with its own generated store key. Nothing is shared between them,
//! because the three modes ship three different `.moc3` models.
//!
//! Detection is deliberately speculative. It only claims a source when the root
//! `config.json` parses *and* a mode that config names really holds a
//! `model3.json`; anything else returns `None`, so the ordinary package import
//! reports its own diagnostic rather than this module guessing.

mod compose;
mod config;
mod convert;
mod error;
#[cfg(test)]
pub(crate) mod fixture;
mod inspect;
mod layout;
mod plan;
mod source;
#[cfg(test)]
mod tests;
mod vocabulary;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the converter's items are one vocabulary, and a list per
// module would be the same list nine times.
pub(crate) use compose::*;
pub(crate) use config::*;
pub(crate) use convert::*;
pub(crate) use error::*;
pub(crate) use inspect::*;
pub(crate) use layout::*;
pub(crate) use plan::*;
pub(crate) use source::*;
pub(crate) use vocabulary::*;

// The public surface the crate root re-exports. The `pub(crate) use` lines above
// already bring the rest of these modules' items into scope here and below, but
// a `pub fn` has to be named directly: a re-export through a `pub(crate)` glob
// would narrow it and the crate root's own `pub use` would stop compiling.
pub use vocabulary::{legacy_gamepad_key_image_names, legacy_keyboard_key_image_names};

use crate::store::{
    CopyStatistics, ImportObservation, ModelImportProgress, ModelImportStage, ModelStoreDiagnostic,
    ModelStoreError, file_count_for_progress,
};
use bongocat_model::{
    ModelPackageLimits, PACKAGE_COVER_FILE, PACKAGE_RESOURCES_DIRECTORY, normalize_reference,
    path_from_reference,
};
use bongocat_storage::{set_private_directory, set_private_file};
use image::{ImageEncoder, RgbaImage};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;

/// One input mode of the legacy application.
///
/// The three modes are independent models in both applications, so this is the
/// unit a conversion produces and the unit the settings service titles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MverInputMode {
    Standard,
    Keyboard,
    Gamepad,
}

impl MverInputMode {
    /// Every mode the legacy application can carry, in the order a conversion
    /// reports them. The order is part of the settings contract: one title is
    /// derived per mode, and the preview must be stable across runs.
    pub const ALL: [Self; 3] = [Self::Standard, Self::Keyboard, Self::Gamepad];

    /// The legacy directory name. Also the stable, non-localized token the
    /// product uses for a converted model's mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Keyboard => "keyboard",
            Self::Gamepad => "gamepad",
        }
    }

    /// The background file name this mode uses.
    pub(crate) const fn background_file(self) -> &'static str {
        match self {
            Self::Standard => LEGACY_STANDARD_BACKGROUND,
            Self::Keyboard | Self::Gamepad => LEGACY_BACKGROUND,
        }
    }
}

/// What a user-picked source turned out to contain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelSourceContent {
    /// One BongoCat model package: the store imports it unchanged.
    Package,
    /// A BongoCatMver source. Each listed mode converts into its own BongoCat
    /// model; a mode whose Live2D package is missing or ambiguous is absent.
    Mver { modes: Vec<MverInputMode> },
}
