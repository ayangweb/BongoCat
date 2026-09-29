//! The one accepted error shape.
//!
//! A code plus, where one helps, the underlying message. The code is what the
//! settings window maps to a localized string, so it is stable and it is a closed
//! list — adding one is a change to six locale files, which is the right friction
//! for an error a user will read.

use serde::{Deserialize, Serialize};

/// A plugin failure, named by a stable code.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginError {
    pub code: PluginErrorCode,
    /// The underlying message, when there is one. Never localized and never shown
    /// to a user; it exists for the log and for a developer reading it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Every way a plugin can be refused.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginErrorCode {
    /// The manifest is not readable JSON, or is not the shape this host knows.
    ManifestInvalid,
    /// The manifest's `schema_version` is not the one this host accepts.
    UnsupportedSchemaVersion,
    /// The manifest needs a feature level this host does not implement.
    UnsupportedApiVersion,
    /// The plugin id is not a safe directory name.
    InvalidPluginId,
    /// The display name is empty or too long.
    InvalidPluginName,
    /// The description is too long.
    InvalidPluginDescription,
    /// A path the plugin named is absolute, escapes its directory or is not a
    /// plain file name.
    InvalidAssetPath,
    /// A capability the manifest declared and the host does not grant.
    CapabilityNotGranted,
    /// More behaviors than the bound allows.
    TooManyBehaviors,
    /// A behavior id is empty, too long or not a plain identifier.
    InvalidBehaviorId,
    /// Two behaviors share a name.
    DuplicateBehaviorId,
    /// A behavior's own values are out of range.
    InvalidBehaviorSpec,
    /// More scene nodes than the bound allows.
    SceneTooLarge,
    /// Nested deeper than the bound allows.
    SceneTooDeep,
    /// A binding names no source, or a path longer than the bound allows.
    InvalidBinding,
    /// A binding or a button target names a behavior that was not declared.
    UnknownBinding,
    /// A button id is empty or too long.
    InvalidButtonId,
    /// Two buttons share an id, so a press would be ambiguous.
    DuplicateButtonId,
    /// A clock format is not a subset of `HH`/`MM`/`SS` and separators.
    InvalidTimeFormat,
    /// The panel's logical size is zero or past the bound.
    InvalidPanelSize,
    /// The panel's placement is not one the model window can honour.
    InvalidPanelPlacement,
    /// The plugin's own directory is missing or unreadable.
    PluginDirectoryUnreadable,
    /// The archive is not a readable zip, or a member inside it is not safe.
    ArchiveInvalid,
    /// The download did not match the digest the catalog announced.
    ChecksumMismatch,
    /// The download was not signed by the release key.
    SignatureInvalid,
    /// A build with no release signing key cannot install anything.
    SignatureKeyMissing,
    /// The request reached no source, or every source failed.
    DownloadFailed,
    /// The catalog was fetched but is not the shape this host knows.
    CatalogInvalid,
    /// The catalog does not announce this plugin, or not for this platform.
    PluginNotPublished,
    /// The installed directory is already occupied by another install.
    AlreadyInstalled,
    /// The plugin is not installed, or its directory is gone.
    NotInstalled,
    /// The plugin is installed and the catalog has nothing newer.
    AlreadyUpToDate,
    /// Writing the plugin store failed.
    StoreWriteFailed,
    /// As many panels as the model window allows are already switched on.
    ///
    /// Its own code rather than a `PluginNotPublished` with a detail: the user
    /// cannot act on "not published" by turning another panel off, and a page that
    /// showed that sentence for this would send them to the wrong place.
    TooManyEnabled,
    /// The layout or the raster could not be produced.
    RenderFailed,
    /// The plugin asked for a font that could not be loaded.
    FontUnavailable,
}

impl PluginErrorCode {
    pub const ALL: [Self; 36] = [
        Self::ManifestInvalid,
        Self::UnsupportedSchemaVersion,
        Self::UnsupportedApiVersion,
        Self::InvalidPluginId,
        Self::InvalidPluginName,
        Self::InvalidPluginDescription,
        Self::InvalidAssetPath,
        Self::CapabilityNotGranted,
        Self::TooManyBehaviors,
        Self::InvalidBehaviorId,
        Self::DuplicateBehaviorId,
        Self::InvalidBehaviorSpec,
        Self::SceneTooLarge,
        Self::SceneTooDeep,
        Self::InvalidBinding,
        Self::UnknownBinding,
        Self::InvalidButtonId,
        Self::DuplicateButtonId,
        Self::InvalidTimeFormat,
        Self::InvalidPanelSize,
        Self::InvalidPanelPlacement,
        Self::PluginDirectoryUnreadable,
        Self::ArchiveInvalid,
        Self::ChecksumMismatch,
        Self::SignatureInvalid,
        Self::SignatureKeyMissing,
        Self::DownloadFailed,
        Self::CatalogInvalid,
        Self::PluginNotPublished,
        Self::AlreadyInstalled,
        Self::NotInstalled,
        Self::AlreadyUpToDate,
        Self::StoreWriteFailed,
        Self::TooManyEnabled,
        Self::RenderFailed,
        Self::FontUnavailable,
    ];

