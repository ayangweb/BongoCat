# ADR-0080: 按住一个物理修饰键临时恢复 overlay 交互

状态：已接受（2026-10-04）
补充：ADR-0053（「开关门禁」统一禁用绑定规则）、ADR-0057（右键拖动期间悬停隐藏不生效）、ADR-0079（按键层叠放）
实现：Issue #970

## 背景

Issue #970 报告：开启「鼠标穿透」和「鼠标悬停时隐藏」之后，窗口既抓不住也看不见，唯一能拖动它的
办法是先关掉这两项、拖过去、再打开，而报告者需要的是**一个能按住不放的键**。issue 原文同时提到
窗口穿透与鼠标悬停隐藏两项，维护者确认按 issue 处理：两者一起被这个键暂停。

范围里有两件互相独立、但都被同一条需求卡住的事：

1. **运行期**：一个已配置的修饰键按下时，overlay 暂停穿透与悬停隐藏；松开后立即恢复。
2. **设置期**：一个录入控件，只能录入**一个**修饰键，按下即完成，并且**必须区分左右**——
   左 Shift 与右 Shift 是两个不同的配置值。

第 2 项决定了整个方案的形状。把左右区分清楚这件事，GPUI 做不到：

- macOS 按下修饰键产生的是 `NSFlagsChanged`，`parse_keystroke` 只为 `NSKeyDown` / `NSKeyUp`
  构造 `Keystroke`，所以**按修饰键根本没有按键事件**；
- `ModifiersChangedEvent` 只有 `modifiers: Modifiers`（四个布尔）与 `capslock`，没有键身份；
- Windows 侧 `VK_SHIFT | VK_CONTROL | VK_MENU | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN` 落在
  同一个分支里，同样只产生 `ModifiersChanged`。

因此现有快捷键录入框无法按需求复用：它本来就靠「修饰键先作为 `Modifiers` 布尔位、再由随后的非修饰
键收尾」工作（`shortcut_from_capture` 对只有修饰键的组合直接返回 `None`，测试固定了这一点），而这里
要的恰恰是「只有一个修饰键」。

唯一知道物理键的地方是应用自己的输入管线。`bongocat-platform` 的 scan code 表与 macOS
`modifier_device_bit` 早就把左右分开映射到 HID `0xE0..=0xE7`，`bongocat_input::PhysicalKey` 是那条
通道的类型，runtime 的 `InputState` 持有按下集合并由 release、reconcile、reset 三条路径清理。因此
**录入与运行判定都必须读 runtime 的 pressed set，不能读 UI 事件**。

## 决策

### 1. `ModifierKey` 是八个修饰键的闭合词汇表，落在 `bongocat-input`

`bongocat_input::ModifierKey` 是 `LeftControl | LeftShift | LeftAlt | LeftMeta` 与对应 `Right*`，
由一张 `(ModifierKey, u16)` 表同时给出 `ALL` 与 `hid_usage()`，因此「顺序」与「usage」不可能各走
各的。`PressedModifiers` 是 pressed set 在这张表上的位投影。

放在 `bongocat-input` 而不是配置 crate，是因为四层必须说同一件事：落盘文档、runtime 的
`OverlaySettings`、设置协议与 overlay 自己的 `OverlaySessionOptions`。任何一层再写一份拼写，它就
可以与平台上报的 usage 悄悄分叉。这也让 `bongocat-config` 与 `bongocat-ui-protocol` 各自多了一条
指向 `bongocat-input` 的依赖边——这是本决策显式接受的代价。

因此 `bongocat-input` 新增 `serde` 与 `schemars` 两个依赖。这是序列化格式而不是子系统，替代方案是
在配置 crate 里为同八个键再写一份 `snake_case` 枚举，而它与 usage 表之间没有任何编译期约束。
`ModifierKey::name()` 的测试同时固定「拼写即变体的 snake_case」，所以重命名变体会让该测试失败，
而不是留下一份指向旧键的旧文档。

### 2. `input.pressed_modifiers` 是这个设置唯一的读法

`InputSnapshot` 新增 `pressed_modifiers: PressedModifiers`，由 `InputState::snapshot` 在同一次遍历
pressed map 的过程中算出（`InputSnapshot` 的四个计数原本就是四次 `keys().filter()`，合并成一次是
顺带的收敛，不是额外开销）。

不复用 `model_input.key_presses`：`snapshot_of.rs` 会丢掉任何当前模型画不出来的键，而修饰键恰好是
模型不会提供的键，因此这个面上它们多半根本不存在。也不复用四个计数：计数答不出「哪一个」。

