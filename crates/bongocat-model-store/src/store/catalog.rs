//! The installed models the store holds.
//!
//! Listing is deliberately forgiving: an entry the store does not recognise, or
//! a package that no longer parses, is reported as such and the rest of the
//! catalogue stays usable. One corrupt directory must not make every installed
//! model disappear from the settings window.

use super::*;

/// Outcome of scanning the installed model store.
///
/// The store root is application-owned, but it is still an ordinary directory
/// on the user's disk: file managers drop metadata beside the model folders and
/// a user can leave unrelated files behind. A single unrecognized entry must
/// never make the whole catalog unavailable, so such entries are dropped during
/// the scan and only counted here instead of failing it. Platform metadata is
/// not even counted: the operating system or file manager owns it and it can
/// never be a model.
///
/// The count is internal store state. Filtering is silent by design, so it is
/// never projected into the settings snapshot, user-facing text or logging.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstalledModelCatalog {
    pub entries: Vec<ModelCatalogEntry>,
    pub skipped_entries: usize,
}

/// File-manager and operating-system metadata that legitimately appears in the
/// store root without being owned by the catalog.
pub(crate) fn is_platform_metadata_name(name: &str) -> bool {
    // AppleDouble sidecars are written next to files on non-native volumes.
    name.starts_with("._")
        || matches!(
            name,
            ".DS_Store" | ".localized" | "Thumbs.db" | "desktop.ini"
        )
}
