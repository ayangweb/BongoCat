//! What each state looks like, and letting the user say otherwise.
//!
//! The defaults here are a guess about what is funny, and a guess about what is funny is
//! exactly the thing a person wants to change. So every state's motion and bubble can be
//! replaced, and the replacement is a small JSON object the user writes — which is also the
//! only way to cover a tool this plugin has never heard of without waiting for a build.
//!
//! The shape is deliberately the smallest one that answers the question:
//!
//! ```json
//! {
//!   "reading":  { "motion": "CAT_motion.0", "bubble": "读文件" },
//!   "running":  { "bubble": "跑命令呢" },
//!   "failed":   { "motion": "", "bubble": "出错了" }
//! }
//! ```
//!
//! A key that is present overrides only what it names: a state with a `bubble` and no
//! `motion` keeps the default motion. An empty string means *nothing* rather than *the
//! default*, because "this state should not make the cat do anything" is a thing to want and
//! it has to be sayable. A malformed file is reported and ignored, in full or in part, so a
//! missing comma costs one line and not the whole mapping.

use crate::event::Activity;
use std::collections::BTreeMap;

/// What one state does and says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Look {
    /// The motion to play, empty for none.
    pub motion: String,
    /// What the cat says, empty for none.
    pub bubble: String,
}

impl Look {
    /// A look that does and says nothing.
    pub const SILENT: Self = Self {
        motion: String::new(),
        bubble: String::new(),
    };
}

/// What every state does and says.
///
/// The eight states of [`Activity`] plus nothing: the defaults are a table rather than
/// something computed, because a table is the thing a user reads when they want to know what
/// they are about to change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    looks: BTreeMap<Activity, Look>,
    /// What the mapping file got wrong, if anything, for the plugin's own log.
    pub complaints: Vec<String>,
}

impl Mapping {
    /// The defaults this plugin ships.
    ///
    /// A child's motion for the motions and a sentence for the bubbles, and the three of them
    /// are not symmetrical on purpose:
    ///
    /// * **Idle is silent.** Idle is also the state the plugin is in the moment it starts,
    ///   before anything has been watched, so a bubble here is a cat that says "waiting" the
    ///   moment the app opens. The panel already says what waiting looks like; a bubble is
    ///   for something that happened.
    /// * **Thinking has no bubble.** It is the state between two tools, so it is entered and
    ///   left many times in a row, and a sentence every time would be a sentence nobody reads.
    /// * **Done and Failed have bubbles**, because they are the two states a person actually
    ///   wants to be told about from across the room.
    pub fn defaults() -> Self {
        let mut looks = BTreeMap::new();
        for (activity, motion, bubble) in [
            (Activity::Idle, "", ""),
            (Activity::Thinking, "CAT_motion.0", ""),
            (Activity::Reading, "CAT_motion.0", "reading"),
            (Activity::Writing, "CAT_motion.1", "writing"),
            (Activity::Searching, "CAT_motion.0", "looking"),
            (Activity::Running, "CAT_motion.1", "running"),
            (Activity::Asking, "CAT_motion.0", "needs you"),
            (Activity::Done, "CAT_motion.2", "all done"),
            (Activity::Failed, "CAT_motion.0", "it broke"),
        ] {
            looks.insert(
                activity,
                Look {
                    motion: motion.to_owned(),
                    bubble: bubble.to_owned(),
                },
            );
        }
        Self {
            looks,
            complaints: Vec::new(),
        }
    }

