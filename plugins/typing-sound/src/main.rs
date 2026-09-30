//! A voice for the cat while you type.
//!
//! Issue #90 asked for typing to make a noise. This is that, and it is built on a fact
//! about the product that is worth stating because it decides the whole design: **a sound
//! in BongoCat belongs to a motion.** The audio device plays the clip a model attaches to a
//! motion, and the only way to make it play is to play that motion. There is no "play the
//! sound without the animation" request, and inventing one would be a change to the
//! product's audio rather than to its plugin system.
//!
//! So this plugin plays a motion, and the sound comes with it. That has two consequences
//! it owns rather than hides:
//!
//! * **The model supplies the voice.** The BongoCat models' first motion carries a clip;
//!   a model with no clip on its motions is silent, and there is nothing this plugin can
//!   do about that.
//! * **A burst of typing is one long motion.** A motion the model loops cannot be
//!   restarted eight times a second, so the plugin spaces its requests: the
//!   "shortest gap between two sounds" setting is not a nicety, it is what keeps a fast
//!   typist from holding one pose for as long as they type.
//!
//! Everything else is the plugin's: which keys count, whether a held key counts once or
//! many times, what the panel says, and the four settings the window renders.

mod copy;

use bongocat_plugin_sdk::prelude::*;

/// The motion played when the user has not chosen another.
///
/// The shipped BongoCat models' first motion, in the protocol's own spelling: a group name
/// and an index. It is the motion that carries a clip, so it is the one that makes a noise
/// — and a model without it answers `NotInModel`, which this plugin handles by saying so
/// on its panel rather than by failing.
const DEFAULT_MOTION: &str = "CAT_motion.0";

/// The shortest gap this plugin will leave between two sounds, in milliseconds.
///
/// A third of a second: fast enough that ordinary typing is one sound per key, slow enough
/// that the few keys a second a fast typist produces do not merge into one held note. Zero
/// is allowed, because somebody who wants a sound per keystroke exactly should have it.
const DEFAULT_INTERVAL_MILLIS: i64 = 320;

/// The longest gap worth offering.
///
/// Longer than this and the setting is not a rhythm any more; it is a sound that happens
/// occasionally, which is a reminder rather than a typing sound.
const MAXIMUM_INTERVAL_MILLIS: i64 = 2_000;

/// How long the panel keeps showing the key that last made a sound.
///
/// Long enough to read at a glance, short enough that the panel is not a permanent fixture
/// on a desktop where the user mostly is not typing. A tenth of a second is about the time
/// it takes to look at a corner of the screen, which is what this is for.
const LABEL_MILLIS: u64 = 900;

const PANEL_WIDTH: u32 = 130;
const PANEL_HEIGHT: u32 = 60;

/// What the user configured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    /// The motion to play, in the model's own `Group.index` spelling.
    pub motion: String,
    /// The shortest gap between two sounds, in milliseconds.
    pub interval_ms: u64,
    /// Whether a key the keyboard is repeating counts once or many times.
    pub skip_repeat: bool,
    /// Whether the mouse buttons count too.
    pub include_mouse: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            motion: DEFAULT_MOTION.to_owned(),
            interval_ms: DEFAULT_INTERVAL_MILLIS as u64,
            skip_repeat: true,
            include_mouse: false,
        }
    }
}

