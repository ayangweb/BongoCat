# ADR-0031: Tray Icon and Menu Library Boundary

状态：已接受（2026-09-13）；2026-09-14 补充 Windows `set_tooltip` 上游缺陷与 tooltip 创建期不变量

> 后续修订（2026-09-23）：ADR-0033 已退役 Windows 原生 ARM64；本文提到的 Windows x64/ARM64 cross-check 只是历史证据，当前 Windows 发布 target 只有 x64。

## 背景

Windows 的 `Shell_NotifyIcon`/`HMENU` 与 macOS 的 `NSStatusItem`/`NSMenu` 原本各自实现，约 690 行
平台代码需要分别维护隐藏窗口、菜单句柄、回调、主线程与生命周期。两边已经共享
`SystemMenuAction`、`SystemMenuError` 和 `SystemMenuPresentation`，但 native owner 的行为与
清理顺序仍容易漂移。项目需要在不改变 runtime/UI 强类型边界的前提下，减少重复平台代码并保留
Windows GUID、菜单顺序、左键打开设置和右键菜单等产品行为。overlay 右键最初复用托盘隐藏窗口，
这会让 Windows 菜单 owner 与实际窗口不一致，并绕过 macOS 的跨平台菜单 API；因此托盘与菜单的
第三方边界在本 ADR 中分别固定。

## 调研结论

已核实（2026-09-13，`cargo search`、`cargo info`、`cargo tree` 与源码阅读）：

- `tray-icon 0.25.0`（MIT OR Apache-2.0，Rust 1.90+）是 crates.io 当日最新稳定版，由 Tauri
  项目维护，同时支持 macOS `NSStatusItem` 与 Windows `Shell_NotifyIcon`，并提供图标、tooltip、
  菜单、固定 GUID 和显隐 API。当前 toolchain 为 Rust 1.97.1，满足其 MSRV。
- `muda 0.20.0`（Apache-2.0 OR MIT，Rust 1.90+）是 crates.io 当日最新稳定版，由 Tauri 项目维护；
  `Menu`/`MenuItem`/`CheckMenuItem` 提供菜单构建与动态文本、状态更新，`ContextMenu` 提供 Windows
  HWND 与 macOS `NSView` 的右键弹出 API。它没有可设置的菜单标题，因此既有
  `SystemMenuPresentation::title` 不再有对应可变标题 API。`tray-icon 0.25.0` 也依赖同一
  `muda 0.20.0` 类型来挂载托盘菜单；项目直接声明 `muda`，确保 overlay 菜单与托盘菜单复用同一份
  强类型 owner，而不是依赖第三方 crate 的重导出。
- `tray-icon` 的 `TrayIcon`、`TrayIconEvent::receiver` 与 `muda::MenuEvent::receiver` 均以进程级
  状态和 `Rc<RefCell<_>>`/静态 channel 实现。一个进程只能有一个长期存活的 `SystemMenu` 业务
  owner；隐藏图标只切换平台表示，不能销毁并重建 owner，否则会丢失菜单事件消费者。
- Windows 固定 GUID 仍使用
  `123f3c6f-7d2a-4ca3-b8cb-9b1d1eaf2f10`。库以 GUID 作为托盘身份并在 Explorer
  `TaskbarCreated` 广播后恢复注册；应用不自行重试或创建平行图标。
- 两个依赖都使用 `default-features = false`：`tray-icon` 关闭 `libappindicator`，`muda` 关闭
  默认的 `gtk3` 与 `libxdo`。它们只在 macOS/Windows target 下声明，因此 Linux 不引入
  GTK/libappindicator 系统依赖。Linux 托盘仍按 ADR-0006 单独评估，不属于本决策。
- Windows PNG 资源来自维护者指定的
  `https://raw.githubusercontent.com/ayangweb/BongoCat/refs/heads/master/src-tauri/assets/tray.png`，
  已逐字节核验为 256×256、8-bit RGBA、非隔行 PNG，SHA-256 为
  `65f3a1e71f60417916c0511945131c7341edbd46d037fa9e703014e6155434dd`。macOS 现有
  `tray-macos.png` 也固定为 256×256、8-bit RGBA、非隔行，SHA-256 为
  `1e39a05cc356501518291acea439c06a7f7e944ad3a227f29387875f3604af9a`。

## 决策

