//! The words this plugin shows.
//!
//! Very few: the label it draws is the system's own name for the current input method, in
//! the user's own language, so there is no table of input methods to maintain here. What
//! this file holds is the card's own copy and the settings' labels.

use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Input Method")
        .with_locale("zh-CN", "输入法")
        .with_locale("zh-TW", "輸入法")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Shows which keyboard input method is selected, so you can see whether you are typing \\
         English or something else without opening the menu.",
    )
    .with_locale(
        "zh-CN",
        "显示当前选中的键盘输入法，让你不用打开菜单就知道自己在打英文还是别的。",
    )
    .with_locale(
        "zh-TW",
        "顯示目前選取的鍵盤輸入法，讓你不用打開選單就知道自己在打英文還是別的。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "⌨️";

/// Shown when the platform has no input methods to speak of.
pub fn unsupported() -> LocalizedText {
    LocalizedText::new("This platform does not report an input method")
        .with_locale("zh-CN", "此平台不提供输入法信息")
        .with_locale("zh-TW", "此平台不提供輸入法資訊")
}

/// Shown when the panel is switched off while no method is selected.
pub fn nothing_selected() -> LocalizedText {
    LocalizedText::new("No input method selected")
        .with_locale("zh-CN", "未选择输入法")
        .with_locale("zh-TW", "未選擇輸入法")
}

/// The label on the "only while not typing English" setting.
pub fn only_when_not_latin_label() -> LocalizedText {
    LocalizedText::new("Show only while a non-Latin method is selected")
        .with_locale("zh-CN", "仅在选中非拉丁输入法时显示")
        .with_locale("zh-TW", "僅在選中非拉丁輸入法時顯示")
}

/// What the "only while not typing English" setting is.
pub fn only_when_not_latin_help() -> LocalizedText {
    LocalizedText::new(
        "The usual arrangement: nothing on screen while you type English, and the method's name \\
         the moment you switch away from it.",
    )
    .with_locale(
        "zh-CN",
        "常见的做法：打英文时不显示任何东西，一旦切换到其他输入法就显示其名称。",
    )
    .with_locale(
        "zh-TW",
        "常見的做法：打英文時不顯示任何東西，一旦切換到其他輸入法就顯示其名稱。",
    )
}

/// The label on the "show the Latin method too" setting.
pub fn show_latin_label() -> LocalizedText {
    LocalizedText::new("Show the Latin method too")
        .with_locale("zh-CN", "也显示拉丁输入法")
        .with_locale("zh-TW", "也顯示拉丁輸入法")
}

/// What the "show the Latin method too" setting is.
pub fn show_latin_help() -> LocalizedText {
    LocalizedText::new(
        "On by default with the setting above switched off: a panel that never appears is a \\
         panel that cannot be checked.",
    )
    .with_locale(
        "zh-CN",
        "在关闭上一项时默认开启：从不出现的面板无法用来确认它在工作。",
    )
    .with_locale(
        "zh-TW",
        "在關閉上一項時預設開啟：從不出現的面板無法用來確認它在工作。",
    )
}

/// The label on the "shorten the name" setting.
pub fn shorten_label() -> LocalizedText {
    LocalizedText::new("Shorten a long name")
        .with_locale("zh-CN", "缩短过长的名称")
        .with_locale("zh-TW", "縮短過長的名稱")
}

/// What the "shorten the name" setting is.
pub fn shorten_help() -> LocalizedText {
    LocalizedText::new(
        "Takes the part after the last dot of the method's own identifier, which is what a \\
         reader recognises — `wetype.pinyin` becomes `pinyin`.",
    )
    .with_locale(
        "zh-CN",
        "取方法自身标识中最后一个点之后的部分，也就是人能认出的那部分——`wetype.pinyin` 会变成 `pinyin`。",
    )
    .with_locale(
        "zh-TW",
        "取方法自身標識中最後一個點之後的部分，也就是人能認出的那部分——`wetype.pinyin` 會變成 `pinyin`。",
    )
}

/// The label on the "play a motion when you switch" setting.
pub fn react_label() -> LocalizedText {
    LocalizedText::new("Have the cat react when you switch")
        .with_locale("zh-CN", "切换输入法时让猫做出反应")
        .with_locale("zh-TW", "切換輸入法時讓貓做出反應")
}

/// What the "play a motion when you switch" setting is.
pub fn react_help() -> LocalizedText {
    LocalizedText::new(
        "Plays one of the model's own motions, so a model with no such motion simply does \\
         nothing.",
    )
    .with_locale(
        "zh-CN",
        "播放模型自带的一个动作，因此没有该动作的模型不会有任何反应。",
    )
    .with_locale(
        "zh-TW",
        "播放模型自帶的一個動作，因此沒有該動作的模型不會有任何反應。",
    )
}

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}
