//! The words this plugin shows, read from the `plugin.json` it ships.
//!
//! A keycap is not a word, so almost nothing here needs translating — the letters and the
//! arrows are the same in every language this product ships. What does need it is the
//! little bit of prose around them: the line shown when nothing is held, and the setting
//! labels, which the settings window renders in the user's own language.
//!
//! The copy lives in the manifest rather than in this file for the reason that made it
//! move. It used to be written twice — once here for the panel and once in `plugin.json`
//! for the card — and the two drifted, so a card changed its own text when the plugin
//! started.

use bongocat_plugin_sdk::{Host, LocalizedText, SelfDescription, describe};
use std::sync::LazyLock;

/// This plugin's own manifest, embedded at compile time.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// The line shown when nothing is held.
pub fn idle() -> LocalizedText {
    SELF.label("idle")
}

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

/// The label on the "hide when nothing is held" setting.
pub fn hide_when_idle_label() -> LocalizedText {
    SELF.label("hide_when_idle_label")
}

/// What the "hide when nothing is held" setting is.
pub fn hide_when_idle_help() -> LocalizedText {
    SELF.label("hide_when_idle_help")
}

/// One of this plugin's strings, in the language the user reads.
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
            "idle",
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
            "hide_when_idle_label",
            "hide_when_idle_help",
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
    fn this_plugin_speaks_every_language_the_product_ships() {
        for locale in ["zh-CN", "zh-TW", "ar-SA", "vi-VN", "pt-BR", "ko-KR"] {
            assert_eq!(
                idle().resolve(locale),
                manifest().text("idle", locale),
                "so the one line of prose the panel draws is in the reader's own language"
            );
        }
    }
}
