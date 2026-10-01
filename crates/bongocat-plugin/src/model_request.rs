//! Turning a plugin's "play the thinking motion" into the product's own command.
//!
//! A plugin may ask for three things by name: a motion, an expression, and a
//! bubble. This file is where those names stop being names. It looks the name up in
//! the active model's own behaviour list and issues the runtime command the product
//! already issues for a keyboard shortcut — which is the point. A plugin that asks for
//! `tap_head` gets the model's motion in group `TapBody` at index 0, or a refusal, and
//! there is nothing in the protocol that names a parameter index, a texture or a
//! device handle, so a plugin cannot reach past the model's own vocabulary.
//!
//! # Every request is answered
//!
//! Including the ones that cannot be carried out, and the reason is worth stating:
//! a plugin with a fallback wants to know. "This model has no motion called
//! `thinking`" is a fact a plugin can show the user or fall back from; a silent
//! timeout is neither. So there is no path where a request produces no answer, and
//! the four refusals name four different things a plugin might do about them.

use crate::sound::{self, SoundOutcome};
use bongocat_audio::{MotionAudioClient, MotionAudioCommand, MotionAudioVolume};
use bongocat_model::ModelBehaviorSnapshot;
use bongocat_plugin_protocol::{ModelAnswer, ModelOutcome, ModelRequest, ModelRequestKind};
use bongocat_runtime::{ExpressionId, MotionId, MotionPriority, RuntimeClient, ShortcutAction};

/// Carries a plugin's model requests out to the runtime.
///
/// Holds a client rather than the runtime itself, so the worker's own thread decides
/// *when* to issue a command and this type only says *what*. The client is cheap to
/// clone and every method on it is a channel send, so a plugin asking for a motion
/// several times a second costs what one send costs.
#[derive(Clone, Default)]
pub struct ModelRequestRouter {
    client: Option<RuntimeClient>,
    /// The product's one audio voice.
    ///
    /// A sound a plugin asks for goes through the same queue and the same voice as a model's
    /// own, which is the whole reason a plugin asks rather than opening the device: one
    /// voice means a plugin's click and a model's motion cannot overlap into the stutter two
    /// output streams produce.
    audio: Option<MotionAudioClient>,
}

impl std::fmt::Debug for ModelRequestRouter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelRequestRouter")
            .field("attached", &self.client.is_some())
            .field("audio", &self.audio.is_some())
            .finish()
    }
}

impl ModelRequestRouter {
    /// A router with no runtime behind it.
    ///
    /// What a worker started before the runtime, or one in a test, has. Every request
    /// is answered `OverlayHidden` rather than dropped, so a plugin can tell "there is
    /// nothing to show it on" from "nobody answered".
    pub fn new(client: Option<RuntimeClient>) -> Self {
        Self {
            client,
            audio: None,
        }
    }

    /// Whether a runtime is behind this router.
    pub fn is_attached(&self) -> bool {
        self.client.is_some()
    }

    /// Attach or replace the runtime behind this router.
    pub fn set_client(&mut self, client: Option<RuntimeClient>) {
        self.client = client;
    }

    /// This router, with an audio service behind it.
    ///
    /// A builder rather than a setter because the worker builds its router once, at start,
    /// from a value the product handed it: there is no moment after which the audio service
    /// appears or disappears, and a `set_` here would suggest there were.
    pub fn with_audio(mut self, audio: MotionAudioClient) -> Self {
        self.audio = Some(audio);
        self
    }

    /// Attach or replace the audio service behind this router.
    pub fn set_audio(&mut self, audio: Option<MotionAudioClient>) {
        self.audio = audio;
    }

    /// Whether an audio service is behind this router.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// The active model's motions, as `(group, index, name)`.
    ///
    /// The name a plugin writes is the group's own name with its index appended —
    /// `TapBody` at 0 is `TapBody.0` — because a model declares a *group* of motions
    /// and the group has the name. Read from the runtime's snapshot rather than from
    /// a table this router caches, because the active model changes under a running
    /// plugin and a stale table would answer "no such motion" for a model that has
    /// one.
    pub fn motions(&self) -> Vec<(MotionId, String)> {
        self.behaviors()
            .into_iter()
            .filter_map(|behavior| match behavior {
                ModelBehaviorSnapshot::Motion { group, index } => {
                    let motion = MotionId::new(group.clone(), index).ok()?;
                    let name = format!("{group}.{index}");
                    Some((motion, name))
                }
                ModelBehaviorSnapshot::Expression { .. } => None,
            })
            .collect()
    }