- macOS 与 Windows 状态图标统一由 `bongocat-platform` 私有 `system_menu_native` adapter 管理：
  `tray-icon 0.25.0` 是托盘/菜单栏图标的 native owner，直接依赖的 `muda 0.20.0` 是菜单和弹出
  行为的 owner；旧 `system_menu_macos.rs`、`system_menu_windows.rs` 与 `tray-windows.ico` 退役。
- adapter 是 `SystemMenu` 的唯一 owner，长期持有 `TrayIcon`、根 `Menu`、全部可变菜单项和
  `muda` 菜单事件 receiver。`set_visible(false)` 只改变平台表示（macOS 移除 `NSStatusItem`，
  Windows 保留注册并设置隐藏），菜单 owner 与事件通道不销毁；重新显示不创建第二套业务状态。
- Windows 使用固定 GUID 与 `tray-windows.png`；macOS 使用 `tray-macos.png` 并标记为 template
  image。两平台在菜单构建时共享相同 action id 到项目自有 `SystemMenuAction` 的映射。
- Windows 左键抬起映射为 `OpenSettings`，右键由 `tray-icon` 弹出其挂载的同一 `muda` 菜单。
  macOS 由 `tray-icon` 在左右键时弹出同一菜单。overlay 右键统一由 adapter 的
  `show_context_menu_for_window` 从 overlay session 获取真实 Windows HWND 或 macOS content
  `NSView`，再调用 `muda::ContextMenu`；不得继续借用托盘隐藏 HWND，也不创建第二套菜单。
- 第三方 `tray_icon`/`muda` 类型、platform handle、错误和回调不得进入 `bongocat-runtime`、
  `bongocat-ui` 或公共协议；adapter 只暴露项目自有的 `SystemMenu`、`SystemMenuAction`、
  `SystemMenuError` 和 `SystemMenuPresentation`。
- 新依赖精确固定为 `tray-icon = "=0.25.0"` 与 `muda = "=0.20.0"`，两者都关闭默认 features，
  只在 macOS/Windows target 声明。直接 `muda` 必须与 `tray-icon` 实际依赖的版本解析为同一
  package，避免菜单类型来自两个版本；升级时以 `cargo tree` 复核。
- PNG 格式契约由 `bongocat-app` build script 与 `product_icon_contract` 测试固定：PNG 签名、
  IHDR、尺寸、通道位深、RGBA、压缩/过滤方法和隔行字段，不把 `image` 类型暴露为产品 API。ADR
  记录字节 hash 作为来源与变更审计证据，不把 hash 写成运行时业务校验。
- `SystemMenuPresentation::title` 保留为兼容输入，但迁移后不映射到 native 菜单标题；菜单中的
  可见本地化文本、状态和可用性仍逐项同步。若未来产品需要可变菜单标题，必须先确认 `muda` 的
  上游 API，不得绕过安全边界直接修改库内部 `NSMenu`。

## 安全与生命周期不变量

- macOS 创建、更新、弹出菜单和销毁均要求 `MainThreadMarker`；`set_presentation` 也执行主线程
  检查，避免在错误线程触碰 AppKit。
- Windows `show_context_menu_for_hwnd` 需要在 `unsafe` block 中传入有效 HWND。应用窗口右键传入
  由 overlay session 持有、覆盖整个同步 `TrackPopupMenu` 生命周期的真实 overlay HWND；不得使用
  托盘隐藏窗口作为替代 owner。菜单 tracking 是同步调用，调用方不得在回调内销毁 overlay 或 adapter。
- macOS `show_context_menu_for_nsview` 要求 main thread 和有效 `NSView`。应用窗口右键传入
  overlay panel 的 content view；panel 保留该 view，调用期间也由 overlay session 保持存活，
  同步弹出结束后才返回。
- `SystemMenu` 的字段顺序保证 `tray_icon` 先于菜单项与根菜单析构；显式 shutdown 先隐藏图标，
  再按既定产品顺序停止输入、runtime、配置、GPU 和 overlay。禁止在多个线程或模块创建第二个
  `SystemMenu`。
- Windows adapter 捕获 `TrayIconEvent` 时按自有的稳定 `TrayIconId` 过滤，不消费其他 owner 的
  事件；菜单事件 receiver 也只在同一个 app owner 线程轮询并转换为强类型队列。