impl Preferences {
    /// Read through the SDK's typed accessors, so a field nobody has touched reads as its
    /// own default and there is no `unwrap_or` written twice.
    fn read(values: &Values) -> Self {
        // The motion name is trimmed and never allowed to be blank, because a blank name
        // can only ever be `NotInModel`, and a user who cleared the box would get silence
        // with no way to tell it from a model that has no such motion. The default is
        // restored instead, which is a sound rather than nothing.
        let motion = values.text("motion");
        let motion = if motion.trim().is_empty() {
            DEFAULT_MOTION.to_owned()
        } else {
            motion.trim().to_owned()
        };
        Self {
            motion,
            interval_ms: values
                .integer("interval_ms")
                .clamp(0, MAXIMUM_INTERVAL_MILLIS) as u64,
            skip_repeat: values.flag("skip_repeat"),
            include_mouse: values.flag("include_mouse"),
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            TextField::new("motion", copy::motion_label())
                .defaulting(DEFAULT_MOTION)
                .described(copy::motion_help())
                .into(),
        )
        .with(
            Integer::ranged(
                "interval_ms",
                copy::interval_label(),
                DEFAULT_INTERVAL_MILLIS,
                0,
                MAXIMUM_INTERVAL_MILLIS,
            )
            .stepping(20)
            .with_unit("ms")
            .described(copy::interval_help())
            .into(),
        )
        .with(
            Toggle::new("skip_repeat", copy::repeat_label())
                .described(copy::repeat_help())
                .into(),
        )
        .with(
            Toggle::new("include_mouse", copy::mouse_label())
                .described(copy::mouse_help())
                .into(),
        )
}

/// The whole plugin.
pub struct TypingSound {
    panel: Panel,
    preferences: Preferences,
    /// The session's elapsed time of the last sound, which is what the interval measures.
    last_sound_ms: Option<u64>,
    /// The session's elapsed time as of the last tick.
    now_ms: u64,
    /// What the panel last showed: the key, or the complaint.
    painted: Option<String>,
    /// Whether the panel is up.
    showing: bool,
    /// The request id of the motion this plugin is waiting on an answer for.
    ///
    /// One at a time, because one answer is all it can act on: a model that has the motion
    /// says so once, and a model that has not says so once, and either way the plugin has
    /// nothing left to learn from the same question.
    awaiting: Option<u64>,
    /// Whether the model's answer was that it has no such motion.
    missing_motion: bool,
    /// When the panel should take itself down, as a session time.
    hide_at_ms: Option<u64>,
}

impl TypingSound {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH, PANEL_HEIGHT)
                .anchored(PluginAnchor::TopRight)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.16)
                .with_opacity(0.9),
            preferences,
            last_sound_ms: None,
            now_ms: 0,
            painted: None,
            showing: false,
            awaiting: None,
            missing_motion: false,
            hide_at_ms: None,
        }
    }

    /// Whether enough time has passed for another sound.
    fn may_speak(&self) -> bool {
        match self.last_sound_ms {
            None => true,
            Some(last) => self.now_ms.saturating_sub(last) >= self.preferences.interval_ms,
        }
    }

    /// One key went down: play the motion, if the interval allows it.
    fn key_down(&mut self, control: &str, repeat: bool, host: &mut Host) {
        if repeat && self.preferences.skip_repeat {
            return;
        }
        if !self.may_speak() {
            return;
        }
        self.speak(control, host);
    }

    /// The mouse went down: the same decision, if the user asked for it.
    fn mouse_down(&mut self, host: &mut Host) {
        if !self.preferences.include_mouse || !self.may_speak() {
            return;
        }
        self.speak("", host);
    }

    /// Play the motion, and show what it was for.
    fn speak(&mut self, control: &str, host: &mut Host) {
        self.last_sound_ms = Some(self.now_ms);
        self.awaiting = Some(host.play_motion(&self.preferences.motion));
        self.painted = None;
        self.hide_at_ms = Some(self.now_ms.saturating_add(LABEL_MILLIS));
        self.draw(control, host);
    }

    /// Draw one chip: the key that just made a sound, or why there was none.
    fn draw(&mut self, control: &str, host: &mut Host) {
        let label = if self.missing_motion {
            copy::say(host, &copy::no_such_motion())
        } else if control.is_empty() {
            control_label("left").to_owned()
        } else {
            control_label(control).to_owned()
        };
        if self.painted.as_deref() == Some(label.as_str()) && self.showing {
            return;
        }
        self.panel.rebuild(|panel| {
            panel.surface(4.0, [10.0, 8.0], |content| {
                content.chip(&label, 13.0, [10.0, 6.0], 6.0);
            })
        });
        self.showing = true;
        host.show(&mut self.panel);
        self.painted = Some(label);
    }

    /// Take the panel down when its time is up.
    fn expire(&mut self, host: &mut Host) {
        if !self.showing {
            return;
        }
        if self.hide_at_ms.is_some_and(|at| self.now_ms >= at) {
            let _ = host.hide_panel();
            self.showing = false;
            self.painted = None;
            self.hide_at_ms = None;
        }
    }
}

