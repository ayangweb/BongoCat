//! The words this plugin shows.
//!
//! Very little: a keycap is not a word, and the two sentences this plugin needs are the
//! only prose in it.

use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Typing Sound")
        .with_locale("zh-CN", "打字音效")
        .with_locale("zh-TW", "打字音效")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Gives the cat a voice while you type: each key plays one of the model's own motions, \\
         and the model's sound for it.",
    )
    .with_locale(
        "zh-CN",
        "打字时让猫发出声音：每按一个键就播放模型自带的一个动作，以及该动作自带的声音。",
    )
    .with_locale(
        "zh-TW",
        "打字時讓貓發出聲音：每按一個鍵就播放模型自帶的一個動作，以及該動作自帶的聲音。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "🔊";

/// Shown on the panel when the model has no motion by the name the user set.
///
/// The one thing this plugin must say out loud, because everything else it does is
/// invisible: a user who set a motion name and hears nothing has no other way to tell a
/// typo from a model that simply has no such motion.
pub fn no_such_motion() -> LocalizedText {
    LocalizedText::new("This model has no motion by that name")
        .with_locale("zh-CN", "这个模型没有同名动作")
        .with_locale("zh-TW", "這個模型沒有同名動作")
}

/// The label on the motion setting.
pub fn motion_label() -> LocalizedText {
    LocalizedText::new("Motion to play")
        .with_locale("zh-CN", "播放的动作")
        .with_locale("zh-TW", "播放的動作")
}

/// What the motion setting is, and how to find the name.
pub fn motion_help() -> LocalizedText {
    LocalizedText::new(
        "A model's motions are named group then number, like CAT_motion.0. The sound played is \\
         the one that belongs to that motion.",
    )
    .with_locale(
        "zh-CN",
        "模型的动作以「组名.序号」命名，例如 CAT_motion.0。播放的声音是该动作自带的声音。",
    )
    .with_locale(
        "zh-TW",
        "模型的動作以「組名.序號」命名，例如 CAT_motion.0。播放的聲音是該動作自帶的聲音。",
    )
}

/// The label on the interval setting.
pub fn interval_label() -> LocalizedText {
    LocalizedText::new("Shortest gap between two sounds")
        .with_locale("zh-CN", "两次声音的最短间隔")
        .with_locale("zh-TW", "兩次聲音的最短間隔")
}

/// What the interval setting is.
pub fn interval_help() -> LocalizedText {
    LocalizedText::new(
        "Raising this turns a burst of typing into a rhythm instead of one long sound.",
    )
    .with_locale(
        "zh-CN",
        "调高后，一串快速输入会变成有节奏的声音，而不是一个长音。",
    )
    .with_locale(
        "zh-TW",
        "調高後，一串快速輸入會變成有節奏的聲音，而不是一個長音。",
    )
}

/// The label on the auto-repeat setting.
pub fn repeat_label() -> LocalizedText {
    LocalizedText::new("Ignore a key the keyboard is repeating")
        .with_locale("zh-CN", "忽略键盘的自动重复")
        .with_locale("zh-TW", "忽略鍵盤的自動重複")
}

/// What the auto-repeat setting is.
pub fn repeat_help() -> LocalizedText {
    LocalizedText::new(
        "A held key is one key, so a held key should be one sound rather than a dozen.",
    )
    .with_locale(
        "zh-CN",
        "按住一个键只是一个键，所以应该只响一次而不是十几次。",
    )
    .with_locale(
        "zh-TW",
        "按住一個鍵只是一個鍵，所以應該只響一次而不是十幾次。",
    )
}

/// The label on the mouse setting.
pub fn mouse_label() -> LocalizedText {
    LocalizedText::new("Play a sound for the mouse buttons too")
        .with_locale("zh-CN", "鼠标按键也播放声音")
        .with_locale("zh-TW", "滑鼠按鍵也播放聲音")
}

/// What the mouse setting is.
pub fn mouse_help() -> LocalizedText {
    LocalizedText::new("Turn this off for keys only.")
        .with_locale("zh-CN", "关闭后只对键盘按键发声。")
        .with_locale("zh-TW", "關閉後只對鍵盤按鍵發聲。")
}

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}
