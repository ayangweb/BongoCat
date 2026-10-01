//! Controls a plugin wants offered *outside* its panel.
//!
//! A plugin's panel is one place on one window, and it is the wrong place for a
//! control the user reaches for often: a pomodoro's Start button lives in a 260-pixel
//! box on the model window, which a person has to find, hover and click a small target
//! inside to use a timer they set up in the settings window an hour ago. So a plugin
//! may also declare **actions** — controls the host draws on its own card, in the
//! settings window, with the product's own buttons.
//!
//! # Why the plugin declares them rather than the host finding them
//!
//! The host could read the buttons out of a panel it already has. It does not, for
//! three reasons, and the first is the one that decides it:
//!
//! 1. **A panel's layout is not an inventory of what a plugin can do.** Key Display
//!    draws a row of keys; asking for those as application controls is asking for
//!    forty-odd buttons the user did not want. An action is something a plugin
//!    *chooses* to offer, and it can offer it whether or not it ever draws a panel.
//! 2. **A plugin with no panel at all** — a plugin that reacts to the model without
//!    putting anything on screen — would have nothing to offer, which is exactly
//!    backwards.
//! 3. **A card is rebuilt from a snapshot, and a panel is not in the snapshot.**
//!    Deriving an action from a panel would make the settings window read the
//!    renderer's state, which is the coupling this system removes twice over already:
//!    the host never evaluates what a panel shows, and the window never sees a scene.
//!
//! # The division, which is the one the whole plugin system uses
//!
//! The plugin says what a control *means* and what it is *called*; the host says what
//! it *looks* like. So [`PluginAction`] carries an id, the plugin's own localized
//! label and a glyph chosen from a closed set — never a colour, a size, a font or a
//! position. The host draws it with the product's own button and the icon set the rest
//! of the window uses, which is why a plugin's action looks like part of BongoCat
//! without the plugin knowing what a theme is.
//!
//! # Actions are live, not a declaration made once
//!
//! An action carries a label, and a label that goes stale is a lie on a card: a
//! pomodoro whose button still reads "Start" while its round is counting tells the
//! user the opposite of what pressing it will do. So actions are sent as a *message*
//! rather than as part of the handshake, and a plugin re-sends the whole list whenever
//! the meaning of any of them has changed. The host replaces the list rather than
//! merging it, so a plugin cannot end up offering a control it has stopped wanting,
//! and a press of a control that is not in the current list is ignored rather than
//! delivered.

use super::descriptor::{LocalizedText, MAXIMUM_BUTTON_ID_BYTES, MAXIMUM_LABEL_CHARS};
use super::error::{PluginError, PluginErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The most actions one plugin may offer at a time.
///
/// A design bound rather than a memory one, and a small one on purpose: these are
/// controls on a card that is about 260 pixels wide, beside a switch and the two
/// buttons every installed plugin has. A plugin wanting more room than that wants a
/// panel — which is the surface built for showing many controls at once.
pub const MAXIMUM_ACTIONS: usize = 4;

/// One control a plugin wants the host to offer for it.
///
/// The press travels back as [`super::HostMessage::Press`] carrying this same `id`,
/// which is deliberately the same vocabulary a panel button uses: an action is *a*
/// button, drawn somewhere else, and a plugin handles both in one `on_press`. That is
/// what makes an action free for a plugin to add — there is no second handler to
/// write and no second meaning to keep in step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginAction {
    /// What a press of this control means, in the plugin's own words.
    ///
    /// The same contract as a panel button's id, and the same bound: short, and
    /// opaque to the host. The host routes by it and never derives meaning from it, so
    /// a plugin may use `"toggle"`, `"play"` or `"restart-session"` and the host is
    /// equally happy with all three.
    pub id: String,
    /// What the control is called, in the plugin's own languages.
    ///
    /// Localized for the same reason a plugin's name and its settings labels are: a
    /// third-party string cannot live in this application's source, and the host is the
    /// only side that knows which language the user reads. Resolved by the host at the
    /// boundary, so a plugin writes one [`LocalizedText`] and every language follows.
    pub label: LocalizedText,
    /// Which of the host's icons stands for this control.
    ///
    /// A closed set rather than a name, because an icon is a host resource and a
    /// plugin naming one the host does not ship is a plugin whose card draws a gap.
    /// The set is small and it is the set that reads as "run, stop, start again",
    /// which is what these controls are for.
    #[serde(default)]
    pub glyph: ActionGlyph,
    /// Whether the control is present but not pressable.
    ///
    /// Concrete rather than bound: a plugin that wants a greyed button sets the flag
    /// in the list it sends, and the host draws exactly that. Same rule as a panel
    /// button's own `disabled`, for the same reason — the plugin knows whether the
    /// action would do anything.
    #[serde(default)]
    pub disabled: bool,
}

impl PluginAction {
    /// Check one action, refusing what a card could not draw or a press could not route.
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.id.is_empty() || self.id.len() > MAXIMUM_BUTTON_ID_BYTES {
            return Err(PluginError::new(PluginErrorCode::InvalidButtonId));
        }
        // The longest language rather than the default, for the reason every other
        // label in this crate is checked that way: a plugin that keeps its default
        // short and writes a sentence for one language must not slip past a check that
        // only ever read the default.
        if self.label.longest_characters() > MAXIMUM_LABEL_CHARS {
            return Err(PluginError::new(PluginErrorCode::ProtocolInvalid));
        }
        Ok(())
    }
}

