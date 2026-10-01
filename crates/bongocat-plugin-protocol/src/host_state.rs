//! What the host tells a plugin about the world, and what a plugin may ask of it.
//!
//! Both directions are closed lists of strongly typed values, and that is the
//! property worth stating: a plugin here is a separate process, so it *could*
//! read a file or open a socket without asking. These types are not a sandbox.
//! They are the surface the product supports, chosen so that what a plugin can do
//! through the host is a list someone wrote down, and so that a plugin author has
//! one place to look for what is available.
//!
//! The input feed is worth a note. It carries keys, buttons and mouse movement
//! because those are what a plugin on the model window has ever wanted — a key
//! display, a click counter, a sound on a keystroke. It does not carry the window
//! state, the configuration, or anything else the product owns, because a plugin
//! that wanted those would be a plugin that wanted to be the app.

use super::descriptor::LocalizedText;
use serde::{Deserialize, Serialize};

/// How far mouse movement may accumulate before the host folds it into one
/// sample.
///
/// The movement channel is latest-value: two samples in the same frame produce
/// one, and the distance is summed rather than dropped, because a counter that
/// loses the distance between two samples is a counter that reads low and nobody
/// can tell why. The bound is on how much one sample may claim, so a plugin
/// cannot be handed a coordinate it did not earn.
pub const MAXIMUM_MOUSE_STEP: f32 = 1.0;

/// One thing that happened, as a plugin is told about it.
///
/// Keys and buttons are named by the same key names the product's own key display
/// already uses (`InputControl`'s spelling), so a plugin showing a key and the cat
/// showing a key agree on what the key is called without either of them owning a
/// table.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    /// A key went down. `repeat` is true for the auto-repeat a held key produces,
    /// so a sound on every keystroke can skip the ones the keyboard invents.
    KeyDown {
        control: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        repeat: bool,
    },
    /// A key came up.
    KeyUp { control: String },
    /// A mouse button went down or up.
    MouseButton { button: String, pressed: bool },
    /// The pointer moved. `distance` is in the model's own normalized units, so it
    /// is the same number whatever the model window's size or scale is — a counter
    /// in one model and a counter in another agree.
    MouseMove { dx: f32, dy: f32, distance: f32 },
    /// The pressed set was reconciled or cleared without a matching edge — after a
    /// lock screen, a sleep, a session switch, or a device being unplugged.
    ///
    /// Sent because a plugin counting keystrokes has its own tally, and a tally
    /// that keeps a key the platform has already forgotten is a tally that never
    /// balances.
    Reset { reason: String },
}

impl InputEvent {
    /// Whether this event changes a pressed/depressed tally.
    ///
    /// The auto-repeat of a held key does not: it is the keyboard saying the same
    /// thing again, and a plugin counting keystrokes would otherwise count one
    /// long press as a hundred.
    pub const fn changes_pressed_tally(&self) -> bool {
        match self {
            Self::KeyDown { repeat, .. } => !*repeat,
            Self::KeyUp { .. } | Self::MouseButton { .. } | Self::Reset { .. } => true,
            Self::MouseMove { .. } => false,
        }
    }
}

/// The facts about the product a plugin may show.
///
/// Read-only, closed, and deliberately small: the active model's display name,
/// whether the model window is on screen, and the user's language so a plugin can
/// pick its own copy. A plugin that wants the model's file paths or the user's
/// configuration does not need a host message for it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostState {
    /// The active model's display name, when a model is loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    /// Whether the model window is currently on screen.
    #[serde(default)]
    pub overlay_visible: bool,
    /// The user's language as a locale tag, e.g. `"zh-CN"`.
    #[serde(default)]
    pub locale: String,
    /// The application version, for a panel that shows one.
    #[serde(default)]
    pub app_version: String,
}

impl HostState {
    /// Build a document from the two facts a panel may bind to.
    ///
    /// Kept as a constructor rather than a set of writers so there is one place
    /// where "the model is not loaded" turns into "the name is absent" instead of
    /// "the name is an empty string", which a plugin cannot tell from a model
    /// actually called "".
    pub fn new(model_name: Option<String>, overlay_visible: bool) -> Self {
        Self {
            model_name: model_name.filter(|name| !name.trim().is_empty()),
            overlay_visible,
            ..Self::default()
        }
    }

