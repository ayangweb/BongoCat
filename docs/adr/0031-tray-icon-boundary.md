# ADR-0031: Tray Icon Library Boundary

状态：已接受（2026-09-13）

## 背景

Windows 的 `Shell_NotifyIcon`/`HMENU` 与 macOS 的 `NSStatusItem`/`NSMenu` 原本各自实现，约 690 行
平台代码需要分别维护隐藏窗口、菜单句柄、回调、主线程与生命周期。两边已经共享
`SystemMenuAction`、`SystemMenuError` 和 `SystemMenuPresentation`，但 native owner 的行为与
清理顺序仍容易漂移。项目需要在不改变 runtime/UI 强类型边界的前提下，减少重复平台代码并保留
Windows GUID、菜单顺序、左键打开设置和右键菜单等产品行为。

## 调研结论

已核实（2026-09-13，`cargo search`、`cargo info`、`cargo tree` 与源码阅读）：

- `tray-icon 0.25.0`（MIT OR Apache-2.0，Rust 1.90+）是 crates.io 当日最新稳定版，由 Tauri
  项目维护，同时支持 macOS `NSStatusItem` 与 Windows `Shell_NotifyIcon`，并提供图标、tooltip、
  菜单、固定 GUID 和显隐 API。当前 toolchain 为 Rust 1.97.1，满足其 MSRV。
- `tray-icon` 重导出匹配版本的 `muda 0.20.0`（Apache-2.0 OR MIT）菜单 API；项目使用该重导出，
  不额外声明直接 `muda` 依赖。`Menu`/`MenuItem`/`CheckMenuItem` 提供菜单构建与动态文本、状态
  更新，但没有可设置的菜单标题，因此既有 `SystemMenuPresentation::title` 不再有对应可变标题 API。
- `tray-icon` 的 `TrayIcon`、`TrayIconEvent::receiver` 与 `muda::MenuEvent::receiver` 均以进程级
  状态和 `Rc<RefCell<_>>`/静态 channel 实现。一个进程只能有一个长期存活的 `SystemMenu` 业务
  owner；隐藏图标只切换平台表示，不能销毁并重建 owner，否则会丢失菜单事件消费者。
- Windows 固定 GUID 仍使用
  `123f3c6f-7d2a-4ca3-b8cb-9b1d1eaf2f10`。库以 GUID 作为托盘身份并在 Explorer
  `TaskbarCreated` 广播后恢复注册；应用不自行重试或创建平行图标。
- `default-features = false` 关闭 `libappindicator` 与 `muda-libxdo`；该依赖只在
  macOS/Windows target 下声明，因此 Linux 不引入 GTK/libappindicator 系统依赖。Linux 托盘仍按
  ADR-0006 单独评估，不属于本决策。
- Windows PNG 资源来自维护者指定的
  `https://raw.githubusercontent.com/ayangweb/BongoCat/refs/heads/master/src-tauri/assets/tray.png`，
  已逐字节核验为 256×256、8-bit RGBA、非隔行 PNG，SHA-256 为
  `65f3a1e71f60417916c0511945131c7341edbd46d037fa9e703014e6155434dd`。macOS 现有
  `tray-macos.png` 也固定为 256×256、8-bit RGBA、非隔行，SHA-256 为
  `1e39a05cc356501518291acea439c06a7f7e944ad3a227f29387875f3604af9a`。

## 决策

- macOS 与 Windows 状态图标统一由 `bongocat-platform` 私有 `system_menu_native` adapter 管理，
  native owner 使用 `tray-icon 0.25.0`；旧 `system_menu_macos.rs`、`system_menu_windows.rs` 与
  `tray-windows.ico` 退役。
- adapter 是 `SystemMenu` 的唯一 owner，长期持有 `TrayIcon`、根 `Menu`、全部可变菜单项和
  `muda` 菜单事件 receiver。`set_visible(false)` 只改变平台表示（macOS 移除 `NSStatusItem`，
  Windows 保留注册并设置隐藏），菜单 owner 与事件通道不销毁；重新显示不创建第二套业务状态。
- Windows 使用固定 GUID 与 `tray-windows.png`；macOS 使用 `tray-macos.png` 并标记为 template
  image。两平台在菜单构建时共享相同 action id 到项目自有 `SystemMenuAction` 的映射。
