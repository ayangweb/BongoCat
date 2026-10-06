# ADR-0083: 重复触发同一表情可以把它关掉

状态：已接受（2026-10-06）
补充：ADR-0079（新增配置字段的可选读取约定）、ADR-0030（实现决策阶梯）
需求：Issue #1109

## 背景

Issue #1109 报告：触发一个表情后，再次触发同一个表情没有任何效果——表情一直保持，只有触发别的
表情或切换模型才会变回去。Mver 版本里再点一次同一个表情可以把它关掉，回到模型默认的表情，现在的
版本缺这个能力。

这不是缺少一个「清空表情」的动作，而是**默认表情没有入口**。`RuntimeRenderer::evaluate` 每帧先
`restore_parameter_defaults()` 再 `apply_expression_layers()`，所以「回到默认表情」在渲染器里就是
「没有任何表达式图层在生效」；但产品里唯一能把图层清空的事件是切换模型或 shutdown，两者都不是用户
能随手做出的选择。因此一个用户想撤掉刚才那个表情，只能换一个模型再换回来。

Mver 的行为是「点第二次 = 关掉」，但 issue 同时明确指出不能照搬：现在的用户已经习惯了重复触发只是
重新应用同一个表情，直接沿用旧行为会让两套语义无法对比。所以这是一个**配置项而不是默认行为**。

## 决策

### 1. `model.toggle_repeated_expression` 是唯一的开关，默认关闭

放在 `ModelConfig`，`bool`、默认 `false`、正向语义，设置页呈现为「重复触发同一表情时关闭它」，
位置紧邻「记住每个模型上次使用的表情」。

默认 `false` 是为了不改变任何现有用户的画面：这是一个可观察的行为差异，只有一次显式选择才引入。

字段带 `#[serde(default)]`，与 `model.show_all_pressed_keys` 同一理由（ADR-0079 §1）：v1 解析入口是
严格的，一个必填新字段会让该字段加入之前写下的每一份 `config.json` 解析失败，走「最新有效备份 →
默认配置」的恢复流程并丢掉用户设置。缺少该字段的旧文档读出 `false`，即升级前既有的行为。

它是**独立开关**而不是 `remember_last_expression` 的一个取值或第二个字段：两者回答不同问题——一个
是模型从哪里开始（记住上次那张脸），另一个是重复触发时做什么。把它们合成一个枚举会让「记住但不
切换」「切换但不记住」两种组合都不可表达。

### 2. 判定在 runtime，因为那里才知道当前生效的是哪个表情

两个触发源都收敛为同一条 `RuntimeCommand::SetExpression`：设置窗口里每个表情行的播放按钮走
`Application::set_expression`，快捷键走 `ShortcutAction::SetExpression` 后的 `trigger_shortcut`。
只有 runtime 同时持有 `active_expression` 和 `renderer`。放在 Application 需要把「当前生效的表情」
复制一份到 Application 会话状态里，那是把 runtime 的事实复制到它外面。

`ModelSettings` 增加 `toggle_repeated_expression`，由 `config_projection::model_settings_from_config`
从配置投影，因此这个字段和其它模型设置走同一条 `SetModelSettings` 路径。设置页的
`SettingsCommand::SetModelSettings` 分支在构造 runtime 设置时从配置里带上这个值，避免用户动其它
模型设置时把它重置。

命令语义：请求的表达式等于当前 `active_expression` 时，命令变成「关闭」——`renderer.clear_expression`
让所有图层开始淡出、不压入新图层，`active_expression` 置空。不等于是普通路径，逐字不变。

### 3. 关闭动作不写「记住上次使用的表情」

`user_expression_memory` 只在「应用」分支写。用户正在撤销一个选择，把刚被关掉的表情记下来会让它
在下次启动、或切回该模型时以「用户选过」的名义回来。因此关闭之后 `last_expressions` 保持不变，
记住的仍然是用户最后选择穿上的那张脸。

这也是它作为独立开关的另一个理由：记忆的语义（「用户选过什么」）和开关的行为（「重复触发做什么」）
在关闭这条路径上必须解耦。

### 4. 随机播放不参与

随机选择器通过 `renderer.set_expression` 直接播放，不走 command 队列——这与「自动选择不能进入
记忆集合」是同一条既有边界（`SetExpression` 是「用户主动」的唯一判据）。把开关的判定放在
command 分支里，随机播放天然不在其中，不需要额外门禁。

### 5. 「恢复上次表情」不会被读成重复触发

`restore_remembered_expression` 在 `prepare_model` / `select_model` 之后发送 `SetExpression`。worker
在 `pending_model` 非空时把除 `Shutdown` 与 `ApplyInput` 以外的所有命令推迟
（`crates/bongocat-runtime/src/worker/mod.rs`），而 `active_expression` 在模型 commit 时被清空，所以
恢复命令执行时 `active_expression` 一定是 `None`，不可能与请求的表达式相等。

这条不是新加的保证，而是**必须被钉住的不变量**：一旦有人让模型切换不再清空 `active_expression`，
或者不再推迟命令，打开开关的用户就会看到「刚恢复的表情立刻自己关掉了」。

## 后果

- 打开开关后，再触发一次当前生效的表情会淡出回到模型默认表情；关闭时行为与升级前逐字一致。
- 渲染器没有新增模型状态：`clear_expression` 复用「替换」已经在用的同一条淡出路径，只是不压入新
  图层，剩余图层由既有的 `evaluate` retain 按 clip 自己声明的淡出时长丢弃。
- 设置页新增一行开关、一个 revision-checked command、一条 client 方法和一份 snapshot 字段；
  `SettingsModelSettings` 不变，因此这个开关不会被其它模型设置覆盖。
- 自动化契约全部落在真实实现上：
  - `bongocat-runtime` 渲染层（`clearing_the_expression_stack_returns_the_model_to_its_default_face`、
    `clearing_twice_is_the_same_as_clearing_once`、
    `clearing_without_an_active_model_reports_nothing_to_close`）读模型参数而不是 render snapshot
    ——呼吸和眨眼每帧都动大部分模型，只有被表情独占写入的参数能说明表情是否还在生效；
  - `bongocat-runtime` worker（`a_repeated_expression_is_applied_again_while_the_toggle_is_off`、
    `a_repeated_expression_turns_itself_off_while_the_toggle_is_on`、
    `the_toggle_does_not_reach_the_automatic_expression_picker`、
    `a_restored_expression_is_never_read_as_a_repeat`）钉住两态、关闭不写记忆、随机播放不受影响、
    以及恢复不被读成重复；
  - `bongocat-app`（`the_expression_toggle_is_one_switch_that_reaches_the_runtime_and_the_document`）
    跑通「开关 → 配置 + runtime 设置 → 表情触发」并确认记忆集合不变；
  - `bongocat-ui`（`the_expression_toggle_switch_sends_its_own_command`）钉住它是自己的行、自己的
    command，并且不会连带移动相邻的记忆开关；
  - `bongocat-config`（`a_configuration_written_before_the_expression_toggle_still_loads`、
    `the_expression_toggle_defaults_to_off_and_is_independent_of_the_memory_switch`）从序列化后的
    当前默认里删掉该字段，模拟旧文档的字节。
- 共享 fixture 只更新 `shared/config/fixtures/default.json`：新增字段是可选的，其余 48 份 fixture
  保持没有该键的样子，正是它们要验证的「旧文档仍能读取」。
- 未运行实机验证：只在本机（macOS）跑了 `just check` 与 `tools/` 下的校验脚本，没有做 macOS 或
  Windows 实机 smoke。淡出回到默认表情的观感需要实机确认。