    /// The stable snake_case name, which is also the key a message is looked up
    /// by. It is the wire form, so it does not change when a message is reworded.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ManifestInvalid => "manifest_invalid",
            Self::UnsupportedSchemaVersion => "unsupported_schema_version",
            Self::UnsupportedApiVersion => "unsupported_api_version",
            Self::InvalidPluginId => "invalid_plugin_id",
            Self::InvalidPluginName => "invalid_plugin_name",
            Self::InvalidPluginDescription => "invalid_plugin_description",
            Self::InvalidAssetPath => "invalid_asset_path",
            Self::CapabilityNotGranted => "capability_not_granted",
            Self::TooManyBehaviors => "too_many_behaviors",
            Self::InvalidBehaviorId => "invalid_behavior_id",
            Self::DuplicateBehaviorId => "duplicate_behavior_id",
            Self::InvalidBehaviorSpec => "invalid_behavior_spec",
            Self::SceneTooLarge => "scene_too_large",
            Self::SceneTooDeep => "scene_too_deep",
            Self::InvalidBinding => "invalid_binding",
            Self::UnknownBinding => "unknown_binding",
            Self::InvalidButtonId => "invalid_button_id",
            Self::DuplicateButtonId => "duplicate_button_id",
            Self::InvalidTimeFormat => "invalid_time_format",
            Self::InvalidPanelSize => "invalid_panel_size",
            Self::InvalidPanelPlacement => "invalid_panel_placement",
            Self::PluginDirectoryUnreadable => "plugin_directory_unreadable",
            Self::ArchiveInvalid => "archive_invalid",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::SignatureInvalid => "signature_invalid",
            Self::SignatureKeyMissing => "signature_key_missing",
            Self::DownloadFailed => "download_failed",
            Self::CatalogInvalid => "catalog_invalid",
            Self::PluginNotPublished => "plugin_not_published",
            Self::AlreadyInstalled => "already_installed",
            Self::NotInstalled => "not_installed",
            Self::AlreadyUpToDate => "already_up_to_date",
            Self::StoreWriteFailed => "store_write_failed",
            Self::TooManyEnabled => "too_many_enabled",
            Self::RenderFailed => "render_failed",
            Self::FontUnavailable => "font_unavailable",
        }
    }
}

impl PluginError {
    pub const fn new(code: PluginErrorCode) -> Self {
        Self { code, detail: None }
    }

    pub fn with_detail(code: PluginErrorCode, detail: impl std::fmt::Display) -> Self {
        Self {
            code,
            detail: Some(detail.to_string()),
        }
    }

    pub const fn code(&self) -> PluginErrorCode {
        self.code
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(formatter, "{}: {}", self.code.as_str(), detail),
            None => formatter.write_str(self.code.as_str()),
        }
    }
}

impl std::error::Error for PluginError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_code_has_a_distinct_stable_name() {
        let names: BTreeSet<&str> = PluginErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect();
        assert_eq!(
            names.len(),
            PluginErrorCode::ALL.len(),
            "two codes share a name, so a message lookup would be ambiguous"
        );
        for code in PluginErrorCode::ALL {
            assert!(
                !code.as_str().is_empty()
                    && code
                        .as_str()
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{} is not a snake_case name",
                code.as_str()
            );
        }
    }

    #[test]
    fn the_all_list_covers_every_variant() {
        // `ALL` is hand-written, so a new variant that nobody added here would
        // make the settings window unable to name it. Comparing the two lists is
        // the only thing that catches that.
        let declared = PluginErrorCode::ALL.len();
        let mut seen = BTreeSet::new();
        for code in PluginErrorCode::ALL {
            seen.insert(code);
        }
        assert_eq!(seen.len(), declared);
    }

    #[test]
    fn a_detail_is_carried_but_never_replaces_the_code() {
        let error = PluginError::with_detail(PluginErrorCode::ManifestInvalid, "bad json");
        assert_eq!(error.code(), PluginErrorCode::ManifestInvalid);
        assert!(error.to_string().starts_with("manifest_invalid: "));
        assert!(
            PluginError::new(PluginErrorCode::ChecksumMismatch)
                .detail
                .is_none()
        );
    }
}
