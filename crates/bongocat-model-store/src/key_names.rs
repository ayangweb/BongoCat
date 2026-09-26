//! Key-image names a BongoCat package may ship under a pre-rename spelling.
//!
//! The old `rdev`-based input layer spelled the two Alt keys `Alt` and `AltGr`,
//! and every model authored against it — including the models this product
//! shipped before the rename — stores its artwork under those names. The same
//! input layer spelled the main Enter key `Return`, the Fn / globe key
//! `Function` and the backslash key `Backslash`, and community models in the
//! wild may still ship all three. The runtime resolves a pressed key against
//! canonical HID names (`AltLeft`/`AltRight`, `Enter`, `Globe`, `BackSlash`,
//! see `bongocat-live2d-render::key_name_candidates`), so an imported package
//! has to speak the same vocabulary. This module rewrites the legacy stems while
//! the package is still the store's own staging copy, which is why an import is
//! the only place that can do it: the user's source folder is a read-only input
//! and is never modified.
//!
//! The gamepad family is renamed for the same reason and with a sharper edge. Its
//! stems came from the old Tauri input layer, which derived a model image name
//! from `format!("{:?}", Button)` of the third-party gamepad library, so a
//! package carries that library's control names: `DPadUp` … `DPadRight`,
//! `LeftTrigger` / `RightTrigger` for the two *shoulder* buttons,
//! `LeftTrigger2` / `RightTrigger2` for the two analog triggers and
//! `LeftThumb` / `RightThumb` for the stick clicks. They become the product's
//! own names (`GamepadButton::key_image_name`): `DpadUp` …, `LeftShoulder` /
//! `RightShoulder`, `LeftTrigger` / `RightTrigger`, `LeftStick` / `RightStick`.
//! The four face buttons and the two menu buttons are spelled the same in both
//! vocabularies and are never touched.
//!
//! A shoulder button and an analog trigger are two different buttons, so this
//! table cannot be mirrored as a runtime alias list: under the legacy spelling
//! the shoulder already owned the stem `LeftTrigger`, so any alias list would
//! resolve one of the two buttons to the other's artwork. Rewriting the
//! package's own stems is the only safe answer, and it is what makes an imported
//! gamepad model drawable at all. For the same reason the runtime keeps every
//! keyboard legacy stem as a last-resort candidate (a hand-copied model still
//! draws `AltGr.png`) and deliberately keeps no gamepad one.
//!
//! `Backslash` → `BackSlash` and `DPadUp` → `DpadUp` are the entries that change
//! only case. A direct `rename` is a no-op on a case-insensitive filesystem —
//! the source and the destination are the same file — so those entries go
//! through a temporary sibling name and really do change their spelling on every
//! platform instead of only on a case-sensitive one. The keyboard runtime alias
//! additionally makes such a package draw.

use crate::store::{ModelStoreDiagnostic, ModelStoreError};
use std::{collections::BTreeSet, fs, path::Path};

/// The directories a package keeps its per-key images in, relative to its root.
const KEY_IMAGE_DIRECTORIES: [&str; 2] = ["resources/left-keys", "resources/right-keys"];

/// The pre-rename key-image stems and the canonical name that replaces each.
///
/// `Function` is `rdev`'s name for the Fn / globe key (`Key::Function`, macOS
/// keycode 63), and the old model store drew whatever `Function.png` a package
/// carried for that key, so the semantics carry over unchanged. It is
/// deliberately not the function-row fallback `Fn`: that stem means the shared
/// F1 … F24 artwork in every model ever authored, and giving one stem two
/// meanings is exactly what the `Globe` name avoids.
///
/// The order matters inside the gamepad half. `LeftTrigger` is a legacy name of
/// the *shoulder* button and the canonical name of the analog *trigger* button,
/// so the shoulder has to be promoted off that stem before `LeftTrigger2` is
/// promoted onto it. Walking the list in this order is what lets one import
/// rewrite a fully legacy package's four trigger and shoulder images correctly.
const LEGACY_KEY_IMAGE_NAMES: [(&str, &str); 15] = [
    ("Alt", "AltLeft"),
    ("AltGr", "AltRight"),
    ("Return", "Enter"),
    ("Function", "Globe"),
    ("Backslash", "BackSlash"),
    ("LeftTrigger", "LeftShoulder"),
    ("RightTrigger", "RightShoulder"),
    ("LeftTrigger2", "LeftTrigger"),
    ("RightTrigger2", "RightTrigger"),
    ("LeftThumb", "LeftStick"),
    ("RightThumb", "RightStick"),
    ("DPadUp", "DpadUp"),
    ("DPadDown", "DpadDown"),
    ("DPadLeft", "DpadLeft"),
    ("DPadRight", "DpadRight"),
];

