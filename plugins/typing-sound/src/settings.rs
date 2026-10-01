//! What the user configured, and the settings that change it.
//!
//! Split from `main` because the settings and the playing are two different kinds of code:
//! one is a declaration the settings window renders, and the other is a decision this
//! plugin makes several times a second. They agree because both read the same
//! [`Preferences`], and a setting that did not reach the playing would be a control in the
//! settings window that does nothing.

use bongocat_plugin_sdk::prelude::*;

use crate::copy;
use crate::sound::{FILE_SOURCE, MODEL_SOURCE, Source};

/// The motion played when the user has not chosen another.
///
/// The shipped BongoCat models' first motion, in the protocol's own spelling: a group name
/// and an index. It is the motion that carries a clip, so it is the one that makes a noise
/// — and a model without it answers `NotInModel`, which this plugin handles by saying so
/// on its panel rather than by failing.
pub const DEFAULT_MOTION: &str = "CAT_motion.0";

/// The shortest gap this plugin will leave between two sounds, in milliseconds.
///
/// A third of a second: fast enough that ordinary typing is one sound per key, slow enough
/// that the few keys a second a fast typist produces do not merge into one held note. Zero
/// is allowed, because somebody who wants a sound per keystroke exactly should have it.
pub const DEFAULT_INTERVAL_MILLIS: i64 = 320;

/// The longest gap worth offering.
///
/// Longer than this and the setting is not a rhythm any more; it is a sound that happens
/// occasionally, which is a reminder rather than a typing sound.
pub const MAXIMUM_INTERVAL_MILLIS: i64 = 2_000;

/// How loud a keystroke is when the user has not chosen.
///
/// Three quarters rather than all of it: a keystroke happens several times a second, and a
/// full-volume click every third of a second is a sound people reach for the volume control
/// to fix within a minute.
pub const DEFAULT_VOLUME_PERCENT: i64 = 75;

/// The quietest a keystroke may be.
///
/// Zero is allowed and means silence, which is a real thing a user wants from a typing sound
/// plugin on a shared machine — so this is a switch to off rather than a lower bound.
pub const MINIMUM_VOLUME_PERCENT: i64 = 0;

/// What the user configured.
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    /// Which sound a keystroke makes.
    pub source: Source,
    /// The shortest gap between two sounds, in milliseconds.
    pub interval_ms: u64,
    /// How loud a keystroke is, from nothing to one.
    pub volume: f32,
    /// Whether a key the keyboard is repeating counts once or many times.
    pub skip_repeat: bool,
    /// Whether the mouse buttons count too.
    pub include_mouse: bool,
    /// Whether the sound happens as a key goes down rather than as it comes up.
    pub play_on_release: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            source: Source::Model {
                motion: DEFAULT_MOTION.to_owned(),
            },
            interval_ms: DEFAULT_INTERVAL_MILLIS as u64,
            volume: DEFAULT_VOLUME_PERCENT as f32 / 100.0,
            skip_repeat: true,
            include_mouse: false,
            play_on_release: false,
        }
    }
}

impl Preferences {
    /// The user's settings, as this build understands them.
    ///
    /// Read through the SDK's typed accessors, so a field nobody has touched reads as its
    /// own default. The volume is read as a percentage rather than as a fraction because a
    /// percentage is what a person can reason about: a spinner reading `75` is a number
    /// somebody can predict, and a spinner reading `0.75` is a number they have to trust.
    pub fn read(values: &Values) -> Self {
        Self {
            source: Source::from(values),
            interval_ms: values
                .integer("interval_ms")
                .clamp(0, MAXIMUM_INTERVAL_MILLIS) as u64,
            // Clamped as well as declared, for the same reason the pomodoro gives: the host
            // fits every document to the schema before it arrives, so this is unreachable
            // through the product — and it is here so that a `Values` built any other way
            // cannot produce a volume the device will refuse.
            volume: (values.integer("volume") as f32 / 100.0).clamp(0.0, 1.0),
            skip_repeat: values.flag("skip_repeat"),
            include_mouse: values.flag("include_mouse"),
            // Anything that is not the release key is a press, which is the same reading a
            // choice from a newer build gets: the value is not this build's to interpret
            // and the ordinary answer is the one it was most likely written as.
            play_on_release: values.text("play_on_release") == RELEASE_EDGE,
        }
    }

    /// The motion this source plays, when it plays the model's.
    pub fn motion(&self) -> Option<&str> {
        match &self.source {
            Source::Model { motion } => Some(motion),
            Source::File { .. } => None,
        }
    }
}

/// The value the edge setting stores for playing as a key comes up.
///
/// A stable key, never localized, because it is what the plugin reads out of its own file.
pub const RELEASE_EDGE: &str = "release";

/// The value the edge setting stores for playing as a key goes down.
pub const PRESS_EDGE: &str = "press";

/// The settings this plugin declares, which *are* the settings panel.
///
/// Seven rows, in the order somebody sets this plugin up in: what the sound is, how loud,
/// how often, and then what counts as a keystroke at all.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Choice::new(
                "sound_source",
                copy::sound_source_label(),
                vec![
                    Option_::new(MODEL_SOURCE, copy::model_sound()),
                    Option_::new(FILE_SOURCE, copy::custom_sound()),
                ],
            )
            .described(copy::sound_source_help())
            .into(),
        )
        .with(
            // A file field rather than a line of text, and the difference is the whole
            // reason this kind exists: a path a person has to know is not a path a person
            // can choose. The extensions are what the dialog offers — a request, not a
            // rule, because whether the bytes can be played is the host's judgement when
            // the sound is asked for.
            FileField::new("sound_path", copy::sound_path_label())
                .accepting(&["mp3", "wav", "flac", "m4a"])
                .described(copy::sound_path_help())
                .into(),
        )
        .with(
            Integer::ranged(
                "volume",
                copy::volume_label(),
                DEFAULT_VOLUME_PERCENT,
                MINIMUM_VOLUME_PERCENT,
                100,
            )
            .stepping(5)
            .with_unit("%")
            .described(copy::volume_help())
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
        .with(
            Choice::new(
                "play_on_release",
                copy::play_on_release_label(),
                vec![
                    Option_::new(PRESS_EDGE, copy::press_edge()),
                    Option_::new(RELEASE_EDGE, copy::release_edge()),
                ],
            )
            .described(copy::play_on_release_help())
            .into(),
        )
}
