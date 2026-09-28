# ADR-0073: 应用级快捷键收窄为五个 command

状态：已接受（2026-09-27）

## 背景

`shortcuts.command_bindings` 的 application command 是一个闭合集合，由
`ShortcutCommand::parse` / `as_str` 双向实现。此前它有八个取值，其中
`toggle_mirror`、`toggle_click_through` 和 `toggle_always_on_top` 只是把三个已经在
别处可见的开关接到键盘上：

- 模型水平翻转：`model.mirror`（设置页「模型行为」）与模型窗口右键菜单。
- 鼠标穿透：`overlay.click_through`（设置页「模型窗口」）与托盘/右键菜单勾选项
  （ADR-0068）。
- 始终置顶：`overlay.always_on_top`，同样在这两处有控件。

也就是说，这三个取值是同一状态在第三个入口的重复暴露。用户要改这三项时手上通常正
拿着鼠标；而真正只能靠键盘完成的场景（不看屏幕地开关模型、打开设置、按设备类型忽略
输入）由保留下来的 `toggle_overlay`、`open_settings` 和三个
`toggle_ignore_*_input` 覆盖。

## 决策

- 闭合集合收窄为五个：`toggle_overlay`、`open_settings`、`toggle_ignore_mouse_input`、
  `toggle_ignore_keyboard_input`、`toggle_ignore_gamepad_input`。三个被移除的
  `as_str` 拼写不再是合法 command。
- 三个对应的设置项和右键菜单项保持不变。收窄的是「可绑定的快捷键」，不是能力：
  `model.mirror`、`overlay.click_through` 和 `overlay.always_on_top` 的默认值、持久化
  路径、runtime 投影和 overlay 窗口行为都不动，ADR-0068 的菜单树与
  `SystemMenuAction` 也不动。
- 该变更直接进入当前 v1，不提供迁移、旧 command 探测或 fallback。一份仍持有这三个
  绑定的 v1 文档按损坏处理，走既有的「最新有效备份 → 默认配置」。默认
  `command_bindings` 是空列表，所以全新配置从未依赖过它们。
- 快捷键页的窗口作用域随之只渲染这五行；`bongocat-ui` 的
  `WINDOW_SHORTCUT_COMMANDS`、本地化 label 映射和
  `shortcuts.command_names.*` 的三个 key 同步删除，而不是留下永远渲染不到的行。

## 验证

- `bongocat-config` 覆盖剩余五个 command 双向拼写仍闭合、被移除的拼写不再被解析。
- `bongocat-ui` 覆盖窗口作用域恰好五行、tab 序号在缩短后的行序上仍不重叠、只有模型
  行为行携带 play 控件；`bongocat-i18n` 双向 key 守门确认三个文案 key 与它们的引用
  同时消失。
- `cargo fmt`、三段 `cargo clippy -D warnings`、`cargo test --workspace`、
  `cargo check --workspace --release` 与 `tools/validate-locales.py` 全部通过。
- 未运行：Windows 10 1903+ 与 macOS 12+ 实机上快捷键注册与触发的目视确认，以及设置
  窗口行的实际排版。这些仍属 ADR-0044 的完成门禁，本 ADR 的自动化证据不替代它们。

## 后果

配置契约少三个 command，UI protocol 少三个 `SettingsApplicationShortcut` 取值，
`apply_application_shortcut` 少三条分支。仍持有这三个绑定的开发配置会整体回退到备份
或默认配置，这是 v1 直接变更的既定代价。
