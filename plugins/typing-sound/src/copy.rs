//! The words this plugin shows, read from the `plugin.json` it ships.
//!
//! Every string is a key in the `copy` table of the manifest, with its own translations,
//! and the application never learns what any of them are called. The copy lives in the
//! manifest rather than in this file for the reason that made it move: it used to be
//! written twice — once here for the panel and once in `plugin.json` for the card — and
//! the two drifted, so a card changed its own text when the plugin started.
//!
//! The host's locale is the only one used. A plugin that picked its own would show one
//! language on one panel and another on the next, and the user would have no way to
//! tell which of the two is wrong.

use bongocat_plugin_sdk::{Host, LocalizedText, SelfDescription, describe};
use std::sync::LazyLock;

/// This plugin's own manifest, embedded at compile time.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// Shown on the panel when the model has no motion by the name the user set.
///
/// The one thing this plugin must say out loud, because everything else it does is
/// invisible: a user who set a motion name and hears nothing has no other way to tell a
/// typo from a model that simply has no such motion.
pub fn no_such_motion() -> LocalizedText {
    SELF.label("no_such_motion")
}

/// Shown on the panel when the audio file the user chose could not be played.
///
/// The same reasoning as [`no_such_motion`], and the same reason it is a sentence rather
/// than a failure: a sound that does not happen is invisible, so the only way a user can
/// learn their file was moved, renamed or in a format the decoder will not read is for
/// the panel to say so.
pub fn no_such_sound_file() -> LocalizedText {
    SELF.label("no_such_sound_file")
}

/// The label on the sound-source setting.
pub fn sound_source_label() -> LocalizedText {
    SELF.label("sound_source_label")
}

/// What the sound-source setting is.
pub fn sound_source_help() -> LocalizedText {
    SELF.label("sound_source_help")
}

/// The first option: the model's own sound.
pub fn model_sound() -> LocalizedText {
    SELF.label("model_sound")
}

/// The second option: an audio file the user chose.
pub fn custom_sound() -> LocalizedText {
    SELF.label("custom_sound")
}

/// The label on the audio-file setting.
pub fn sound_path_label() -> LocalizedText {
    SELF.label("sound_path_label")
}

/// What the audio-file setting is.
pub fn sound_path_help() -> LocalizedText {
    SELF.label("sound_path_help")
}

/// The label on the volume setting.
pub fn volume_label() -> LocalizedText {
    SELF.label("volume_label")
}

/// What the volume setting is.
pub fn volume_help() -> LocalizedText {
    SELF.label("volume_help")
}

/// The label on the interval setting.
pub fn interval_label() -> LocalizedText {
    SELF.label("interval_label")
}

/// What the interval setting is.
pub fn interval_help() -> LocalizedText {
    SELF.label("interval_help")
}

/// The label on the auto-repeat setting.
pub fn repeat_label() -> LocalizedText {
    SELF.label("repeat_label")
}

/// What the auto-repeat setting is.
pub fn repeat_help() -> LocalizedText {
    SELF.label("repeat_help")
}

/// The label on the mouse setting.
pub fn mouse_label() -> LocalizedText {
    SELF.label("mouse_label")
}

/// What the mouse setting is.
pub fn mouse_help() -> LocalizedText {
    SELF.label("mouse_help")
}

/// The label on the play-on-release setting.
pub fn play_on_release_label() -> LocalizedText {
    SELF.label("play_on_release_label")
}

/// What the play-on-release setting is.
pub fn play_on_release_help() -> LocalizedText {
    SELF.label("play_on_release_help")
}

/// The label on the press-edge option.
pub fn press_edge() -> LocalizedText {
    SELF.label("press_edge")
}

/// The label on the release-edge option.
pub fn release_edge() -> LocalizedText {
    SELF.label("release_edge")
}

/// One of this plugin's strings, in the language the user reads.
///
/// Every caller has a [`Host`] to hand — this plugin is one session in one process — so
/// the locale is never remembered here. A module-level cache of "the language" would be a
/// second copy of a fact the host already publishes on every tick, and it would be wrong
/// for one tick after a user switched language.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest, read without the `LazyLock`, so a test can say what it is asserting
    /// about rather than about a static that happens to be initialised already.
    fn manifest() -> &'static SelfDescription {
        &SELF
    }

    #[test]
    fn every_string_this_plugin_asks_for_is_one_it_ships() {
        for key in [
            "no_such_motion",
            "no_such_sound_file",
            "sound_source_label",
            "sound_source_help",
            "model_sound",
            "custom_sound",
            "sound_path_label",
            "sound_path_help",
            "volume_label",
            "volume_help",
            "interval_label",
            "interval_help",
            "repeat_label",
            "repeat_help",
            "mouse_label",
            "mouse_help",
            "play_on_release_label",
            "play_on_release_help",
        ] {
            assert!(
                manifest().has(key),
                "this plugin asks for {key:?}, which it does not ship"
            );
        }
    }

    #[test]
    fn the_card_and_the_panel_read_one_document() {
        assert_eq!(manifest().id(), "typing-sound");
        assert_eq!(manifest().version(), "1.0.0");
        assert_eq!(manifest().author(), "BongoCat");
        assert_eq!(manifest().emoji(), Some("🔊"));
        assert_eq!(manifest().name().resolve("zh-CN"), "打字音效");
    }

    #[test]
    fn this_plugin_speaks_every_language_the_product_ships() {
        for locale in ["zh-CN", "zh-TW", "ar-SA", "vi-VN", "pt-BR", "ko-KR"] {
            assert_eq!(
                no_such_motion().resolve(locale),
                manifest().text("no_such_motion", locale),
                "so a complaint a user can read in their own language is one the manifest \
                 actually carries"
            );
        }
    }
}