    pub fn with_locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = locale.into();
        self
    }

    pub fn with_app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = version.into();
        self
    }
}

/// A request a plugin makes of the model window's owner.
///
/// Three things, because three things are what a plugin on the model window has
/// ever needed to say: show something, play a motion, set an expression. Both
/// model requests name something by name rather than by parameter, so the plugin
/// cannot reach into the model: it asks for "the thinking motion" and either the
/// model has one or the host says it does not.
///
/// Every request may be refused, and refusal is a normal answer rather than an
/// error the plugin has to handle specially — the host has the model, the plugin
/// has the intent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "request", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelRequest {
    /// Play a motion once, by its name in the model.
    PlayMotion {
        name: String,
        /// Play even while another motion of the same priority is running,
        /// rather than being folded into it.
        #[serde(default)]
        restart: bool,
    },
    /// Set an expression by its name in the model, replacing whatever is set.
    SetExpression { name: String },
    /// Clear the expression the plugin set, restoring the model's own default.
    ClearExpression,
    /// Show a short bubble beside the model: text, with a duration.
    ///
    /// The text is a plugin's own, so it is bounded here rather than trusted —
    /// a bubble is a fixed box and a paragraph in it reads as a bug in the product.
    ShowBubble {
        text: LocalizedText,
        /// How long the bubble stays, in milliseconds, clamped to the bounds below.
        #[serde(default)]
        duration_ms: u32,
    },
    /// Take the bubble down now.
    HideBubble,
}

/// The longest a bubble may be, in characters.
pub const MAXIMUM_BUBBLE_CHARS: usize = 120;

/// The shortest a bubble may stay up, in milliseconds.
pub const MINIMUM_BUBBLE_MILLIS: u32 = 800;

/// The longest a bubble may stay up, in milliseconds.
///
/// Long enough for a sentence, short enough that a plugin that forgets to take it
/// down cannot leave something on the user's desktop until the app exits.
pub const MAXIMUM_BUBBLE_MILLIS: u32 = 30_000;

impl ModelRequest {
    /// This request with its own bounds applied.
    pub fn sanitized(mut self) -> Self {
        if let Self::ShowBubble { duration_ms, .. } = &mut self {
            *duration_ms = (*duration_ms).clamp(MINIMUM_BUBBLE_MILLIS, MAXIMUM_BUBBLE_MILLIS);
        }
        if let Self::ShowBubble { text, .. } = &mut self {
            *text = LocalizedText {
                default: text.default.chars().take(MAXIMUM_BUBBLE_CHARS).collect(),
                by_locale: text
                    .by_locale
                    .iter()
                    .map(|(locale, value)| {
                        (
                            locale.clone(),
                            value.chars().take(MAXIMUM_BUBBLE_CHARS).collect(),
                        )
                    })
                    .collect(),
            };
        }
        self
    }
}

/// Which half of the model a request named.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRequestKind {
    Motion,
    Expression,
}

