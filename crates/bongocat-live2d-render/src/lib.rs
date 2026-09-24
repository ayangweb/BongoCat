#![forbid(unsafe_code)]

//! Model-package render resource preparation and key-image resolution.
//!
//! This crate owns the platform-neutral boundary between a committed model
//! package and `bongocat-render` resources. It does not load Cubism Core,
//! allocate GPU handles, or own an overlay window.

use bongocat_model::CommittedModel;
use bongocat_render::{
    BackgroundAsset, KeyAsset, KeyAssetId, KeyOverlay, KeyPressSet, KeySide, RenderResources,
    TextureAsset, TextureId,
};
use image::ImageReader;
use std::{
    collections::BTreeSet,
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct RenderResourceError {
    detail: String,
}

impl RenderResourceError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for RenderResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for RenderResourceError {}

/// Prepare the immutable resources consumed by the native renderers.
pub fn prepare_render_resources(
    model: &CommittedModel,
) -> Result<RenderResources, RenderResourceError> {
    let textures = model
        .index()
        .textures
        .iter()
        .enumerate()
        .map(|(index, texture)| TextureAsset {
            id: TextureId::new(index),
            path: model.root().join(&texture.file),
            width: texture.width,
            height: texture.height,
        })
        .collect();
    Ok(RenderResources {
        textures,
        key_assets: load_key_assets(model.root())?,
        background: load_background_asset(model.root())?,
    })
}

pub fn resolve_key_overlays(resources: &RenderResources, presses: KeyPressSet) -> Vec<KeyOverlay> {
    let mut selected = [None, None];
    for press in presses.iter() {
        let side_index = match press.side {
            KeySide::Left => 0,
            KeySide::Right => 1,
        };
        // Runtime supplies the most recently pressed key for each side. Clear
        // the slot before resolving so an unavailable current key never
        // reuses an older overlay from that side.
        selected[side_index] = None;
        let candidates = key_name_candidates(press.hid_usage);
        let Some(asset) = candidates.iter().find_map(|candidate| {
            resources
                .key_assets
                .iter()
                .find(|asset| asset.side == press.side && asset.name == *candidate)
        }) else {
            continue;
        };
        selected[side_index] = Some(KeyOverlay {
            asset_id: asset.id,
            side: press.side,
        });
    }
    selected.into_iter().flatten().collect()
}

/// The key images a model package ships, grouped by the hand that draws them.
///
/// The inventory is the same directory scan [`load_key_assets`] performs and is
/// answered against the same candidate names [`resolve_key_overlays`] walks, so
/// it decides exactly what the renderer will decide: whether a press of a key
/// has an image to show. That is what lets the product check *before* it reacts
/// to a key at all — a key whose artwork the model does not ship must not move
/// the paw either, because `CatParamLeftHandDown`/`CatParamRightHandDown`
/// without a key image on screen is feedback for something that cannot be seen
/// (`bongocat-app::input_bindings_for_model`).
///
/// Reading it never decodes an image, so building it once per model activation
/// costs no more than the directory listing the renderer does anyway.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyImageInventory {
    left: BTreeSet<String>,
    right: BTreeSet<String>,
}

impl KeyImageInventory {
    /// Read the key images `root` provides.
    ///
    /// A missing or unreadable key directory contributes no names, exactly as it
    /// contributes no assets to the renderer: the model simply draws no keys of
    /// that hand.
    pub fn read(root: &Path) -> Self {
        let mut inventory = Self::default();
        for (side, name, _) in key_image_files(root) {
            inventory.names_mut(side).insert(name);
        }
        inventory
    }

    /// Whether the model ships an image called `name` for `side`.
    pub fn provides(&self, side: KeySide, name: &str) -> bool {
        self.names(side).contains(name)
    }

    /// Whether the model can draw a press of `hid_usage` on `side`.
    ///
    /// True when any candidate of that key is an image the model ships for that
    /// side: the exact name, a shared family image (`Fn`, `Control`), a legacy
    /// alias, or the main keyboard key a keypad key duplicates. This is the same
    /// lookup `resolve_key_overlays` performs, so "can draw" and "draws" cannot
    /// disagree.
    pub fn can_draw(&self, side: KeySide, hid_usage: u16) -> bool {
        key_name_candidates(hid_usage)
            .iter()
            .any(|name| self.provides(side, name))
    }

