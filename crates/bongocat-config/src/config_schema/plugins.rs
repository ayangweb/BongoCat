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
    /// The ids whose panels the user has switched off.
    ///
    /// A second list rather than one list of the opposite polarity, and the reason is the
    /// one thing a single list cannot say: a plugin the user has never touched is on, and
    /// "the ids that are on" cannot also record the one they turned off. With only
    /// `enabled`, a fresh configuration — an empty list — reads as *every* installed
    /// plugin switched off, so a plugin somebody had just installed did nothing at all
    /// until they went and turned it on, which is not what installing means.
    ///
    /// An id in both lists is a contradiction and is refused by [`Self::validate`]
    /// rather than resolved, because a hand-edited file with two answers to one question
    /// is a file whose author can be told which one is wrong.
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl PluginsConfig {
    /// Whether a plugin's panel is switched on.
    ///
    /// On unless the user switched it off, which is the default that makes installing a
    /// plugin mean something. The `enabled` list is not consulted here: it records the
    /// same decision from the other side so the settings page can show a switch, and
    /// reading it as the truth is what would put the two lists out of step.
    pub fn is_enabled(&self, id: &str) -> bool {
        !self.is_disabled(id)
    }

    /// Whether the user has switched this plugin's panel off.
    pub fn is_disabled(&self, id: &str) -> bool {
        self.disabled.iter().any(|candidate| candidate == id)
    }

    /// The same list with a plugin added or removed, sorted and deduplicated.
    ///
    /// Sorted so the document is stable: a file whose ordering changes on every
    /// write is a file that shows a diff the user did not make.
    pub fn with(&self, id: &str, enabled: bool) -> Self {
        // Each list loses the id first and one of them gains it, so the two can never
        // both hold it however this is called.
        let without = |ids: &[String]| -> Vec<String> {
            ids.iter()
                .filter(|candidate| candidate.as_str() != id)
                .cloned()
                .collect()
        };
        let mut on = without(&self.enabled);
        let mut off = without(&self.disabled);
        if enabled {
            on.push(id.to_string());
        } else {
            off.push(id.to_string());
        }
        on.sort();
        on.dedup();
        off.sort();
        off.dedup();
        Self {
            enabled: on,
            disabled: off,
        }
    }

    /// The first problem with these lists, or `None`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.enabled.len() > MAXIMUM_ENABLED_PLUGINS {
            return Err(ConfigError::InvalidValue("plugins.enabled"));
        }
        for (ids, field) in [
            (&self.enabled, "plugins.enabled"),
            (&self.disabled, "plugins.disabled"),
        ] {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for id in ids {
                if id.is_empty() || id.len() > MAXIMUM_PLUGIN_ID_BYTES {
                    return Err(ConfigError::InvalidValue(field));
                }
                if !seen.insert(id.as_str()) {
                    return Err(ConfigError::InvalidValue(field));
                }
            }
        }
        if self
            .enabled
            .iter()
            .any(|id| self.disabled.iter().any(|other| other == id))
        {
            return Err(ConfigError::InvalidValue("plugins.disabled"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enabled: &[&str], disabled: &[&str]) -> PluginsConfig {
        PluginsConfig {
            enabled: enabled.iter().map(|id| (*id).to_owned()).collect(),
            disabled: disabled.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    #[test]
    fn a_plugin_nobody_has_mentioned_is_on() {
        // The rule the second list exists for. With only an on-list, a fresh configuration
        // is an empty list, and an empty on-list has to mean either "nothing is on" or
        // "nothing has been decided" — and choosing the first reading means a plugin the
        // user just installed draws nothing and says nothing.
        let fresh = PluginsConfig::default();
        assert!(fresh.enabled.is_empty() && fresh.disabled.is_empty());
        assert!(
            fresh.is_enabled("pomodoro"),
            "so a plugin nobody has decided about is on, which is what installing one means"
        );
    }

    #[test]
    fn switching_a_plugin_off_is_recorded_as_a_decision_rather_than_as_an_absence() {
        let after = PluginsConfig::default().with("pomodoro", false);
        assert!(after.is_disabled("pomodoro"));
        assert!(!after.is_enabled("pomodoro"));
        assert!(
            after.enabled.is_empty(),
            "and it is not in the on-list either"
        );
        assert!(
            after.is_enabled("key-stats"),
            "one plugin being off says nothing about any other"
        );
        assert!(
            after.with("pomodoro", true).is_enabled("pomodoro"),
            "and switching it back on takes the decision back"
        );
    }

    #[test]
    fn a_plugin_is_in_at_most_one_of_the_two_lists_however_it_is_written() {
        for enabled in [true, false] {
            let after = PluginsConfig::default()
                .with("pomodoro", enabled)
                .with("pomodoro", !enabled)
                .with("pomodoro", enabled);
            assert!(after.validate().is_ok(), "{:?}", after);
            assert!(
                !(after.enabled.contains(&"pomodoro".to_string())
                    && after.disabled.contains(&"pomodoro".to_string())),
                "so the two lists cannot answer the same question differently: {after:?}"
            );
        }
    }

    #[test]
    fn a_file_that_puts_one_plugin_in_both_lists_is_refused_rather_than_resolved() {
        // A hand-edited document with two answers to one question is a document whose
        // author can be told which answer is wrong. Picking one silently would make the
        // product's behaviour depend on an accident of ordering.
        let contradiction = config(&["pomodoro"], &["pomodoro"]);
        assert!(contradiction.validate().is_err());
        assert!(
            PluginsConfig::default()
                .with("pomodoro", false)
                .validate()
                .is_ok(),
            "and a file the product wrote itself is always one of the answers"
        );
    }

    #[test]
    fn the_two_lists_stay_sorted_so_the_file_shows_no_diff_the_user_did_not_make() {
        let after = PluginsConfig::default()
            .with("pomodoro", false)
            .with("key-stats", false)
            .with("agent-watch", false);
        assert_eq!(after.disabled, ["agent-watch", "key-stats", "pomodoro"]);
    }
}