impl Plugin for TypingSound {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("typing-sound", copy::plugin_name().resolve(""))
            .version(1, 0, 0)
            .author("BongoCat")
            .named(copy::plugin_name())
            .described(copy::plugin_description())
            .icon(copy::ICON)
            // Input for the keystrokes, and model reactions for the answer: without the
            // second the plugin could ask a question and never learn whether it worked.
            .subscribe(Subscription::Input)
            .subscribe(Subscription::ModelReaction)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> bongocat_plugin_sdk::Result<()> {
        self.preferences = Preferences::read(host.values());
        // Nothing is drawn until something has been said: a sound plugin that starts with a
        // panel on the desktop is a panel nobody asked for.
        let _ = host.hide_panel();
        Ok(())
    }

    fn on_input(&mut self, events: Vec<InputEvent>, host: &mut Host) {
        for event in &events {
            match event {
                InputEvent::KeyDown { control, repeat } => {
                    self.key_down(control, *repeat, host);
                }
                InputEvent::MouseButton { pressed: true, .. } => self.mouse_down(host),
                // A release, a move and a reset say nothing about when to make a noise. A
                // reset in particular must not clear `last_sound_ms`: the interval is about
                // not stacking sounds, and a lock screen is not a keystroke.
                _ => {}
            }
        }
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.now_ms = tick.elapsed_ms;
        self.expire(host);
    }

    fn on_answer(&mut self, id: u64, outcome: Outcome, host: &mut Host) {
        if self.awaiting != Some(id) {
            return;
        }
        self.awaiting = None;
        self.missing_mismatch(&outcome, host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // A different motion is a different question, so the old answer no longer applies:
        // leaving the complaint up after the user fixed the name would be a complaint about
        // a setting they have already changed.
        self.missing_motion = false;
        self.painted = None;
    }
}

impl TypingSound {
    /// Act on what the model said about the motion.
    fn missing_mismatch(&mut self, outcome: &Outcome, host: &mut Host) {
        use bongocat_plugin_sdk::ModelOutcome;
        let missing = matches!(
            outcome,
            ModelOutcome::NotInModel {
                kind: bongocat_plugin_sdk::ModelRequestKind::Motion
            }
        );
        if missing == self.missing_motion {
            return;
        }
        self.missing_motion = missing;
        if missing {
            // Say it once, on the panel, and keep it up: a user who set a name that does
            // not resolve needs to be told, and the panel is the only place this plugin has
            // to say anything.
            self.hide_at_ms = None;
            self.draw("", host);
        }
    }
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    TypingSound::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
        values_from,
    };
    use bongocat_plugin_sdk::{Host, ModelOutcome, Session, Values};

    fn configured(
        motion: &str,
        interval_ms: i64,
        skip_repeat: bool,
        include_mouse: bool,
    ) -> ConfigDocument {
        document(
            [
                ("motion".to_string(), ConfigValue::Text(motion.to_owned())),
                ("interval_ms".to_string(), ConfigValue::Integer(interval_ms)),
                ("skip_repeat".to_string(), ConfigValue::Bool(skip_repeat)),
                (
                    "include_mouse".to_string(),
                    ConfigValue::Bool(include_mouse),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn the_defaults() -> ConfigDocument {
        configured(DEFAULT_MOTION, DEFAULT_INTERVAL_MILLIS, true, false)
    }

    fn serve(
        plugin: &mut TypingSound,
        written: &WrittenMessages,
        locale: &str,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema = declared_settings().to_schema().expect("a valid schema");
        let values: Values = values_from(&config, &schema);
        let host = Host::new(
            written.writer(),
            IdentityBuilder::new()
                .id("typing-sound")
                .locale(locale)
                .build(),
            schema,
            values,
        )
        .expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session.serve(plugin, messages).expect("served");
    }

    fn key(control: &str) -> InputEvent {
        InputEvent::KeyDown {
            control: control.to_owned(),
            repeat: false,
        }
    }

    #[test]
    fn a_key_asks_the_model_to_play_the_motion_the_user_chose() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured("CAT_motion.1", DEFAULT_INTERVAL_MILLIS, true, false),
            Inbox::new().input(key("KeyA")).into_messages(),
        );
        let requests = model_requests(&written);
        assert_eq!(
            requests.len(),
            1,
            "one request per key, and nothing else asked"
        );
        assert!(
            matches!(&requests[0].1, ModelRequest::PlayMotion { name, .. } if name == "CAT_motion.1"),
            "and it names the motion the user set rather than the default: {:?}",
            requests[0].1
        );
    }

    #[test]
    fn a_burst_of_typing_is_a_rhythm_rather_than_one_held_note() {
        // A motion the model loops cannot be restarted eight times a second, so the
        // interval is what keeps a fast typist from holding one pose for as long as they
        // type. This is the whole reason the setting exists.
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        let mut inbox = Inbox::new();
        for (index, control) in ["KeyA", "KeyB", "KeyC", "KeyD"].into_iter().enumerate() {
            inbox = inbox.tick(index as u64 * 50).input(key(control));
        }
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        assert_eq!(
            model_requests(&written).len(),
            1,
            "four keys 50 ms apart inside a {DEFAULT_INTERVAL_MILLIS} ms gap, so one sound"
        );
    }

    #[test]
    fn a_key_after_the_gap_does_make_a_sound() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .tick(0)
                .input(key("KeyA"))
                .tick(DEFAULT_INTERVAL_MILLIS as u64)
                .input(key("KeyB"))
                .into_messages(),
        );
        assert_eq!(model_requests(&written).len(), 2);
    }

