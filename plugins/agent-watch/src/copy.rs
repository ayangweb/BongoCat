//! The words this plugin shows.
//!
//! Most of what appears on the panel is *not* from here: a state that has a bubble uses the
//! text the user's mapping gave it, and the event's own name and the tool's own name come
//! from the tool that sent them. What is in here is the card's copy, the labels, and the
//! English defaults — which a user reading the panel in another language replaces by writing
//! their own mapping, because a sentence about what an AI is doing is not one anybody else
//! can translate on the user's behalf.

use crate::event::Activity;
use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("AI Watch")
        .with_locale("zh-CN", "AI 监控")
        .with_locale("zh-TW", "AI 監控")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Watches what an AI coding tool is doing from its hook events, and has the cat react: \
         reading, writing, running, asking, finished, broken.",
    )
    .with_locale(
        "zh-CN",
        "通过 AI 编码工具的 hook 事件观察它正在做什么，并让猫做出反应：读、写、运行、询问、完成、出错。",
    )
    .with_locale(
        "zh-TW",
        "透過 AI 編碼工具的 hook 事件觀察它正在做什麼，並讓貓做出反應：讀、寫、執行、詢問、完成、出錯。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "👀";

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}

/// What a state is called on the panel, in the user's language.
///
/// The label rather than the stored name, because the stored name is a key in a mapping file
/// and a key is not something to put in front of a person. `Activity::name()` is for files;
/// this is for the panel.
pub fn activity_label(activity: Activity) -> LocalizedText {
    match activity {
        Activity::Idle => LocalizedText::new("waiting")
            .with_locale("zh-CN", "等待中")
            .with_locale("zh-TW", "等待中"),
        Activity::Thinking => LocalizedText::new("thinking")
            .with_locale("zh-CN", "思考中")
            .with_locale("zh-TW", "思考中"),
        Activity::Reading => LocalizedText::new("reading")
            .with_locale("zh-CN", "读文件")
            .with_locale("zh-TW", "讀檔案"),
        Activity::Writing => LocalizedText::new("writing")
            .with_locale("zh-CN", "写代码")
            .with_locale("zh-TW", "寫程式"),
        Activity::Searching => LocalizedText::new("looking")
            .with_locale("zh-CN", "搜索中")
            .with_locale("zh-TW", "搜尋中"),
        Activity::Running => LocalizedText::new("running")
            .with_locale("zh-CN", "运行命令")
            .with_locale("zh-TW", "執行指令"),
        Activity::Asking => LocalizedText::new("needs you")
            .with_locale("zh-CN", "等待你确认")
            .with_locale("zh-TW", "等待你確認"),
        Activity::Done => LocalizedText::new("all done")
            .with_locale("zh-CN", "完成了")
            .with_locale("zh-TW", "完成了"),
        Activity::Failed => LocalizedText::new("it broke")
            .with_locale("zh-CN", "出错了")
            .with_locale("zh-TW", "出錯了"),
    }
}

/// The line under the state when nothing has ever arrived.
pub fn nothing_watched() -> LocalizedText {
    LocalizedText::new("nothing yet — wire up the hook below")
        .with_locale("zh-CN", "还没有事件——按下面的说明接入 hook")
        .with_locale("zh-TW", "還沒有事件——按下面的說明接入 hook")
}

/// The line under the state when a tool has gone quiet.
pub fn quiet() -> LocalizedText {
    LocalizedText::new("quiet")
        .with_locale("zh-CN", "安静")
        .with_locale("zh-TW", "安靜")
}

/// Said about how many conversations are being watched.
///
/// The count is put in by the caller rather than by the text, because a `LocalizedText`
/// carries text and nothing else: there is no substitution in it, so a `{count}` here would
/// reach the panel as the literal characters `{count}`. One conversation is also worded
/// differently from several in every language here, because "1 sessions" is the kind of
/// thing a reader notices and forgives once.
pub fn sessions_label(host: &Host, count: usize) -> String {
    let text = if count == 1 {
        LocalizedText::new("1 session")
            .with_locale("zh-CN", "1 个会话")
            .with_locale("zh-TW", "1 個工作階段")
    } else {
        LocalizedText::new("{count} sessions")
            .with_locale("zh-CN", "{count} 个会话")
            .with_locale("zh-TW", "{count} 個工作階段")
    };
    say(host, &text).replace("{count}", &count.to_string())
}

/// The heading over the hook command the user has to paste somewhere.
///
/// A heading rather than a paragraph, because the command is the answer and everything else
/// is the explanation of it, and a person setting this up wants the command first.
pub fn hook_command_label() -> LocalizedText {
    LocalizedText::new("Hook command")
        .with_locale("zh-CN", "hook 命令")
        .with_locale("zh-TW", "hook 命令")
}

/// What to do with the hook command.
pub fn hook_command_help() -> LocalizedText {
    LocalizedText::new(
        "Paste this into your AI tool's hook settings, so it runs when the tool starts. The \
         command only writes one line to a file this plugin reads; it never sends anything \
         anywhere, and it never blocks your tool call.",
    )
    .with_locale(
        "zh-CN",
        "把这条命令填入 AI 工具的 hook 设置，让它在工具启动时运行。该命令只向本插件读取的一个文件追加一行，不会向外发送任何内容，也不会阻塞你的工具调用。",
    )
    .with_locale(
        "zh-TW",
        "把這條命令填入 AI 工具的 hook 設定，讓它在工具啟動時執行。該命令只向本外掛讀取的一個檔案追加一行，不會對外傳送任何內容，也不會阻塞你的工具呼叫。",
    )
}