- Windows 左键抬起映射为 `OpenSettings`，右键由 `tray-icon` 弹出同一 `muda` 菜单。macOS 左右键
  都由库弹出同一菜单。应用窗口右键的恢复入口继续调用 adapter 的 `show_context_menu`，不创建
  第二套菜单。
- 第三方 `tray_icon`/`muda` 类型、platform handle、错误和回调不得进入 `bongocat-runtime`、
  `bongocat-ui` 或公共协议；adapter 只暴露项目自有的 `SystemMenu`、`SystemMenuAction`、
  `SystemMenuError` 和 `SystemMenuPresentation`。
- 新依赖精确固定为 `tray-icon = "=0.25.0"`，关闭默认 features，只在 macOS/Windows target
  声明；不直接声明 `muda`，其版本由 `tray-icon` 的重导出锁定。
- PNG 格式契约由 `bongocat-app` build script 与 `product_icon_contract` 测试固定：PNG 签名、
  IHDR、尺寸、通道位深、RGBA、压缩/过滤方法和隔行字段，不把 `image` 类型暴露为产品 API。ADR
  记录字节 hash 作为来源与变更审计证据，不把 hash 写成运行时业务校验。
- `SystemMenuPresentation::title` 保留为兼容输入，但迁移后不映射到 native 菜单标题；菜单中的
  可见本地化文本、状态和可用性仍逐项同步。若未来产品需要可变菜单标题，必须先确认 `muda` 的
  上游 API，不得绕过安全边界直接修改库内部 `NSMenu`。

## 安全与生命周期不变量

- macOS 创建、更新、弹出菜单和销毁均要求 `MainThreadMarker`；`set_presentation` 也执行主线程
  检查，避免在错误线程触碰 AppKit。
- Windows `show_context_menu_for_hwnd` 需要在 `unsafe` block 中传入有效 HWND。该 HWND 由
  `tray_icon` 拥有且与 `SystemMenu` 同生命周期；菜单 tracking 是同步调用，调用方不得在回调内
  销毁 adapter。
- `SystemMenu` 的字段顺序保证 `tray_icon` 先于菜单项与根菜单析构；显式 shutdown 先隐藏图标，
  再按既定产品顺序停止输入、runtime、配置、GPU 和 overlay。禁止在多个线程或模块创建第二个
  `SystemMenu`。
- Windows adapter 捕获 `TrayIconEvent` 时按自有的稳定 `TrayIconId` 过滤，不消费其他 owner 的
  事件；菜单事件 receiver 也只在同一个 app owner 线程轮询并转换为强类型队列。
- `tray-icon 0.25.0` 的 Windows `set_visible` 会发送 `WM_USER_SHOW_TRAYICON` 并在库内部忽略
  `Shell_NotifyIconW(NIM_MODIFY)` 的失败结果，可能对已失效的 shell registration 返回 `Ok`；
  同样，初次 `NIM_ADD` 失败会等待 `TaskbarCreated` 恢复。adapter 的 `Ok` 不能替代真实托盘
  可见性验收，发布前必须在 Windows 10 1903+ 实机验证隐藏、恢复、Explorer 重启和右键菜单。

## 验证

- `product_icon_contract` 覆盖两个 PNG 的容器/尺寸/RGBA 格式，build script 在编译前验证资源，
  Windows RC 只嵌入产品 ICO，不再把托盘 PNG/ICO 作为 executable icon group。
- macOS/Windows target 的 `cargo check`、release build、完整 workspace fmt/clippy/test/check 和
  native dependency policy 必须通过。
- macOS release system-menu smoke 需验证 `tray-icon` owner、菜单 action、显隐恢复与 shutdown；
  Windows x64/ARM64 cross-check 只能证明编译与边界，不能证明 tray registration、右键菜单、
  左键打开设置、Explorer 重启恢复或资源管理器交互。
- 当前环境未运行 Windows 实机 smoke；Windows 托盘行为仍是发布门禁，不得以 cross-compile
  结果宣称完成。

## 替换边界

替换点只有 `bongocat-platform` 私有 `system_menu_native` adapter。升级或替换
`tray-icon`/`muda` 时必须复验：双平台菜单顺序与启用状态、Windows 固定 GUID、左键/右键行为、
macOS template image 与主线程约束、隐藏后的恢复、shutdown 清理、依赖 target/features、
Windows shell failure 语义和 PNG 资源格式。adapter 之外不得依赖第三方 tray/menu 类型。