    /// Return the image names available for one hand.
    pub fn names(&self, side: KeySide) -> &BTreeSet<String> {
        match side {
            KeySide::Left => &self.left,
            KeySide::Right => &self.right,
        }
    }

    fn names_mut(&mut self, side: KeySide) -> &mut BTreeSet<String> {
        match side {
            KeySide::Left => &mut self.left,
            KeySide::Right => &mut self.right,
        }
    }
}

/// Every key image a package provides, in the order the loader consumes them:
/// `resources/left-keys` before `resources/right-keys`, each sorted by path.
///
/// Only a regular `.png` directly inside one of those directories counts, and
/// the name is the file stem — the model author's contract with the key
/// vocabulary. [`load_key_assets`] and [`KeyImageInventory::read`] share this
/// one scan so the images a model reports and the images it draws can never
/// drift apart.
fn key_image_files(root: &Path) -> Vec<(KeySide, String, PathBuf)> {
    let mut images = Vec::new();
    for (side, directory) in [(KeySide::Left, "left-keys"), (KeySide::Right, "right-keys")] {
        let path = root.join("resources").join(directory);
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        let mut files = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|file| {
                file.is_file()
                    && file
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            })
            .collect::<Vec<_>>();
        files.sort();
        for file in files {
            let Some(name) = file.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            images.push((side, name.to_owned(), file));
        }
    }
    images
}

/// Load and dimension-check the key images shipped by a model package.
pub fn load_key_assets(root: &Path) -> Result<Vec<KeyAsset>, RenderResourceError> {
    let mut assets = Vec::new();
    for (side, name, path) in key_image_files(root) {
        let image = ImageReader::open(&path)
            .map_err(|error| RenderResourceError::new(error.to_string()))?
            .decode()
            .map_err(|error| RenderResourceError::new(error.to_string()))?;
        assets.push(KeyAsset {
            id: KeyAssetId::new(assets.len()),
            side,
            name,
            path,
            width: image.width(),
            height: image.height(),
        });
    }
    Ok(assets)
}

