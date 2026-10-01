//! The sound a keystroke makes.
//!
//! Two sources, and the plugin does not have an opinion about which is better: the model's
//! own motion — which is where a sound belongs in this product, because the audio device
//! plays the clip a model attaches to a motion — and an audio file the user chose, for
//! somebody who wants a click rather than a cat.
//!
//! # Why the file is the host's to play
//!
//! A plugin is a separate process that knows nothing about the product's audio device, and
//! the honest way for it to make a noise is to *ask* for one rather than to open a device of
//! its own: two processes with the output device open is a thing the operating system
//! arbitrates badly and users hear as a stutter. So the file path is a value in this
//! plugin's own settings, and asking the host to play it is one more of the requests the
//! protocol already carries.
//!
//! What the host does with the path is its own business and is checked there: the plugin
//! hands over a string, and the host is the only side that knows which files it is willing
//! to open and how large one may be.

use bongocat_plugin_sdk::{LocalizedText, ModelRequest, SelfDescription};

use crate::settings::DEFAULT_MOTION;

/// The most a path may be, in bytes.
///
/// The protocol's own bound on a text value, restated here so a plugin that builds a
/// request from a hand-typed path fails on its own terms rather than at the host's.
/// A path longer than this is not a path on either platform this product ships.
pub const MAXIMUM_PATH_BYTES: usize = 4096;

/// Which sound a keystroke makes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    /// The model's own motion, and the clip that belongs to it.
    ///
    /// The default, and the answer for a model that has sounds at all: it is the sound the
    /// user already has, on a model they already chose.
    Model {
        /// The motion, in the model's own `Group.index` spelling.
        motion: String,
    },
    /// An audio file on this machine.
    ///
    /// A file rather than an upload because a plugin has no notion of an upload: a plugin
    /// reads a path, and the file is already on the machine the user is typing on.
    File {
        /// The path as the user wrote it.
        path: String,
    },
}

impl Source {
    /// The sound these settings ask for.
    ///
    /// Read through the SDK's typed accessors, so a field nobody has touched reads as its
    /// own default. The choice is a two-valued one rather than a path that may be empty,
    /// because "empty means the model's sound" and "a file I typed" are two different
    /// intentions and a single field cannot say which one the user meant.
    pub fn from(values: &bongocat_plugin_sdk::Values) -> Self {
        if values.text("sound_source") == FILE_SOURCE {
            let path = values.text("sound_path").trim().to_string();
            if path.is_empty() {
                // An empty path with "an audio file" chosen is a user who has not typed one
                // yet, and falling back to the model's sound is a sound rather than silence
                // with no explanation.
                return Self::Model {
                    motion: motion_name(values),
                };
            }
            return Self::File {
                path: path.chars().take(MAXIMUM_PATH_BYTES).collect(),
            };
        }
        Self::Model {
            motion: motion_name(values),
        }
    }

    /// The path this sound needs the host to open, when it needs one.
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Model { .. } => None,
            Self::File { path } => Some(path),
        }
    }

    /// The request this source becomes, or `None` when it is a motion.
    ///
    /// Built here rather than at each call site so a path this plugin would ask for is
    /// bounded in exactly one place: the protocol's own text bound is stricter than this
    /// one, so it is unreachable through the settings form — and a plugin that assembles a
    /// `Source` in its own code would otherwise put an unbounded string on the wire.
    pub fn as_request(&self, volume: f32) -> Option<ModelRequest> {
        match self {
            Self::Model { .. } => None,
            Self::File { path } => Some(ModelRequest::PlaySound {
                path: path.chars().take(MAXIMUM_PATH_BYTES).collect(),
                volume,
            }),
        }
    }

    /// What this source is called, in the language the user reads.
    ///
    /// Taken from the manifest rather than from a function per arm, so the two options in
    /// the settings form and the two sources here are two readings of one table — which is
    /// what stops a card saying "an audio file" and a settings form saying "a custom
    /// sound".
    pub fn label(&self, manifest: &SelfDescription) -> LocalizedText {
        match self {
            Self::Model { .. } => manifest.label("model_sound"),
            Self::File { .. } => manifest.label("custom_sound"),
        }
    }
}

/// The choice value that means "an audio file of your own".
pub const FILE_SOURCE: &str = "file";

/// The choice value that means "the model's own sound".
pub const MODEL_SOURCE: &str = "model";

/// The motion to play, never blank.
///
/// Blank is restored to the default rather than sent, for the same reason the pomodoro gives
/// for a blank round: a blank motion name can only ever be `NotInModel`, and a user who
/// cleared the box would get silence with no way to tell it from a model that has no such
/// motion.
fn motion_name(values: &bongocat_plugin_sdk::Values) -> String {
    let motion = values.text("motion");
    let motion = motion.trim();
    if motion.is_empty() {
        DEFAULT_MOTION.to_owned()
    } else {
        motion.to_owned()
    }
}
