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
//! different things per mode; [`legacy_key_name`] is the one place that knows
//! which space applies.
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

use crate::{
    ModelPackageLimits,
    archive::{ArchivePlan, create_package_directory},
    normalize_reference, path_from_reference,
    store::{
        CopyStatistics, ImportObservation, ModelImportProgress, ModelImportStage,
        ModelStoreDiagnostic, ModelStoreError, file_count_for_progress, set_private_file,
    },
};
use image::{ImageEncoder, RgbaImage};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// Root config file of a legacy application folder.
const LEGACY_CONFIG_FILE: &str = "config.json";
/// Directory the legacy application keeps its per-mode resources in. A source
/// that ships the mode folders directly at its root is accepted too.
const LEGACY_RESOURCE_ROOT: &str = "img";
/// Directory holding a mode's Live2D package inside its resource folder.
const LEGACY_MODEL_DIRECTORY: &str = "cat_model";
/// The composed-image sources of one mode.
const LEGACY_HAND_DIRECTORY: &str = "hand";
const LEGACY_KEYBOARD_DIRECTORY: &str = "keyboard";
const LEGACY_LEFT_HAND_DIRECTORY: &str = "lefthand";
const LEGACY_RIGHT_HAND_DIRECTORY: &str = "righthand";
/// Standard mode draws the cat at a mouse, so its background has its own name.
const LEGACY_STANDARD_BACKGROUND: &str = "mousebg.png";
const LEGACY_BACKGROUND: &str = "bg.png";
const LEGACY_COVER: &str = "cat.png";

/// Package-relative destinations of the converted resources.
const OUTPUT_RESOURCES: &str = "resources";
const OUTPUT_LEFT_KEYS: &str = "left-keys";
const OUTPUT_RIGHT_KEYS: &str = "right-keys";
const OUTPUT_BACKGROUND: &str = "background.png";
const OUTPUT_COVER: &str = "cover.png";

/// The suffix entry discovery looks for at a package root.
const MODEL_ENTRY_SUFFIX: &str = ".model3.json";

/// Largest legacy resource the converter buffers at once. Model files are a few
/// megabytes at most (a `.moc3`, or a 2048x2048 texture), so this bound only
/// ever fires on a hostile or corrupt source. It is deliberately separate from
/// [`ModelPackageLimits::maximum_file_bytes`], which bounds what may be
/// *installed* rather than what is transiently read to produce it.
const LEGACY_RESOURCE_MAXIMUM_BYTES: u64 = 64 * 1024 * 1024;

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
    const fn background_file(self) -> &'static str {
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

/// The legacy root config, reduced to the sections the conversion consumes.
///
/// Every field is optional and every list defaults to empty: the legacy
/// application writes this file from a global settings object, so a source that
/// only ever ran in one mode may well omit the other sections, and an unknown
/// key must not make an otherwise usable model unreadable.
#[derive(Debug, Default, Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    standard: Option<LegacySection>,
    #[serde(default)]
    keyboard: Option<LegacySection>,
    #[serde(default)]
    gamepad: Option<LegacySection>,
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
struct LegacySection {
    #[serde(default)]
    hand: Vec<Vec<i64>>,
    #[serde(default)]
    lefthand: Vec<Vec<i64>>,
    #[serde(default)]
    righthand: Vec<Vec<i64>>,
}

impl LegacyConfig {
    const fn section(&self, mode: MverInputMode) -> Option<&LegacySection> {
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
    fn bindings(&self, mode: MverInputMode) -> Vec<LegacyBinding> {
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

fn first_control_code(entry: &[i64]) -> Option<i64> {
    entry.first().copied()
}

/// One output key image the legacy table asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LegacyBinding {
    /// The `resources` subdirectory the composed image belongs in.
    output_directory: &'static str,
    /// The legacy hand-image directory inside the mode's resource folder.
    hand_directory: &'static str,
    hand_index: usize,
    keyboard_index: usize,
    /// A Windows virtual key for the pointer modes and an XInput button index
    /// for the gamepad mode; [`legacy_key_name`] knows which space applies.
    control_code: i64,
}

/// How one output key image is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MverSlotImage {
    /// The legacy source already draws a composed image, so its bytes are
    /// installed as they are and are never re-encoded.
    Verbatim(String),
    /// The paw and the key cap are separate layers and are composed with the
    /// paw on top, matching how the legacy application draws them.
    Composite { hand: String, keyboard: String },
}

/// One key image a mode contributes, with its destination inside `resources`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MverSlot {
    reference: String,
    image: MverSlotImage,
}

/// Everything one legacy mode converts into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MverModePlan {
    mode: MverInputMode,
    /// Package-relative directory holding the mode's legacy resources.
    root: String,
    /// Package-relative directory holding the mode's Live2D package.
    model: String,
    background: Option<String>,
    cover: Option<String>,
    slots: Vec<MverSlot>,
}

/// The modes a legacy source carries, in [`MverInputMode::ALL`] order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MverPlan {
    modes: Vec<MverModePlan>,
}

impl MverPlan {
    pub(crate) fn modes(&self) -> impl Iterator<Item = MverInputMode> + '_ {
        self.modes.iter().map(|plan| plan.mode)
    }

    pub(crate) fn mode(&self, mode: MverInputMode) -> Option<&MverModePlan> {
        self.modes.iter().find(|plan| plan.mode == mode)
    }
}

/// Where a legacy source's bytes are read from.
///
/// A directory is read in place; an archive is never extracted. The archive
/// variant reads the individual entries a conversion names from the
/// already-planned central directory, so importing a legacy `.zip` never writes
/// a decompressed copy of the whole application — its executables, DLLs and
/// unrelated modes included — to disk.
pub(crate) enum MverSource<'a> {
    Directory(PathBuf),
    Archive {
        path: PathBuf,
        plan: &'a ArchivePlan,
    },
}

impl<'a> MverSource<'a> {
    /// Read a directory in place.
    ///
    /// The root is resolved once, here, because every read walks it: a source
    /// reached through a symbolic link (a temporary directory on macOS is the
    /// everyday case) would otherwise compare unresolved reads against an
    /// unresolved root and reject its own files.
    pub(crate) fn directory(root: impl AsRef<Path>) -> Result<Self, ModelStoreError> {
        let canonical = root.as_ref().canonicalize().map_err(|error| {
            conversion_error(
                None,
                format!("legacy source directory cannot be opened: {error}"),
            )
        })?;
        Ok(Self::Directory(canonical))
    }