/// Where the gamepad half of [`LEGACY_KEY_IMAGE_NAMES`] starts.
///
/// A slice, not a second table: the walk order and the published vocabulary have
/// to be the same rows, and one table is the only way to keep them that way.
const LEGACY_GAMEPAD_TABLE_START: usize = 5;

/// The legacy gamepad stems and the product name each one becomes.
///
/// A slice of [`LEGACY_KEY_IMAGE_NAMES`] rather than a second table, so the walk
/// order and the published vocabulary are the same rows. The contract test below
/// pins it against `GamepadButton::key_image_name()`: a legacy name that is not a
/// product name, or a product name that no legacy stem maps to, means some
/// package's artwork becomes unreachable.
fn legacy_gamepad_key_image_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    LEGACY_KEY_IMAGE_NAMES[LEGACY_GAMEPAD_TABLE_START..]
        .iter()
        .copied()
}

/// Whether `stem` is a gamepad name that only the legacy vocabulary uses.
///
/// A stem another entry promotes *to* is a canonical name as well — `LeftTrigger`
/// is the legacy name of the shoulder button and the canonical name of the
/// analog trigger — and a directory that already speaks the canonical vocabulary
/// must not have it moved. `LeftTrigger2`, `LeftThumb` and `DPadUp` have no such
/// second meaning, so their presence in a directory is what identifies the whole
/// directory as legacy-named.
fn is_legacy_only_gamepad_stem(stem: &str) -> bool {
    let names = legacy_gamepad_key_image_names().collect::<Vec<_>>();
    let is_legacy = names.iter().any(|(legacy, _)| *legacy == stem);
    let is_canonical = names.iter().any(|(_, canonical)| *canonical == stem);
    is_legacy && !is_canonical
}

/// Whether `directory` carries a stem that only the legacy gamepad vocabulary
/// uses, and therefore speaks that vocabulary as a whole.
///
/// A package is rewritten as a unit or not at all. A package already written in
/// the product's names also ships `LeftTrigger.png` for the analog trigger, and
/// promoting that to `LeftShoulder.png` would hand the trigger's artwork to the
/// shoulder button. The four face buttons and the two menu buttons cannot
/// decide this either way, because both vocabularies spell them the same.
///
/// `entries` is the directory's exact file stems, not a path probe: neither
/// Windows nor macOS distinguishes case in a path, so probing for `DPadUp.png`
/// succeeds in a directory that ships the canonical `DpadUp.png`, and every
/// package would be classified as legacy.
fn speaks_legacy_gamepad_names(entries: &BTreeSet<String>) -> bool {
    legacy_gamepad_key_image_names()
        .any(|(legacy, _)| is_legacy_only_gamepad_stem(legacy) && entries.contains(legacy))
}

/// The exact file stems of the regular `.png` files directly inside `directory`.
///
/// One read per key directory per import. It is the only way to compare a
/// package's vocabulary on a case-insensitive filesystem, and the renderer
/// already lists the same two directories once per model activation.
fn key_image_entries(directory: &Path) -> BTreeSet<String> {
    let Ok(read) = fs::read_dir(directory) else {
        return BTreeSet::new();
    };
    read.filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        })
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .collect()
}

/// Rename every pre-rename key image inside `root` to its canonical name.
///
/// Only a regular file directly inside a key-image directory is considered: the
/// names are package-level vocabulary, not a search pattern, so a directory
/// called `Alt.png` or a nested path is left to package validation, which
/// rejects both on its own terms. Each directory is listed once and the listing
/// is carried across the walk, because the decision of *which* files a package
/// has has to be made on exact names — see [`speaks_legacy_gamepad_names`].
///
/// A canonical file already present wins and the legacy file is left alone: the
/// package then ships both spellings, the runtime prefers the canonical one,
/// and the model keeps working. Guessing which of the two files the author
/// meant is not this function's call to make.
pub(crate) fn normalize_legacy_key_image_names(root: &Path) -> Result<(), ModelStoreError> {
    for directory in KEY_IMAGE_DIRECTORIES {
        let directory = root.join(directory);
        let mut entries = key_image_entries(&directory);
        let legacy_gamepad = speaks_legacy_gamepad_names(&entries);
        for (legacy, canonical) in LEGACY_KEY_IMAGE_NAMES {
            if !entries.contains(legacy) {
                continue;
            }
            if !legacy_gamepad && is_gamepad_legacy_name(legacy) {
                continue;
            }
            rename_legacy_image(&directory, legacy, canonical, &mut entries)?;
        }
    }
    Ok(())
}

