//! The words this plugin shows, read from the `plugin.json` it ships.
//!
//! A keycap is not a word, so nothing on the panel needs translating — the letters, the
//! arrows and the modifier glyphs are the same in every language this product ships, which
//! is the whole reason a key display needs no copy of its own. What does need it is the
//! settings window's labels, which the settings form renders in the user's own language.
//!
//! The copy lives in the manifest rather than in this file for the reason that made it
//! move. It used to be written twice — once here for the panel and once in `plugin.json`
//! for the card — and the two drifted, so a card changed its own text when the plugin
//! started.

use bongocat_plugin_sdk::{LocalizedText, SelfDescription, describe};
use std::sync::LazyLock;

/// This plugin's own manifest, embedded at compile time.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// The label on the "how many keys" setting.
pub fn maximum_keys_label() -> LocalizedText {
    SELF.label("maximum_keys_label")
}

/// What the "how many keys" setting is.
pub fn maximum_keys_help() -> LocalizedText {
    SELF.label("maximum_keys_help")
}

/// The label on the font-size setting.
pub fn font_size_label() -> LocalizedText {
    SELF.label("font_size_label")
}

/// What the font-size setting is.
pub fn font_size_help() -> LocalizedText {
    SELF.label("font_size_help")
}

/// The label on the font-weight setting.
pub fn font_weight_label() -> LocalizedText {
    SELF.label("font_weight_label")
}

/// What the font-weight setting is.
pub fn font_weight_help() -> LocalizedText {
    SELF.label("font_weight_help")
}

/// The first weight option: the product's normal weight.
pub fn regular() -> LocalizedText {
    SELF.label("regular")
}

/// The second weight option: bold.
pub fn bold() -> LocalizedText {
    SELF.label("bold")
}

/// The label on the "include the mouse" setting.
pub fn mouse_label() -> LocalizedText {
    SELF.label("mouse_label")
}

/// What the "include the mouse" setting is.
pub fn mouse_help() -> LocalizedText {
    SELF.label("mouse_help")
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
            "maximum_keys_label",
            "maximum_keys_help",
            "font_size_label",
            "font_size_help",
            "font_weight_label",
            "font_weight_help",
            "regular",
            "bold",
            "mouse_label",
            "mouse_help",
        ] {
            assert!(
                manifest().has(key),
                "this plugin asks for {key:?}, which it does not ship"
            );
        }
    }

    #[test]
    fn the_card_and_the_panel_read_one_document() {
        assert_eq!(manifest().id(), "keyboard-display");
        assert_eq!(manifest().version(), "1.0.0");
        assert_eq!(manifest().author(), "BongoCat");
        assert_eq!(manifest().emoji(), Some("⌨️"));
        assert_eq!(manifest().name().resolve("zh-CN"), "按键显示");
    }

    #[test]
    fn a_word_the_plugin_no_longer_draws_is_not_still_shipped() {
        // The panel used to say "No keys held" when nothing was held. It no longer draws
        // anything then, so the line is gone from the panel — and a copy entry left behind
        // is a sentence in seven languages that nothing in this plugin can ever show.
        for key in ["idle", "hide_when_idle_label", "hide_when_idle_help"] {
            assert!(
                !manifest().has(key),
                "{key:?} went with the feature that drew it, rather than staying in the \
                 manifest as copy nothing reads"
            );
        }
    }
}
