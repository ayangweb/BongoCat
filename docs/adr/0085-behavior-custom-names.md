# ADR-0085: 动作和表情支持自定义名称

状态：已接受（2026-10-06）
补充：ADR-0067（配置域名与模型身份）、ADR-0079（新增配置字段的可选读取约定）
需求：Issue #1111

## 背景

Issue #1111 报告：快捷键页面里模型的动作和表情是按位置编号显示的，像「动作 1」「表情 1」，而模型包
里的资源名是内部的编号拼写，对不上用户心里想的是哪一个，所以每次要重新数一遍位置才能确认录的是哪个键。
动作和表情较多时尤其麻烦：想给第 7 个动作绑键，得先确认它到底显示成「动作几」。

issue 同时给出了边界：未设置名称的行仍显示现在的编号，不强制用户都去命名；名称跟随模型保存；需要限制
长度并拒绝空白名称；改名不影响已有的快捷键绑定和播放预览。

已经存在的两件事决定了实现形状：

- **行标签是按位置编号的，而且是唯一一处编号实现。** `presentation::shortcut_rows` 把模型声明的行为
  按「先所有 motion 组、再所有 expression」摊平成 `BehaviorOrdinal`，`shortcut_behavior_name` 据此得到
  「动作 3」。编号跨 motion 组连续、每类内不重复，另算一套编号会让「动作 3」在两页指不同行为。
- **行的标签紧邻一个按下即录制的控件。** `shortcut_page` 里每一行的 chord frame 在框内任意点击都会
  开始录制，Enter 也是；三个控件的 tab 序号按行 ×3 分配，并且被测试逐个钉住。

## 决策

### 1. `model.behavior_names` 是每模型每行为一行，默认空

`Vec<ModelBehaviorName>`，每行是 `{ model: ModelIdentity, behavior_id: String, name: String }`。默认
`[]`，字段带 `#[serde(default)]`。

默认值就是 issue 要的「未命名保持编号」，而必填新字段会让该字段加入之前写下的每一份 `config.json`
解析失败、走「最新有效备份 → 默认配置」并丢掉用户设置——与 ADR-0079 §1 同一理由。

行身份用完整的 `ModelIdentity` 加 `behavior_id`，与快捷键绑定、随机播放勾选共用**同一种拼法**
（`bongocat_config::ModelBehaviorAction::behavior_id`）。这一层是刻意的：这个字符串在配置里出现于三个
地方，任何一处另写一个格式化函数就是「同一声明顺序下指两个行为」或「模型更新后指错行为」两种故障的来源，
而且两者都不会编译失败。窗口侧与应用侧都走 `bongocat-config` 那个唯一的实现。

校验：模型 id 合法、`behavior_id` 可解析、一个模型的一个行为最多一行、名字 trim 后非空、限长
`MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS`（64）、不含控制字符。名字是行自己的显示文本，超长或不可打印都会变成
页面自己的问题而不是模型的问题。

### 2. 空白名字是删除，不是名字

`set_model_behavior_name` 先删掉该行，名字 trim 后非空才写回。清空一个文本字段的含义是「回到编号名称」，
所以它是删除请求；把空字符串存下来会让页面自己判断空名字算不算名字。窗口与配置两侧各做一次 trim 与
长度限制，用的是**同一个常量**：窗口不能送出配置会拒绝的值。

### 3. 标签优先用名字，行的其余部分一字不改

`ShortcutRow` 多一个 `custom_name: Option<String>`，`name()` 先用它再回落到编号标签。`Option` 而不是
`String`：未命名是常态，`None` 让页面不需要为它编造占位文本。

改名不动 capture target、播放控件、清除控件和已录制的组合键——它只换标签文字。应用 command 因此
不需要碰 shortcut table，改名期间正在录制的组合键也不会被取消。

### 4. 改名在行内完成：名称后面一个编辑图标，只有图标开启编辑

这是本 ADR 唯一的非显然取舍。

最初的决定是独立弹框，理由是行的 chord frame 在框内任意点击都开始录制、Enter 也是，把一个文本框放进
这一行似乎就要与「按 Enter 开始录制」的控件共享焦点与 tab 顺序。但那个共享其实不存在：行的标签和
chord frame 是**兄弟**而不是父子，键盘事件只沿焦点路径冒泡，字段获得焦点时 frame 的按键处理器根本
不在路径上。为一个不存在的冲突付出弹框的代价不划算——弹框把「要改的名字」和「正在看的名字」拆到两个
表面上，用户改完还要自己确认改的是哪一行。

因此改为组件库里可编辑文本的标准形状，但入口只留一个：

- **名称后面一个编辑图标**（与模型卡片改名同一枚 `SquarePen`），点击它把名称就地换成文本框。字段预填
  该行当前的标签，用户改的是看得见的东西而不是要凭记忆重打一遍。
- **名称本身保持纯文本，不可点**。读过一遍标题就见过这种陷阱：想选中一行文字复制，落点稍偏就开始改
  名。入口只有图标一个，「看名字」和「改名字」两种意图在指针下就分开了。