/// Asset names a pressed key can be drawn with, most specific first.
///
/// A model may ship one image per key or a single shared image for a whole key
/// family. The HID function keys F1 … F24 therefore resolve to their own
/// `F1.png` … `F24.png` when the model provides one and fall back to the shared
/// `Fn.png` otherwise; the keypad block falls back to the main keyboard key each
/// of its keys duplicates (see below); every other key only ever has an exact
/// name plus, for the four modifier pairs, the shared side-independent asset
/// (`Control`, `Shift`, `Alt`, `Meta`). A name that the model does not provide is
/// skipped, so an incomplete model simply draws nothing for that key.
///
/// **`Fn` is the name of that shared function-row image, not of the Fn key.**
/// The old `rdev`-based input layer derived it by rewriting an unsupported
/// `F<number>` to `Fn`, so every model authored against it — including both
/// shipped keyboard presets — stores its shared function-row artwork under that
/// stem. The physical Fn / globe key is a different key with a different name
/// (`Globe`), and the two share no candidate: a model's `Fn.png` can never be
/// drawn by the globe key, and its `Globe.png` can never be drawn by a function
/// key.
///
/// The table covers the whole standard 104/105-key layout plus the keypad, not
/// just the keys the shipped models draw. A name is a contract with model
/// authors: `Dot`, `Minus`, `Insert` and the rest ship no artwork today, but a
/// model that provides one has to be honoured without a product change, and it
/// cannot be if the name does not exist. Every physical key the platform
/// adapters can report is therefore named here. A name only becomes reachable if
/// the runtime also assigns that key a hand
/// (`bongocat-app::input_bindings_for_model`), so the two tables cover the same
/// set.
///
/// `AltGr`, `Return` and `Function` are the legacy names in this table. BongoCat
/// models written before the import normalizer existed ship the right Alt
/// artwork as `AltGr.png` and the main Enter artwork as `Return.png` — the names
/// the old `rdev`-based input layer used — and a package that reaches the model
/// store without passing through that normalizer (an install predating it, or a
/// model directory placed by hand) still has to draw them. `AltGr` is
/// deliberately right-Alt-only: `Alt.png` stays the shared family image, exactly
/// as it was before. `Return` is the old spelling of the main Enter key (HID
/// `0x28`) and ranks after the canonical `Enter`, so both spellings can never
/// disagree on which artwork a key means. `Function` is the same treatment for
/// the globe key: `rdev`'s name for that key, ranked after the canonical
/// `Globe`.
///
/// The keypad block (HID `0x53` … `0x63`) carries the `Kp*` vocabulary that the
/// Mver conversion has always emitted, plus `NumLock`, and falls back to the
/// main keyboard key it duplicates. Every keypad key that produces the same
/// character as a main keyboard key falls back to it — `Kp1` … `Kp9`, `Kp0` to
/// the number row's `Num1` … `Num9`, `Num0`; `KpEnter` to `Enter`; `KpDivide` to
/// `Slash`; `KpMinus` to `Minus`; `KpDecimal` to `Dot` — so a keypad key no model
/// ever drew still draws something whenever its counterpart has artwork.
/// `NumLock`, `KpMultiply` and `KpPlus` have no counterpart at all: `*` and `+`
/// are reachable on the main keyboard only through `Shift`, so they keep their
/// exact name and draw nothing until a model provides artwork of its own. The
/// keypad is the left hand's cluster for the same reason — `left-keys` is the
/// directory the shared digit, Enter and Slash artwork lives in, and the runtime
/// resolves an overlay against the pressed side only.
/// Return the ordered artwork names that can satisfy a HID key press.
pub fn key_name_candidates(hid_usage: u16) -> Vec<&'static str> {
    let function_key = bongocat_render::function_key_name(hid_usage);
    let exact = match hid_usage {
        0x04..=0x1d => Some(KEY_LETTERS[usize::from(hid_usage - 0x04)]),
        0x1e..=0x27 => Some(KEY_NUMBERS[usize::from(hid_usage - 0x1e)]),
        0x28 => Some("Enter"),
        0x58 => Some("KpEnter"),
        0x29 => Some("Escape"),
        0x2a => Some("Backspace"),
        0x2b => Some("Tab"),
        0x2c => Some("Space"),
        // The punctuation block, in HID order. None of these keys ships artwork
        // today, and that is deliberately not a reason to leave them unnamed:
        // the vocabulary is a contract with model authors, not a list of the
        // images the shipped models happen to carry. A model that provides
        // `Dot.png` or `Minus.png` has to work without a product change, and it
        // cannot if the name does not exist.
        0x2d => Some("Minus"),
        0x2e => Some("Equal"),
        0x2f => Some("LeftBracket"),
        0x30 => Some("RightBracket"),
        0x31 => Some("BackSlash"),
        0x32 => Some("IntlHash"),
        0x33 => Some("SemiColon"),
        0x34 => Some("Quote"),
        0x35 => Some("BackQuote"),
        0x36 => Some("Comma"),
        0x37 => Some("Dot"),
        0x38 => Some("Slash"),
        0x39 => Some("CapsLock"),
        // PrintScreen, ScrollLock, Pause and the navigation cluster. PrintScreen
        // is not a function key — `FUNCTION_KEY_USAGES` deliberately stops at
        // `0x45` and resumes at `0x68` — but it is still a key, with its own name
        // and hand like every other one.
        0x46 => Some("PrintScreen"),
        0x47 => Some("ScrollLock"),
        0x48 => Some("Pause"),
        0x49 => Some("Insert"),
        0x4a => Some("Home"),
        0x4b => Some("PageUp"),
        0x4c => Some("Delete"),
        0x4d => Some("End"),
        0x4e => Some("PageDown"),
        0x4f => Some("RightArrow"),
        0x50 => Some("LeftArrow"),
        0x51 => Some("DownArrow"),
        0x52 => Some("UpArrow"),
        // The keypad block, minus `0x58`: keypad Enter sits with the main Enter
        // above because the two are one key split in two.
        0x53 => Some("NumLock"),
        0x54 => Some("KpDivide"),
        0x55 => Some("KpMultiply"),
        0x56 => Some("KpMinus"),
        0x57 => Some("KpPlus"),
        0x59..=0x62 => Some(KEYPAD_DIGIT_NAMES[usize::from(hid_usage - 0x59)]),
        0x63 => Some("KpDecimal"),
        // ISO layouts carry an extra key beside the left Shift, and macOS reports
        // the keypad `=` as a usage of its own.
        0x64 => Some("IntlBackslash"),
        0x65 => Some("Apps"),
        0x67 => Some("KpEqual"),
        0xe0 => Some("ControlLeft"),
        0xe1 => Some("ShiftLeft"),
        0xe2 => Some("AltLeft"),
        0xe3 => Some("MetaLeft"),
        0xe4 => Some("ControlRight"),
        0xe5 => Some("ShiftRight"),
        0xe6 => Some("AltRight"),
        0xe7 => Some("MetaRight"),
        // The Apple Fn / globe key — the one key in this table that is not on
        // the HID Keyboard/Keypad page. Its usage folds Apple's vendor page into
        // the same `u16` (see `bongocat_render::GLOBE_KEY_USAGE`), so no range
        // over page `0x07` can ever reach it and it has to be named explicitly
        // here and bound explicitly in `bongocat-app`.
        bongocat_render::GLOBE_KEY_USAGE => Some("Globe"),
        // Function keys are the only named keys left, and the whole HID range is
        // covered by one arithmetic lookup instead of 24 arms here.
        _ => function_key,
    };
    let mut candidates = Vec::with_capacity(2);
    if let Some(exact) = exact {
        candidates.push(exact);
    }
    if function_key.is_some() {
        // The model's shared function-key image, and always the last candidate:
        // a dedicated `F1.png` … `F24.png` wins, every function key the model
        // did not draw individually lands on `Fn.png`.
        candidates.push("Fn");
    }
    if hid_usage == bongocat_render::GLOBE_KEY_USAGE {
        // The globe key's pre-rename name, and always the last candidate. The
        // old `rdev`-based input layer spelled this key `Function`
        // (`Key::Function`, macOS keycode 63), and a model that shipped
        // `Function.png` did draw it: the model store's key set was the
        // package's own file stems, so the name was reachable even though no
        // shipped model ever carried the artwork. Ranking it after `Globe`
        // keeps a normalised package and an already-installed one agreeing on
        // which image the key means.
        //
        // Deliberately not `Fn`: that name is the F1 … F24 fallback above, and
        // one image name may only ever carry one meaning. This is the whole
        // reason the key is `Globe` and not `Fn`.
        candidates.push("Function");
    }
    // Pre-rename spellings of a canonical name, always ranked after it so a
    // package that still carries the old file and one that carries the new one
    // can never disagree about which artwork a key means. Both are needed for
    // already-installed models: an import rewrites the package's own staging
    // copy, but a store entry that predates the normalizer is never rewritten
    // in place (`next` has no migration path).
    match hid_usage {
        // The main Enter key: community models in the wild ship `Return.png`.
        0x28 => candidates.push("Return"),
        // `BackSlash` is spelled `Backslash` by the legacy converter's key
        // table and by `rdev`. The two differ only in case, and neither macOS
        // nor Windows distinguishes case in a path, so a package that ships the
        // old spelling keeps it — `KeyImageInventory::provides` compares the
        // file's real stem, which is why only this candidate can make it draw.
        0x31 => candidates.push("Backslash"),
        _ => {}
    }
    match hid_usage {
        // Keypad fallbacks: the exact `Kp*` name above wins when the model drew
        // that key, and the main keyboard key the keypad key duplicates draws
        // otherwise. A fallback only exists where the keypad key produces the
        // same character as a named main keyboard key — `.` and `-` have main
        // keyboard keys of their own, while `*` and `+` are reachable only
        // through `Shift` and `NumLock` has no counterpart at all. Always the
        // last candidate.
        0x54 => candidates.push("Slash"),
        0x56 => candidates.push("Minus"),
        0x58 => candidates.push("Enter"),
        0x59..=0x62 => {
            // `Kp1` … `Kp9`, `Kp0` duplicate the number row's `Num1` … `Num9`,
            // `Num0`. `KEY_NUMBERS` is ordered `1` … `9`, `0`, the same order as
            // `KEYPAD_DIGIT_NAMES`, so one index drives both tables.
            candidates.push(KEY_NUMBERS[usize::from(hid_usage - 0x59)]);
        }
        0x63 => candidates.push("Dot"),
        0xe0 | 0xe4 => candidates.push("Control"),
        0xe1 | 0xe5 => candidates.push("Shift"),
        0xe2 => candidates.push("Alt"),
        0xe6 => {
            // Right Alt keeps its pre-rename name as an alias between the exact
            // `AltRight` and the shared `Alt`: a legacy model draws its own
            // right artwork when it has one, and the family image otherwise.
            candidates.push("AltGr");
            candidates.push("Alt");
        }
        0xe3 | 0xe7 => candidates.push("Meta"),
        _ => {}
    }
    candidates
}

