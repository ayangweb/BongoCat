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
use std::collections::{BTreeMap, BTreeSet};

/// The most plugins that may be switched on at once.
///
/// The number of positions the model window has, and the same bound the plugin worker
/// enforces. It used to be four — a guess at how many panels a person could stand to
/// look at — and a guess is exactly what this is not any more: **a plugin costs a position,
/// and a position is all there is.** One plugin per corner means nine is the most that can
/// be drawn without two of them overlapping, and a tenth would be a plugin the model window
/// has no room for rather than a panel the user can see.
///
/// Recorded here as well as in the worker so the settings window can grey out the last
/// switch instead of accepting a press that would be refused. Two bounds in two places is a
/// duplication; the one that decides is the worker's, and this one exists so the user learns
/// about the limit before pressing the button.
pub const MAXIMUM_ENABLED_PLUGINS: usize = 9;

/// The longest a plugin id may be in this document, in bytes.
///
/// Matches the protocol's own bound, so an id that could not be a directory name
/// cannot be recorded here either.
pub const MAXIMUM_PLUGIN_ID_BYTES: usize = 64;

/// The longest a position's name may be in this document, in bytes.
///
/// The longest the protocol's own names are, with room to spare, so a name that cannot be a
/// position is caught here rather than reaching the host as a string nothing will match.
pub const MAXIMUM_POSITION_BYTES: usize = 32;