    pub(crate) fn archive(path: impl Into<PathBuf>, plan: &'a ArchivePlan) -> Self {
        Self::Archive {
            path: path.into(),
            plan,
        }
    }

    /// Whether `reference` names a regular file of the source.
    ///
    /// A symbolic link is reported as an unsupported source rather than
    /// followed or ignored: an overlay that silently disappears because the
    /// model reached outside itself is worse than a stable diagnostic.
    fn is_file(&self, reference: &str) -> Result<bool, ModelStoreError> {
        match self {
            Self::Directory(root) => {
                let Ok(metadata) = fs::symlink_metadata(root.join(path_from_reference(reference)))
                else {
                    return Ok(false);
                };
                if metadata.file_type().is_symlink() {
                    return Err(symlink_unsupported(reference));
                }
                Ok(metadata.is_file())
            }
            Self::Archive { plan, .. } => Ok(plan.contains_file(reference)),
        }
    }

    /// Whether `reference` names a directory that holds at least one entry.
    fn is_directory(&self, reference: &str) -> Result<bool, ModelStoreError> {
        match self {
            Self::Directory(root) => {
                let Ok(metadata) = fs::symlink_metadata(root.join(path_from_reference(reference)))
                else {
                    return Ok(false);
                };
                if metadata.file_type().is_symlink() {
                    return Err(symlink_unsupported(reference));
                }
                Ok(metadata.is_dir())
            }
            Self::Archive { plan, .. } => {
                Ok(plan.file_references().any(|file| is_below(file, reference)))
            }
        }
    }

    /// Every regular file below `prefix`, as package-relative references.
    ///
    /// A source that does not contain `prefix` is empty rather than an error:
    /// the caller is probing for folders the legacy application makes optional.
    fn files_below(
        &self,
        prefix: &str,
        limits: ModelPackageLimits,
    ) -> Result<Vec<String>, ModelStoreError> {
        match self {
            Self::Directory(root) => {
                let directory = root.join(path_from_reference(prefix));
                let Ok(metadata) = fs::symlink_metadata(&directory) else {
                    return Ok(Vec::new());
                };
                if metadata.file_type().is_symlink() {
                    return Err(symlink_unsupported(prefix));
                }
                if !metadata.is_dir() {
                    return Ok(Vec::new());
                }
                let mut files = Vec::new();
                collect_source_files(&directory, prefix, 0, limits, &mut files)?;
                files.sort();
                Ok(files)
            }
            Self::Archive { plan, .. } => {
                let mut files = plan
                    .file_references()
                    .filter(|file| is_below(file, prefix))
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                files.sort();
                Ok(files)
            }
        }
    }

    /// Read one source resource into memory.
    fn read(&self, reference: &str) -> Result<Vec<u8>, ModelStoreError> {
        match self {
            Self::Directory(root) => read_source_file(root, reference),
            Self::Archive { path, plan } => {
                // The bound is checked against the declared size before the
                // entry is decompressed, so an oversized resource costs a
                // header read rather than an allocation.
                match plan.file_size(reference) {
                    Some(bytes) if bytes > LEGACY_RESOURCE_MAXIMUM_BYTES => Err(conversion_error(
                        Some(reference),
                        format!("legacy resource is {bytes} bytes"),
                    )),
                    Some(_) => plan.read_file(path, reference),
                    None => Err(conversion_error(
                        Some(reference),
                        "legacy resource is missing from the archive",
                    )),
                }
            }
        }
    }

    /// Read the marker file that decides whether this source is a legacy one.
    ///
    /// Detection is speculative, so "the file is not there" and "the file could
    /// not be read" are the same answer here: not a legacy source. Nothing is
    /// lost by treating them alike, because the package import path is what
    /// runs next and it reports the real diagnostic.
    fn read_legacy_config(&self) -> Option<Vec<u8>> {
        match self.is_file(LEGACY_CONFIG_FILE) {
            Ok(true) => self.read(LEGACY_CONFIG_FILE).ok(),
            _ => None,
        }
    }
}

fn symlink_unsupported(reference: &str) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::SourceSymlinkUnsupported,
        Some(reference.to_owned()),
        "legacy models are converted without following symbolic links",
    )
}

fn read_source_file(root: &Path, reference: &str) -> Result<Vec<u8>, ModelStoreError> {
    let candidate = root.join(path_from_reference(reference));
    let canonical = candidate.canonicalize().map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be opened: {error}"),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(symlink_unsupported(reference));
    }
    let metadata = canonical.metadata().map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be inspected: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(conversion_error(
            Some(reference),
            "legacy resource is not a regular file",
        ));
    }
    read_bounded(&canonical, reference, metadata.len())
}

/// Read a file whose size has already been checked against the legacy bound.
fn read_bounded(path: &Path, reference: &str, size: u64) -> Result<Vec<u8>, ModelStoreError> {
    if size > LEGACY_RESOURCE_MAXIMUM_BYTES {
        return Err(conversion_error(
            Some(reference),
            format!("legacy resource is {size} bytes"),
        ));
    }
    fs::read(path).map_err(|error| {
        conversion_error(
            Some(reference),
            format!("legacy resource cannot be read: {error}"),
        )
    })
}