    /// The active model's expressions, as `(id, name)`.
    pub fn expressions(&self) -> Vec<(ExpressionId, String)> {
        self.behaviors()
            .into_iter()
            .filter_map(|behavior| match behavior {
                ModelBehaviorSnapshot::Expression { name } => {
                    let expression = ExpressionId::new(name.clone()).ok()?;
                    Some((expression, name))
                }
                ModelBehaviorSnapshot::Motion { .. } => None,
            })
            .collect()
    }

    fn behaviors(&self) -> Vec<ModelBehaviorSnapshot> {
        self.client
            .as_ref()
            .and_then(|client| client.snapshot().active_model)
            .map(|model| model.behaviors)
            .unwrap_or_default()
    }

    /// Whether a motion by this name is in the active model.
    pub fn has_motion(&self, name: &str) -> bool {
        self.find_motion(name).is_some()
    }

    /// Whether an expression by this name is in the active model.
    pub fn has_expression(&self, name: &str) -> bool {
        self.find_expression(name).is_some()
    }

    /// Carry one request out, or say why not.
    ///
    /// The subscription check comes first, then the overlay's visibility, then the
    /// model's own vocabulary — in that order because each is cheaper than the next
    /// and each answers a question the plugin would otherwise have to guess at. A
    /// plugin that did not subscribe is never asked to move the model, whatever the
    /// model happens to have.
    pub fn answer(&self, id: u64, request: &ModelRequest, subscribed: bool) -> ModelAnswer {
        ModelAnswer {
            id,
            outcome: self.outcome(request, subscribed),
        }
    }

    /// Whether a request is one this router answers, rather than one the worker draws.
    ///
    /// The split is by *owner*: a bubble is chrome above the model on the layer channel and
    /// belongs to the worker, and everything else is the model itself or the audio device
    /// and belongs here. One predicate rather than two lists of variants, because a
    /// variant added to the protocol and forgotten in a hand-written match is a request
    /// that silently goes nowhere.
    pub const fn routes_here(request: &ModelRequest) -> bool {
        !request.is_drawn_not_acted()
    }

    /// Play an audio file a plugin named, and say whether it was played.
    ///
    /// The subscription is checked first for the same reason it is for a motion: a plugin
    /// that did not ask for model reactions is never handed one, whatever the model happens
    /// to be doing. The path is checked by [`sound::resolve`] and the command is published
    /// to the product's own queue — a plugin never touches the audio device itself.
    ///
    /// A refused publication is `HostCannot` rather than `Done`, which is the honest answer:
    /// the audio queue refused the command, nothing was played, and a plugin told `Done`
    /// would believe otherwise.
    pub fn play_sound(&self, request: &ModelRequest, subscribed: bool) -> SoundOutcome {
        if !subscribed {
            return SoundOutcome::NotSubscribed;
        }
        let Some(audio) = &self.audio else {
            return SoundOutcome::Refused(sound::Refusal::NoVoice);
        };
        let (path, volume) = match sound::resolve(request) {
            Ok(resolved) => resolved,
            // The refusal is the path's own, so the log says which of the four it was
            // rather than collapsing every bad path into one reason.
            Err(SoundOutcome::Refused(refusal)) => return SoundOutcome::Refused(refusal),
            Err(other) => return other,
        };
        let Some(volume) = MotionAudioVolume::new(volume) else {
            return SoundOutcome::Refused(sound::Refusal::TooLarge);
        };
        match audio.try_publish_with_sequence(|sequence| MotionAudioCommand::Play {
            sequence,
            path,
            volume,
        }) {
            Ok(_) => SoundOutcome::Queued,
            // A queue that is full, recovering, or stopped is the same fact to a plugin:
            // this sound did not happen. Which of the four it was is in the audio service's
            // own diagnostics, which is where a refused publication is counted.
            Err(_) => SoundOutcome::Refused(sound::Refusal::NoVoice),
        }
    }

