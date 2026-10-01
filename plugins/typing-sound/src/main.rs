//! A voice for the cat while you type.
//!
//! Issue #90 asked for typing to make a noise. This is that, and it is built on two facts
//! about the product that are worth stating because between them they decide the whole
//! design:
//!
//! * **A sound in BongoCat belongs to a motion, or to a file the user chose.** The audio
//!   device plays the clip a model attaches to a motion, and the only way to make it play
//!   is to play that motion. There is no "play the sound without the animation" request for
//!   a model's own clips, and inventing one would be a change to the product's audio rather
//!   than to its plugin system. What a plugin *can* do is ask the host to play an audio file,
//!   which is what the second source is: a click of the user's own rather than a meow.
//! * **A plugin cannot make a noise by opening the device itself.** Two processes with the
//!   output device open is a thing the operating system arbitrates badly, and users hear it
//!   as a stutter. So this plugin *asks*, and the host is the only side that knows which
//!   files it will open.
//!
//! So this plugin asks for one of two things, and owns the consequences rather than hiding
//! them:
//!
//! * **The model may be silent.** A model with no clip on its motions makes no sound at all,
//!   and there is nothing this plugin can do about that — so it says so on its panel rather
//!   than leaving the user to wonder.
//! * **An audio file may be unreachable.** A path the user typed can be moved, renamed, or in
//!   a format the decoder will not read, and the host answers that as a refusal rather than
//!   as silence, so this plugin can say which happened.
//! * **A burst of typing is one long sound.** Whatever the source, a sound that repeats
//!   eight times a second is a held note, so the plugin spaces its requests: the
//!   "shortest gap between two sounds" setting is not a nicety.
//!
//! Everything else is the plugin's: which keys count, whether a held key counts once or many
//! times, whether the sound happens as a key goes down or comes up, what the panel says, and
//! the seven settings the window renders.

mod copy;
mod settings;
mod sound;

#[cfg(test)]
mod tests;

use bongocat_plugin_sdk::prelude::*;
use settings::Preferences;
use sound::Source;
use std::sync::LazyLock;

/// This plugin's own manifest, embedded at compile time.
///
/// One document for the identity the card shows and the words the panel draws. See
/// [`bongocat_plugin_sdk::SelfDescription`] for why it is embedded rather than read, and
/// [`copy`] for the words themselves.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// How long the panel keeps showing the key that last made a sound.
///
/// Long enough to read at a glance, short enough that the panel is not a permanent fixture
/// on a desktop where the user mostly is not typing. A tenth of a second is about the time
/// it takes to look at a corner of the screen, which is what this is for.
const LABEL_MILLIS: u64 = 900;

const PANEL_WIDTH: u32 = 130;
const PANEL_HEIGHT: u32 = 60;

/// The settings this plugin declares, re-exported for the tests that assert on them.
use settings::declared_settings;

/// The whole plugin.
pub struct TypingSound {
    panel: Panel,
    preferences: Preferences,
    /// The session's elapsed time of the last sound, which is what the interval measures.
    last_sound_ms: Option<u64>,
    /// The session's elapsed time as of the last tick.
    now_ms: u64,
    /// The keys currently held, when the sound is set to happen on release.
    ///
    /// A set rather than one string because a chord is several keys: the release of the
    /// second finger must not be read as the end of the chord, and the sound is wanted per
    /// key that comes up rather than per chord.
    pressed: Vec<String>,
    /// What the panel last showed: the key, or the complaint.
    painted: Option<String>,
    /// Whether the panel is up.
    showing: bool,
    /// The request id of the sound this plugin is waiting on an answer for.
    ///
    /// One at a time, because one answer is all it can act on: a model that has the motion
    /// says so once, a model that has not says so once, and either way the plugin has
    /// nothing left to learn from the same question.
    awaiting: Option<u64>,
    /// Whether the last answer was that the model has no such motion.
    missing_motion: bool,
    /// Whether the last answer was that the audio file could not be played.
    missing_file: bool,
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
            pressed: Vec::new(),
            painted: None,
            showing: false,
            awaiting: None,
            missing_motion: false,
            missing_file: false,
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

    /// One key went down.
    fn key_down(&mut self, control: &str, repeat: bool, host: &mut Host) {
        if repeat && self.preferences.skip_repeat {
            return;
        }
        if self.preferences.play_on_release {
            // The press is only remembered here; the sound waits for the release. Held as a
            // set rather than as one string because a chord is several keys and the release
            // of the second one must not be the end of the chord.
            self.pressed.push(control.to_owned());
            return;
        }
        self.speak(control, host);
    }

    /// One key came up.
    ///
    /// Only speaks for a key this plugin believes was held, which is the whole reason the
    /// held set exists on the release edge: a release the platform sends for a key that was
    /// never pressed — because a reset arrived in between, or because the platform's own set
    /// and ours disagree — would otherwise make a sound for a keystroke that did not happen.
    fn key_up(&mut self, control: &str, host: &mut Host) {
        if !self.preferences.play_on_release {
            return;
        }
        let Some(position) = self.pressed.iter().position(|held| held == control) else {
            return;
        };
        self.pressed.remove(position);
        self.speak(control, host);
    }

