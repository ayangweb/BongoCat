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
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginErrorCode {
    /// `plugin.json` is not readable JSON, or is not the shape this host knows.
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
    /// More scene nodes than the bound allows.
    SceneTooLarge,
    /// Nested deeper than the bound allows.
    SceneTooDeep,
    /// A button id is empty or too long.
    InvalidButtonId,
    /// Two buttons share an id, so a press would be ambiguous.
    DuplicateButtonId,
    /// The panel's logical size is zero or past the bound.
    InvalidPanelSize,
    /// The panel's placement is not one the model window can honour.
    InvalidPanelPlacement,
    /// The configuration schema the plugin declared is not one the host can show.
    InvalidConfigSchema,
    /// A configuration value does not fit the field the plugin declared for it.
    InvalidConfigValue,
    /// A line on the wire was not a message this host knows.
    ProtocolInvalid,
    /// The plugin speaks a protocol version this host does not implement.
    ProtocolVersionMismatch,
    /// The plugin's process could not be started.
    PluginSpawnFailed,
    /// The plugin started but never announced itself, or announced something the
    /// host would not accept.
    PluginHandshakeFailed,
    /// The plugin's process ended. Not always a failure — `shutdown` exits zero —
    /// but always a fact the center has to show.
    PluginExited,
    /// The plugin asked the host for something the host cannot do.
    HostCommandUnavailable,
    /// The plugin named a motion or an expression the active model does not have.
    ModelRequestUnknown,
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
    pub const ALL: [Self; 37] = [
        Self::ManifestInvalid,
        Self::UnsupportedSchemaVersion,
        Self::UnsupportedApiVersion,
        Self::InvalidPluginId,
        Self::InvalidPluginName,
        Self::InvalidPluginDescription,
        Self::InvalidAssetPath,
        Self::SceneTooLarge,
        Self::SceneTooDeep,
        Self::InvalidButtonId,
        Self::DuplicateButtonId,
        Self::InvalidPanelSize,
        Self::InvalidPanelPlacement,
        Self::InvalidConfigSchema,
        Self::InvalidConfigValue,
        Self::ProtocolInvalid,
        Self::ProtocolVersionMismatch,
        Self::PluginSpawnFailed,
        Self::PluginHandshakeFailed,
        Self::PluginExited,
        Self::HostCommandUnavailable,
        Self::ModelRequestUnknown,
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
            Self::SceneTooLarge => "scene_too_large",
            Self::SceneTooDeep => "scene_too_deep",
            Self::InvalidButtonId => "invalid_button_id",
            Self::DuplicateButtonId => "duplicate_button_id",
            Self::InvalidPanelSize => "invalid_panel_size",
            Self::InvalidPanelPlacement => "invalid_panel_placement",
            Self::InvalidConfigSchema => "invalid_config_schema",
            Self::InvalidConfigValue => "invalid_config_value",
            Self::ProtocolInvalid => "protocol_invalid",
            Self::ProtocolVersionMismatch => "protocol_version_mismatch",
            Self::PluginSpawnFailed => "plugin_spawn_failed",
            Self::PluginHandshakeFailed => "plugin_handshake_failed",
            Self::PluginExited => "plugin_exited",
            Self::HostCommandUnavailable => "host_command_unavailable",
            Self::ModelRequestUnknown => "model_request_unknown",
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
