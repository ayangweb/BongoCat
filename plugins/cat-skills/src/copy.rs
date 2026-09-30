//! The words this plugin shows.
//!
//! Short, because most of what appears on this panel is *not* from here: a skill's answer is
//! whatever the command it ran said, and a reminder says whatever the user typed. What is in
//! here is the card's copy, the labels, and the sentences the plugin needs when a thing did
//! not work.

use bongocat_plugin_sdk::{Host, LocalizedText};

/// The card's name.
pub fn plugin_name() -> LocalizedText {
    LocalizedText::new("Cat Skills")
        .with_locale("zh-CN", "小猫技能")
        .with_locale("zh-TW", "小貓技能")
}

/// The card's one-sentence description.
pub fn plugin_description() -> LocalizedText {
    LocalizedText::new(
        "Lets the cat do things: run a command you choose and show what it said, and remind \
         you of something when the time comes.",
    )
    .with_locale(
        "zh-CN",
        "让猫去做事：运行你选择的命令并显示它的回复，并在时间到了的时候提醒你。",
    )
    .with_locale(
        "zh-TW",
        "讓貓去做事：執行你選擇的命令並顯示它的回覆，並在時間到了的時候提醒你。",
    )
}

/// The plugin's own emoji, which is the card's icon.
pub const ICON: &str = "🐈";

/// One of this plugin's strings, in the language the user reads.
pub fn say(host: &Host, text: &LocalizedText) -> String {
    text.resolve_bounded(host.locale())
}

/// Said on the panel while no skill has been set up.
pub fn no_skill() -> LocalizedText {
    LocalizedText::new("no skill set up")
        .with_locale("zh-CN", "还没有设置技能")
        .with_locale("zh-TW", "還沒有設定技能")
}

/// Said on the panel while the skill is running.
pub fn running() -> LocalizedText {
    LocalizedText::new("running…")
        .with_locale("zh-CN", "运行中…")
        .with_locale("zh-TW", "執行中…")
}

/// Said on the panel when the skill's command printed nothing.
pub fn said_nothing() -> LocalizedText {
    LocalizedText::new("the command said nothing")
        .with_locale("zh-CN", "命令没有输出")
        .with_locale("zh-TW", "命令沒有輸出")
}

/// Said when a skill's command did not finish in time.
pub fn timed_out() -> LocalizedText {
    LocalizedText::new("the command took too long and was stopped")
        .with_locale("zh-CN", "命令超时，已停止")
        .with_locale("zh-TW", "命令逾時，已停止")
}

/// The label over the answer.
pub fn answer_label() -> LocalizedText {
    LocalizedText::new("answer")
        .with_locale("zh-CN", "回复")
        .with_locale("zh-TW", "回覆")
}

/// The label over the armed reminder.
pub fn reminder_label() -> LocalizedText {
    LocalizedText::new("reminder")
        .with_locale("zh-CN", "提醒")
        .with_locale("zh-TW", "提醒")
}

/// Said when nothing is armed.
pub fn no_reminder() -> LocalizedText {
    LocalizedText::new("nothing armed")
        .with_locale("zh-CN", "没有提醒")
        .with_locale("zh-TW", "沒有提醒")
}

/// Said once, on the panel, that a reminder was armed.
pub fn armed() -> LocalizedText {
    LocalizedText::new("armed")
        .with_locale("zh-CN", "已设定")
        .with_locale("zh-TW", "已設定")
}

/// Said once, on the panel, that the reminder is done.
pub fn reminder_done() -> LocalizedText {
    LocalizedText::new("done")
        .with_locale("zh-CN", "已提醒")
        .with_locale("zh-TW", "已提醒")
}

/// The button that runs the skill now.
pub fn run_label() -> LocalizedText {
    LocalizedText::new("Run now")
        .with_locale("zh-CN", "立即运行")
        .with_locale("zh-TW", "立即執行")
}

/// The button that arms the reminder.
pub fn arm_label() -> LocalizedText {
    LocalizedText::new("Arm it")
        .with_locale("zh-CN", "设定提醒")
        .with_locale("zh-TW", "設定提醒")
}