/// Whether a model request did what it asked.
///
/// Every model request carries an id and gets one of these back, because a refusal
/// is a normal answer rather than an error the plugin has to anticipate: a model
/// with no motion called "thinking" is a fact the plugin may want to show, and
/// only the plugin knows what to do about it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ModelOutcome {
    /// The host carried the request out.
    Done,
    /// The active model has no motion or expression by that name.
    ///
    /// Named rather than a bare failure, because "this model has no such motion"
    /// is something a plugin can act on — show a different label, fall back to the
    /// default animation — while "the request failed" is not.
    NotInModel { kind: ModelRequestKind },
    /// The plugin did not subscribe to model requests, so the host did not act.
    NotSubscribed,
    /// The model window is hidden, so nothing would have been seen.
    OverlayHidden,
    /// The product has no command that would do what was asked.
    ///
    /// Distinct from `NotInModel` because the two call for different responses: a
    /// motion the model does not have is something the plugin can fall back from, and
    /// a capability the product lacks is something no amount of retrying will produce.
    /// Saying "not in this model" for the second would send a plugin looking for a
    /// motion that is not the problem.
    HostCannot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facts_a_plugin_is_told_survive_the_wire() {
        // The whole path for these facts is: the runtime publishes them, the worker puts
        // them on every tick, and the plugin reads them. Nothing between those three points
        // can be checked here, but the wire can — and the wire is where a renamed field
        // would silently become "no model" rather than an error.
        let state = HostState::new(Some("cat.model3.json".to_owned()), true)
            .with_locale("zh-CN")
            .with_app_version("1.0.0");
        let line = serde_json::to_vec(&state).expect("serializes");
        let back: HostState = serde_json::from_slice(&line).expect("parses");
        assert_eq!(back.model_name.as_deref(), Some("cat.model3.json"));
        assert_eq!(back.locale, "zh-CN");
        assert_eq!(back.app_version, "1.0.0");
        assert!(back.overlay_visible);
    }

    #[test]
    fn a_state_that_names_no_field_is_refused() {
        // A typo in a plugin is a plugin whose panel silently shows the wrong thing, which
        // is quieter than a refusal — so the wire shape denies unknown fields.
        assert!(
            serde_json::from_str::<HostState>(r#"{"model_nam":"cat.model3.json"}"#).is_err(),
            "and the field this build no longer carries is refused rather than ignored, so a \
             plugin built against another host learns that instead of showing a blank"
        );
    }

    #[test]
    fn an_outcome_survives_the_wire() {
        for outcome in [
            ModelOutcome::Done,
            ModelOutcome::NotInModel {
                kind: ModelRequestKind::Motion,
            },
            ModelOutcome::NotSubscribed,
            ModelOutcome::OverlayHidden,
            ModelOutcome::HostCannot,
        ] {
            let line = serde_json::to_vec(&outcome).expect("serializes");
            assert_eq!(
                serde_json::from_slice::<ModelOutcome>(&line).expect("parses"),
                outcome
            );
        }
    }

    #[test]
    fn an_auto_repeated_key_does_not_count_as_another_keystroke() {
        let first: InputEvent =
            serde_json::from_str(r#"{"kind":"key_down","control":"KeyA"}"#).unwrap();
        let repeated: InputEvent =
            serde_json::from_str(r#"{"kind":"key_down","control":"KeyA","repeat":true}"#).unwrap();
        assert!(first.changes_pressed_tally());
        assert!(!repeated.changes_pressed_tally());
    }

    #[test]
    fn movement_and_a_reset_have_the_reading_a_tally_wants() {
        let moved: InputEvent =
            serde_json::from_str(r#"{"kind":"mouse_move","dx":0.1,"dy":0.0,"distance":0.1}"#)
                .unwrap();
        assert!(!moved.changes_pressed_tally());
        let reset: InputEvent =
            serde_json::from_str(r#"{"kind":"reset","reason":"lock_screen"}"#).unwrap();
        assert!(reset.changes_pressed_tally());
    }

    #[test]
    fn an_input_event_that_names_no_field_is_refused() {
        // A typo in a plugin is a plugin that stops counting, which is quieter than
        // a load failure — so the wire shape denies unknown fields.
        assert!(
            serde_json::from_str::<InputEvent>(r#"{"kind":"key_dn","control":"KeyA"}"#).is_err()
        );
    }

    #[test]
    fn a_model_that_is_not_loaded_has_no_name_rather_than_an_empty_one() {
        assert_eq!(HostState::new(None, true).model_name, None);
        assert_eq!(
            HostState::new(Some("   ".to_string()), true).model_name,
            None,
            "a blank name is not a model called blank"
        );
        assert_eq!(
            HostState::new(Some("Cat".to_string()), true)
                .model_name
                .as_deref(),
            Some("Cat")
        );
    }

    #[test]
    fn a_bubble_is_clamped_to_a_length_and_a_lifetime_a_desktop_can_carry() {
        let ModelRequest::ShowBubble { text, duration_ms } = ModelRequest::ShowBubble {
            text: "x".repeat(500).into(),
            duration_ms: 0,
        }
        .sanitized() else {
            panic!("expected a bubble");
        };
        assert_eq!(text.default.chars().count(), MAXIMUM_BUBBLE_CHARS);
        assert_eq!(duration_ms, MINIMUM_BUBBLE_MILLIS);

        let ModelRequest::ShowBubble { duration_ms, .. } = ModelRequest::ShowBubble {
            text: "hi".into(),
            duration_ms: u32::MAX,
        }
        .sanitized() else {
            panic!("expected a bubble");
        };
        assert_eq!(duration_ms, MAXIMUM_BUBBLE_MILLIS);
    }
}