overlay 侧只在 `update_hover_presentation` 里读一次，两个平台共用
`OverlaySessionOptions::hold_modifier_pressed`。呈现结果是
`enabled: hide_on_pointer_hover && input_running && !held` 与 `(click_through && !held) || hover.hidden() || idle.hidden()`。
这与 ADR-0057 的右键拖动是同一条既有规则：拖动进行期间把「指针在窗口内」当作不成立。

不把它做成新的状态机，也不新增第三个 alpha 乘数：暂停悬停隐藏恰好就是「该状态机关闭」，而
`PointerHoverHide` 在 `enabled == false` 时已经显式复位 pending 与 completed 两种隐藏。代价是松开键
后指针通常仍在窗口内，于是延迟**重新开始计时**而不是恢复一个已过期的截止时间——这是 ADR-0057
已经接受过的同一形状，并有测试固定。

`requires_window_recreation` 不含这个字段：它必须像悬停隐藏一样在窗口运行期间生效，因此改设置
不得替换原生窗口资源。

### 3. 设置页的录入控件读 runtime 的 pressed set，因此是「按住」语义

新增 `SettingsCommand::ReadPressedModifiers`，设置服务读 `runtime.client().snapshot().input.pressed_modifiers`
并直接应答。它必须是独立命令而不是 `ReadSnapshot` 的一层：控件每 50ms 问一次，而构建 snapshot 会
扫描磁盘上的模型库。它是 runtime 已有的原子读，不新增输入消费者，也不新增服务到 UI 的推送通道——
设置协议至今仍是请求-应答，UI 靠轮询，加一条推送会是远更大的改动。

「按住」语义由事实决定而不是由偏好决定：读的是**按下集合**而不是边沿，快捷的点击可能整个落在两次
轮询之间，而按住不会。设置本身要求用户做的事也正是按住不放，所以录入动作与使用动作是同一个手势。

录入提示的措辞跟着快捷键录入框而不是跟着这个机制：`按下组合键以录制` / `点击以录制快捷键`
对应 `按下一个修饰键以录制` / `点击以录制修饰键`。这一行因此在同一页面上与快捷键页读作同一个手势，
而机制上的差别（必须在下一次轮询时仍按着）由描述里的「按住时」承担，不再由提示语重复一遍——一个
只在录入控件里出现的「长按」警告会让两个录入框看起来像两种不同的东西。

控件的其余部分尽量复用现有逻辑：帧的外观、tab 位置、清除按钮与 `Escape` 取消都照快捷键录入框写，
显示格式读同一个 `macos_shortcut_token` 符号表，并在符号**前面**加一个表示左右的词（`左⇧` 而非
`⇧L`：字母跟在符号后面会被读成符号的一部分）；非 macOS 平台拼出 `左 Shift` 这类名称，与快捷键页的
`Control+Shift+A` 用同一批词。

侧标记是**界面语言里的词**，随已解析语言切换（`side_left` / `side_right`），不是固定字母。修饰键
符号本身在任何语言里都不翻译，但侧标记是这一行里唯一跟着界面语言走的部分——它和这一行的标题一样
属于产品文案；写成 `L` / `R` 会得到一个没人能读的键帽，也是唯一一段任何语言里都不像人话的标签。

这一行是 Model window 页唯一带描述的设置项，按的是 ADR-0066 的判据而不是新增惯例：**描述只在标题与
控件说不清时出现**。它逐字写出被暂停的两项设置在页面上的标题（「鼠标悬停时隐藏」和「鼠标穿透」），因为
那两个开关就在这一行上方——引用它们的标题是把句子接到屏幕上某个东西上的唯一方式；改成描述效果，则要
用户自己再把「接收鼠标点击」对回「鼠标穿透」。缺了它，用户面对一个「会让窗口暂时看不见也摸不着」的开关
却看不到它影响哪两项、也不知道松开会不会有事，宁可留着不开。描述同时进入搜索索引，因此搜索「穿透」或
「click-through」能从设置窗口任何位置找到这一行。

测试反过来也固定了这个连接：`the_recorder_description_names_both_settings_it_suspends` 拿描述去和
catalog 里那两个标题比对，所以改设置名而忘了改描述会在这里失败，而不是留下一行指向页面上已不存在的开关。

### 4. 这一行不受开关门禁，只受结构性编辑阻塞

ADR-0053 把「开关门禁」定义为**一个**开关绑定它下方的设置项。这一行被两个开关绑定：
`click_through` 在它上方，`hide_on_pointer_hover` 在页面更下方，两者都不相邻，任选其一都是任意的。
一个因为某个不相干的开关没开而置灰的行，比一个「存下取值、等任一开关打开即生效」的行更糟。

因此它只接 `editing_blocked`，与 ADR-0053 第 2 节标准流程的第 5 步（变更方法守卫）保持一致：
录制与清除都在 `editing_blocked` 时拒绝。这是显式偏离该 ADR 的绑定前提，不是遗漏。