/// Collect the regular files below `directory`, as package-relative references.
///
/// Symbolic links are rejected rather than followed, for the same reason the
/// ordinary package import rejects them: a source that reaches outside itself
/// is not a model.
fn collect_source_files(
    directory: &Path,
    prefix: &str,
    depth: usize,
    limits: ModelPackageLimits,
    files: &mut Vec<String>,
) -> Result<(), ModelStoreError> {
    if depth > limits.maximum_directory_depth {
        return Err(conversion_error(
            Some(prefix),
            "legacy source is nested deeper than the package limit allows",
        ));
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            conversion_error(
                Some(prefix),
                format!("legacy source directory cannot be listed: {error}"),
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            conversion_error(
                Some(prefix),
                format!("legacy source entry cannot be read: {error}"),
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| conversion_error(None, "legacy source contains a non-UTF-8 entry name"))?;
        let reference = join_reference(prefix, &name);
        let file_type = entry.file_type().map_err(|error| {
            conversion_error(
                Some(&reference),
                format!("legacy source entry cannot be inspected: {error}"),
            )
        })?;
        if file_type.is_symlink() {
            return Err(symlink_unsupported(&reference));
        }
        if file_type.is_dir() {
            collect_source_files(&entry.path(), &reference, depth + 1, limits, files)?;
        } else if file_type.is_file() {
            files.push(reference);
        }
    }
    Ok(())
}

/// Recognize a legacy source, or report that it is something else.
///
/// Two independent pieces of evidence are required: a root `config.json` that
/// parses into the legacy section shape, and at least one mode it names that
/// really holds a `model3.json` directly under its `cat_model/` directory. A
/// converted BongoCat package puts its entry at the package root instead, so
/// the two formats cannot be confused from the outside.
pub(crate) fn inspect(
    source: &MverSource<'_>,
    limits: ModelPackageLimits,
) -> Result<Option<MverPlan>, ModelStoreError> {
    let Some(bytes) = source.read_legacy_config() else {
        return Ok(None);
    };
    let Ok(config) = serde_json::from_slice::<LegacyConfig>(&bytes) else {
        return Ok(None);
    };
    let resources = if source.is_directory(LEGACY_RESOURCE_ROOT)? {
        LEGACY_RESOURCE_ROOT
    } else {
        ""
    };

    let mut modes = Vec::new();
    for mode in MverInputMode::ALL {
        let Some(section) = config.section(mode) else {
            continue;
        };
        let root = join_reference(resources, mode.as_str());
        let model = join_reference(&root, LEGACY_MODEL_DIRECTORY);
        let entries = source.files_below(&model, limits)?;
        let mut model_entries = entries.iter().filter(|reference| {
            is_direct_child(reference, &model) && reference.ends_with(MODEL_ENTRY_SUFFIX)
        });
        // Exactly one entry, exactly like package entry discovery: zero means
        // the mode was configured but never given a model, and more than one is
        // ambiguous and would be rejected after conversion anyway.
        let (Some(_), None) = (model_entries.next(), model_entries.next()) else {
            continue;
        };

        let background_reference = join_reference(&root, mode.background_file());
        let background = source
            .is_file(&background_reference)?
            .then_some(background_reference);
        let cover_reference = join_reference(&root, LEGACY_COVER);
        let cover = source.is_file(&cover_reference)?.then_some(cover_reference);

        let keyboard_root = join_reference(&root, LEGACY_KEYBOARD_DIRECTORY);
        let composites = !source.files_below(&keyboard_root, limits)?.is_empty();
        let mut slots = Vec::new();
        let mut references = BTreeSet::new();
        for binding in section.bindings(mode) {
            let names = legacy_key_names(mode, binding.control_code);
            if names.is_empty() {
                continue;
            }
            let hand = indexed_image_reference(&root, binding.hand_directory, binding.hand_index);
            if !source.is_file(&hand)? {
                continue;
            }
            let image = if composites {
                let keyboard = indexed_image_reference(
                    &root,
                    LEGACY_KEYBOARD_DIRECTORY,
                    binding.keyboard_index,
                );
                if !source.is_file(&keyboard)? {
                    continue;
                }
                MverSlotImage::Composite { hand, keyboard }
            } else {
                MverSlotImage::Verbatim(hand)
            };
            // One binding can address more than one key image — the legacy table
            // has codes for a whole key family rather than for a single key — and
            // each name gets the same composed overlay.
            for name in names {
                let reference = format!("{}/{name}.png", binding.output_directory);
                if !references.insert(reference.clone()) {
                    // Two legacy bindings that resolve to one BongoCat key image:
                    // the first wins, and the duplicate is not a second write.
                    continue;
                }
                slots.push(MverSlot {
                    reference,
                    image: image.clone(),
                });
            }
        }

        modes.push(MverModePlan {
            mode,
            root,
            model,
            background,
            cover,
            slots,
        });
    }

    if modes.is_empty() {
        return Ok(None);
    }
    Ok(Some(MverPlan { modes }))
}

/// Convert one legacy mode into a BongoCat package inside `destination`.
///
/// The destination is the store's own staging directory, so a conversion that
/// fails leaves no trace and a successful one is still committed by a single
/// rename. Nothing is written outside that directory.
pub(crate) fn convert_mode<Observe, IsCancelled>(
    source: &MverSource<'_>,
    plan: &MverModePlan,
    destination: &Path,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    let mut created = BTreeSet::new();
    // The Live2D package moves out of `cat_model/` onto the package root,
    // because that is the only place entry discovery looks.
    for reference in source.files_below(&plan.model, limits)? {
        observation.check_cancelled()?;
        let Some(relative) = reference.strip_prefix(&plan.model) else {
            continue;
        };
        let target = relative.trim_start_matches('/');
        if target.is_empty() {
            continue;
        }
        let bytes = source.read(&reference)?;
        write_staging_file(
            destination,
            target,
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }

    // The background and the cover are installed byte for byte: they are the
    // model's own artwork, not something the conversion produces.
    for (reference, target) in [
        (plan.background.as_deref(), OUTPUT_BACKGROUND),
        (plan.cover.as_deref(), OUTPUT_COVER),
    ] {
        let Some(reference) = reference else {
            continue;
        };
        observation.check_cancelled()?;
        let bytes = source.read(reference)?;
        write_staging_file(
            destination,
            &format!("{OUTPUT_RESOURCES}/{target}"),
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }

    for slot in &plan.slots {
        observation.check_cancelled()?;
        let target = format!("{OUTPUT_RESOURCES}/{}", slot.reference);
        let bytes = match &slot.image {
            MverSlotImage::Verbatim(hand) => source.read(hand)?,
            MverSlotImage::Composite { hand, keyboard } => {
                let hand = source.read(hand)?;
                let keyboard = source.read(keyboard)?;
                compose_key_image(&keyboard, &hand, &target)?
            }
        };
        write_staging_file(
            destination,
            &target,
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }
    Ok(())
}

/// Compose one key image: the paw drawn over the key cap.
///
/// Both layers are the same canvas in every real model, and the legacy
/// application draws them into a canvas no larger than the smaller of the two,
/// anchored at the origin — so a layer larger than the canvas is cropped rather
/// than scaled, and the canvas size is the minimum of the two.
fn compose_key_image(
    keyboard: &[u8],
    hand: &[u8],
    reference: &str,
) -> Result<Vec<u8>, ModelStoreError> {
    let keyboard = decode_png(keyboard, reference)?;
    let hand = decode_png(hand, reference)?;
    let width = keyboard.width().min(hand.width());
    let height = keyboard.height().min(hand.height());
    if width == 0 || height == 0 {
        return Err(conversion_error(
            Some(reference),
            "legacy key image has no pixels",
        ));
    }

    let mut canvas = vec![0_u8; width as usize * height as usize * 4];
    for layer in [&keyboard, &hand] {
        let stride = layer.width() as usize * 4;
        let source = layer.as_raw();
        for y in 0..height as usize {
            let source_row = y * stride;
            let canvas_row = y * (width as usize) * 4;
            for x in 0..width as usize {
                composite_pixel(
                    &mut canvas[canvas_row + x * 4..canvas_row + x * 4 + 4],
                    &source[source_row + x * 4..source_row + x * 4 + 4],
                );
            }
        }
    }

    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&canvas, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| {
            conversion_error(
                Some(reference),
                format!("composed key image cannot be encoded: {error}"),
            )
        })?;
    Ok(optimize_png(encoded))
}

fn decode_png(bytes: &[u8], reference: &str) -> Result<RgbaImage, ModelStoreError> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map(|image| image.to_rgba8())
        .map_err(|error| {
            conversion_error(
                Some(reference),
                format!("legacy key image is not a readable PNG: {error}"),
            )
        })
}

/// Composite one straight-alpha source pixel over one straight-alpha
/// destination pixel, the way a canvas `source-over` draw does.
///
/// The integer form of the Porter-Duff `over` operator is exact for the two
/// cases that carry all the visual weight — an opaque source replaces the
/// destination and an empty source leaves it untouched — and rounds once for
/// the antialiased edge pixels in between, where a floating-point form would
/// differ by at most one step per channel.
fn composite_pixel(destination: &mut [u8], source: &[u8]) {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 0 {
        return;
    }
    if source_alpha == 255 {
        destination.copy_from_slice(source);
        return;
    }
    let destination_alpha = u32::from(destination[3]);
    let inverse = 255 - source_alpha;
    let output_alpha = source_alpha + (destination_alpha * inverse + 127) / 255;
    if output_alpha == 0 {
        destination.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    let scale = 255 * output_alpha;
    for channel in 0..3 {
        let numerator = u32::from(source[channel]) * source_alpha * 255
            + u32::from(destination[channel]) * destination_alpha * inverse;
        destination[channel] = ((numerator + scale / 2) / scale) as u8;
    }
    destination[3] = output_alpha as u8;
}

/// Recode a composed PNG losslessly.
///
/// The composed pixels are produced by this module, so correctness never
/// depends on this step: every reduction the library applies by default (bit
/// depth, colour type, palette and greyscale) preserves the decoded pixels, and
/// `optimize_alpha` only rewrites the colour channels of pixels that are
/// already fully transparent. A recode the library refuses is written as the
/// plain encoding instead of failing the conversion, because the file is a
/// valid PNG either way and only its size is at stake.
fn optimize_png(encoded: Vec<u8>) -> Vec<u8> {
    let options = oxipng::Options {
        optimize_alpha: true,
        ..oxipng::Options::default()
    };
    match oxipng::optimize_from_memory(&encoded, &options) {
        Ok(optimized) if optimized.len() <= encoded.len() => optimized,
        _ => encoded,
    }
}

fn write_staging_file<Observe, IsCancelled>(
    destination: &Path,
    reference: &str,
    bytes: &[u8],
    created: &mut BTreeSet<PathBuf>,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
    let normalized = normalize_reference(reference)
        .map_err(|error| conversion_error(Some(reference), error.to_string()))?;
    if let Some((parent, _)) = normalized.rsplit_once('/') {
        create_package_directory(destination, &path_from_reference(parent), created)?;
    }
    let target = destination.join(path_from_reference(&normalized));
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let next_file_count = statistics.file_count.saturating_add(1);
    let next_total_bytes = statistics
        .total_bytes
        .checked_add(size)
        .ok_or_else(|| conversion_error(Some(&normalized), "converted package size overflowed"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| {
            conversion_error(
                Some(&normalized),
                format!("converted file cannot be created: {error}"),
            )
        })?;
    set_private_file(&output).map_err(|error| {
        conversion_error(
            Some(&normalized),
            format!("converted file permissions cannot be set: {error}"),
        )
    })?;
    output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| {
            conversion_error(
                Some(&normalized),
                format!("converted file cannot be written: {error}"),
            )
        })?;
    statistics.file_count = next_file_count;
    statistics.total_bytes = next_total_bytes;
    observation.report(ModelImportProgress {
        stage: ModelImportStage::Copying,
        files_copied: file_count_for_progress(statistics.file_count),
        bytes_copied: statistics.total_bytes,
    });
    Ok(())
}

fn indexed_image_reference(root: &str, directory: &str, index: usize) -> String {
    join_reference(&join_reference(root, directory), &format!("{index}.png"))
}

fn join_reference(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Whether `reference` is a direct child of the `directory` reference.
fn is_direct_child(reference: &str, directory: &str) -> bool {
    let Some(rest) = reference.strip_prefix(directory) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('/') else {
        return false;
    };
    !rest.is_empty() && !rest.contains('/')
}

/// Whether `reference` is inside `directory`.
fn is_below(reference: &str, directory: &str) -> bool {
    if directory.is_empty() {
        return !reference.is_empty();
    }
    reference
        .strip_prefix(directory)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn conversion_error(resource: Option<&str>, detail: impl Into<String>) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::SourceConversionFailed,
        resource.map(str::to_owned),
        detail,
    )
}

/// The BongoCat key-image name for one legacy control code.
///
/// The two pointer modes address keys with a Windows virtual key, and the
/// gamepad mode addresses buttons with an XInput button index — the same code
/// space the shipped gamepad model's images are named in. Both are translated
/// here into the vocabulary the product already uses: the names a keyboard key
/// resolves to are exactly the ones `bongocat-live2d` derives from its HID
/// usage, and the gamepad names are exactly the ones the bundled gamepad model
/// ships, so a converted model is reachable by the runtime instead of being a
/// parallel naming scheme nothing looks up.
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
const fn legacy_key_name(mode: MverInputMode, control_code: i64) -> Option<&'static str> {
    match mode {
        MverInputMode::Standard | MverInputMode::Keyboard => legacy_virtual_key_name(control_code),
        MverInputMode::Gamepad => legacy_gamepad_button_name(control_code),
    }
}

/// `VK_MENU`: the legacy keyboard chart's `18`, worn by both Alt keys.
const LEGACY_VK_MENU: i64 = 0x12;

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
fn legacy_key_names(mode: MverInputMode, control_code: i64) -> Vec<&'static str> {
    if mode != MverInputMode::Gamepad && control_code == LEGACY_VK_MENU {
        return vec!["AltLeft", "AltRight"];
    }
    legacy_key_name(mode, control_code).into_iter().collect()
}

const fn legacy_virtual_key_name(virtual_key: i64) -> Option<&'static str> {
    match virtual_key {
        0x08 => Some("Backspace"),
        0x09 => Some("Tab"),
        0x0D => Some("Return"),
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
        0xDC => Some("Backslash"),
        0xDD => Some("RightBracket"),
        0xDE => Some("Quote"),
        _ => None,
    }
}