- `tray-icon 0.25.0` 的 Windows `set_visible` 会发送 `WM_USER_SHOW_TRAYICON` 并在库内部忽略
  `Shell_NotifyIconW(NIM_MODIFY)` 的失败结果，可能对已失效的 shell registration 返回 `Ok`；
  同样，初次 `NIM_ADD` 失败会等待 `TaskbarCreated` 恢复。adapter 的 `Ok` 不能替代真实托盘
  可见性验收，发布前必须在 Windows 10 1903+ 实机验证隐藏、恢复、Explorer 重启和右键菜单。
- `tray-icon 0.25.0` 的 Windows `set_tooltip` 对以固定 GUID 注册的图标必然失败：它发出的
  `NIM_MODIFY` 未带 `NIF_GUID`，而 shell 对以 `guidItem` 标识的图标忽略 `uID`，并要求后续每次
  `Shell_NotifyIcon` 调用都携带同一 GUID
  （<https://learn.microsoft.com/windows/win32/api/shellapi/ns-shellapi-notifyicondataw#troubleshooting>）。
  库内 `set_icon` 与内部 `set_tray_visible` 都调用了 `apply_guid`，只有 `set_tooltip` 漏掉，属上游
  缺陷；上游 `dev` 分支同样如此，`0.25.0` 已是 crates.io 最新稳定版，没有可升级的修复版本。
  因此 `SystemMenuPresentation::tooltip` 是**创建期输入**：由 `start_with_presentation` 经
  `with_tooltip` 一次性写入（该路径的 `NIM_ADD` 携带 `NIF_GUID`，可正常工作），
  `set_presentation` 不得再调用 `set_tooltip`。当前产品文案 `system_menu.title` 在中英目录中
  恒为 `BongoCat`，运行期不发生变化；若将来需要运行期更换 tooltip 文案，必须连同 GUID 一起
  重建托盘 owner，或先确认上游已修复该调用。

## 验证

- `product_icon_contract` 覆盖两个 PNG 的容器/尺寸/RGBA 格式，build script 在编译前验证资源，
  Windows RC 只嵌入产品 ICO，不再把托盘 PNG/ICO 作为 executable icon group。
- macOS/Windows target 的 `cargo check`、release build、完整 workspace fmt/clippy/test/check 和
  native dependency policy 必须通过。
- macOS release system-menu smoke 需验证 `tray-icon` 托盘 owner、直接 `muda` 菜单 action、显隐恢复
  与 shutdown；现有 smoke 不合成 overlay 右键，因此不能替代真实菜单弹出验证。
- 发布前必须在 Windows 10 1903+ 与受支持 macOS 实机验证 overlay 右键菜单的 cursor 定位、缩放/DPI、
  窗口层级、点击外部关闭、action 派发和 shutdown；Windows x64/ARM64 cross-check 只能证明编译与
  边界，不能证明 tray registration、右键菜单、左键打开设置、Explorer 重启恢复或资源管理器交互。
- `native-workspace` 的 `Smoke Windows D3D11 product overlay` 在真实 Windows runner 上运行
  `--settings-window-open-smoke` 与 `--settings-window-smoke --models-page-smoke`，是上述
  `set_tooltip` 缺陷的唯一回归来源：2026-09-14 的 job 日志显示 `set_presentation` 每 50ms 重试一次，
  在 30 秒内累计 380 条完全相同的 `StatusItemUpdateFailed`。该 job 和其中每个 smoke step 都必须
  保持启用，不得以跳过、忽略错误或放宽断言的方式让 CI 通过。
- 当前环境未运行 Windows 实机 smoke；Windows 托盘与右键菜单行为仍是发布门禁，不得以
  cross-compile 结果宣称完成。

## 替换边界

替换点只有 `bongocat-platform` 私有 `system_menu_native` adapter 和 overlay 的
`HasWindowHandle` 实现。升级或替换 `tray-icon`/`muda` 时必须复验：两者仍解析到同一 `muda`
package、双平台菜单顺序与启用状态、Windows 固定 GUID、GUID 注册下的 `set_tooltip` 是否已修复、
左键/右键行为、macOS template image 与主线程约束、overlay HWND/`NSView` 生命周期、cursor 定位、
隐藏后的恢复、shutdown 清理、依赖 target/features、Windows shell failure 语义和 PNG 资源格式。
adapter 之外不得依赖第三方 tray/menu 类型。