const KEY_LETTERS: [&str; 26] = [
    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL",
    "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX",
    "KeyY", "KeyZ",
];
const KEY_NUMBERS: [&str; 10] = [
    "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9", "Num0",
];
/// Keypad asset names for the ten digits, in the same HID and artwork order as
/// [`KEY_NUMBERS`]: `Kp1` … `Kp9`, `Kp0`.
///
/// The Mver conversion has emitted the `Kp*` vocabulary since ADR-0037; the two
/// tables line up index for index so a keypad digit's exact name and the number
/// row key it duplicates come from a single offset.
const KEYPAD_DIGIT_NAMES: [&str; 10] = [
    "Kp1", "Kp2", "Kp3", "Kp4", "Kp5", "Kp6", "Kp7", "Kp8", "Kp9", "Kp0",
];

/// Load the optional background image shipped by a model package.
pub fn load_background_asset(root: &Path) -> Result<Option<BackgroundAsset>, RenderResourceError> {
    let path = root.join("resources/background.png");
    if !path.is_file() {
        return Ok(None);
    }
    let image = ImageReader::open(&path)
        .map_err(|error| RenderResourceError::new(error.to_string()))?
        .decode()
        .map_err(|error| RenderResourceError::new(error.to_string()))?;
    Ok(Some(BackgroundAsset {
        path,
        width: image.width(),
        height: image.height(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::Path;

    fn preset_model(id: &str) -> CommittedModel {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
        PresetModelCatalog::open(root, ModelPackageLimits::default())
            .expect("preset model catalog")
            .load(&ModelId::parse(id).expect("model id"))
            .expect("preset model")
    }

    #[test]
    fn every_preset_model_prepares_render_resources() {
        for id in ["standard", "keyboard", "gamepad"] {
            let resources = prepare_render_resources(&preset_model(id)).expect("render resources");
            assert!(!resources.textures.is_empty());
        }
    }

    #[test]
    fn key_overlay_resolution_uses_the_render_resource_contract() {
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: vec![
                KeyAsset {
                    id: KeyAssetId::new(0),
                    side: KeySide::Left,
                    name: "KeyA".to_owned(),
                    path: PathBuf::from("KeyA.png"),
                    width: 1,
                    height: 1,
                },
                KeyAsset {
                    id: KeyAssetId::new(1),
                    side: KeySide::Right,
                    name: "Meta".to_owned(),
                    path: PathBuf::from("Meta.png"),
                    width: 1,
                    height: 1,
                },
            ],
            background: None,
        };
        let mut presses = KeyPressSet::default();
        presses.push(bongocat_render::KeyPress {
            hid_usage: 0x04,
            side: KeySide::Left,
        });
        presses.push(bongocat_render::KeyPress {
            hid_usage: 0xe7,
            side: KeySide::Right,
        });
        assert_eq!(
            resolve_key_overlays(&resources, presses),
            vec![
                KeyOverlay {
                    asset_id: KeyAssetId::new(0),
                    side: KeySide::Left,
                },
                KeyOverlay {
                    asset_id: KeyAssetId::new(1),
                    side: KeySide::Right,
                },
            ]
        );
    }

    #[test]
    fn key_inventory_reports_only_shipped_artwork() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models/standard");
        let inventory = KeyImageInventory::read(&root);
        assert!(inventory.can_draw(KeySide::Left, 0x04));
        assert!(!inventory.can_draw(KeySide::Left, 0x2d));
    }
}