/// Whether `legacy` is one of the gamepad rows of [`LEGACY_KEY_IMAGE_NAMES`].
fn is_gamepad_legacy_name(legacy: &str) -> bool {
    legacy_gamepad_key_image_names().any(|(name, _)| name == legacy)
}

/// Move one legacy key image to its canonical stem inside `directory`.
///
/// A canonical file that is a *different* file wins and the legacy one is kept,
/// so a package shipping both spellings keeps drawing. A destination that is the
/// very same file under a different spelling — which is what a case-insensitive
/// filesystem reports for `DPadUp.png` versus `DpadUp.png` — is not a collision:
/// only the directory entry's spelling has to change, and a direct rename cannot
/// do that there, so the move goes through a temporary sibling name.
///
/// `entries` is the caller's listing of the directory and is updated to match
/// what the move really did, so a later row of the walk sees the stem a previous
/// row released.
fn rename_legacy_image(
    directory: &Path,
    legacy: &str,
    canonical: &str,
    entries: &mut BTreeSet<String>,
) -> Result<(), ModelStoreError> {
    let source = directory.join(format!("{legacy}.png"));
    let destination = directory.join(format!("{canonical}.png"));
    if is_same_file(&source, &destination) {
        rename_case_only(directory, legacy, canonical)?;
        entries.remove(legacy);
        entries.insert(canonical.to_owned());
        return Ok(());
    }
    if entries.contains(canonical) {
        return Ok(());
    }
    fs::rename(&source, &destination).map_err(|error| rename_error(legacy, canonical, error))?;
    entries.remove(legacy);
    entries.insert(canonical.to_owned());
    Ok(())
}

fn rename_case_only(
    directory: &Path,
    legacy: &str,
    canonical: &str,
) -> Result<(), ModelStoreError> {
    let source = directory.join(format!("{legacy}.png"));
    let destination = directory.join(format!("{canonical}.png"));
    // The intermediate name is not a package asset: it exists only between the
    // two renames inside the store's own staging copy, and an import abandoned
    // between them is removed by the store's abandoned-import recovery.
    let temporary = directory.join(format!(".bongocat-key-image-{legacy}.png"));
    let _ = fs::remove_file(&temporary);
    fs::rename(&source, &temporary).map_err(|error| rename_error(legacy, canonical, error))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        // Put the artwork back under the name the package still refers to, so a
        // failed import cannot also lose the file it was renaming.
        let _ = fs::rename(&temporary, &source);
        return Err(rename_error(legacy, canonical, error));
    }
    Ok(())
}

fn rename_error(legacy: &str, canonical: &str, error: std::io::Error) -> ModelStoreError {
    ModelStoreError::new(
        ModelStoreDiagnostic::IoError,
        Some(format!("{legacy}.png")),
        format!("legacy key image cannot be renamed to {canonical}.png: {error}"),
    )
}

