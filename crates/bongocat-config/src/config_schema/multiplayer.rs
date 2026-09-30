//! Where the multiplayer room service lives, and the identity the user
//! presents to it.
//!
//! Both fields start empty and an empty value is the unconfigured steady
//! state rather than an error, so the document bounds the values and the
//! multiplayer page decides when they are required.

use super::*;

/// Upper bound on the nickname shown to other room members, in characters of
/// the trimmed value. Shared with the settings window so both sides reject at
/// the same limit.
pub const MAXIMUM_MULTIPLAYER_NICKNAME_CHARS: usize = 24;

/// Upper bound on the persisted server URL in bytes, so a stray paste cannot
/// bloat the document.
pub const MAXIMUM_MULTIPLAYER_SERVER_URL_BYTES: usize = 2_048;

/// Multiplayer room settings. The whole section carries a field-level default
/// on the document: a configuration written before it existed keeps parsing,
/// and its absence means the multiplayer page starts unconfigured.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MultiplayerConfig {
    /// Base URL of the room service, for example `http://192.168.1.10:3000`.
    /// Empty means unconfigured; otherwise the scheme must be `http` or
    /// `https`, and the URL must not carry a path, query or fragment because
    /// the service is reached at its root.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(length(max = 2_048))
    )]
    pub server_url: String,
    /// Nickname shown to other room members. Empty means unconfigured; the
    /// trimmed value must fit [`MAXIMUM_MULTIPLAYER_NICKNAME_CHARS`].
    #[cfg_attr(any(test, feature = "schema-generation"), schemars(length(max = 96)))]
    pub nickname: String,
}

impl MultiplayerConfig {
    /// Whether the section names a reachable service. The window uses it to
    /// gate the room controls instead of re-deciding what "configured" means.
    pub fn is_configured(&self) -> bool {
        !self.server_url.trim().is_empty()
    }
}
