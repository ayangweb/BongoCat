# ADR-0044: 全局快捷键改用 global-hotkey 注册制后端

状态：Accepted
日期：2026-09-17；2026-09-25 修订应用级输入门禁快捷键
取代：无（首次为快捷键子系统建立 ADR）

## Context

BongoCat 的快捷键（应用命令与模型行为）此前由输入管线内匹配实现：
Windows Raw Input / macOS CGEventTap 把按键边沿映射为 HID usage 后喂给
`ShortcutMatcher`，`ShortcutDispatcher` 在进程内解析目标并分发。该匹配是
被动的：注册过的组合键仍会继续传递给前台应用，同时快捷键依赖输入服务的
生命周期（输入服务因权限或恢复失败退出时快捷键一并失效）。

产品需要对齐历史 Tauri 版本的全局快捷键语义：组合键由操作系统注册并
消费，应用失焦或处于后台时必须可靠触发。经评审选择 tauri-apps 维护的
`global-hotkey` crate（crates.io 最新稳定版 0.8.0，Apache-2.0 OR MIT，
活跃维护）：Windows 走 `RegisterHotKey`，macOS 走 Carbon
`RegisterEventHotKey`，与历史实现的注册模型一致。

## Decision

- 引入 `global-hotkey = "=0.8.0"`，仅作为 `bongocat-platform` 的
  macOS/Windows 目标依赖。组合键解析（`ShortcutChord`）留在
  `bongocat-config`，平台 token → `keyboard_types::Code` 映射与注册管理
  在 `bongocat-platform/src/shortcut.rs`。
- 新增 `GlobalShortcutService`：单一 owner 线程持有
  `GlobalHotKeyManager`，以 50ms 轮询共享 `ShortcutTable` 并做增量
  register/unregister，消费 `GlobalHotKeyEvent::receiver()` 后把目标交给
  精简后的 `ShortcutDispatcher::execute`（runtime 触发 + application
  sink）。`ShortcutTable::replace` 的全部发布点（`set_shortcuts`、录制
  suspend/resume、行为快捷键开关、恢复默认）无需新增通知通道即被覆盖。
- 平台线程模型：
  - Windows：`WM_HOTKEY` 投递到创建 manager 隐藏窗口的线程队列，owner
    线程运行 `MsgWaitForMultipleObjectsEx` + `PeekMessageW` 消息泵。
  - macOS：热键事件经 Carbon handler 在主事件循环投递（应用 runloop
    泵送）；在 owner 后台线程上创建/注册/注销
    `RegisterEventHotKey` 已通过本机反向控制实验验证可行。
- 删除 `ShortcutMatcher` 与输入管线中的全部快捷键喂点
  （`apply`/`reset`/`reconcile`），`WindowsInputService` /
  `MacInputService` 移除 `_and_shortcuts` 启动入口，
  `OverlayInteractionSinks` 移除 `shortcut_dispatcher`。app 生命周期：
  服务在产品 run 启动，`begin_product_shutdown` 中先于
  `overlay.stop_input()` 停止并注销全部注册。
- 注册失败不阻塞其他绑定：被其他应用占用或平台无 scancode（macOS 的
  ScrollLock/Pause 没有 Carbon scancode）的组合键记入
  `registration_failures` 诊断，其余正常注册。
- 配置 schema、行为动作与 UI 录制流程均不变。`ShortcutCommand` 的 application target
  在原有集合上增加 `toggle_ignore_mouse_input`、`toggle_ignore_keyboard_input` 和
  `toggle_ignore_gamepad_input`；三个 target 都经同一 settings-service typed handoff
  切换并持久化对应的模型输入门禁，不直接修改 runtime 或平台输入状态。

## Consequences

- 行为变化：注册的组合键会被操作系统消费，不再同时传递给前台应用
  （与历史 Tauri 版本一致，属于预期语义）。
- macOS 上快捷键注册本身不要求辅助功能权限（Carbon 注册），但猫爪动画
  的全键盘监听仍依赖 CGEventTap，TCC 授权要求不变。
- 快捷键不再随输入服务失败而失效；输入服务的恢复/重置路径与快捷键状态
  完全解耦，Issue #47 的 pressed-state 校正只服务动画输入。
- 服务按 50ms 轮询感知改绑，录制 suspend/resume 的生效延迟 ≤ 100ms。
- 三个输入门禁快捷键与普通应用快捷键共享同一录制、冲突和 `commands_enabled` 门禁；它们只切换模型输入投影，键盘/鼠标/手柄的可靠采集与 pressed-state 恢复不受影响。
