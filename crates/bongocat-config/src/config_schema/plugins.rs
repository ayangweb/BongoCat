//! Which plugins are switched on, and which the product is allowed to run.
//!
//! The installed set is *not* in this document: what is on disk is discovered from
//! the plugin store, because a directory is the only thing that can be true after
//! an update, an uninstall or a hand edit. What belongs here is the user's
//! preference — which of the installed plugins shows a panel — and the policy the
//! product enforces on them.
//!
//! Every field has a default, so a configuration written before plugins existed
//! reads unchanged, and a configuration written by a newer build is refused by the
//! `schema_version` gate rather than half-read.

use super::*;
use std::collections::BTreeSet;

/// The most plugins that may be switched on at once.
///
/// The same bound the plugin worker enforces, recorded here so the settings window
/// can grey out the last switch rather than accepting a press that would be
/// refused. Two bounds in two places is a duplication; the one that decides is the
/// worker's, and this one exists so the user learns about the limit before
/// pressing the button.
pub const MAXIMUM_ENABLED_PLUGINS: usize = 4;

/// The longest a plugin id may be in this document, in bytes.
///
/// Matches the protocol's own bound, so an id that could not be a directory name
/// cannot be recorded here either.
pub const MAXIMUM_PLUGIN_ID_BYTES: usize = 64;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    /// The ids whose panels the user has switched on.
    ///
    /// An id here that is not installed is not an error: it is a plugin the user
    /// enabled and then removed, and it is ignored until it is installed again.
    /// Writing an error here would make an uninstall fail, which is the wrong
    /// outcome for the thing the user just did.
    #[serde(default)]
    pub enabled: Vec<String>,
}

impl PluginsConfig {
    /// Whether a plugin's panel is switched on.
    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.iter().any(|candidate| candidate == id)
    }

    /// The same list with a plugin added or removed, sorted and deduplicated.
    ///
    /// Sorted so the document is stable: a file whose ordering changes on every
    /// write is a file that shows a diff the user did not make.
    pub fn with(&self, id: &str, enabled: bool) -> Self {
        let mut ids: Vec<String> = self
            .enabled
            .iter()
            .filter(|candidate| candidate.as_str() != id)
            .cloned()
            .collect();
        if enabled {
            ids.push(id.to_string());
        }
        ids.sort();
        ids.dedup();
        Self { enabled: ids }
    }

    /// The first problem with this list, or `None`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.enabled.len() > MAXIMUM_ENABLED_PLUGINS {
            return Err(ConfigError::InvalidValue("plugins.enabled"));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for id in &self.enabled {
            if id.is_empty() || id.len() > MAXIMUM_PLUGIN_ID_BYTES {
                return Err(ConfigError::InvalidValue("plugins.enabled"));
            }
            if !seen.insert(id.as_str()) {
                return Err(ConfigError::InvalidValue("plugins.enabled"));
            }
        }
        Ok(())
    }
}
