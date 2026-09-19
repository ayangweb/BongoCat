//! Key-image names a BongoCat package may ship under a pre-rename spelling.
//!
//! The old `rdev`-based input layer spelled the two Alt keys `Alt` and `AltGr`,
//! and every model authored against it — including the models this product
//! shipped before the rename — stores its artwork under those names. The same
//! input layer spelled the main Enter key `Return`, the Fn / globe key
//! `Function` and the backslash key `Backslash`, and community models in the
//! wild may still ship all three. The runtime resolves a pressed key against
//! canonical HID names (`AltLeft`/`AltRight`, `Enter`, `Globe`, `BackSlash`,
//! see `bongocat-live2d::key_name_candidates`), so an imported package has to
//! speak the same vocabulary. This module rewrites the legacy stems while the
//! package is still the store's own staging copy, which is why an import is
//! the only place that can do it: the user's source directory and archive are
//! read-only inputs and are never modified.
//!
//! The runtime additionally keeps every legacy stem as a last-resort candidate,
//! so a model that reaches the store without this rewrite (an install that
//! predates it, or a directory placed by hand) still draws the right artwork.
//!
//! `Backslash` → `BackSlash` is the one entry that changes only case, and
//! neither macOS nor Windows distinguishes case in a path: `destination.exists()`
//! is already true for the very file being renamed, so the rename is skipped
//! and the package keeps the old spelling. The runtime alias is what makes that
//! package draw; the rewrite only takes effect on a case-sensitive filesystem.

use crate::store::{ModelStoreDiagnostic, ModelStoreError};
use std::{fs, path::Path};

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
const LEGACY_KEY_IMAGE_NAMES: [(&str, &str); 5] = [
    ("Alt", "AltLeft"),
    ("AltGr", "AltRight"),
    ("Return", "Enter"),
    ("Function", "Globe"),
    ("Backslash", "BackSlash"),
];

/// Rename every pre-rename key image inside `root` to its canonical name.
///
/// Only a regular file directly inside a key-image directory is considered: the
/// names are package-level vocabulary, not a search pattern, so a directory
/// called `Alt.png` or a nested path is left to package validation, which
/// rejects both on its own terms.
///
/// A canonical file already present wins and the legacy file is left alone: the
/// package then ships both spellings, the runtime prefers the canonical one,
/// and the model keeps working. Guessing which of the two files the author
/// meant is not this function's call to make.
pub(crate) fn normalize_legacy_key_image_names(root: &Path) -> Result<(), ModelStoreError> {
    for directory in KEY_IMAGE_DIRECTORIES {
        let directory = root.join(directory);
        for (legacy, canonical) in LEGACY_KEY_IMAGE_NAMES {
            let resource = format!("{legacy}.png");
            let source = directory.join(&resource);
            if !is_regular_file(&source) {
                continue;
            }
            let destination = directory.join(format!("{canonical}.png"));
            if destination.exists() {
                continue;
            }
            fs::rename(&source, &destination).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(resource),
                    format!("legacy key image cannot be renamed to {canonical}.png: {error}"),
                )
            })?;
        }
    }
    Ok(())
}

/// Whether `path` is an existing regular file, without following symlinks.
///
/// A source package cannot contain a symlink — package validation rejects it
/// before a copy is ever made — so treating one as "absent" here cannot hide a
/// symlink that validation would have accepted.
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// `Backslash` → `BackSlash` is the one entry that changes only case.
    ///
    /// Neither macOS nor Windows distinguishes case in a path, so the
    /// destination *is* the source file and the rename is skipped — the package
    /// keeps the old spelling. What has to hold is that the canonical name still
    /// reaches the artwork; the runtime's candidate alias is what makes such a
    /// package draw at all, and the cross-table contract in `bongocat-live2d`
    /// is what pins the two spellings together.
    #[test]
    fn a_case_only_legacy_stem_keeps_its_artwork_under_the_canonical_name() {
        let root = tempdir().expect("root");
        write(root.path(), "resources/left-keys/Backslash.png", b"slash");

        normalize_legacy_key_image_names(root.path()).expect("normalize");

        let canonical = root.path().join("resources/left-keys/BackSlash.png");
        assert_eq!(fs::read(&canonical).expect("canonical backslash"), b"slash");
        assert!(is_regular_file(&canonical));
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
