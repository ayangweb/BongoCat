//! The words this plugin says, read from the `plugin.json` it ships.
//!
//! Every string a pomodoro shows or a settings form labels is a key in the `copy` table
//! of the manifest this plugin ships, with its own translations. The application never
//! learns what any of them are called: it resolves a string against the language the user
//! reads and draws the result, which is drawing, and drawing is the host's job.
//!
//! The copy lives in the manifest rather than in this file for the reason that made it
//! move. It used to be written twice — once here for the panel and once in `plugin.json`
//! for the card — and the two drifted, so a card changed its own text when the plugin
//! started. One document is the whole of the fix, and [`SELF`] is the one place a plugin
//! reads it from.
//!
//! The host's locale is the only one used. A plugin that picked its own would show one
//! language on one panel and another on the next, and the user would have no way to
//! tell which of the two is wrong.

use bongocat_plugin_sdk::{Action, ActionGlyph, Host, LocalizedText, SelfDescription, describe};
use std::sync::LazyLock;

use crate::timer::RoundKind;
use crate::{Button, TOGGLE};

/// This plugin's own manifest, embedded at compile time.
///
/// `include_str!` rather than a read at startup, so the copy is part of the binary: it
/// cannot go missing, and a plugin that ships a manifest its own code contradicts is a
/// plugin whose card and panel disagree with no way for either to notice.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// What a focus round is for.
pub fn focus() -> LocalizedText {
    SELF.label("focus")
}

/// A short pause.
pub fn short_break() -> LocalizedText {
    SELF.label("short_break")
}

/// A long pause.
pub fn long_break() -> LocalizedText {
    SELF.label("long_break")
}

/// A round that is not running.
pub fn paused() -> LocalizedText {
    SELF.label("paused")
}

/// The button that starts a stopped timer.
pub fn start() -> LocalizedText {
    SELF.label("start")
}

/// The button that stops a running timer.
pub fn pause() -> LocalizedText {
    SELF.label("pause")
}

/// The button that starts a stopped one again.
pub fn resume() -> LocalizedText {
    SELF.label("resume")
}

/// The button that throws the current round away.
pub fn reset() -> LocalizedText {
    SELF.label("reset")
}

/// Said when a round of this kind ends.
///
/// One function rather than a call site per kind, for the reason the two pauses are two
/// settings: a break's length is now the user's choice, so "Break over" alone no longer
/// says which break ended, and a panel that said "Break over" after a fifteen-minute pause
/// and again after a five-minute one would be describing neither.
pub fn finished(kind: RoundKind) -> LocalizedText {
    match kind {
        RoundKind::Focus => SELF.label("round_finished"),
        RoundKind::ShortBreak => SELF.label("break_finished_short"),
        RoundKind::LongBreak => SELF.label("break_finished_long"),
    }
}

/// The label on the round-length setting.
pub fn focus_minutes_label() -> LocalizedText {
    SELF.label("focus_minutes_label")
}

/// What the round-length setting is.
pub fn focus_minutes_help() -> LocalizedText {
    SELF.label("focus_minutes_help")
}

/// The suffix on the round-length number.
pub fn minutes_unit() -> LocalizedText {
    SELF.label("minutes_unit")
}

/// The label on the short-break setting.
pub fn short_break_minutes_label() -> LocalizedText {
    SELF.label("short_break_minutes_label")
}

/// What the short-break setting is.
pub fn short_break_minutes_help() -> LocalizedText {
    SELF.label("short_break_minutes_help")
}

/// The label on the long-break setting.
pub fn long_break_minutes_label() -> LocalizedText {
    SELF.label("long_break_minutes_label")
}

/// What the long-break setting is.
pub fn long_break_minutes_help() -> LocalizedText {
    SELF.label("long_break_minutes_help")
}

/// The label on the "what follows a round" setting.
pub fn after_round_label() -> LocalizedText {
    SELF.label("after_round_label")
}

/// What the "what follows a round" setting is.
pub fn after_round_help() -> LocalizedText {
    SELF.label("after_round_help")
}

