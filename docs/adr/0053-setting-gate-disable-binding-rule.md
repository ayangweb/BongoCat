# ADR-0053: 「开关门禁」与被控设置项的统一禁用绑定规则

状态：已接受（2026-09-21；项目 AccessKit tree 部分由 ADR-0054 取代）
依赖：ADR-0052（快捷键门禁各自跟随所属分组）、ADR-0054（visual-first UI）

## 背景

维护者要求（2026-09-21）：快捷键页两个门禁开关（`启用窗口快捷键` / `启用模型快捷键`）中任意一个
关闭时，应自动禁用其作用域的全部设置项（置灰且不可交互），参照「鼠标悬停隐藏延迟」开关关闭时禁用
延迟行的既有实现；并要求把这类"开关控制下属设置项"的场景收敛为一套统一标准，此后每遇到新开关不再
单独编写一次禁用逻辑。

决定本 ADR 形态的事实：

1. **参照实现本身是三层分散的逻辑。** 悬停隐藏延迟的禁用规则由三处各自表达：渲染层的
   `.disabled(!hover_hide_delay_available)`、无障碍层的 `disabled || !hover_hide_delay_available`、
   变更方法 `adjust_overlay_hover_hide_delay` 的守卫。三处判的是同一件事，却没有任何一处命名它——
   第三个门禁出现时就会各自再写一遍并开始漂移。
2. **ADR-0052 明确决策过相反方向。** 其「明确不做」一节写明"不给行列表加禁用态样式"，理由是用户
   可能想先把组合键排好再打开开关。维护者现在的要求直接推翻这一条；按工作协议，本 ADR 显式取代该
   条目而非留下两份互相矛盾的文档，ADR-0052 的其余内容（每分组一个正向开关、只投影不清空、
   `active_bindings` 唯一实现点、全链路不取反）全部保持不变。
3. **自定义渲染行的"禁用"必须自己拼。** 打包字段（`SettingField::switch` 等）由
   `SettingItem::disabled` 一并处理置灰与不可交互；快捷键行是 `SettingItem::render` 的自定义元素，
   置灰只是 `opacity(0.5)`，`on_click` / `on_key_down` handler 仍会注册、命中测试仍有效（gpui-kit
   的 disabled 投影不吞事件）。"不可交互"必须通过不再注册 handler 来实现，这恰恰是最容易漏的一半。
4. **两种"禁用"来源是正交的。** 结构性编辑阻塞（无快照、配置不可用、模型导入进行中）与门禁关闭是
   两个独立布尔；渲染层与守卫层必须合成同一个值。若两层各判一次，任何一处漏加来源就会出现
   "看着灰但能改"或"能点但请求被拒"的不一致。
5. **`pending` 不能进门禁。** 保存往返期间的 in-flight 标志每次保存都会翻转，接入后整页会随每次
   控件变更明暗一轮，读作页面在刷新（ADR-0052 前身已踩过：d4f0155f）。

## 决策

### 1. 统一机制：`bongocat-ui` 的 `window::setting_gate::SettingGate`

每个"开关门禁"场景构造一个 `SettingGate::new(editing_blocked, enabled)`，其中 `enabled` 是该开关的
配置真值（正向、不取反），`editing_blocked` 只在结构性编辑不可能时为真（不含 `pending`）。规则由
两个方法固定：

- `disables_switch()` = `editing_blocked`——**开关自身永不被自己的状态禁用**，否则关掉的门禁永远
  无法再打开；
- `disables_controls()` = `editing_blocked || !enabled`——被门禁控件的禁用条件。

三条固有性质写进模块文档，对所有场景生效：

- **门禁只投影可用性，从不改写被门禁的值**：重开开关即恢复用户原值，无需重录（与 ADR-0052 的
  "只过滤、不改写"一致）。
- **`pending` 不进门禁**：头部状态栏是保存指示器。
- **三层读同一个 gate 值**：可见行、无障碍树（disabled 且不可点击/聚焦）、变更方法守卫必须同源，
  以保证"在开关翻转前渲染的树上操作"也不生效。

### 2. 新场景的接入方式（标准流程）

1. 定义一个从 snapshot 读开关真值的正向谓词（如 `ShortcutScope::is_enabled`、
   `hover_hide_delay_applies`），全链路复用，不取反；
2. 渲染时为每个被门禁分组构造一个 `SettingGate`；
3. 开关行走 `disables_switch()`，被控行走 `disables_controls()`：打包字段用
   `SettingItem::disabled(...)`；自定义行**整行置灰（标签与控件一起，与 `SettingItem::disabled`
   的整行置灰一致——禁用的是"这条设置"，不是仅控件）**并停止注册交互 handler；
