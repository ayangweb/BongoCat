//! The replacement cover images of the models the build ships.
//!
//! A preset's package lives inside the application bundle — a signed `.app` on
//! macOS, the installation directory on Windows — and the product may not write
//! there. A replacement cover therefore cannot live beside the package it
//! belongs to the way an installed model's cover does, so it is kept on the
//! user side, in its own root, in exactly the per-model shape a package uses:
//! `<root>/<id>/resources/cover.png`.
//!
//! That shape is the point of this module. The settings page resolves a cover
//! from either origin through one layout constant ([`package_cover_path`]), so
//! a replacement and a package's own artwork are the same thing to every reader;
//! only the root differs. Nothing else about a preset changes: the package, its
//! model data and its key artwork stay where the build put them.

use crate::{ModelStoreDiagnostic, ModelStoreError};
use bongocat_model::{
    ModelId, PACKAGE_COVER_FILE, PACKAGE_RESOURCES_DIRECTORY, package_cover_path,
};
use bongocat_storage::{create_private_dir_all, write_private_atomic};
use std::{fs, path::PathBuf};

/// The covers a user chose for the models the build ships.
///
/// No writer lock is taken. Every write here is a single file replaced by an
/// atomic rename, so the worst a concurrent write can do is decide which of two
/// complete covers wins; there is no multi-file invariant to protect, and a
/// reader can never observe a half-written image.
pub struct PresetCoverStore {
    root: PathBuf,
}

impl PresetCoverStore {
    /// Open (and create) the user-side root for preset covers.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ModelStoreError> {
        let root = root.into();
        create_private_dir_all(&root).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("preset cover root cannot be created: {error}"),
            )
        })?;
        let root = root.canonicalize().map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                format!("preset cover root cannot be opened: {error}"),
            )
        })?;
        if !root.is_dir() {
            return Err(ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                None,
                "preset cover root is not a directory",
            ));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Where a preset's replacement cover is kept, whether or not one exists.
    ///
    /// The path is the same one a package would use, under this store's root
    /// instead of the package's.
    pub fn cover_path(&self, id: &ModelId) -> PathBuf {
        package_cover_path(&self.root.join(id.as_str()))
    }

    /// Replace a preset's cover with PNG bytes, and answer with the path.
    ///
    /// The caller owns the format decision, exactly as it does for an installed
    /// model: the bytes are stored verbatim, and the replace is atomic so a
    /// failure leaves the previous cover in place.
    pub fn replace_cover(&self, id: &ModelId, bytes: &[u8]) -> Result<PathBuf, ModelStoreError> {
        // Each level is created privately on purpose: `create_dir_all` applies
        // the process umask to parents, so creating `<id>/resources` in one call
        // would leave `<id>` readable by everyone while the cover inside it is
        // not.
        let directory = self.root.join(id.as_str());
        let resources = directory.join(PACKAGE_RESOURCES_DIRECTORY);
        for level in [&directory, &resources] {
            create_private_dir_all(level).map_err(|error| {
                ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    Some(id.as_str().to_owned()),
                    format!("preset cover directory cannot be created: {error}"),
                )
            })?;
        }
        // A replacement that fails after the directory was created leaves an
        // empty one behind. That is harmless and deliberately not cleaned up:
        // the only question a reader asks is whether the cover *file* is there
        // (see [`preset_cover_exists`]), so an empty directory reads as "this
        // preset has not been customised".
        let cover = resources.join(PACKAGE_COVER_FILE);
        write_private_atomic(&cover, bytes).map_err(|error| {
            ModelStoreError::new(
                ModelStoreDiagnostic::IoError,
                Some(id.as_str().to_owned()),
                format!("preset cover cannot be replaced: {error}"),
            )
        })?;
        Ok(cover)
    }
}

/// Whether a replacement cover exists at `path`.
///
/// Read through the same definition of the layout the writer uses, so a caller
/// that only needs to know whether a preset has been customised does not have
/// to rebuild the path.
pub fn preset_cover_exists(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_replacement_lands_on_the_package_layout_and_leaves_no_leftovers() {
        let base = tempdir().expect("data root");
        let store = PresetCoverStore::open(base.path().join("model-overrides")).expect("open");
        let id = ModelId::parse("standard").expect("model id");
        let bytes = b"\x89PNG\r\n\x1a\nreplacement".to_vec();

        let cover = store.replace_cover(&id, &bytes).expect("replace cover");

        assert_eq!(cover, store.root().join("standard/resources/cover.png"));
        assert_eq!(cover, package_cover_path(&store.root().join("standard")));
        assert_eq!(fs::read(&cover).expect("stored cover"), bytes);
        assert!(preset_cover_exists(&cover));
        let leftovers = fs::read_dir(cover.parent().expect("resources"))
            .expect("resources directory")
            .map(|entry| entry.expect("entry").file_name())
            .filter(|name| name.to_string_lossy().ends_with(".new"))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn a_preset_without_a_replacement_reports_no_cover() {
        let base = tempdir().expect("data root");
        let store = PresetCoverStore::open(base.path().join("model-overrides")).expect("open");
        let id = ModelId::parse("standard").expect("model id");

        assert!(!preset_cover_exists(&store.cover_path(&id)));
        assert!(
            !store.root().join(id.as_str()).exists(),
            "reading a preset's cover must not create anything"
        );
    }

    #[test]
    fn a_second_replacement_replaces_the_first_one() {
        let base = tempdir().expect("data root");
        let store = PresetCoverStore::open(base.path().join("model-overrides")).expect("open");
        let id = ModelId::parse("standard").expect("model id");

        store.replace_cover(&id, b"first").expect("first cover");
        let cover = store.replace_cover(&id, b"second").expect("second cover");

        assert_eq!(fs::read(&cover).expect("stored cover"), b"second");
    }

    #[cfg(unix)]
    #[test]
    fn preset_cover_data_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempdir().expect("data root");
        let store = PresetCoverStore::open(base.path().join("model-overrides")).expect("open");
        let id = ModelId::parse("standard").expect("model id");
        let cover = store.replace_cover(&id, b"bytes").expect("cover");

        for (path, mode) in [
            (store.root().to_owned(), 0o700),
            (store.root().join("standard"), 0o700),
            (store.root().join("standard/resources"), 0o700),
            (cover, 0o600),
        ] {
            assert_eq!(
                fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
                mode,
                "{path:?}"
            );
        }
    }
}