    /// The defaults with the user's own JSON laid over them.
    ///
    /// Never fails. A file that is not JSON at all leaves every default in place; a file that
    /// is JSON with one bad entry in it leaves that one default in place. Both are reported
    /// through [`Mapping::complaints`] so the plugin can put them in its own log, where the
    /// user can read them — a mapping that silently did not apply is a mapping the user will
    /// assume they configured.
    pub fn from_json(source: &str) -> Self {
        let mut mapping = Self::defaults();
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return mapping;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            mapping
                .complaints
                .push("the mapping is not JSON, so every state kept its default".to_owned());
            return mapping;
        };
        let Some(object) = value.as_object() else {
            mapping
                .complaints
                .push("the mapping is not an object, so every state kept its default".to_owned());
            return mapping;
        };
        for (key, entry) in object {
            let Some(activity) = Activity::from_name(key) else {
                mapping.complaints.push(format!(
                    "`{key}` is not a state this plugin knows, so it was not used; the states \
                     are {}",
                    Activity::ALL.map(|activity| activity.name()).join(", ")
                ));
                continue;
            };
            let Some(entry) = entry.as_object() else {
                mapping.complaints.push(format!(
                    "`{key}` is not an object with `motion` and `bubble` in it, so it was not \
                     used"
                ));
                continue;
            };
            let current = mapping
                .looks
                .get(&activity)
                .cloned()
                .unwrap_or(Look::SILENT);
            let motion = entry.get("motion");
            let bubble = entry.get("bubble");
            if motion.is_some_and(|value| !value.is_string())
                || bubble.is_some_and(|value| !value.is_string())
            {
                mapping.complaints.push(format!(
                    "`{key}` has a `motion` or `bubble` that is not text, so it was not used"
                ));
                continue;
            }
            // Absent means "keep the default" and present-but-empty means "nothing", and the
            // difference is the whole reason the mapping is worth having.
            let look = Look {
                motion: motion
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&current.motion)
                    .to_owned(),
                bubble: bubble
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&current.bubble)
                    .to_owned(),
            };
            mapping.looks.insert(activity, look);
        }
        mapping
    }

    /// Whether this mapping is the one the plugin ships.
    ///
    /// The question the plugin actually asks — "did the user change anything?" — rather than
    /// comparing nine states itself at two call sites.
    pub fn is_default(&self) -> bool {
        *self == Self::defaults()
    }

    /// What this state does and says.
    ///
    /// A state with no entry at all is silent rather than an error: a mapping file that
    /// covers two of nine states is a mapping, not a mistake.
    pub fn look(&self, activity: Activity) -> Look {
        self.looks.get(&activity).cloned().unwrap_or(Look::SILENT)
    }

    /// This mapping as the JSON that would produce it, for the plugin's own log.
    ///
    /// Only the states that *differ* from the defaults, and including a state mapped to
    /// silence — because a state that does and says nothing is a change, and writing it out
    /// by leaving it off would turn "the user mapped this to nothing" back into "the user
    /// never mentioned this" the next time it was read. Round-tripping is the property that
    /// matters here, and it is what the test pins.
    pub fn to_json(&self) -> String {
        let defaults = Self::defaults();
        let mut object = serde_json::Map::new();
        for activity in Activity::ALL {
            let look = self.look(activity);
            if look == defaults.look(activity) {
                continue;
            }
            object.insert(
                activity.name().to_owned(),
                serde_json::json!({ "motion": look.motion, "bubble": look.bubble }),
            );
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(object))
            .unwrap_or_else(|_| "{}".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_cover_every_state() {
        // A state with no default is a state the panel can reach and nothing can show, and
        // that is a hole in the middle of a feature rather than at its edge.
        let mapping = Mapping::defaults();
        for activity in Activity::ALL {
            assert!(
                mapping.looks.contains_key(&activity),
                "so `{}` has a default",
                activity.name()
            );
        }
        assert!(mapping.complaints.is_empty());
    }

    #[test]
    fn an_empty_mapping_is_the_defaults_rather_than_nothing() {
        for source in ["", "   ", "\n"] {
            let mapping = Mapping::from_json(source);
            assert_eq!(mapping, Mapping::defaults(), "for {source:?}");
            assert!(
                mapping.complaints.is_empty(),
                "and nothing is complained about, because a blank setting is not a mistake"
            );
        }
    }

    #[test]
    fn a_user_can_replace_one_state_without_naming_the_others() {
        let mapping = Mapping::from_json(r#"{"running": {"bubble": "跑命令呢"}}"#);
        assert_eq!(mapping.look(Activity::Running).bubble, "跑命令呢");
        assert_eq!(
            mapping.look(Activity::Running).motion,
            Mapping::defaults().look(Activity::Running).motion,
            "so the motion it did not name is still the default one"
        );
        assert_eq!(
            mapping.look(Activity::Reading),
            Mapping::defaults().look(Activity::Reading),
            "and every other state is untouched"
        );
    }

    #[test]
    fn an_empty_string_means_nothing_rather_than_the_default() {
        // "This state should not move the cat" is a thing to want, and it has to be
        // sayable — a mapping that could only add things could not say this.
        let mapping = Mapping::from_json(r#"{"failed": {"motion": "", "bubble": ""}}"#);
        assert_eq!(mapping.look(Activity::Failed), Look::SILENT);
    }

    #[test]
    fn a_mapping_that_is_not_json_leaves_every_default_and_says_so() {
        for source in ["not json", "{", "[1,2]", "\"a string\""] {
            let mapping = Mapping::from_json(source);
            assert_eq!(
                mapping.looks,
                Mapping::defaults().looks,
                "so a broken file costs nothing but the change: {source:?}"
            );
            assert_eq!(mapping.complaints.len(), 1, "for {source:?}");
        }
    }

    #[test]
    fn one_bad_entry_costs_one_entry_and_not_the_whole_file() {
        // A missing comma in a hand-written file is the likeliest mistake there is, and it
        // should not take the three states below it with it.
        let mapping = Mapping::from_json(
            r#"{
                "reading": {"bubble": "reading"},
                "broken": {"bubble": "nope"},
                "running": {"bubble": "running"},
                "thinking": {"bubble": 7}
            }"#,
        );
        assert_eq!(mapping.look(Activity::Reading).bubble, "reading");
        assert_eq!(mapping.look(Activity::Running).bubble, "running");
        assert_eq!(
            mapping.look(Activity::Thinking),
            Mapping::defaults().look(Activity::Thinking),
            "because a number where text belongs is not a bubble"
        );
        assert_eq!(
            mapping.complaints.len(),
            2,
            "and both mistakes are reported, so a user who cannot see their change knows \
             why: {:?}",
            mapping.complaints
        );
        assert!(
            mapping
                .complaints
                .iter()
                .any(|complaint| complaint.contains("broken")),
            "including which state it was: {:?}",
            mapping.complaints
        );
    }

    #[test]
    fn a_state_name_this_plugin_does_not_have_is_reported_with_the_ones_it_does() {
        // A user guessing at a name should be told the names, not just told no.
        let mapping = Mapping::from_json(r#"{"typing": {"bubble": "typing"}}"#);
        assert_eq!(mapping.looks, Mapping::defaults().looks);
        assert!(
            mapping.complaints[0].contains("idle") && mapping.complaints[0].contains("failed"),
            "so the complaint lists the states: {:?}",
            mapping.complaints
        );
    }

    #[test]
    fn a_mapping_reads_back_as_what_it_is() {
        // The panel shows the mapping in effect, so writing one out has to be the same
        // operation as reading one in. Otherwise the thing shown is a description of the
        // mapping rather than the mapping.
        let source =
            r#"{"running": {"bubble": "跑命令呢"}, "failed": {"motion": "", "bubble": ""}}"#;
        let mapping = Mapping::from_json(source);
        let round_trip = Mapping::from_json(&mapping.to_json());
        assert_eq!(
            round_trip.looks, mapping.looks,
            "because what the panel shows and what the plugin does have to be one thing"
        );
        assert!(round_trip.complaints.is_empty());
    }

    #[test]
    fn only_the_states_that_differ_are_written_out() {
        // The written form is what the plugin logs when a mapping is in effect, so a user who
        // changed two of nine states sees two states rather than nine.
        let mapping = Mapping::from_json(
            r#"{"thinking": {"bubble": "想事情"}, "failed": {"motion": "", "bubble": ""}}"#,
        );
        let written = mapping.to_json();
        assert!(!written.contains("idle"), "{written}");
        assert!(written.contains("thinking"), "{written}");
        assert!(
            written.contains("failed"),
            "and a state mapped to silence is written out rather than left out, because \
             leaving it off would turn \"the user mapped this to nothing\" back into \"the \
             user never mentioned this\": {written}"
        );
        assert_eq!(
            Mapping::from_json(&written).looks,
            mapping.looks,
            "so reading the written form back gives the mapping that was written"
        );
    }
}