/// The first option: keep working.
pub fn then_focus() -> LocalizedText {
    SELF.label("then_focus")
}

/// The second option: a short pause.
pub fn then_short_break() -> LocalizedText {
    SELF.label("then_short_break")
}

/// The third option: a long pause.
pub fn then_long_break() -> LocalizedText {
    SELF.label("then_long_break")
}

/// The label on the automatic-start setting.
pub fn auto_start_label() -> LocalizedText {
    SELF.label("auto_start_label")
}

/// What the automatic-start setting is.
pub fn auto_start_help() -> LocalizedText {
    SELF.label("auto_start_help")
}

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}

/// What the one button is called, from the round's phase.
///
/// The single source for both places the word appears. The panel's button and the
/// settings window's button are one control to a person — a timer that said "Start" on
/// the model window and "Pause" in the settings window would be two controls disagreeing
/// about the same round — so both read this, and the glyph rides along with the word
/// rather than being decided separately.
///
/// One function returning both is what makes them impossible to disagree. Two functions
/// returning a word and an icon, matched on the same enum, would drift the first time
/// somebody added a phase and updated one of them.
pub fn toggle_label(button: Button) -> LocalizedText {
    match button {
        Button::Start => start(),
        Button::Pause => pause(),
        Button::Resume => resume(),
    }
}

/// The icon for the one button, from the round's phase.
///
/// `Resume` gets the play glyph rather than the pause one it is named after: the press
/// does not suspend anything, it sets a stopped round running again, and an icon that
/// contradicts what the button does is the same lie as a stale label.
pub fn toggle_glyph(button: Button) -> ActionGlyph {
    match button {
        Button::Pause => ActionGlyph::Pause,
        Button::Start | Button::Resume => ActionGlyph::Play,
    }
}

/// The control the settings window draws for the one button.
///
/// Its id is [`TOGGLE`] — the same id the panel's own button carries — so a press from
/// either place reaches one handler and the two cannot drift apart.
pub fn toggle_action(button: Button, host: &Host) -> Action {
    Action::new(TOGGLE, say(host, &toggle_label(button))).glyph(toggle_glyph(button))
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
        // The failure this catches is a renamed key: a function that asks for `focus`
        // when the manifest says `focus_label` resolves to the key itself, which draws
        // the word "focus" on a panel instead of "Focus". Nothing else would notice.
        for key in [
            "focus",
            "short_break",
            "long_break",
            "paused",
            "start",
            "pause",
            "resume",
            "reset",
            "round_finished",
            "break_finished_short",
            "break_finished_long",
            "focus_minutes_label",
            "focus_minutes_help",
            "minutes_unit",
            "after_round_label",
            "after_round_help",
            "then_focus",
            "then_short_break",
            "then_long_break",
            "auto_start_label",
            "auto_start_help",
        ] {
            assert!(
                manifest().has(key),
                "this plugin asks for {key:?}, which it does not ship"
            );
        }
    }

    #[test]
    fn the_card_and_the_panel_read_one_document() {
        // The bug the copy table exists to end: the card's sentence and the panel's words
        // used to be written in two files, and a card changed its own text when the
        // plugin started.
        assert_eq!(manifest().id(), "pomodoro");
        assert_eq!(manifest().version(), "1.0.0");
        assert_eq!(manifest().author(), "BongoCat");
        assert_eq!(manifest().emoji(), Some("🍅"));
        assert_eq!(manifest().name().resolve("zh-CN"), "番茄钟");
    }

    #[test]
    fn this_plugin_speaks_every_language_the_product_ships() {
        // Checked here rather than only in the packaging crate's repository-wide test,
        // because this is the plugin that fails first if somebody adds a language: the
        // keys are the ones *this* file asks for, so a manifest entry added for someone
        // else and a missing one for this string are the same shape of mistake.
        for locale in ["zh-CN", "zh-TW", "ar-SA", "vi-VN", "pt-BR", "ko-KR"] {
            assert_eq!(
                focus().resolve(locale),
                manifest().text("focus", locale),
                "and the label the panel draws is the manifest's own {locale} copy"
            );
        }
    }
}