    /// The mouse went down: the same decision, if the user asked for it.
    fn mouse_down(&mut self, host: &mut Host) {
        if !self.preferences.include_mouse || !self.may_speak() {
            return;
        }
        self.speak("", host);
    }

    /// Ask for the sound, and show what it was for.
    fn speak(&mut self, control: &str, host: &mut Host) {
        if !self.may_speak() {
            return;
        }
        self.last_sound_ms = Some(self.now_ms);
        // The complaint is cleared here rather than on the answer, so a user who fixes the
        // path and types again sees the key rather than the old error for a moment.
        self.missing_motion = false;
        self.missing_file = false;
        self.awaiting = Some(match &self.preferences.source {
            Source::Model { motion } => host.play_motion(motion),
            // Built through `as_request` so the bound on a path this plugin asks for lives
            // in one place rather than in the call and in the reader of the setting.
            Source::File { .. } => {
                let request = self
                    .preferences
                    .source
                    .as_request(self.preferences.volume)
                    .expect("a file is a request");
                host.request(request)
            }
        });
        self.painted = None;
        self.hide_at_ms = Some(self.now_ms.saturating_add(LABEL_MILLIS));
        self.draw(control, host);
    }

    /// Draw one chip: the key that just made a sound, or why there was none.
    fn draw(&mut self, control: &str, host: &mut Host) {
        let label = self.label_for(control, host);
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

    /// What the panel says right now.
    ///
    /// One function rather than a decision at each call site, so the complaint and the key
    /// cannot both be drawn and so the two complaints cannot be confused: both are "no
    /// sound", and which one is the only thing the user can act on.
    fn label_for(&self, control: &str, host: &Host) -> String {
        if self.missing_motion {
            return copy::say(host, &copy::no_such_motion());
        }
        if self.missing_file {
            return copy::say(host, &copy::no_such_sound_file());
        }
        if control.is_empty() {
            control_label("left").to_owned()
        } else {
            control_label(control).to_owned()
        }
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
        // The manifest says who this plugin is, and the descriptor is a projection of it
        // rather than a second place spelling the same six fields out. Adding a plugin that
        // keeps its metadata in its own `plugin.json` is then a change to that one file, and
        // a card that said one thing before the plugin started and another after is not
        // expressible.
        SELF.descriptor()
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
                InputEvent::KeyUp { control } => self.key_up(control, host),
                InputEvent::MouseButton { pressed: true, .. } => self.mouse_down(host),
                // A mouse release and a move say nothing about when to make a noise: the
                // setting is about keys, and a move is not an edge at all. A reset says
                // nothing either, and in particular must not clear `last_sound_ms` — the
                // interval is about not stacking sounds, and a lock screen is not a
                // keystroke. It does clear the held set, because a key the platform has
                // forgotten is not a key that is still down.
                InputEvent::MouseButton { pressed: false, .. } | InputEvent::MouseMove { .. } => {}
                InputEvent::Reset { .. } => self.pressed.clear(),
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
        self.note_outcome(&outcome, host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // A different source is a different question, so the old answer no longer applies:
        // leaving the complaint up after the user fixed the name or the path would be a
        // complaint about a setting they have already changed.
        self.missing_motion = false;
        self.missing_file = false;
        self.painted = None;
    }
}

/// What the plugin does with one answer.
impl TypingSound {
    /// Act on what the host said about the sound it asked for.
    ///
    /// The two refusals are different facts and the panel has to say which: "this model has
    /// no motion called that" is fixed in the model settings, and "that file could not be
    /// read" is fixed in the file. A user told the wrong one looks in the wrong place.
    ///
    /// The model request and the sound request answer the same way — `HostCannot` for
    /// anything the product will not do — so which of the two this plugin asked is read off
    /// the source it holds rather than off the answer, which is the only place the
    /// distinction exists.
    fn note_outcome(&mut self, outcome: &Outcome, host: &mut Host) {
        use bongocat_plugin_sdk::{ModelOutcome, ModelRequestKind};
        let from_model = matches!(self.preferences.source, Source::Model { .. });
        let refused = match outcome {
            ModelOutcome::Done => false,
            ModelOutcome::NotInModel { kind } => {
                // A model that answered "no such motion" to a sound request cannot happen —
                // the sound path never names a motion — so the kind is read rather than
                // assumed, and an unexpected one is treated as "the file".
                from_model && *kind == ModelRequestKind::Motion
            }
            _ => !matches!(outcome, ModelOutcome::NotSubscribed),
        };
        let missing = from_model && refused;
        let file = !from_model && refused;
        if missing == self.missing_motion && file == self.missing_file {
            return;
        }
        self.missing_motion = missing;
        self.missing_file = file;
        if missing || file {
            // Say it once, on the panel, and keep it up: a user who set a name or a path
            // that does not resolve needs to be told, and the panel is the only place this
            // plugin has to say anything.
            self.hide_at_ms = None;
            self.draw("", host);
        }
    }
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    TypingSound::new(Preferences::default()).run()
}