/// Whether `left` and `right` name the same file, including the case-insensitive
/// case where a directory entry resolves to itself under a different spelling.
///
/// `canonicalize` reports the real on-disk path, so a case-insensitive filesystem
/// answers `true` for `DPadUp.png` and `DpadUp.png` and a case-sensitive one
/// answers `false` (or fails to resolve the absent name).
fn is_same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_render::GamepadButton;
    use std::collections::BTreeSet;
    use tempfile::tempdir;

    fn write(root: &Path, reference: &str, bytes: &[u8]) {
        let path = root.join(reference);
        fs::create_dir_all(path.parent().expect("reference parent")).expect("create directory");
        fs::write(path, bytes).expect("write key image");
    }

    #[test]
    fn legacy_stems_are_renamed_inside_both_key_directories() {
        let root = tempdir().expect("root");
        write(root.path(), "resources/left-keys/Alt.png", b"left");
        write(root.path(), "resources/left-keys/AltGr.png", b"right");
        write(root.path(), "resources/left-keys/Return.png", b"enter");
        write(
            root.path(),
            "resources/right-keys/Alt.png",
            b"right hand left alt",
        );
        write(root.path(), "resources/left-keys/KeyA.png", b"untouched");

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        assert!(!root.path().join("resources/left-keys/Alt.png").exists());
        assert!(!root.path().join("resources/left-keys/AltGr.png").exists());
        assert!(!root.path().join("resources/left-keys/Return.png").exists());
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/AltLeft.png")).expect("left alt"),
            b"left"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/AltRight.png")).expect("right alt"),
            b"right"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Enter.png")).expect("main enter"),
            b"enter"
        );
        assert_eq!(
            fs::read(root.path().join("resources/right-keys/AltLeft.png")).expect("right hand"),
            b"right hand left alt"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/KeyA.png")).expect("other key"),
            b"untouched"
        );
    }

    #[test]
    fn a_canonical_image_already_present_keeps_its_bytes() {
        let root = tempdir().expect("root");
        write(root.path(), "resources/left-keys/Alt.png", b"legacy");
        write(root.path(), "resources/left-keys/AltLeft.png", b"canonical");
        write(
            root.path(),
            "resources/left-keys/Return.png",
            b"legacy enter",
        );
        write(
            root.path(),
            "resources/left-keys/Enter.png",
            b"canonical enter",
        );

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        assert_eq!(
            fs::read(root.path().join("resources/left-keys/AltLeft.png")).expect("canonical"),
            b"canonical"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Alt.png")).expect("legacy kept"),
            b"legacy"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Enter.png")).expect("enter canonical"),
            b"canonical enter"
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Return.png")).expect("return kept"),
            b"legacy enter"
        );
    }

    /// The globe key's pre-rename stem is a real rename, and the function-row
    /// fallback is not touched by it.
    ///
    /// `Function` is `rdev`'s name for the Fn / globe key; `Fn` is the shared
    /// image the old layer derived for an unsupported `F<number>`, which every
    /// model authored against it carries. Renaming the wrong one would move
    /// every existing model's function-row artwork onto the globe key, so this
    /// pins which stem moves.
    #[test]
    fn the_globe_key_image_is_renamed_from_its_pre_rename_stem() {
        let root = tempdir().expect("root");
        write(root.path(), "resources/left-keys/Function.png", b"globe");
        write(root.path(), "resources/left-keys/Fn.png", b"function row");

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Globe.png")).expect("globe"),
            b"globe"
        );
        assert!(
            !root
                .path()
                .join("resources/left-keys/Function.png")
                .exists()
        );
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/Fn.png")).expect("function row"),
            b"function row"
        );
    }

    /// A case-only legacy stem has to change its actual spelling, on a
    /// case-insensitive filesystem as much as on a case-sensitive one. A direct
    /// rename is a no-op where the source and the destination are the same file,
    /// which would leave the package drawing nothing on Windows and macOS.
    #[test]
    fn a_case_only_legacy_stem_is_renamed_to_its_canonical_spelling() {
        for (legacy, canonical) in [("Backslash", "BackSlash"), ("DPadUp", "DpadUp")] {
            let root = tempdir().expect("root");
            write(
                root.path(),
                &format!("resources/left-keys/{legacy}.png"),
                b"image",
            );
            // The other D-pad direction is what tells the normalizer this
            // directory speaks the legacy gamepad vocabulary at all.
            write(root.path(), "resources/left-keys/DPadDown.png", b"image");

            normalize_legacy_key_image_names(root.path()).expect("normalize");

            assert_eq!(
                key_image_entries(&root.path().join("resources/left-keys")),
                BTreeSet::from([canonical.to_owned(), "DpadDown".to_owned()]),
                "{legacy} must become {canonical} as a real directory entry"
            );
            assert_eq!(
                fs::read(
                    root.path()
                        .join("resources/left-keys")
                        .join(format!("{canonical}.png"))
                )
                .expect("canonical image"),
                b"image"
            );
        }
    }

    /// The reported bug's own shape: a package carrying the third-party gamepad
    /// library's control names has to come out of an import speaking the
    /// product's names, with the shoulder artwork on the shoulder and the analog
    /// trigger artwork on the trigger. The two share the stem `LeftTrigger` in
    /// the legacy vocabulary, so the order of the two renames is the whole test.
    #[test]
    fn legacy_gamepad_stems_are_renamed_to_the_product_button_names() {
        let root = tempdir().expect("root");
        for (directory, names) in [
            (
                "resources/left-keys",
                &[
                    "DPadUp",
                    "DPadDown",
                    "DPadLeft",
                    "DPadRight",
                    "LeftTrigger",
                    "LeftTrigger2",
                ][..],
            ),
            (
                "resources/right-keys",
                &["South", "East", "RightTrigger", "RightTrigger2"][..],
            ),
        ] {
            for name in names {
                write(
                    root.path(),
                    &format!("{directory}/{name}.png"),
                    name.as_bytes(),
                );
            }
        }

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        // The exact entries are the assertion: `LeftTrigger` has to end up being
        // the analog trigger's file and `LeftShoulder` the shoulder's, so a pass
        // that only checked "both files exist" would accept the two artworks
        // swapped.
        assert_eq!(
            key_image_entries(&root.path().join("resources/left-keys")),
            BTreeSet::from([
                "DpadDown".to_owned(),
                "DpadLeft".to_owned(),
                "DpadRight".to_owned(),
                "DpadUp".to_owned(),
                "LeftShoulder".to_owned(),
                "LeftTrigger".to_owned(),
            ])
        );
        assert_eq!(
            key_image_entries(&root.path().join("resources/right-keys")),
            BTreeSet::from([
                "East".to_owned(),
                "RightShoulder".to_owned(),
                "RightTrigger".to_owned(),
                "South".to_owned(),
            ])
        );
        for (directory, canonical, bytes) in [
            ("left-keys", "DpadUp", b"DPadUp".as_slice()),
            ("left-keys", "LeftShoulder", b"LeftTrigger".as_slice()),
            ("left-keys", "LeftTrigger", b"LeftTrigger2".as_slice()),
            ("right-keys", "RightShoulder", b"RightTrigger".as_slice()),
            ("right-keys", "RightTrigger", b"RightTrigger2".as_slice()),
        ] {
            assert_eq!(
                fs::read(
                    root.path()
                        .join("resources")
                        .join(directory)
                        .join(format!("{canonical}.png"))
                )
                .expect(canonical),
                bytes,
                "{directory}/{canonical}.png must carry its own button's artwork"
            );
        }
        // The face buttons are spelled the same in both vocabularies.
        assert_eq!(
            fs::read(root.path().join("resources/right-keys/South.png")).expect("south"),
            b"South"
        );
    }

    /// A package already written in the product's names must not be rewritten:
    /// its `LeftTrigger.png` is the *analog trigger*, and promoting it to
    /// `LeftShoulder.png` would hand the trigger's artwork to the shoulder.
    #[test]
    fn canonical_gamepad_stems_are_left_alone() {
        let root = tempdir().expect("root");
        for name in ["LeftTrigger", "DpadUp", "LeftStick", "Start"] {
            write(
                root.path(),
                &format!("resources/left-keys/{name}.png"),
                name.as_bytes(),
            );
        }

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        assert_eq!(
            key_image_entries(&root.path().join("resources/left-keys")),
            BTreeSet::from([
                "DpadUp".to_owned(),
                "LeftStick".to_owned(),
                "LeftTrigger".to_owned(),
                "Start".to_owned(),
            ])
        );
    }

    /// Every legacy gamepad stem this module rewrites has to land on a real
    /// product button name, and no two of them may land on the same one — the
    /// shoulder/trigger pair is the case that makes this a real invariant
    /// rather than a shape check.
    #[test]
    fn every_legacy_gamepad_stem_maps_onto_a_distinct_product_button_name() {
        let product_names = GamepadButton::ALL
            .iter()
            .map(|button| button.key_image_name())
            .collect::<BTreeSet<_>>();
        let mut canonical = BTreeSet::new();
        for (legacy, name) in legacy_gamepad_key_image_names() {
            assert!(
                product_names.contains(name),
                "{legacy} maps to {name}, which is not a gamepad button's image name"
            );
            assert!(
                canonical.insert(name),
                "{name} is the target of more than one legacy stem"
            );
        }
        // And the walk order has to release a stem before another entry takes
        // it: the shoulder vacates `LeftTrigger` for the analog trigger.
        let order = legacy_gamepad_key_image_names().collect::<Vec<_>>();
        let shoulder = order
            .iter()
            .position(|(legacy, _)| *legacy == "LeftTrigger")
            .expect("shoulder row");
        let trigger = order
            .iter()
            .position(|(legacy, _)| *legacy == "LeftTrigger2")
            .expect("trigger row");
        assert!(shoulder < trigger);
    }

    #[test]
    fn a_package_without_key_directories_or_legacy_names_is_untouched() {
        let root = tempdir().expect("root");
        write(root.path(), "resources/background.png", b"background");
        normalize_legacy_key_image_names(root.path()).expect("no key directories");
        assert_eq!(
            fs::read(root.path().join("resources/background.png")).expect("background"),
            b"background"
        );

        write(root.path(), "resources/left-keys/AltLeft.png", b"canonical");
        write(
            root.path(),
            "resources/left-keys/AltGr.txt",
            b"not an image",
        );
        normalize_legacy_key_image_names(root.path()).expect("nothing to rename");
        assert_eq!(
            fs::read(root.path().join("resources/left-keys/AltGr.txt")).expect("other file"),
            b"not an image"
        );
    }
}