- **字段自身不带任何控件**：Enter 保存，Escape 放弃，字段焦点被别处拿走视为保存——用户敲了一半的
  名字不因为看了一眼别处就被丢掉。字段仍显示该行当前标签时，保存是空操作，不发送请求。
- **一次只有一个字段打开**；打开另一行的编辑会先提交已打开的那份编辑，而不是丢弃它。
- 字段宽度、字号与行高复用模型改名输入框的常量：两个改名输入框长得不一样会让同一段文字读起来像
  两种不同的值。

行的三个控件（录制、播放、清除）位置、tab 序号与 `shortcut_page` 的既有测试全部保持不变——改名面
是**从名称长出来的**，不是这一行的第四个控件；编辑图标也不是 tab stop。编辑图标只带一个词的提示
（「重命名」），改名面没有别的说明文案：空字段恢复编号名称这条规则写在配置契约里，而不是写进界面。

代价要说清楚：**这一行没有第四个 tab stop**，所以纯键盘用户无法只靠 Tab 到达改名。本项目按 ADR-0054 的
visual-first 约定不维护辅助技术树与隐藏动作，可见标签就是入口；这是一处有意识的取舍，不是遗漏。

### 5. 名字随模型走，删除模型时一并删除

snapshot 只带**当前模型**的已命名行，由配置的 catalog entry 过滤掉该模型已不再声明的行为——是否还存在是
模型包的属性，一个指向已不存在的名字的行在页面上无处安放。

删除导入模型时清掉它的名字，与 `last_expressions`、`gamepad_auto_switch` 的清理走**同一次 commit**：
分开写会留下一个「配置里还记着一个 store 已经没有了」的窗口。内置模型与导入模型同名时保留内置那一行，
因为那指的是另一个模型（与记忆表情的既有规则一致）。

## 后果

- 用户可以为每个动作和表情起自己的名字；未命名的行显示与升级前完全相同的「动作 N / 表情 N」。
- 快捷键页面的三个控件、tab 序号、录制语义、播放与清除都不变。
- 名字是纯显示文本：runtime 不感知它，改名不触碰 shortcut table，也不取消正在进行的录制。
- 自动化契约全部落在真实实现上：
  - `bongocat-config`（`a_configuration_written_before_behaviors_could_be_named_still_loads`、
    `a_behavior_name_is_parsed_bounded_and_held_once_per_behavior`）从序列化后的当前默认里删掉该字段，
    模拟旧文档的字节，并钉住解析、长度、可打印性与「一个行为一行」；
  - `bongocat-app`（`a_behavior_name_is_stored_and_read_back_for_its_own_model`、
    `renaming_the_same_behavior_replaces_its_row`、
    `clearing_a_behavior_name_removes_its_row`、
    `the_same_behavior_of_two_models_is_two_rows`、`a_name_is_trimmed_before_it_is_stored`）覆盖写入、
    覆盖写、清除、跨模型与 trim；
  - `bongocat-ui`（`a_named_behavior_shows_its_name_instead_of_its_number`、
    `an_unnamed_behavior_keeps_its_numbered_label`、
    `a_name_only_lands_on_the_behavior_it_belongs_to`、
    `naming_a_behavior_leaves_its_binding_and_play_control_alone`、
    `an_application_command_is_never_named`、
    `a_stored_name_is_bounded_and_carries_no_control_characters`、
    `a_confirmed_rename_sends_the_row_identity_it_was_opened_from`、
    `the_pencil_opens_the_field_the_label_opens`、
    `escape_leaves_the_field_without_writing`、
    `saving_an_unchanged_label_writes_nothing`、
    `the_field_replaces_the_label_and_the_row_keeps_its_controls`、
    `opening_another_rows_editor_commits_the_one_that_was_open`）覆盖标签解析、名字不落在别的行为上、
    「改名不动绑定与播放控件」、命令自身的身份与 revision、窗口与配置共用同一个长度上限，以及渲染出来的
    行内编辑：点标签、点图标、Enter、Esc、空操作与提交已打开的那份编辑；
  - `shared/config/fixtures` 新增 1 份 accept 与 5 份 reject 用例（无法解析的 `behavior_id`、空白名字、
    含控制字符、同一行为两行、非法模型 id）。
- 共享 fixture 只有 `default.json` 增加该键：新增字段可选，其余 48 份保持没有该键的样子，正是它们要验证的
  「旧文档仍能读取」。
- `spikes/config-store` 保留自己的 `NativeConfig` 副本，并断言它序列化后与 `shared/config/fixtures/
  default.json` 逐字相同，所以产品默认值新增字段时这里也必须同步。`just check` 不覆盖 `spikes/`，
  漏掉它只有一个 CI job 会报，所以它是单独写的。
- 未运行实机验证：只在本机（macOS）跑了 `just check` 与 `tools/` 下的校验脚本，没有做 macOS 或 Windows
  实机 smoke。行内字段在 800×600 与 Windows 125/150/200% 下的观感、以及「标签可点击」这一入口的可发现性
  需要实机确认。