const fn legacy_gamepad_button_name(button: i64) -> Option<&'static str> {
    match button {
        0 => Some("South"),
        1 => Some("East"),
        2 => Some("West"),
        3 => Some("North"),
        4 => Some("LeftTrigger"),
        5 => Some("RightTrigger"),
        6 => Some("LeftTrigger2"),
        7 => Some("RightTrigger2"),
        8 => Some("LeftThumb"),
        9 => Some("RightThumb"),
        10 => Some("DPadLeft"),
        11 => Some("DPadRight"),
        12 => Some("DPadUp"),
        13 => Some("DPadDown"),
        14 => Some("Start"),
        15 => Some("Select"),
        _ => None,
    }
}

/// Synthetic legacy sources, shared by this module's tests and the store's.
///
/// The images are deliberately tiny so the tests stay fast. The paw is
/// semi-transparent red and the key cap opaque blue, so a composed result is
/// distinguishable from either input.
#[cfg(test)]
pub(crate) mod fixture {
    use super::*;

    pub(crate) fn encode_png(
        width: u32,
        height: u32,
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> Vec<u8> {
        let mut raw = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&pixel(x, y));
            }
        }
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&raw, width, height, image::ExtendedColorType::Rgba8)
            .expect("encode png");
        encoded
    }

    pub(crate) fn write(directory: &Path, reference: &str, bytes: &[u8]) {
        let path = directory.join(reference);
        fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
        fs::write(path, bytes).expect("write file");
    }

    pub(crate) fn flat(colour: [u8; 4]) -> Vec<u8> {
        encode_png(4, 4, move |_, _| colour)
    }

    pub(crate) fn legacy_config(sections: &[(MverInputMode, &str)]) -> Vec<u8> {
        let mut config = serde_json::Map::new();
        for (mode, body) in sections {
            let section: serde_json::Value = serde_json::from_str(body).expect("section json");
            config.insert(mode.as_str().to_owned(), section);
        }
        serde_json::to_vec(&serde_json::Value::Object(config)).expect("config json")
    }

    /// The config sections of a source that carries all three modes.
    ///
    /// The gamepad section addresses buttons with XInput button indices, the
    /// way the legacy application does, so its overlays land on
    /// `DPadLeft` / `LeftTrigger` / `South` / `RightTrigger`.
    pub(crate) fn all_modes() -> Vec<(MverInputMode, &'static str)> {
        vec![
            (
                MverInputMode::Standard,
                r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#,
            ),
            (
                MverInputMode::Keyboard,
                r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
            ),
            (
                MverInputMode::Gamepad,
                r#"{"lefthand":[[10],[4]],"righthand":[[0],[5]],"keyboard":[[10],[4],[0],[5]]}"#,
            ),
        ]
    }

    /// Write a minimal but valid legacy source under `root`.
    pub(crate) fn legacy_source(
        root: &Path,
        sections: &[(MverInputMode, &str)],
        keyboard_layer: bool,
    ) {
        write(root, LEGACY_CONFIG_FILE, &legacy_config(sections));
        for (mode, _) in sections {
            let base = format!("img/{}", mode.as_str());
            write(
                root,
                &format!("{base}/{LEGACY_MODEL_DIRECTORY}/cat.model3.json"),
                br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
            );
            write(
                root,
                &format!("{base}/{LEGACY_MODEL_DIRECTORY}/model.moc3"),
                b"moc",
            );
            if keyboard_layer {
                // Enough key caps for the widest section any fixture uses; the
                // companion image a binding reads is picked by index, and a
                // missing one is what the skip tests remove on purpose.
                for (index, colour) in [
                    [0, 0, 255, 255],
                    [0, 255, 0, 255],
                    [255, 255, 0, 255],
                    [255, 0, 255, 255],
                ]
                .into_iter()
                .enumerate()
                {
                    write(
                        root,
                        &format!("{base}/{LEGACY_KEYBOARD_DIRECTORY}/{index}.png"),
                        &flat(colour),
                    );
                }
            }
            let hand_directories = match mode {
                MverInputMode::Standard => vec![LEGACY_HAND_DIRECTORY],
                MverInputMode::Keyboard | MverInputMode::Gamepad => {
                    vec![LEGACY_LEFT_HAND_DIRECTORY, LEGACY_RIGHT_HAND_DIRECTORY]
                }
            };
            for hand_directory in hand_directories {
                for index in 0..2 {
                    write(
                        root,
                        &format!("{base}/{hand_directory}/{index}.png"),
                        &flat([255, 0, 0, 128]),
                    );
                }
            }
            write(
                root,
                &format!("{base}/{}", mode.background_file()),
                &encode_png(2, 2, |_, _| [10, 20, 30, 255]),
            );
            write(
                root,
                &format!("{base}/{LEGACY_COVER}"),
                &encode_png(2, 2, |_, _| [40, 50, 60, 255]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;
    use crate::archive;
    use std::fs::File;
    use tempfile::tempdir;

    /// Inspect a directory source, panicking when it is not recognized.
    fn inspect_directory(root: &Path) -> Option<MverPlan> {
        inspect(
            &MverSource::directory(root).expect("open legacy source"),
            ModelPackageLimits::default(),
        )
        .expect("inspect")
    }

    /// Convert a directory-source mode into a fresh staging directory.
    fn convert(root: &Path, plan: &MverModePlan, staging: &Path) -> Result<(), ModelStoreError> {
        let mut statistics = CopyStatistics::default();
        let mut observe = |_progress| {};
        let mut cancelled = || false;
        let mut observation = ImportObservation::new(&mut observe, &mut cancelled);
        convert_mode(
            &MverSource::directory(root).expect("open legacy source"),
            plan,
            staging,
            ModelPackageLimits::default(),
            &mut statistics,
            &mut observation,
        )
    }

    fn plan_mode(plan: &MverPlan, mode: MverInputMode) -> MverModePlan {
        plan.mode(mode).expect("planned mode").clone()
    }

    #[test]
    fn a_configured_mode_without_a_model_is_skipped_rather_than_failing() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        // The keyboard mode is configured, but its model was never installed.
        write(
            root.path(),
            LEGACY_CONFIG_FILE,
            &legacy_config(&[
                (
                    MverInputMode::Standard,
                    r#"{"hand":[[65]],"keyboard":[[65]]}"#,
                ),
                (
                    MverInputMode::Keyboard,
                    r#"{"lefthand":[[65]],"keyboard":[[65]]}"#,
                ),
            ]),
        );

        let plan = inspect_directory(root.path()).expect("legacy plan");
        assert_eq!(
            plan.modes().collect::<Vec<_>>(),
            vec![MverInputMode::Standard]
        );
    }

    #[test]
    fn a_source_without_a_legacy_config_is_not_a_legacy_source() {
        let root = tempdir().expect("root");
        assert!(inspect_directory(root.path()).is_none());

        // A `config.json` that parses but names no mode with a model is still
        // not a legacy source: the second condition is what makes detection
        // unambiguous.
        write(
            root.path(),
            LEGACY_CONFIG_FILE,
            &legacy_config(&[(MverInputMode::Standard, r#"{"hand":[[65]]}"#)]),
        );
        assert!(inspect_directory(root.path()).is_none());

        // A config file that does not parse is the ordinary package import's
        // problem to report, not this module's.
        write(root.path(), LEGACY_CONFIG_FILE, b"not json");
        assert!(inspect_directory(root.path()).is_none());
    }

    #[test]
    fn detection_requires_exactly_one_model_entry_per_mode() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        assert!(inspect_directory(root.path()).is_some());

        // A second `.model3.json` in the same mode directory is ambiguous, the
        // same way two entries at a package root are.
        write(
            root.path(),
            "img/standard/cat_model/other.model3.json",
            br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        );
        assert!(inspect_directory(root.path()).is_none());
    }

    #[test]
    fn every_configured_mode_is_inspected_and_converts_on_its_own() {
        let root = tempdir().expect("root");
        legacy_source(root.path(), &all_modes(), true);
        let plan = inspect_directory(root.path()).expect("legacy plan");
        assert_eq!(
            plan.modes().collect::<Vec<_>>(),
            MverInputMode::ALL.to_vec()
        );
        assert_eq!(
            plan_mode(&plan, MverInputMode::Standard).model,
            "img/standard/cat_model"
        );

        // Each mode reads the background under its own name.
        assert_eq!(
            plan_mode(&plan, MverInputMode::Standard)
                .background
                .as_deref(),
            Some("img/standard/mousebg.png")
        );
        assert_eq!(
            plan_mode(&plan, MverInputMode::Keyboard)
                .background
                .as_deref(),
            Some("img/keyboard/bg.png")
        );

        // Every mode converts into a package of its own.
        for mode in MverInputMode::ALL {
            let staging = tempdir().expect("staging");
            convert(root.path(), &plan_mode(&plan, mode), staging.path()).expect("convert");
            assert!(
                staging.path().join("cat.model3.json").is_file(),
                "{} must place its entry at the root",
                mode.as_str()
            );
            assert!(
                staging.path().join("resources/left-keys").is_dir(),
                "{} must expose left key images",
                mode.as_str()
            );
        }
    }

    #[test]
    fn a_source_that_keeps_mode_folders_at_its_root_is_accepted() {
        let root = tempdir().expect("root");
        write(
            root.path(),
            LEGACY_CONFIG_FILE,
            &legacy_config(&[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )]),
        );
        write(
            root.path(),
            "standard/cat_model/cat.model3.json",
            br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        );
        write(root.path(), "standard/cat_model/model.moc3", b"moc");
        write(
            root.path(),
            "standard/keyboard/0.png",
            &flat([0, 0, 255, 255]),
        );
        write(root.path(), "standard/hand/0.png", &flat([255, 0, 0, 255]));

        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        assert_eq!(mode.model, "standard/cat_model");
        assert_eq!(
            mode.slots,
            vec![MverSlot {
                reference: "left-keys/KeyA.png".to_owned(),
                image: MverSlotImage::Composite {
                    hand: "standard/hand/0.png".to_owned(),
                    keyboard: "standard/keyboard/0.png".to_owned(),
                },
            }]
        );
    }

    #[test]
    fn the_split_hand_sections_share_one_keyboard_list() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Keyboard,
                r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
            )],
            true,
        );
        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Keyboard);
        // The right hand continues where the left hand stopped, so it reads
        // keyboard image `1` rather than restarting at `0`.
        assert_eq!(
            mode.slots,
            vec![
                MverSlot {
                    reference: "left-keys/KeyA.png".to_owned(),
                    image: MverSlotImage::Composite {
                        hand: "img/keyboard/lefthand/0.png".to_owned(),
                        keyboard: "img/keyboard/keyboard/0.png".to_owned(),
                    },
                },
                MverSlot {
                    reference: "right-keys/LeftArrow.png".to_owned(),
                    image: MverSlotImage::Composite {
                        hand: "img/keyboard/righthand/0.png".to_owned(),
                        keyboard: "img/keyboard/keyboard/1.png".to_owned(),
                    },
                },
            ]
        );
    }

    #[test]
    fn a_mode_without_companion_key_images_copies_the_hand_image_unchanged() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            false,
        );
        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        assert_eq!(
            mode.slots,
            vec![MverSlot {
                reference: "left-keys/KeyA.png".to_owned(),
                image: MverSlotImage::Verbatim("img/standard/hand/0.png".to_owned()),
            }]
        );

        let staging = tempdir().expect("staging");
        convert(root.path(), &mode, staging.path()).expect("convert");
        assert_eq!(
            fs::read(staging.path().join("resources/left-keys/KeyA.png")).expect("verbatim image"),
            fs::read(root.path().join("img/standard/hand/0.png")).expect("source image")
        );
    }

    #[test]
    fn converted_mode_places_the_package_at_the_root_and_composes_key_images() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        let staging = tempdir().expect("staging");
        convert(root.path(), &mode, staging.path()).expect("convert");

        // The entry and the moc sit at the root, which is the only place entry
        // discovery looks, and the mode's own folder name is gone.
        assert!(staging.path().join("cat.model3.json").is_file());
        assert!(staging.path().join("model.moc3").is_file());
        assert!(!staging.path().join("cat_model").exists());
        assert!(staging.path().join("resources/cover.png").is_file());
        assert_eq!(
            fs::read(staging.path().join("resources/background.png")).expect("background"),
            fs::read(root.path().join("img/standard/mousebg.png")).expect("source background")
        );

        // The composed image is the paw over the key cap: the semi-transparent
        // red paw mixes with the opaque blue cap and ends fully opaque.
        let composed = image::open(staging.path().join("resources/left-keys/KeyA.png"))
            .expect("composed image")
            .to_rgba8();
        assert_eq!(composed.dimensions(), (4, 4));
        assert_eq!(composed.get_pixel(0, 0).0, [128, 0, 127, 255]);

        // Converting the same source twice produces the same bytes.
        let again = tempdir().expect("staging");
        convert(root.path(), &mode, again.path()).expect("convert again");
        assert_eq!(
            fs::read(staging.path().join("resources/left-keys/KeyA.png")).expect("first"),
            fs::read(again.path().join("resources/left-keys/KeyA.png")).expect("second")
        );
    }

    #[test]
    fn composite_is_the_porter_duff_over_operator() {
        // An opaque source replaces the destination outright.
        let mut destination = [1, 2, 3, 255];
        composite_pixel(&mut destination, &[9, 8, 7, 255]);
        assert_eq!(destination, [9, 8, 7, 255]);
        // A fully transparent source leaves it untouched.
        let mut destination = [1, 2, 3, 255];
        composite_pixel(&mut destination, &[9, 8, 7, 0]);
        assert_eq!(destination, [1, 2, 3, 255]);
        // Half-transparent white over opaque black is half grey.
        let mut destination = [0, 0, 0, 255];
        composite_pixel(&mut destination, &[255, 255, 255, 128]);
        assert_eq!(destination, [128, 128, 128, 255]);
        // A transparent destination keeps the source's own colour.
        let mut destination = [0, 0, 0, 0];
        composite_pixel(&mut destination, &[200, 100, 50, 64]);
        assert_eq!(destination, [200, 100, 50, 64]);
        // A transparent canvas survives a transparent source.
        let mut destination = [0, 0, 0, 0];
        composite_pixel(&mut destination, &[9, 9, 9, 0]);
        assert_eq!(destination, [0, 0, 0, 0]);
    }

    #[test]
    fn the_png_recode_preserves_every_visible_pixel_and_shrinks_the_file() {
        // A composed-looking image: a transparent field, a filled shape and a
        // semi-transparent disc, which is what the real key images are made of.
        let encoded = encode_png(128, 96, |x, y| {
            let dx = i64::from(x) - 90;
            let dy = i64::from(y) - 48;
            if (20..60).contains(&x) && (20..60).contains(&y) {
                [200, 30, 40, 255]
            } else if dx * dx + dy * dy < 400 {
                [10, 10, 10, 128]
            } else {
                [0, 0, 0, 0]
            }
        });
        let optimized = optimize_png(encoded.clone());
        assert!(
            optimized.len() < encoded.len(),
            "recode must reduce {} bytes but produced {}",
            encoded.len(),
            optimized.len()
        );

        let before = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png)
            .expect("decode")
            .to_rgba8();
        let after = image::load_from_memory_with_format(&optimized, image::ImageFormat::Png)
            .expect("decode")
            .to_rgba8();
        assert_eq!(before.dimensions(), after.dimensions());
        for (index, (before, after)) in before
            .as_raw()
            .chunks_exact(4)
            .zip(after.as_raw().chunks_exact(4))
            .enumerate()
        {
            if before[3] == 0 {
                // Colour under a fully transparent pixel is not part of the
                // image; only its transparency has to survive.
                assert_eq!(after[3], 0, "pixel {index} must stay transparent");
                continue;
            }
            assert_eq!(before, after, "pixel {index} must be unchanged");
        }
    }

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
    fn a_missing_layer_skips_its_binding_without_failing_the_conversion() {
        // Neither layer of a binding may be missing: a binding whose paw, or
        // whose companion key image, is absent has no overlay to install and is
        // left out without failing the rest of the conversion.
        for missing in ["img/standard/hand/1.png", "img/standard/keyboard/1.png"] {
            let root = tempdir().expect("root");
            legacy_source(
                root.path(),
                &[(
                    MverInputMode::Standard,
                    r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#,
                )],
                true,
            );
            fs::remove_file(root.path().join(missing)).expect("remove layer");
            let plan = inspect_directory(root.path()).expect("legacy plan");
            let mode = plan_mode(&plan, MverInputMode::Standard);
            assert_eq!(
                mode.slots
                    .iter()
                    .map(|slot| slot.reference.as_str())
                    .collect::<Vec<_>>(),
                vec!["left-keys/KeyA.png"],
                "removing {missing}"
            );
        }
    }

    #[test]
    fn two_legacy_keys_that_share_one_overlay_write_it_once() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65],[91]],"keyboard":[[65],[91]]}"#,
            )],
            true,
        );
        write(
            root.path(),
            "img/standard/keyboard/1.png",
            &flat([0, 0, 255, 255]),
        );
        write(
            root.path(),
            "img/standard/hand/1.png",
            &flat([255, 0, 0, 255]),
        );

        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        assert_eq!(
            mode.slots
                .iter()
                .map(|slot| slot.reference.as_str())
                .collect::<Vec<_>>(),
            vec!["left-keys/KeyA.png", "left-keys/MetaLeft.png"]
        );

        let staging = tempdir().expect("staging");
        convert(root.path(), &mode, staging.path()).expect("convert");
        assert!(
            staging
                .path()
                .join("resources/left-keys/KeyA.png")
                .is_file()
        );
        assert!(
            staging
                .path()
                .join("resources/left-keys/MetaLeft.png")
                .is_file()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_inside_the_source_is_rejected_rather_than_followed() {
        use std::os::unix::fs::symlink;

        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        write(
            outside.path(),
            "secret.png",
            &encode_png(2, 2, |_, _| [1, 2, 3, 255]),
        );
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        fs::remove_file(root.path().join("img/standard/hand/0.png")).expect("remove hand image");
        symlink(
            outside.path().join("secret.png"),
            root.path().join("img/standard/hand/0.png"),
        )
        .expect("symlink hand image");

        let error = inspect(
            &MverSource::directory(root.path()).expect("open legacy source"),
            ModelPackageLimits::default(),
        )
        .expect_err("symlinked legacy source");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceSymlinkUnsupported);
    }

    #[test]
    fn an_archive_source_converts_without_being_extracted() {
        use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        let archives = tempdir().expect("archives");
        let archive_path = archives.path().join("legacy.zip");
        let mut writer = ZipWriter::new(File::create(&archive_path).expect("create archive"));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        // Model packs are exported by compressing the application folder, so
        // every entry carries one wrapper directory.
        writer
            .add_directory("Bongo Cat Mver/", options)
            .expect("wrapper directory");
        for entry in [
            "config.json",
            "img/standard/cat_model/cat.model3.json",
            "img/standard/cat_model/model.moc3",
            "img/standard/keyboard/0.png",
            "img/standard/hand/0.png",
            "img/standard/mousebg.png",
        ] {
            writer
                .start_file(format!("Bongo Cat Mver/{entry}"), options)
                .expect("start entry");
            writer
                .write_all(&fs::read(root.path().join(entry)).expect("source entry"))
                .expect("write entry");
        }
        writer.finish().expect("finish archive");

        let limits = ModelPackageLimits::default();
        let archive_plan = archive::plan_archive(&archive_path, limits).expect("archive plan");
        let source = MverSource::archive(&archive_path, &archive_plan);
        let plan = inspect(&source, limits)
            .expect("inspect")
            .expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);

        let staging = tempdir().expect("staging");
        let mut statistics = CopyStatistics::default();
        let mut observe = |_progress| {};
        let mut cancelled = || false;
        let mut observation = ImportObservation::new(&mut observe, &mut cancelled);
        convert_mode(
            &source,
            &mode,
            staging.path(),
            limits,
            &mut statistics,
            &mut observation,
        )
        .expect("convert");

        assert!(staging.path().join("cat.model3.json").is_file());
        assert!(
            staging
                .path()
                .join("resources/left-keys/KeyA.png")
                .is_file()
        );
        // The wrapper directory is archive tooling, so it never reaches the
        // package.
        assert!(!staging.path().join("Bongo Cat Mver").exists());
    }

    #[test]
    fn conversion_refuses_resources_beyond_the_legacy_read_bound() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            )],
            true,
        );
        // A sparse file, so the test never allocates the bytes it is bounding.
        let oversized = root.path().join("img/standard/cat_model/oversized.bin");
        File::create(&oversized)
            .expect("create oversized resource")
            .set_len(LEGACY_RESOURCE_MAXIMUM_BYTES + 1)
            .expect("size oversized resource");

        let plan = inspect_directory(root.path()).expect("legacy plan");
        let staging = tempdir().expect("staging");
        let error = convert(
            root.path(),
            &plan_mode(&plan, MverInputMode::Standard),
            staging.path(),
        )
        .expect_err("oversized resource");
        assert_eq!(error.code, ModelStoreDiagnostic::SourceConversionFailed);
        assert_eq!(
            error.resource.as_deref(),
            Some("img/standard/cat_model/oversized.bin")
        );
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
            (Standard, 0xC0, "BackQuote"),
            (Standard, 0xBF, "Slash"),
            (Keyboard, 0x52, "KeyR"),
            // The side-independent Alt code keeps the legacy table's own name:
            // `legacy_key_names` is what turns it into the product's two sides.
            (Standard, 0x12, "Alt"),
            (Standard, 0xA4, "AltLeft"),
            (Keyboard, 0xA5, "AltRight"),
            // The gamepad mode addresses buttons, not virtual keys: index 13 is
            // the D-pad down button, while virtual key 0x0D is Return.
            (Gamepad, 13, "DPadDown"),
            (Gamepad, 10, "DPadLeft"),
            (Gamepad, 4, "LeftTrigger"),
            (Gamepad, 0, "South"),
            (Gamepad, 5, "RightTrigger"),
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

    /// Left and right Alt may never collapse into one name again. The legacy
    /// chart numbers both Alt keys `18`, so that one code has to reach both
    /// sides; a table that does name a side keeps it.
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
        // Every other code still names exactly one image.
        assert_eq!(legacy_key_names(Standard, 0x41), vec!["KeyA"]);
        assert_eq!(legacy_key_names(Gamepad, 10), vec!["DPadLeft"]);
        assert!(legacy_key_names(Standard, 1).is_empty());
        assert!(
            legacy_key_names(Gamepad, 0x12).is_empty(),
            "gamepad control codes are button indexes, not virtual keys"
        );
    }

    /// The planned output for one ambiguous Alt binding: both sides are
    /// destinations for the same composed overlay, and each side-specific code
    /// still produces exactly one image.
    #[test]
    fn one_shared_alt_binding_installs_the_same_overlay_for_both_sides() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[18],[164],[165]],"keyboard":[[18],[164],[165]]}"#,
            )],
            true,
        );
        for index in 0..3 {
            write(
                root.path(),
                &format!("img/standard/hand/{index}.png"),
                &flat([255, 0, 0, 255]),
            );
            write(
                root.path(),
                &format!("img/standard/keyboard/{index}.png"),
                &flat([0, 0, 255, 255]),
            );
        }

        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        assert_eq!(
            mode.slots
                .iter()
                .map(|slot| slot.reference.as_str())
                .collect::<Vec<_>>(),
            vec!["left-keys/AltLeft.png", "left-keys/AltRight.png",]
        );
        // The shared code expands first, so the two side images carry the same
        // overlay bytes and the later per-side bindings are duplicates.
        assert_eq!(mode.slots[0].image, mode.slots[1].image);

        let staging = tempdir().expect("staging");
        convert(root.path(), &mode, staging.path()).expect("convert");
        for name in ["AltLeft", "AltRight"] {
            assert!(
                staging
                    .path()
                    .join(format!("resources/left-keys/{name}.png"))
                    .is_file(),
                "{name} must be installed"
            );
        }
    }

    #[test]
    fn the_gamepad_mode_produces_the_shipped_gamepad_vocabulary() {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Gamepad,
                r#"{"lefthand":[[10],[4]],"righthand":[[0],[5]],"keyboard":[[10],[4],[0],[5]]}"#,
            )],
            true,
        );
        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Gamepad);
        assert_eq!(
            mode.slots
                .iter()
                .map(|slot| slot.reference.as_str())
                .collect::<Vec<_>>(),
            vec![
                "left-keys/DPadLeft.png",
                "left-keys/LeftTrigger.png",
                "right-keys/South.png",
                "right-keys/RightTrigger.png",
            ]
        );
    }
}
