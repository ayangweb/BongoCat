//! The words this plugin shows.
//!
//! A tally is mostly numbers, so the prose here is labels, units, and the one sentence
//! that has to exist: what a "day" means when the plugin is the only thing counting.

use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Key Stats")
        .with_locale("zh-CN", "按键统计")
        .with_locale("zh-TW", "按鍵統計")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Counts the keys you press and how far the pointer travels, per day, on the model \\
         window. The tally is the plugin's own and stays in the plugin's own files.",
    )
    .with_locale(
        "zh-CN",
        "在模型窗口上按天统计你按下的按键与指针移动距离。记录由插件自己维护，保存在插件自己的文件里。",
    )
    .with_locale(
        "zh-TW",
        "在模型視窗上按天統計你按下的按鍵與指標移動距離。記錄由插件自己維護，存放在插件自己的檔案裡。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "📊";

/// The label over the key count.
pub fn keys_label() -> LocalizedText {
    LocalizedText::new("keys today")
        .with_locale("zh-CN", "今日按键")
        .with_locale("zh-TW", "今日按鍵")
}

/// The label over the pointer distance.
pub fn distance_label() -> LocalizedText {
    LocalizedText::new("pointer today")
        .with_locale("zh-CN", "今日移动")
        .with_locale("zh-TW", "今日移動")
}

/// Shown under a distance of nothing, because "0 cm" is a number and this is a fact.
pub fn no_movement() -> LocalizedText {
    LocalizedText::new("no movement yet")
        .with_locale("zh-CN", "尚未移动")
        .with_locale("zh-TW", "尚未移動")
}

/// Shown under a tally of nothing, for the same reason.
pub fn no_keys_yet() -> LocalizedText {
    LocalizedText::new("nothing pressed yet")
        .with_locale("zh-CN", "尚未按键")
        .with_locale("zh-TW", "尚未按鍵")
}

/// The line under the day's numbers, saying what the kept days add up to.
pub fn since_label() -> LocalizedText {
    LocalizedText::new("in the days kept")
        .with_locale("zh-CN", "在保留的天数内")
        .with_locale("zh-TW", "在保留的天數內")
}

/// The label over the list of the keys pressed most.
pub fn top_keys_label() -> LocalizedText {
    LocalizedText::new("most pressed")
        .with_locale("zh-CN", "按得最多")
        .with_locale("zh-TW", "按得最多")
}

/// The button that clears today's tally.
pub fn reset_label() -> LocalizedText {
    LocalizedText::new("Reset today")
        .with_locale("zh-CN", "重置今日")
        .with_locale("zh-TW", "重設今日")
}

/// The label on the "count the mouse" setting.
pub fn count_mouse_label() -> LocalizedText {
    LocalizedText::new("Count the pointer as well")
        .with_locale("zh-CN", "同时统计指针移动")
        .with_locale("zh-TW", "同時統計指標移動")
}

/// What the "count the mouse" setting is.
pub fn count_mouse_help() -> LocalizedText {
    LocalizedText::new("Turning this off counts keys only, and costs nothing while it is off.")
        .with_locale("zh-CN", "关闭后只统计按键，关闭时不产生任何开销。")
        .with_locale("zh-TW", "關閉後只統計按鍵，關閉時不產生任何開銷。")
}

/// The label on the units setting.
pub fn units_label() -> LocalizedText {
    LocalizedText::new("Distance in")
        .with_locale("zh-CN", "距离单位")
        .with_locale("zh-TW", "距離單位")
}

/// What the units setting is.
pub fn units_help() -> LocalizedText {
    LocalizedText::new(
        "Screen widths are exact. Centimetres assume a 24-inch screen, because the host \\
         measures distance in fractions of the model window and this plugin has to put a \\
         physical size on it somehow.",
    )
    .with_locale(
        "zh-CN",
        "「屏宽」是精确值。「厘米」假定屏幕为 24 英寸——宿主按模型窗口的比例给出距离，具体尺寸由本插件假定。",
    )
    .with_locale(
        "zh-TW",
        "「螢幕寬」是精確值。「公分」假定螢幕為 24 吋——宿主按模型視窗的比例給出距離，具體尺寸由本外掛假定。",
    )
}

/// The label on the history setting.
pub fn history_label() -> LocalizedText {
    LocalizedText::new("Days kept")
        .with_locale("zh-CN", "保留天数")
        .with_locale("zh-TW", "保留天數")
}

/// What the history setting is.
pub fn history_help() -> LocalizedText {
    LocalizedText::new(
        "Older days are dropped once this many are stored, so the file cannot grow without \\
         bound.",
    )
    .with_locale(
        "zh-CN",
        "存满这些天数后更早的记录会被丢弃，文件不会无限增长。",
    )
    .with_locale(
        "zh-TW",
        "存滿這些天數後更早的記錄會被丟棄，檔案不會無限增長。",
    )
}

/// The label on the "count auto-repeat" setting.
pub fn repeat_label() -> LocalizedText {
    LocalizedText::new("Count a key the keyboard is repeating")
        .with_locale("zh-CN", "统计键盘的自动重复")
        .with_locale("zh-TW", "統計鍵盤的自動重複")
}

/// What the "count auto-repeat" setting is.
pub fn repeat_help() -> LocalizedText {
    LocalizedText::new(
        "Off by default: a held key is one key, and counting the repeats counts the keyboard's \
         timer rather than your work.",
    )
    .with_locale(
        "zh-CN",
        "默认关闭：按住一个键只是一个键，统计重复统计的是键盘的定时器，而不是你的工作。",
    )
    .with_locale(
        "zh-TW",
        "預設關閉：按住一個鍵只是一個鍵，統計重複統計的是鍵盤的計時器，而不是你的工作。",
    )
}

/// The label on the "only count while the window is up" setting.
pub fn only_when_visible_label() -> LocalizedText {
    LocalizedText::new("Count only while the model window is up")
        .with_locale("zh-CN", "仅在模型窗口显示时统计")
        .with_locale("zh-TW", "僅在模型視窗顯示時統計")
}

/// What the "only count while the window is up" setting is.
pub fn only_when_visible_help() -> LocalizedText {
    LocalizedText::new(
        "For a tally that should measure working time rather than time away from the desk.",
    )
    .with_locale("zh-CN", "适合只统计实际工作时长，而不是离开电脑的时间。")
    .with_locale("zh-TW", "適合只統計實際工作時長，而不是離開電腦的時間。")
}

/// Centimetres, as a unit.
pub fn centimetres() -> LocalizedText {
    LocalizedText::new("cm")
        .with_locale("zh-CN", "厘米")
        .with_locale("zh-TW", "公分")
}

/// Screen widths, as a unit.
pub fn screen_widths() -> LocalizedText {
    LocalizedText::new("screen widths")
        .with_locale("zh-CN", "屏宽")
        .with_locale("zh-TW", "螢幕寬")
}

/// The number of centimetres a full model window is taken to be, on a 24-inch screen.
///
/// A sixteenth of a 24-inch diagonal: the *width* of a 16:9 screen is shorter than its
/// diagonal, and using the diagonal would make every distance about a fifth too large. The
/// arithmetic is in [`crate::distance`] so the number and the arithmetic that uses it stay
/// together.
pub const ASSUMED_DIAGONAL_INCHES: f64 = 24.0;

/// The aspect ratio a screen is assumed to have, for the same reason.
pub const ASSUMED_ASPECT: f64 = 16.0 / 9.0;

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}