/// The most plugins this document may place.
///
/// The number of positions the model window has, and the same bound the worker enforces on
/// the enabled set: a plugin costs a position, so this cannot be larger than there are
/// positions, and a value that is only ever checked by the worker would be a switch the
/// settings window greys out for a reason it cannot explain.
pub const MAXIMUM_PLUGINS: usize = 9;

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
    /// Where the user put each plugin's panel, by plugin id.
    ///
    /// A third list rather than a field on the two above, and the reason is the same as the
    /// one that put `disabled` beside `enabled`: a plugin nobody has moved is not at any
    /// position the user chose, and a document whose every row is a decision cannot say
    /// "nothing has been decided".
    ///
    /// The value is a position's name in the plugin protocol's own spelling. This crate
    /// checks that the value is a usable name and not that it is one this build has: the
    /// protocol owns the vocabulary, and a name a newer build wrote is read as "the plugin's
    /// own corner" rather than refused, because refusing it would drop a panel over a
    /// spelling this build has not heard of.
    ///
    /// Two entries may name the same position. That is a file a person edited, not a
    /// contradiction the product can resolve at load — the plugin worker allocates one plugin
    /// per position and gives the other its own corner, so the duplicate costs nothing and is
    /// visible in the settings form as the position one of them actually got.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, String>,
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
            positions: self.positions.clone(),
        }
    }

    /// This arrangement with one plugin's position set, in the protocol's spelling.
    ///
    /// Sorted by the map's own key, so the document is stable: a file whose ordering
    /// changes on every write is a file that shows a diff the user did not make.
    pub fn with_position(&self, id: &str, position: Option<&str>) -> Self {
        let mut positions = self.positions.clone();
        match position {
            Some(position) => {
                positions.insert(id.to_owned(), position.to_owned());
            }
            None => {
                positions.remove(id);
            }
        }
        Self {
            enabled: self.enabled.clone(),
            disabled: self.disabled.clone(),
            positions,
        }
    }

    /// Where the user put this plugin's panel, when they have.
    pub fn position_of(&self, id: &str) -> Option<&str> {
        self.positions.get(id).map(String::as_str)
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
        if self.positions.len() > MAXIMUM_PLUGINS {
            return Err(ConfigError::InvalidValue("plugins.positions"));
        }
        for (id, position) in &self.positions {
            if id.is_empty() || id.len() > MAXIMUM_PLUGIN_ID_BYTES {
                return Err(ConfigError::InvalidValue("plugins.positions"));
            }
            if position.is_empty() || position.len() > MAXIMUM_POSITION_BYTES {
                return Err(ConfigError::InvalidValue("plugins.positions"));
            }
            // The protocol owns the vocabulary of positions, so what this can check is that
            // the value is a name rather than a sentence — a hand-edited path or a stray
            // paste. Whether it is a position this build has is the host's to say, and it
            // says it by falling back rather than by refusing the file.
            if !position
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            {
                return Err(ConfigError::InvalidValue("plugins.positions"));
            }
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
            positions: BTreeMap::new(),
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
    fn a_position_a_plugin_was_moved_to_is_remembered_and_can_be_forgotten() {
        let after = PluginsConfig::default()
            .with_position("pomodoro", Some("bottom_left"))
            .with_position("typing-sound", Some("top_right"));
        assert_eq!(after.position_of("pomodoro"), Some("bottom_left"));
        assert_eq!(after.position_of("typing-sound"), Some("top_right"));
        assert_eq!(
            after.position_of("keyboard-display"),
            None,
            "and a plugin nobody moved reads as having no position, which is what lets its \
             own corner be the answer"
        );

        let moved = after.with_position("pomodoro", Some("top_center"));
        assert_eq!(moved.position_of("pomodoro"), Some("top_center"));
        assert!(
            moved.validate().is_ok(),
            "and moving it leaves one position per plugin, not two"
        );

        let forgotten = after.with_position("pomodoro", None);
        assert_eq!(forgotten.position_of("pomodoro"), None);
        assert_eq!(
            forgotten.position_of("typing-sound"),
            Some("top_right"),
            "while the others keep theirs"
        );
    }

    #[test]
    fn switching_a_plugin_off_keeps_where_it_was() {
        // The position is a preference, not a reservation: a plugin switched off and back on
        // should come back where it was, and its corner should be free in the meantime —
        // which the worker's allocation decides, not this document.
        let after = PluginsConfig::default()
            .with_position("pomodoro", Some("bottom_left"))
            .with("pomodoro", false);
        assert_eq!(
            after.position_of("pomodoro"),
            Some("bottom_left"),
            "because the two lists are two different decisions"
        );
    }

    #[test]
    fn two_plugins_naming_the_same_position_is_a_file_and_not_a_failure() {
        // A hand-edited document, and the product's rule is one plugin per position. The
        // worker resolves it — the first by id keeps it and the other takes its own corner —
        // so refusing the file here would drop a panel over a duplicate line the user can
        // see and fix in the settings form.
        let duplicate = PluginsConfig {
            positions: BTreeMap::from([
                ("pomodoro".to_owned(), "top_left".to_owned()),
                ("typing-sound".to_owned(), "top_left".to_owned()),
            ]),
            ..PluginsConfig::default()
        };
        assert!(duplicate.validate().is_ok());
    }

    #[test]
    fn a_position_that_is_not_a_name_is_refused() {
        for (id, position) in [
            ("pomodoro", ""),
            ("pomodoro", "../somewhere"),
            ("pomodoro", "Top Left"),
            ("pomodoro", &"x".repeat(MAXIMUM_POSITION_BYTES + 1)),
            ("", "top_left"),
        ] {
            let document = PluginsConfig {
                positions: BTreeMap::from([(id.to_owned(), position.to_owned())]),
                ..PluginsConfig::default()
            };
            assert!(
                document.validate().is_err(),
                "{id:?} -> {position:?} is not a place a panel can be"
            );
        }
    }

    #[test]
    fn more_positions_than_the_window_has_is_refused() {
        let document = PluginsConfig {
            positions: (0..=MAXIMUM_PLUGINS)
                .map(|index| (format!("plugin-{index}"), "top_left".to_owned()))
                .collect(),
            ..PluginsConfig::default()
        };
        assert!(
            document.validate().is_err(),
            "because the model window has {MAXIMUM_PLUGINS} positions and one is for one plugin"
        );
    }

    #[test]
    fn a_document_written_before_positions_existed_reads_unchanged() {
        // The compatibility that matters: a configuration from a build that had no positions
        // has no such field, and it must read with every position free rather than failing
        // or inventing one.
        let read: PluginsConfig =
            serde_json::from_str(r#"{"enabled":["pomodoro"],"disabled":["typing-sound"]}"#)
                .expect("a document from before positions existed");
        assert!(read.positions.is_empty());
        assert!(read.validate().is_ok());
        assert!(read.is_enabled("pomodoro"));
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
