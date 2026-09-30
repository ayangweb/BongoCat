//! The words this plugin shows.
//!
//! A keycap is not a word, so almost nothing here needs translating — the letters and the
//! arrows are the same in every language this product ships. What does need it is the
//! little bit of prose around them: the line shown when nothing is held, and the setting
//! labels, which the settings window renders in the user's own language.

use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Key Display")
        .with_locale("zh-CN", "按键显示")
        .with_locale("zh-TW", "按鍵顯示")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Shows the keys you are holding, in the corner of the model window, so a screencast \\
         does not need a second window to prove which key you pressed.",
    )
    .with_locale(
        "zh-CN",
        "在模型窗口角落显示你正在按的键，录屏时不必再开一个窗口来证明按了哪个键。",
    )
    .with_locale(
        "zh-TW",
        "在模型視窗角落顯示你正在按的鍵，錄影時不必再開一個視窗來證明按了哪個鍵。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "⌨️";

/// The line shown when nothing is held.
pub fn idle() -> LocalizedText {
    LocalizedText::new("No keys held")
        .with_locale("zh-CN", "未按任何键")
        .with_locale("zh-TW", "未按任何鍵")
}

/// The label on the "how many keys" setting.
pub fn maximum_keys_label() -> LocalizedText {
    LocalizedText::new("Keys shown at once")
        .with_locale("zh-CN", "同时显示的按键数")
        .with_locale("zh-TW", "同時顯示的按鍵數")
}

/// What the "how many keys" setting is.
pub fn maximum_keys_help() -> LocalizedText {
    LocalizedText::new(
        "The newest keys are the ones that stay when you are holding more than this.",
    )
    .with_locale("zh-CN", "按下的按键超过这个数量时，保留最新的那些。")
    .with_locale("zh-TW", "按下的按鍵超過這個數量時，保留最新的那些。")
}

/// The label on the "include the mouse" setting.
pub fn mouse_label() -> LocalizedText {
    LocalizedText::new("Show the mouse buttons too")
        .with_locale("zh-CN", "同时显示鼠标按键")
        .with_locale("zh-TW", "同時顯示滑鼠按鍵")
}

/// What the "include the mouse" setting is.
pub fn mouse_help() -> LocalizedText {
    LocalizedText::new("Turn this off for a keyboard-only display.")
        .with_locale("zh-CN", "关闭后只显示键盘按键。")
        .with_locale("zh-TW", "關閉後只顯示鍵盤按鍵。")
}

/// The label on the "hide when nothing is held" setting.
pub fn hide_when_idle_label() -> LocalizedText {
    LocalizedText::new("Hide the panel when nothing is held")
        .with_locale("zh-CN", "没有按键时隐藏面板")
        .with_locale("zh-TW", "沒有按鍵時隱藏面板")
}

/// What the "hide when nothing is held" setting is.
pub fn hide_when_idle_help() -> LocalizedText {
    LocalizedText::new("A panel that says nothing is a box on your desktop.")
        .with_locale("zh-CN", "什么都不显示的面板只是桌面上一个空盒子。")
        .with_locale("zh-TW", "什麼都不顯示的面板只是桌面上一個空盒子。")
}

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}