/// The label on the "how long before it is quiet" setting.
pub fn idle_seconds_label() -> LocalizedText {
    LocalizedText::new("Seconds before it counts as quiet")
        .with_locale("zh-CN", "多少秒算安静")
        .with_locale("zh-TW", "多少秒算安靜")
}

/// What the "how long before it is quiet" setting is.
pub fn idle_seconds_help() -> LocalizedText {
    LocalizedText::new(
        "A tool that has not reported anything for this long is treated as waiting on you. \
         Tools that are slow between steps need more; a long value means the cat keeps \
         working after the work stopped.",
    )
    .with_locale(
        "zh-CN",
        "工具这么久没有报告任何事件，就认为它在等你。步骤之间较慢的工具需要更大值；值太大会让猫在工作结束后还继续忙。",
    )
    .with_locale(
        "zh-TW",
        "工具這麼久沒有回報任何事件，就認為它在等你。步驟之間較慢的工具需要更大值；值太大會讓貓在工作結束後還繼續忙。",
    )
}

/// The label on the "only while the window is up" setting.
pub fn only_when_visible_label() -> LocalizedText {
    LocalizedText::new("Watch only while the model window is up")
        .with_locale("zh-CN", "仅在模型窗口显示时监控")
        .with_locale("zh-TW", "僅在模型視窗顯示時監控")
}

/// What the "only while the window is up" setting is.
pub fn only_when_visible_help() -> LocalizedText {
    LocalizedText::new(
        "The cat is hidden on the desktop and should be resting, so its reactions are too. \
         The event queue keeps filling either way; only the reactions stop.",
    )
    .with_locale(
        "zh-CN",
        "猫不在桌面上时应该在休息，它的反应也该一样。事件队列照常记录，只有反应会停。",
    )
    .with_locale(
        "zh-TW",
        "貓不在桌面上時應該在休息，它的反應也該一樣。事件佇列照常記錄，只有反應會停。",
    )
}

/// The label on the "show the tool's own words" setting.
pub fn show_event_label() -> LocalizedText {
    LocalizedText::new("Show the event and tool names")
        .with_locale("zh-CN", "显示事件名与工具名")
        .with_locale("zh-TW", "顯示事件名與工具名")
}

/// What the "show the tool's own words" setting is.
pub fn show_event_help() -> LocalizedText {
    LocalizedText::new(
        "Off shows just the state. On adds the tool's own event and tool names, which is what \
         you want while you are wiring the hook up and off afterwards.",
    )
    .with_locale(
        "zh-CN",
        "关闭时只显示状态。打开后会附带工具自身的事件名与工具名——刚接入 hook 时需要，接好之后就不需要了。",
    )
    .with_locale(
        "zh-TW",
        "關閉時只顯示狀態。開啟後會附上工具自身的事件名與工具名——剛接入 hook 時需要，接好之後就不需要了。",
    )
}

/// The label on the mapping setting.
pub fn mapping_label() -> LocalizedText {
    LocalizedText::new("Custom mapping")
        .with_locale("zh-CN", "自定义映射")
        .with_locale("zh-TW", "自訂對應")
}

/// What the mapping setting is, and what a state with no entry means.
pub fn mapping_help() -> LocalizedText {
    LocalizedText::new(
        "A JSON object keyed by state — idle, thinking, reading, writing, searching, running, \
         asking, done, failed — with `motion` and `bubble` in each. A state you do not name \
         keeps its default; a state you name with an empty string does and says nothing.",
    )
    .with_locale(
        "zh-CN",
        "一个以状态为键的 JSON 对象——idle、thinking、reading、writing、searching、running、asking、done、failed——每个值里可写 `motion` 与 `bubble`。没有提到的状态保持默认；写成空字符串则既不做动作也不说话。",
    )
    .with_locale(
        "zh-TW",
        "一個以狀態為鍵的 JSON 物件——idle、thinking、reading、writing、searching、running、asking、done、failed——每個值裡可寫 `motion` 與 `bubble`。沒有提到的狀態保持預設；寫成空字串則既不做動作也不說話。",
    )
}

/// The label on the "replay the mapping" setting.
pub fn reset_mapping_label() -> LocalizedText {
    LocalizedText::new("Use the built-in mapping")
        .with_locale("zh-CN", "使用内置映射")
        .with_locale("zh-TW", "使用內建對應")
}

/// What the "replay the mapping" setting is.
pub fn reset_mapping_help() -> LocalizedText {
    LocalizedText::new(
        "Clears the mapping above, so the plugin's own defaults apply again. Your mapping is \
         only ever stored in the settings, so clearing it is all this does.",
    )
    .with_locale(
        "zh-CN",
        "清空上面的映射，改回插件自带的默认。你的映射只保存在设置里，所以清空就是全部。",
    )
    .with_locale(
        "zh-TW",
        "清空上面的對應，改回外掛自帶的預設。你的對應只保存在設定裡，所以清空就是全部。",
    )
}