/// The button that forgets the reminder.
pub fn clear_label() -> LocalizedText {
    LocalizedText::new("Forget it")
        .with_locale("zh-CN", "取消提醒")
        .with_locale("zh-TW", "取消提醒")
}

/// The label on the skill's command setting.
pub fn command_label() -> LocalizedText {
    LocalizedText::new("Command")
        .with_locale("zh-CN", "命令")
        .with_locale("zh-TW", "命令")
}

/// What the command setting is, including that nothing is run until it is filled in.
pub fn command_help() -> LocalizedText {
    LocalizedText::new(
        "The program and its arguments, split on spaces — quote a path that has spaces in it. \
         Nothing runs while this is empty. It is started directly rather than through a shell, \
         so nothing here can be a chain of commands, and `%s` in an argument is replaced with \
         the question below.",
    )
    .with_locale(
        "zh-CN",
        "程序及其参数，按空格分隔——路径中含空格时请加引号。此项为空时不会运行任何东西。命令会直接启动而不经过 shell，因此其中无法成为命令链；参数中的 `%s` 会替换成下面的问题。",
    )
    .with_locale(
        "zh-TW",
        "程式及其參數，按空白分隔——路徑中含有空白時請加引號。此項為空時不會執行任何東西。指令會直接啟動而不經過 shell，因此其中無法成為指令鏈；參數中的 `%s` 會替換成下面的問題。",
    )
}

/// The label on the question setting.
pub fn question_label() -> LocalizedText {
    LocalizedText::new("Question")
        .with_locale("zh-CN", "问题")
        .with_locale("zh-TW", "問題")
}

/// What the question setting is.
pub fn question_help() -> LocalizedText {
    LocalizedText::new(
        "Put this where `%s` is in the command. It is the text your command will be asked — \
         which is what makes it an agent rather than a lookup: point it at your own AI \
         command-line tool and the answer is whatever that tool says.",
    )
    .with_locale(
        "zh-CN",
        "放在命令中 `%s` 的位置。它就是命令会被问到的内容——这也是它成为 agent 而非查询的原因：把它指向你自己的 AI 命令行工具，回复就是那个工具给出的内容。",
    )
    .with_locale(
        "zh-TW",
        "放在指令中 `%s` 的位置。它就是指令會被問到的內容——這也是它成為 agent 而非查詢的原因：把它指向你自己的 AI 命令列工具，回覆就是那個工具給出的內容。",
    )
}

/// The label on the "run it again every so often" setting.
pub fn every_label() -> LocalizedText {
    LocalizedText::new("Run it again every")
        .with_locale("zh-CN", "重复运行间隔")
        .with_locale("zh-TW", "重複執行間隔")
}

/// What the "run it again every so often" setting is.
pub fn every_help() -> LocalizedText {
    LocalizedText::new(
        "In minutes. Zero runs it only when you press the button — which is the right answer \
         for anything that costs money or has a side effect. The shortest repeat is one minute, \
         so a command that was meant to run twice a second cannot be configured here.",
    )
    .with_locale(
        "zh-CN",
        "以分钟计。填 0 表示只在按下按钮时运行——对于任何会产生费用或副作用的命令，这通常是正确的选择。最短重复间隔为一分钟，因此无法把本来打算每秒运行两次的命令配置到这里。",
    )
    .with_locale(
        "zh-TW",
        "以分鐘計。填 0 表示只在按下按鈕時執行——對於任何會產生費用或副作用的指令，這通常是正確的選擇。最短重複間隔為一分鐘，因此無法把本來打算每秒執行兩次的指令設定到這裡。",
    )
}

/// The label on the "how long to wait for the answer" setting.
pub fn timeout_label() -> LocalizedText {
    LocalizedText::new("Wait at most")
        .with_locale("zh-CN", "最多等待")
        .with_locale("zh-TW", "最多等待")
}