    /// The outcome alone, for a caller that is counting rather than answering.
    pub fn outcome(&self, request: &ModelRequest, subscribed: bool) -> ModelOutcome {
        if !subscribed {
            return ModelOutcome::NotSubscribed;
        }
        // Checked before the runtime, because a request the product has no command for
        // is refused identically whether or not a model is loaded — and a plugin that
        // got "the window is hidden" for something the window was never asked to show
        // would wait for a window.
        if matches!(request, ModelRequest::ClearExpression) {
            return ModelOutcome::HostCannot;
        }
        let Some(client) = &self.client else {
            // No runtime at all is the same fact as no window to show it on, from a
            // plugin's point of view: nothing was seen. The two are separated in the
            // log rather than in the answer, because the plugin cannot act on the
            // difference and a fourth refusal code would be one more thing to match.
            return ModelOutcome::OverlayHidden;
        };
        if !client.snapshot().overlay_visible {
            return ModelOutcome::OverlayHidden;
        }
        match request {
            ModelRequest::PlayMotion { name, .. } => match self.find_motion(name) {
                Some(motion) => {
                    self.trigger(
                        client,
                        ShortcutAction::StartMotion {
                            motion,
                            priority: MotionPriority::Normal,
                        },
                    );
                    ModelOutcome::Done
                }
                None => ModelOutcome::NotInModel {
                    kind: ModelRequestKind::Motion,
                },
            },
            ModelRequest::SetExpression { name } => match self.find_expression(name) {
                Some(expression) => {
                    self.trigger(client, ShortcutAction::SetExpression(expression));
                    ModelOutcome::Done
                }
                None => ModelOutcome::NotInModel {
                    kind: ModelRequestKind::Expression,
                },
            },
            // Unreachable: cleared above, before the runtime is consulted.
            ModelRequest::ClearExpression => ModelOutcome::HostCannot,
            // A bubble is drawn by the worker and a sound goes to the audio service, so
            // neither reaches this function: the worker takes both out of the message
            // before asking. Reaching here means one was routed the wrong way, and
            // answering "the host cannot do this" is the safe reading.
            ModelRequest::ShowBubble { .. }
            | ModelRequest::HideBubble
            | ModelRequest::PlaySound { .. } => ModelOutcome::HostCannot,
        }
    }

    /// Issue a shortcut action, treating a refused queue as a refusal.
    ///
    /// The runtime's queue is bounded and drops rather than blocks, so a model busy
    /// at 240 Hz can refuse. A refused request is answered `Done` anyway: the plugin
    /// asked for the model to react, the reaction was attempted, and whether the
    /// runtime had room at that instant is not something a plugin can act on.
    fn trigger(&self, client: &RuntimeClient, action: ShortcutAction) {
        let _ = client.trigger_shortcut(action);
    }

    /// The model's motion with this name, if it has one.
    ///
    /// Case-insensitive, because a group's name is written by whoever made the model
    /// and a plugin author will not know whether it was capitalised. The exact match
    /// is tried first so a model with two groups differing only in case still
    /// resolves the way its author wrote it.
    pub fn find_motion(&self, name: &str) -> Option<MotionId> {
        exact_then_insensitive(self.motions(), name, |(id, _)| id.clone())
    }

    /// The model's expression with this name, if it has one.
    pub fn find_expression(&self, name: &str) -> Option<ExpressionId> {
        exact_then_insensitive(self.expressions(), name, |(id, _)| id.clone())
    }
}

