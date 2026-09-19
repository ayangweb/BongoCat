# ADR-0043: Startup Item Backend via auto-launch

状态：Accepted
日期：2026-09-17
取代：ADR-0013

## Context

ADR-0013 建立了环境隔离的双平台启动项能力：Windows 使用 HKCU Run，macOS 13+
Production `.app` 使用 `SMAppService.mainAppService`，macOS 12 与 Development
构建明确报告 unsupported。该实现（`P7-STARTUP-ITEM-PLATFORM`、
`P5-STARTUP-ITEM-UI`）已通过双平台 smoke 与 CI 验收。

产品决策改变：启动项是用户可直接感知的功能，macOS 12 用户（产品声明的首发
平台）和 Development 构建不应被排除。同时，维护两套平台后端（HKCU 注册表
raw binding、objc2 ServiceManagement）只为一项可选能力付出不成比例的成本。
经评审选择成熟的开源方案 `auto-launch` crate（crates.io 最新稳定版 0.6.0，
MIT，活跃维护）统一双平台后端。

## Decision

- 引入 `auto-launch = "=0.6.0"` 作为 `bongocat-platform` 的双平台启动项后端，
  替换 `startup_item_macos.rs` 与 `startup_item_windows.rs` 的手写实现。
- macOS 使用 `MacOSLaunchMode::LaunchAgent`：enable 写
  `~/Library/LaunchAgents/{app_name}.plist`（`Label`、
  `AssociatedBundleIdentifiers`、`ProgramArguments`、`RunAtLoad`），disable
  删除 plist，`is_enabled` 检查 plist 存在。该机制在 macOS 12/13+ 行为一致，
  无 TCC 授权、无特权要求、不触碰 SMAppService 状态。注册项在系统设置的
  后台项目列表中可见（经 `AssociatedBundleIdentifiers` 关联应用）。
- Windows 使用 `WindowsEnableMode::CurrentUser`：写入
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，并同步写入
  `StartupApproved\Run` 启用标记，`is_enabled` 同时尊重任务管理器的禁用覆盖。
- 环境隔离保留：`StartupItemEnvironment` 仍映射为不同的
  `app_name`（`BongoCat Development` / `BongoCat Production`）。macOS 上是
  不同的 plist Label 与文件名，Windows 上是不同的注册表 value name，开发与
  生产互不覆盖，Development 构建从此支持启动项。
- 启动命令统一携带 `--run-seconds 0`（显式产品启动语义）；Windows 侧由
  auto-launch 按 MSVCRT 参数规则对 executable 加引号拼接。
- 共享 contract（`StartupItemState`、`StartupItemError`、
  `StartupItemUnsupportedReason`）保持不变以稳定 UI/typed command 边界；
  后端现只会产生 `Enabled`、`Disabled` 与错误。`Stale`、`RequiresApproval`、
  `NotFound` 与 `Unsupported(OperatingSystem/BuildEnvironment)` 保留为契约
  变体但当前无生产者，UI 对其的处理分支继续作为防御性路径存在。
- 启用、禁用仍只能由显式用户 command 触发；失败不改变配置、不阻塞
  input/runtime/renderer，与 ADR-0013 的隔离与容错原则一致。

## Consequences

- macOS 12 获得启动项支持；macOS 13+ 的注册方式从 SMAppService 变为
  LaunchAgent plist，不再有 requires-approval 状态。
- LaunchAgent 项随登录由 launchd 拉起（`RunAtLoad`），enable 后不立即启动、
  也无 launchctl bootstrap；ProgramArguments 直接运行 bundle 内可执行文件，
  与用户双击 `.app` 的启动语义存在差异（工作目录、环境变量更空），属已知
  且可接受的行为。
- Windows 不再检测 stale（安装位置变化后 `is_enabled` 仍为 enabled，启动时
  才会失败）；用户重新开关一次即可修复。ADR-0013 的 stale 状态由此退役。
- plist 由 auto-launch 以字符串模板生成，路径含 XML 特殊字符时可能生成
  无效 plist；产品路径不含此类字符，作为上游已知边界记录。
- `objc2-service-management` 依赖随 SMAppService 后端一并移除。

## Verification

- 单元测试覆盖环境命名隔离、错误 code 稳定性与后端构造。
- opt-in smoke（`cargo test -p bongocat-platform -- --ignored`）驱动当前
  平台 disabled -> enabled -> disabled 并恢复原状态，且不触及另一环境的
  注册项。
- macOS `--startup-item-smoke` 产品 smoke 继续验证 Production 安装态
  lifecycle（现映射到 LaunchAgent plist）。
- 双平台完整 Native 门禁（fmt、严格 Clippy、workspace test）通过；Windows
  实机回归由 CI 承担。