/// What the wait setting is.
pub fn timeout_help() -> LocalizedText {
    LocalizedText::new(
        "In seconds, up to thirty. A command that has not answered by then is stopped, because \
         the answer to something that takes a minute is not worth having this late.",
    )
    .with_locale(
        "zh-CN",
        "以秒计，最多三十秒。到时仍未回复的命令会被停止，因为一分钟后的回复已经不值得等待。",
    )
    .with_locale(
        "zh-TW",
        "以秒計，最多三十秒。到時仍未回覆的指令會被停止，因為一分鐘後的回覆已經不值得等待。",
    )
}

/// The label on the motion setting.
pub fn motion_label() -> LocalizedText {
    LocalizedText::new("Motion when something arrives")
        .with_locale("zh-CN", "收到内容时的动作")
        .with_locale("zh-TW", "收到內容時的動作")
}

/// What the motion setting is.
pub fn motion_help() -> LocalizedText {
    LocalizedText::new(
        "A motion of your model's own, named group then number. Left blank the cat does not \
         move, which is fine — the bubble is the answer.",
    )
    .with_locale(
        "zh-CN",
        "模型自带的一个动作，名称为组名加序号。留空则猫不做动作，这没问题——气泡本身就是回复。",
    )
    .with_locale(
        "zh-TW",
        "模型自帶的一個動作，名稱為群組名稱加序號。留空則貓不做動作，這沒問題——氣泡本身就是回覆。",
    )
}

/// The label on the reminder's text setting.
pub fn reminder_text_label() -> LocalizedText {
    LocalizedText::new("Reminder text")
        .with_locale("zh-CN", "提醒内容")
        .with_locale("zh-TW", "提醒內容")
}

/// What the reminder text setting is.
pub fn reminder_text_help() -> LocalizedText {
    LocalizedText::new(
        "What the cat says when the reminder is due. Arm it from the panel. One reminder at a \
         time, and arming a second replaces the first.",
    )
    .with_locale(
        "zh-CN",
        "提醒到点时猫说的话。在面板上设定。同时只能有一个提醒，设定新的会替换旧的。",
    )
    .with_locale(
        "zh-TW",
        "提醒到點時貓說的話。在面板上設定。同時只能有一個提醒，設定新的會取代舊的。",
    )
}

/// The label on the reminder's delay setting.
pub fn reminder_minutes_label() -> LocalizedText {
    LocalizedText::new("Remind me in")
        .with_locale("zh-CN", "多少分钟后提醒")
        .with_locale("zh-TW", "幾分鐘後提醒")
}

/// What the reminder delay setting is.
pub fn reminder_minutes_help() -> LocalizedText {
    LocalizedText::new(
        "In minutes. Zero means the reminder is due the moment you arm it, which is a useful \
         way to check the text before trusting it with a real deadline.",
    )
    .with_locale(
        "zh-CN",
        "以分钟计。填 0 表示设定的瞬间就提醒，这是检验文字、再决定是否真的用它设一个截止时间的好办法。",
    )
    .with_locale(
        "zh-TW",
        "以分鐘計。填 0 表示設定的當下就提醒，這是檢視文字、再決定是否真的用它設一個截止時間的好辦法。",
    )
}

/// The label on the "only while the window is up" setting.
pub fn only_when_visible_label() -> LocalizedText {
    LocalizedText::new("Speak only while the model window is up")
        .with_locale("zh-CN", "仅在模型窗口显示时说话")
        .with_locale("zh-TW", "僅在模型視窗顯示時說話")
}

/// What the "only while the window is up" setting is.
pub fn only_when_visible_help() -> LocalizedText {
    LocalizedText::new(
        "A cat that is not on screen should be resting, so it does not pop a bubble at nothing. \
         The skill still runs on its schedule either way; only the saying is held back.",
    )
    .with_locale(
        "zh-CN",
        "猫不在桌面上时应该在休息，因此不会凭空弹出气泡。技能仍会按计划运行，只是暂不说话。",
    )
    .with_locale(
        "zh-TW",
        "貓不在桌面上時應該在休息，因此不會憑空彈出氣泡。技能仍會按計畫執行，只是暫不說話。",
    )
}