/// Which of the host's icons a control wears.
///
/// Closed, and in this crate rather than in the host, because adding an entry is a
/// change to what the product will draw for a plugin and that has to be made in one
/// file a reviewer will see — the same rule [`super::Subscription`] follows.
///
/// The default is [`ActionGlyph::None`]: a label with no icon beside it is the honest
/// default, because it is always right, and a card should not claim an icon for a
/// control the plugin could not describe.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionGlyph {
    /// No icon. The label is the whole of the control.
    #[default]
    None,
    /// Start something that is stopped. A triangle.
    Play,
    /// Suspend something that is running. Two bars.
    Pause,
    /// Begin again from the beginning. A circular arrow.
    Reset,
}

impl ActionGlyph {
    pub const ALL: [Self; 4] = [Self::None, Self::Play, Self::Pause, Self::Reset];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Reset => "reset",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|entry| entry.as_str() == value)
    }

    /// Whether this glyph asks for an icon at all.
    ///
    /// The distinction that matters to a reader: `None` is not a fifth icon, it is the
    /// absence of one.
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// Check a whole list of actions, in the order a card draws them.
///
/// Two checks and one rule. The per-action check is each one on its own; the
/// uniqueness check is because a press comes back as an id, so two actions sharing one
/// would make the answer ambiguous — exactly the reason a panel refuses two buttons
/// with one id, and the reason it is checked here rather than trusted.
pub fn check_actions(actions: &[PluginAction]) -> Result<(), PluginError> {
    if actions.len() > MAXIMUM_ACTIONS {
        return Err(PluginError::new(PluginErrorCode::ProtocolInvalid));
    }
    let mut ids = BTreeSet::new();
    for action in actions {
        action.validate()?;
        if !ids.insert(action.id.as_str()) {
            return Err(PluginError::new(PluginErrorCode::DuplicateButtonId));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str) -> PluginAction {
        PluginAction {
            id: id.to_string(),
            label: LocalizedText::from("Start"),
            glyph: ActionGlyph::Play,
            disabled: false,
        }
    }

    #[test]
    fn an_action_round_trips_through_the_wire() {
        let line = serde_json::to_string(&action("toggle")).expect("serializes");
        assert_eq!(
            serde_json::from_str::<PluginAction>(&line).expect("parses"),
            action("toggle")
        );
        // The closed set spelled the way it reads, so a document in the repository is
        // the same document the protocol describes.
        assert_eq!(
            line, r#"{"id":"toggle","label":{"default":"Start"},"glyph":"play","disabled":false}"#,
            "spelled the way it reads, so a document in a repository is the same document the \
             protocol describes — and `disabled` is written out even when it is false, because \
             an action is a button and a button's `disabled` always travels"
        );
    }

    #[test]
    fn an_action_that_declares_nothing_still_reads() {
        let read: PluginAction =
            serde_json::from_str(r#"{"id":"go","label":"Go","disabled":false}"#).expect("parses");
        assert!(read.glyph.is_none());
        assert!(!read.disabled);
    }

    #[test]
    fn every_glyph_has_one_spelling_and_an_unknown_one_is_refused() {
        for glyph in ActionGlyph::ALL {
            assert_eq!(ActionGlyph::parse(glyph.as_str()), Some(glyph));
        }
        assert_eq!(ActionGlyph::parse("triangle"), None);
        assert!(
            serde_json::from_str::<PluginAction>(r#"{"id":"a","label":"a","glyph":"triangle"}"#)
                .is_err(),
            "an icon this host does not ship is a card with a gap where the icon goes"
        );
    }

    #[test]
    fn a_control_the_user_could_never_reach_is_refused() {
        assert_eq!(
            action("").validate().unwrap_err().code(),
            PluginErrorCode::InvalidButtonId
        );
        let mut long = action("go");
        long.id = "x".repeat(MAXIMUM_BUTTON_ID_BYTES + 1);
        assert_eq!(
            long.validate().unwrap_err().code(),
            PluginErrorCode::InvalidButtonId
        );
    }

    #[test]
    fn a_label_only_too_long_in_one_language_is_still_too_long() {
        let mut translated = action("go");
        translated.label =
            LocalizedText::from("Go").with_locale("zh-CN", "x".repeat(MAXIMUM_LABEL_CHARS + 1));
        assert!(
            translated.validate().is_err(),
            "otherwise a card draws a label three rows tall, in one language only, and nothing \
             notices"
        );
    }

    #[test]
    fn two_actions_may_not_share_an_id() {
        // A press comes back as an id, so two controls with one id would make the
        // answer ambiguous rather than merely wrong.
        assert_eq!(
            check_actions(&[action("go"), action("go")])
                .unwrap_err()
                .code(),
            PluginErrorCode::DuplicateButtonId
        );
        assert!(check_actions(&[action("play"), action("reset")]).is_ok());
    }

    #[test]
    fn a_card_has_room_for_a_few_controls_and_not_for_a_list() {
        // The bound is a layout claim: these are controls on a card about 260 pixels
        // wide, beside a switch and the two buttons every installed plugin has.
        let many: Vec<PluginAction> = (0..MAXIMUM_ACTIONS)
            .map(|index| action(&format!("a{index}")))
            .collect();
        assert!(check_actions(&many).is_ok());
        let too_many: Vec<PluginAction> = (0..MAXIMUM_ACTIONS + 1)
            .map(|index| action(&format!("a{index}")))
            .collect();
        assert!(check_actions(&too_many).is_err());
        assert!(
            check_actions(&[]).is_ok(),
            "a plugin that wants no card controls says so by offering none, which is how a \
             plugin that was never meant to be pressed from a card opts out"
        );
    }
}
