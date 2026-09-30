//! Who a plugin is: an id that is safe on disk, and a version that orders.
//!
//! Both are read from documents and both become filesystem paths, so both
//! validate on the way in rather than on the way to use. A `#[serde(transparent)]`
//! newtype would accept `"../secrets"` and hand it straight to the store.

use super::error::{PluginError, PluginErrorCode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The longest a plugin id may be, in bytes.
///
/// The id becomes a directory name under the plugin store, so the bound is
/// about what a filesystem accepts rather than about anything a plugin needs.
pub const MAXIMUM_PLUGIN_ID_BYTES: usize = 64;

/// A plugin's stable identity.
///
/// Lowercase ASCII letters, digits and single hyphens, starting and ending with a
/// letter or a digit. The rule is the one Obsidian settled on for the same
/// reason: the id becomes a directory name on two filesystems and a lookup key in
/// a published catalog, and a spelling that can differ by case between them is a
/// plugin that is installed twice.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(id: impl Into<String>) -> Result<Self, PluginError> {
        let id = id.into();
        if id.is_empty() || id.len() > MAXIMUM_PLUGIN_ID_BYTES {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginId));
        }
        if !is_safe_plugin_id(&id) {
            return Err(PluginError::new(PluginErrorCode::InvalidPluginId));
        }
        Ok(Self(id))
    }

    /// Read an id that is already known good.
    ///
    /// For the paths where the id came out of [`Self::new`] or off a validated
    /// manifest, so re-checking it would only be a second way to be wrong.
    pub fn from_validated(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for PluginId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

fn is_safe_plugin_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    let edge = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if !edge(bytes[0]) || !edge(bytes[bytes.len() - 1]) {
        return false;
    }
    let mut previous_was_hyphen = false;
    for &byte in bytes {
        if byte == b'-' {
            if previous_was_hyphen {
                return false;
            }
            previous_was_hyphen = true;
        } else if edge(byte) {
            previous_was_hyphen = false;
        } else {
            return false;
        }
    }
    true
}

/// A semantic version, ordered well enough to compare two plugin releases.
///
/// Written rather than borrowed from a version crate: the only comparison the
/// plugin center needs is "is the catalog's version newer than what is
/// installed", and a three-number tuple does that without a dependency and
/// without a pre-release rule nobody would apply to a plugin catalogue anyway.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct PluginVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl From<String> for PluginVersion {
    fn from(value: String) -> Self {
        Self::parse(&value).unwrap_or_default()
    }
}

impl From<PluginVersion> for String {
    fn from(value: PluginVersion) -> Self {
        value.to_string()
    }
}

impl Default for PluginVersion {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

impl PluginVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for PluginVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A plugin as the plugin store holds it on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPlugin {
    pub id: PluginId,
    pub version: PluginVersion,
    /// The version's own directory under the store.
    pub directory: PathBuf,
}

impl InstalledPlugin {
    /// The descriptor inside this version's directory.
    pub fn manifest_path(&self) -> PathBuf {
        self.directory
            .join(super::descriptor::PLUGIN_MANIFEST_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_that_could_escape_a_directory_is_refused() {
        for id in ["..", "a/b", "A-b", "-ab", "ab-", "a--b", "ab_c", ""] {
            assert!(PluginId::new(id).is_err(), "{id:?} must not be a plugin id");
        }
        assert!(PluginId::new("ab").is_err(), "two characters is too short");
        assert!(PluginId::new("a1-b2").is_ok());
    }

    #[test]
    fn an_id_round_trips_through_json_as_itself() {
        let id = PluginId::new("pomodoro").expect("valid");
        let json = serde_json::to_string(&id).expect("serializes");
        assert_eq!(json, "\"pomodoro\"");
        assert_eq!(
            serde_json::from_str::<PluginId>(&json).expect("parses"),
            id,
            "and a read id is validated on the way in, not trusted"
        );
        assert!(serde_json::from_str::<PluginId>("\"../secrets\"").is_err());
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(
            PluginVersion::parse("1.2.3"),
            Some(PluginVersion::new(1, 2, 3))
        );
        assert!(PluginVersion::parse("1.2").is_none());
        assert!(PluginVersion::parse("1.2.3.4").is_none());
        assert!(PluginVersion::parse("x.y.z").is_none());
        assert!(PluginVersion::new(1, 0, 0) < PluginVersion::new(1, 0, 1));
        assert!(PluginVersion::new(1, 9, 0) < PluginVersion::new(2, 0, 0));
    }

    #[test]
    fn a_version_read_from_a_string_that_is_not_one_falls_back_to_zero() {
        // Derived through `from = "String"`, so a version in a catalog entry is a
        // `String` on the wire and a malformed one must not fail the whole parse —
        // it orders as 0.0.0, which is older than anything real.
        assert_eq!(
            PluginVersion::from("not-a-version".to_string()),
            PluginVersion::new(0, 0, 0)
        );
    }
}