/// The first entry whose name matches exactly, then the first that matches case
/// insensitively.
fn exact_then_insensitive<T, K: Clone>(
    entries: Vec<(T, String)>,
    name: &str,
    key: impl Fn(&(T, String)) -> K,
) -> Option<K> {
    entries
        .iter()
        .find(|(_, candidate)| candidate == name)
        .or_else(|| {
            entries
                .iter()
                .find(|(_, candidate)| candidate.eq_ignore_ascii_case(name))
        })
        .map(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_protocol::LocalizedText;

    #[test]
    fn a_router_with_no_runtime_answers_rather_than_dropping() {
        let router = ModelRequestRouter::new(None);
        assert!(!router.is_attached());
        let answer = router.answer(
            1,
            &ModelRequest::PlayMotion {
                name: "wave".to_string(),
                restart: false,
            },
            true,
        );
        assert_eq!(
            answer.outcome,
            ModelOutcome::OverlayHidden,
            "a plugin can tell it was answered, which a dropped request would not let it do"
        );
        assert_eq!(answer.id, 1);
    }

    #[test]
    fn a_request_from_a_plugin_that_did_not_subscribe_is_refused_before_anything_else() {
        // The subscription is checked first because it is the cheapest and because it
        // is the only refusal a plugin can do something structural about.
        let router = ModelRequestRouter::new(None);
        let answer = router.answer(
            1,
            &ModelRequest::PlayMotion {
                name: "wave".to_string(),
                restart: false,
            },
            false,
        );
        assert_eq!(answer.outcome, ModelOutcome::NotSubscribed);
    }

    #[test]
    fn a_bubble_is_bounded_before_the_router_ever_sees_it() {
        // The bound is the protocol's, applied where the message is read, so the
        // router never sees a bubble longer than the model window can draw.
        let request = ModelRequest::ShowBubble {
            text: LocalizedText::from("x".repeat(500)),
            duration_ms: 0,
        }
        .sanitized();
        let ModelRequest::ShowBubble { text, duration_ms } = &request else {
            panic!("expected a bubble");
        };
        assert!(
            text.default.chars().count() <= bongocat_plugin_protocol::MAXIMUM_BUBBLE_CHARS,
            "a bubble is a fixed box, and a paragraph in it reads as a bug in the product"
        );
        assert!(*duration_ms >= bongocat_plugin_protocol::MINIMUM_BUBBLE_MILLIS);
    }

    #[test]
    fn a_model_with_nothing_loaded_has_no_motions_and_no_expressions() {
        let router = ModelRequestRouter::new(None);
        assert!(router.motions().is_empty());
        assert!(router.expressions().is_empty());
        assert!(!router.has_motion("TapBody.0"));
        assert!(!router.has_expression("happy"));
    }

    #[test]
    fn a_name_matches_exactly_before_it_matches_loosely() {
        // A model with both spellings resolves the way its author wrote it, and a
        // name that differs only in case still resolves rather than failing.
        let entries: Vec<(u32, String)> = vec![(1, "Wave".to_string()), (2, "wave".to_string())];
        assert_eq!(
            exact_then_insensitive(entries.clone(), "Wave", |(id, _)| *id),
            Some(1)
        );
        assert_eq!(
            exact_then_insensitive(entries.clone(), "wave", |(id, _)| *id),
            Some(2)
        );
        assert_eq!(
            exact_then_insensitive(entries, "WAVE", |(id, _)| *id),
            Some(1),
            "and a plugin that guessed the case still finds one"
        );
    }

    #[test]
    fn a_name_no_entry_matches_is_not_in_the_model() {
        let entries: Vec<(u32, String)> = vec![(1, "Wave".to_string())];
        assert_eq!(
            exact_then_insensitive(entries, "tap_head", |(id, _)| *id),
            None,
            "which is a fact the plugin is told, not an error it has to time out on"
        );
    }

    #[test]
    fn a_motions_name_is_the_group_and_its_index() {
        // A model declares a *group* of motions and the group carries the name, so a
        // plugin has to be able to name one clip within it.
        let behaviors = vec![
            ModelBehaviorSnapshot::Motion {
                group: "TapBody".to_string(),
                index: 0,
            },
            ModelBehaviorSnapshot::Motion {
                group: "TapBody".to_string(),
                index: 3,
            },
        ];
        let motions: Vec<(String, String)> = behaviors
            .into_iter()
            .filter_map(|behavior| match behavior {
                ModelBehaviorSnapshot::Motion { group, index } => {
                    let motion = MotionId::new(group.clone(), index).ok()?;
                    Some((format!("{group}.{index}"), motion.group().to_string()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            motions,
            vec![
                ("TapBody.0".to_string(), "TapBody".to_string()),
                ("TapBody.3".to_string(), "TapBody".to_string()),
            ],
            "so a plugin writes `TapBody.0` and gets that clip"
        );
    }

    #[test]
    fn a_request_the_runtime_has_no_command_for_is_answered_rather_than_dropped() {
        // The runtime has one expression command and no way to unset one. A plugin
        // that asks to clear is told the host cannot do it, rather than waiting for an
        // expression to change that never will.
        let router = ModelRequestRouter::new(None);
        let answer = router.answer(1, &ModelRequest::ClearExpression, true);
        assert_eq!(answer.outcome, ModelOutcome::HostCannot);
    }

    #[test]
    fn a_bubble_is_the_workers_and_not_this_routers() {
        // Two owners rather than one, because they are two systems: a bubble is
        // chrome on the layer channel and a motion is the model itself.
        assert!(!ModelRequestRouter::routes_here(
            &ModelRequest::ShowBubble {
                text: "hi".into(),
                duration_ms: 1000,
            }
        ));
        assert!(!ModelRequestRouter::routes_here(&ModelRequest::HideBubble));
        assert!(ModelRequestRouter::routes_here(&ModelRequest::PlayMotion {
            name: "wave".to_string(),
            restart: false
        }));
        assert!(ModelRequestRouter::routes_here(
            &ModelRequest::SetExpression {
                name: "happy".to_string()
            }
        ));
    }
}
