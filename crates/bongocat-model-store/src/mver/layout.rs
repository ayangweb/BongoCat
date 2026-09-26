//! Where the legacy application keeps things, and where each one lands.
//!
//! Both halves live together because they are the same table read forwards and
//! backwards: a name here is either a directory the source ships or a path the
//! package will carry. The destination half keeps the resources directory and
//! the cover name from the crate root, because the settings cover editor reads
//! the same file this conversion writes.

/// Root config file of a legacy application folder.
pub(crate) const LEGACY_CONFIG_FILE: &str = "config.json";

/// Directory the legacy application keeps its per-mode resources in. A source
/// that ships the mode folders directly at its root is accepted too.
pub(crate) const LEGACY_RESOURCE_ROOT: &str = "img";

/// Directory holding a mode's Live2D package inside its resource folder.
pub(crate) const LEGACY_MODEL_DIRECTORY: &str = "cat_model";

/// The composed-image sources of one mode.
pub(crate) const LEGACY_HAND_DIRECTORY: &str = "hand";

pub(crate) const LEGACY_KEYBOARD_DIRECTORY: &str = "keyboard";

pub(crate) const LEGACY_LEFT_HAND_DIRECTORY: &str = "lefthand";

pub(crate) const LEGACY_RIGHT_HAND_DIRECTORY: &str = "righthand";

/// Standard mode draws the cat at a mouse, so its background has its own name.
pub(crate) const LEGACY_STANDARD_BACKGROUND: &str = "mousebg.png";

pub(crate) const LEGACY_BACKGROUND: &str = "bg.png";

pub(crate) const LEGACY_COVER: &str = "cat.png";

/// Package-relative destinations of the converted resources. The resources
/// directory and the cover keep their names from [`crate`], because the
/// settings cover editor reads the same file this conversion writes.
pub(crate) const OUTPUT_LEFT_KEYS: &str = "left-keys";

pub(crate) const OUTPUT_RIGHT_KEYS: &str = "right-keys";

pub(crate) const OUTPUT_BACKGROUND: &str = "background.png";

/// The suffix entry discovery looks for at a package root.
pub(crate) const MODEL_ENTRY_SUFFIX: &str = ".model3.json";

/// Largest legacy resource the converter buffers at once. Model files are a few
/// megabytes at most (a `.moc3`, or a 2048x2048 texture), so this bound only
/// ever fires on a hostile or corrupt source. It is deliberately separate from
/// [`ModelPackageLimits::maximum_file_bytes`], which bounds what may be
/// *installed* rather than what is transiently read to produce it.
pub(crate) const LEGACY_RESOURCE_MAXIMUM_BYTES: u64 = 64 * 1024 * 1024;