4. 无障碍节点对被控行报 disabled 且不加 `clickable().focusable()`；
5. 变更方法以同一谓词做守卫。

### 3. 快捷键页的落地

- `ShortcutScope::for_target(target)` 是"目标 → 门禁它的作用域"的唯一映射（Command → Window，
  ModelBehavior → Model），渲染层、无障碍层与变更方法都经它取谓词，不再各自 match 目标类型。
- 每个分组渲染时构造自己的 `SettingGate`（`editing_blocked` 共享、`enabled` 各自取
  `is_enabled(snapshot)`）；行**整行置灰（标签与控件一起）**，捕获按钮、清除按钮不再注册
  `on_click` / `on_key_down`。整行置灰与 `SettingItem::disabled` 对打包字段的处理一致
  （标题、描述、控件一起 0.5 透明度），两类行在外观上不可区分。
- `sync_shortcut_row_focus` 按同一谓词决定 tab stop，并在门禁翻转后取消仍挂在该作用域目标上的
  捕获（与"目标离开行序即取消捕获"同一机制）；`finish_shortcut_capture_if_valid` 再次守卫，
  门禁在捕获中途关闭时结束捕获而不是写入一条界面上不可见的绑定。
- `begin_shortcut_capture`、`begin_shortcut_capture_from_accessibility`、`clear_shortcut` 增加同一
  守卫：结构性检查（`shortcut_commands_available`）之外再判目标作用域的门禁真值。
- 悬停隐藏延迟行改读同一规则：`SettingGate::new(editing_blocked, hover_hide_delay_applies(..))
  .disables_controls()`，与其 mutator 守卫、无障碍节点同源。**行为差异**：结构性编辑阻塞期间
  （导入进行中等）该行现在也会置灰——此前它只随开关禁用；这是统一规则的直接结果，且与其变更守卫
  `set_overlay_hover_hide_delay_value` 的既有早退对齐。

### 4. ADR-0052 的对应修订

ADR-0052「明确不做」中"不给行列表加禁用态样式"一条由本 ADR 取代（维护者 2026-09-21 决策变更），
该条内注明的理由（"先把组合键排好再打开开关"）不再是产品方向；该 ADR 其余决策不受影响。

## 明确不做

- **不改 runtime 投影语义。** 门禁的唯一实现点仍是 `ShortcutConfig::active_bindings`；本 ADR 只把
  UI 层的可用性投影对齐到同一开关真值，配置、命令通道、平台表均不变。
- **不在门禁关闭时清空或改写绑定。** 置灰只是 UI 投影；落盘配置不动。
- **不把 `pending` 计入门禁。** 防整页明暗刷新；保存指示器在头部状态栏。
- **不为打包字段重写禁用样式。** `SettingItem::disabled` 已同时处理置灰与字段不可交互，统一机制
  直接复用，不再自绘。
- **不为开关关闭加提示行。** 开关就在被控行上方，置灰即提示（延续 ADR-0052 的立场）。

## 后果

- 快捷键页两个门禁关闭时，各自作用域的行**整行置灰**（标签与控件一起）且不可交互；无障碍节点报
  disabled、不可聚焦；键盘 tab 跳过这些行；辅助技术触发的捕获/清除请求被守卫拒绝。
- 门禁关闭期间无法再通过 UI 录制或清除该作用域的绑定（ADR-0052 曾刻意保留这条路，本 ADR 收窄它）。
  绕过 UI 的路径（直接编辑配置文件）不受影响，配置层从不过滤或改写绑定。
- 悬停隐藏延迟行与两个快捷键分组共用同一条规则与同一份实现，新增门禁场景按第 2 节流程接入。
- 捕获中途翻转门禁会取消捕获并恢复平台表（`resume_shortcut_capture`），不会留下悬空的录制会话。

## 残余风险与待验证项（不得当作已确认）

1. **未在 Windows 实机验证。** 改动全部在 `bongocat-ui`，无平台代码，但 `cfg(windows)` 的无障碍与
   设置窗口路径本机无法执行。
2. **无布局/视觉断言。** 置灰效果（`opacity(0.5)`，与 `editing_blocked` 既有禁用态一致）依赖组件
   机制与人工查看；125/150/200% DPI 无截图证据。
3. **无障碍树的门禁行为只有节点属性断言可覆盖**，读屏器实际表现（VoiceOver/Narrator 对 disabled
   按钮的播报）未实测。
4. **"关闭期间录制"被收窄**意味着喜欢"先排键再开闸"的用户失去这条路；若反馈需要恢复，可在
   `disables_controls()` 之外为捕获行单独放宽——那是独立决策，不预先实现。

## 验证

见 `docs/BongoCat Native Rewrite Implementation TODO.md` 第 102 项。