    #[test]
    fn a_gap_of_nothing_says_the_user_wants_one_sound_per_key() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        let mut inbox = Inbox::new();
        for (index, control) in ["KeyA", "KeyB", "KeyC"].into_iter().enumerate() {
            inbox = inbox.tick(index as u64).input(key(control));
        }
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(DEFAULT_MOTION, 0, true, false),
            inbox.into_messages(),
        );
        assert_eq!(
            model_requests(&written).len(),
            3,
            "so an interval of zero is a choice and not a bound"
        );
    }

    #[test]
    fn a_key_the_keyboard_is_repeating_is_one_key_and_so_is_one_sound() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .tick(DEFAULT_INTERVAL_MILLIS as u64)
                .input(InputEvent::KeyDown {
                    control: "KeyA".to_owned(),
                    repeat: true,
                })
                .into_messages(),
        );
        assert_eq!(
            model_requests(&written).len(),
            1,
            "because a held key is one key, and a sound per repeat is a dozen sounds for one \\
             keystroke"
        );
    }

    #[test]
    fn a_user_who_wants_a_sound_per_repeat_gets_one() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(DEFAULT_MOTION, 0, false, false),
            Inbox::new()
                .input(key("KeyA"))
                .input(InputEvent::KeyDown {
                    control: "KeyA".to_owned(),
                    repeat: true,
                })
                .into_messages(),
        );
        assert_eq!(model_requests(&written).len(), 2);
    }

    #[test]
    fn the_mouse_is_silent_unless_the_user_asked_for_it() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(InputEvent::MouseButton {
                    button: "left".to_owned(),
                    pressed: true,
                })
                .into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "a typing sound that clicked too would be a sound for everything"
        );

        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(DEFAULT_MOTION, DEFAULT_INTERVAL_MILLIS, true, true),
            Inbox::new()
                .input(InputEvent::MouseButton {
                    button: "left".to_owned(),
                    pressed: true,
                })
                .into_messages(),
        );
        assert_eq!(model_requests(&written).len(), 1);
    }

    #[test]
    fn the_panel_shows_the_key_that_made_the_sound() {
        // The sound itself may be off, muted, or absent from this model, so the one piece
        // of feedback this plugin can give is visual — and a user who set a motion name and
        // heard nothing has no other way to tell a typo from a silent model.
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new().input(key("KeyA")).into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "A"),
            "as a keycap rather than as a word: {labels:?}"
        );
    }

    #[test]
    fn a_model_without_the_motion_is_told_once_rather_than_failing() {
        // `NotInModel` is an answer, not a failure: a model that has no motion by that name
        // is a fact the plugin can show, and the alternative — a plugin that said nothing —
        // is indistinguishable from one that is not running.
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .answer(
                    1,
                    ModelOutcome::NotInModel {
                        kind: ModelRequestKind::Motion,
                    },
                )
                .into_messages(),
        );
        assert!(plugin.missing_motion);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels
                .iter()
                .any(|label| label == "This model has no motion by that name"),
            "and the panel says so: {labels:?}"
        );
    }

    #[test]
    fn an_answer_about_some_other_request_changes_nothing() {
        // The ids are the plugin's own, and a plugin that acted on any answer would be
        // reporting a model that has no motion when it has answered a different question.
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .answer(
                    99,
                    ModelOutcome::NotInModel {
                        kind: ModelRequestKind::Motion,
                    },
                )
                .into_messages(),
        );
        assert!(
            !plugin.missing_motion,
            "so a request id nobody is waiting for is not an answer to this one"
        );
    }

    #[test]
    fn a_model_that_has_the_motion_says_nothing_and_shows_the_key() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .answer(1, ModelOutcome::Done)
                .into_messages(),
        );
        assert!(!plugin.missing_motion);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(labels.iter().any(|label| label == "A"), "{labels:?}");
    }

    #[test]
    fn the_panel_takes_itself_down_so_it_is_not_a_fixture_on_the_desktop() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .tick(LABEL_MILLIS + 1)
                .into_messages(),
        );
        assert!(
            !plugin.showing,
            "because a sound plugin that leaves a keycap on the desktop is a fixture"
        );
    }

    #[test]
    fn a_release_a_move_and_a_reset_are_not_keystrokes() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(InputEvent::KeyUp {
                    control: "KeyA".to_owned(),
                })
                .input(InputEvent::MouseMove {
                    dx: 4.0,
                    dy: 4.0,
                    distance: 5.6,
                })
                .input(InputEvent::Reset {
                    reason: "session_lock".to_owned(),
                })
                .into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "and a lock screen is not a keystroke, so the interval is not reset by one either"
        );
    }

    #[test]
    fn a_motion_name_the_user_emptied_falls_back_to_the_default_rather_than_to_silence() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured("   ", DEFAULT_INTERVAL_MILLIS, true, false),
            Inbox::new().input(key("KeyA")).into_messages(),
        );
        assert_eq!(
            plugin.preferences.motion, DEFAULT_MOTION,
            "because a blank name can only ever be NotInModel, and a user who cleared the box \\
             would get silence with no way to tell it from a model that has no such motion"
        );
    }

    #[test]
    fn a_motion_name_with_spaces_around_it_is_trimmed() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured("  CAT_motion.0  ", DEFAULT_INTERVAL_MILLIS, true, false),
            Inbox::new().input(key("KeyA")).into_messages(),
        );
        assert_eq!(plugin.preferences.motion, "CAT_motion.0");
    }

    #[test]
    fn a_hand_written_interval_past_the_bound_lands_on_it() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(DEFAULT_MOTION, 1_000_000, true, false),
            Inbox::new().into_messages(),
        );
        assert_eq!(
            plugin.preferences.interval_ms, MAXIMUM_INTERVAL_MILLIS as u64,
            "because past the bound a sound is a reminder rather than a typing sound"
        );
    }

    #[test]
    fn changing_the_motion_forgets_the_old_answer() {
        // Otherwise the complaint stays up after the user has fixed the name, and it is a
        // complaint about a setting they have already changed.
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA"))
                .answer(
                    1,
                    ModelOutcome::NotInModel {
                        kind: ModelRequestKind::Motion,
                    },
                )
                .config(configured(
                    "CAT_motion.0",
                    DEFAULT_INTERVAL_MILLIS,
                    true,
                    false,
                ))
                .into_messages(),
        );
        assert!(!plugin.missing_motion);
    }

    #[test]
    fn the_plugin_asks_for_both_feeds_it_needs() {
        // Input for the keystrokes, and model reactions for the answer: without the second
        // the plugin could ask a question and never learn whether it worked.
        let plugin = TypingSound::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "typing-sound");
        assert!(descriptor.subscribes_to(Subscription::Input));
        assert!(descriptor.subscribes_to(Subscription::ModelReaction));
        descriptor
            .check()
            .expect("this plugin's own descriptor is one the host accepts");
    }

    #[test]
    fn nothing_is_drawn_until_something_has_been_said() {
        let written = WrittenMessages::new();
        let mut plugin = TypingSound::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new().tick(0).tick(1000).into_messages(),
        );
        assert!(
            panels(&written).is_empty(),
            "because a sound plugin that starts with a panel on the desktop is a panel nobody \\
             asked for"
        );
        assert!(!plugin.showing);
    }
}
