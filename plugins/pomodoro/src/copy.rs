//! The words this plugin says, in the user's language.
//!
//! Every string a pomodoro shows or a settings form labels lives here, with its own
//! translations, and the application never learns what any of them are called. That is
//! the point of a plugin owning its copy: adding a language is a change to this file
//! and to nothing else in the repository.
//!
//! The host's locale is the only one used. A plugin that picked its own would show one
//! language on one panel and another on the next, and the user would have no way to
//! tell which of the two is wrong.

use bongocat_plugin_sdk::{Action, ActionGlyph, Host, LocalizedText};

use crate::{Button, TOGGLE};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Pomodoro")
        .with_locale("zh-CN", "番茄钟")
        .with_locale("zh-TW", "番茄鐘")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "A focus timer on the model window. Start it, stop it, and let the countdown run while \
         you work.",
    )
    .with_locale(
        "zh-CN",
        "模型窗口上的专注计时器。开始、暂停，让倒计时在你工作期间继续走。",
    )
    .with_locale(
        "zh-TW",
        "模型視窗上的專注計時器。開始、暫停，讓倒數計時在你工作期間繼續走。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "🍅";

/// What a focus round is for.
pub fn focus() -> LocalizedText {
    LocalizedText::new("Focus")
        .with_locale("zh-CN", "专注")
        .with_locale("zh-TW", "專注")
}

/// A short pause.
pub fn short_break() -> LocalizedText {
    LocalizedText::new("Short break")
        .with_locale("zh-CN", "短休息")
        .with_locale("zh-TW", "短休息")
}

/// A long pause.
pub fn long_break() -> LocalizedText {
    LocalizedText::new("Long break")
        .with_locale("zh-CN", "长休息")
        .with_locale("zh-TW", "長休息")
}

/// A round that is not running.
pub fn paused() -> LocalizedText {
    LocalizedText::new("Paused")
        .with_locale("zh-CN", "已暂停")
        .with_locale("zh-TW", "已暫停")
}

/// The button that starts a stopped timer.
pub fn start() -> LocalizedText {
    LocalizedText::new("Start")
        .with_locale("zh-CN", "开始")
        .with_locale("zh-TW", "開始")
}

/// The button that stops a running timer.
pub fn pause() -> LocalizedText {
    LocalizedText::new("Pause")
        .with_locale("zh-CN", "暂停")
        .with_locale("zh-TW", "暫停")
}

/// The button that starts a stopped one again.
pub fn resume() -> LocalizedText {
    LocalizedText::new("Resume")
        .with_locale("zh-CN", "继续")
        .with_locale("zh-TW", "繼續")
}

/// The button that throws the current round away.
pub fn reset() -> LocalizedText {
    LocalizedText::new("Reset")
        .with_locale("zh-CN", "重置")
        .with_locale("zh-TW", "重設")
}

/// Said when a stretch of work ends.
pub fn round_finished() -> LocalizedText {
    LocalizedText::new("Round finished")
        .with_locale("zh-CN", "本轮完成")
        .with_locale("zh-TW", "本輪完成")
}

/// Said when a pause ends.
pub fn break_over() -> LocalizedText {
    LocalizedText::new("Break over")
        .with_locale("zh-CN", "休息结束")
        .with_locale("zh-TW", "休息結束")
}

/// The label on the round-length setting.
pub fn focus_minutes_label() -> LocalizedText {
    LocalizedText::new("Focus round")
        .with_locale("zh-CN", "专注时长")
        .with_locale("zh-TW", "專注時長")
}

/// What the round-length setting is.
pub fn focus_minutes_help() -> LocalizedText {
    LocalizedText::new("How long one stretch of work lasts.")
        .with_locale("zh-CN", "每一段专注持续多久。")
        .with_locale("zh-TW", "每一段專注持續多久。")
}

/// The suffix on the round-length number.
pub fn minutes_unit() -> LocalizedText {
    LocalizedText::new("min")
        .with_locale("zh-CN", "分钟")
        .with_locale("zh-TW", "分鐘")
}

/// The label on the "what follows a round" setting.
pub fn after_round_label() -> LocalizedText {
    LocalizedText::new("After a round")
        .with_locale("zh-CN", "一轮结束后")
        .with_locale("zh-TW", "一輪結束後")
}

/// What the "what follows a round" setting is.
pub fn after_round_help() -> LocalizedText {
    LocalizedText::new("What happens when the timer reaches zero.")
        .with_locale("zh-CN", "计时归零时会发生什么。")
        .with_locale("zh-TW", "計時歸零時會發生什麼。")
}

/// The first option: keep working.
pub fn then_focus() -> LocalizedText {
    LocalizedText::new("Another round straight away")
        .with_locale("zh-CN", "直接开始下一轮")
        .with_locale("zh-TW", "直接開始下一輪")
}

/// The second option: a short pause.
pub fn then_short_break() -> LocalizedText {
    LocalizedText::new("A short break")
        .with_locale("zh-CN", "短休息")
        .with_locale("zh-TW", "短休息")
}

/// The third option: a long pause.
pub fn then_long_break() -> LocalizedText {
    LocalizedText::new("A long break")
        .with_locale("zh-CN", "长休息")
        .with_locale("zh-TW", "長休息")
}

/// The label on the automatic-start setting.
pub fn auto_start_label() -> LocalizedText {
    LocalizedText::new("Start the next round automatically")
        .with_locale("zh-CN", "自动开始下一轮")
        .with_locale("zh-TW", "自動開始下一輪")
}

/// What the automatic-start setting is.
pub fn auto_start_help() -> LocalizedText {
    LocalizedText::new("Turn this off to decide each round yourself.")
        .with_locale("zh-CN", "关闭后每一轮都由你决定何时开始。")
        .with_locale("zh-TW", "關閉後每一輪都由你決定何時開始。")
}

/// One of this plugin's strings, in the language the user reads.
///
/// `resolve` rather than a match on the tag, because a plugin that wrote Traditional
/// Chinese for `zh-TW` and Simplified for `zh-CN` must not have to answer "which of
/// the ten tags is this?" before it can show a word.
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