### 5. 只作用于穿透与悬停隐藏，不作用于无操作隐藏

issue 与维护者的确认都只点了这两项。无操作隐藏期间用户本来就在操作输入（否则计时不会重置），
把它一起暂停没有可观察的收益，却会让「暂时失效」覆盖三个行为而不是两个。范围因此留在两项。

## 明确不做

- **不做「按住任意修饰键」的模式**。需求明确要求录入一个具体的物理键。
- **不把 Caps Lock 纳入词汇表**。它没有左右之分，与「必须区分左右」冲突。
- **不在 UI 侧新增输入观察者或推送通道**。录入读 runtime，与运行判定同一来源。
- **不缓存按住状态**。任何缓存都会让松开键后 overlay 继续可交互。
- **不在 `Schema` 版本上做迁移**。新字段 `#[serde(default)]`，缺失即 `null`。

## 后果

- 开启穿透或悬停隐藏后，可以按住配置的修饰键直接拖动窗口，不必来回切换设置。
- 左右两侧是两个独立设置，拖动时惯用哪只手就用哪只。
- 配置、runtime 设置与 overlay options 之间只有一份拼写；平台上报的 usage 与落盘 token 由同一张表
  绑定。
- 录入机制是「按住」语义：一次快速点击可能被漏掉，这正是它读取按下集合而非边沿的直接后果。提示语
  仍按快捷键录入框的写法说「按下」，因此这个差别对用户不可见——实际使用中按住即可，与设置本身要求的
  动作是同一个手势。
- 该行不随开关置灰；在两个开关都关闭时设置它不会报错，取值保留到任一开关打开时生效。
- 该设置不参与无操作隐藏。

## 残余风险与待验证项（不得当作已确认）

1. **未在 Windows 实机验证。** 改动覆盖两个平台的 `update_hover_presentation`，但 Windows Raw Input
   与 D3D11 路径本机无法执行；只有 macOS 实机跑通。
2. **按住期间的实际拖动手感未实测。** 逻辑有测试固定（穿透收窄、悬停复位、延迟重新计时），但真机上
   从「按住」到「抓住窗口」之间是否有可感知的延迟没有测量证据。
3. **左右区分依赖平台上报的 usage。** 平台侧映射（Windows E0/E1 前缀、macOS `modifier_device_bit`）
   已有各自测试，本次未新增；两平台组合下每个物理键都映射到预期 usage 这一点未在本次实机核对。
4. **录入控件的键盘可达性只有单元级证据。** tab 位置、清除按钮与 `Escape` 有渲染测试点击覆盖，
   VoiceOver / Narrator 下的实际播报未实测。
5. **50ms 轮询的实机成本未测量。** 它只在控件武装期间运行且每次是一次原子读，但未在真实负载下确认
   对设置线程无可观察影响。

## 验证

- `bongocat-input`：`every_modifier_names_one_distinct_left_and_right_hid_usage`、
  `the_two_sides_of_a_modifier_are_separate_configuration_values`、
  `a_stored_token_round_trips_and_anything_else_is_refused`、
  `the_pressed_set_projection_answers_per_modifier_and_keeps_the_sides_apart`。
- `bongocat-config`：`the_hold_modifier_stores_one_physical_key_and_defaults_to_none`、
  `the_hold_modifier_defaults_when_missing_from_older_data`、
  `an_unknown_hold_modifier_is_refused_by_the_strict_parse`，以及
  `shared/config/fixtures` 的 `hold-modifier-to-interact` / `-unknown` 一对。
- `bongocat-runtime`：`the_pressed_modifier_projection_keeps_the_two_sides_apart`、
  `a_reset_clears_the_pressed_modifier_projection`、
  `a_reconciled_release_ends_the_modifier_hold`。
- `bongocat-overlay`：`the_hold_modifier_answers_for_the_configured_physical_key_only`、
  `recalling_the_hold_modifier_never_replaces_the_window`、
  `a_hold_restores_the_overlay_and_letting_go_starts_a_fresh_delay`。
- `bongocat-app`：`the_pressed_modifier_read_reports_the_physical_key_and_moves_no_revision`。
- `bongocat-ui`：`a_held_modifier_is_recorded_as_the_physical_key_it_is`、
  `two_held_modifiers_resolve_to_the_same_key_every_time`、
  `a_cancelled_recording_writes_nothing_even_if_a_key_is_held_afterwards`、
  `the_recorder_frame_arms_the_recording_and_the_clear_control_does_not`、
  `structural_editing_blocking_renders_the_recorder_inert`、
  `the_recorder_names_the_side_of_the_key_it_stores`、
  `the_side_word_follows_the_interface_language`、
  `the_recorder_description_says_what_the_title_cannot`、
  `the_recorder_copy_is_named_in_every_shipped_language`。