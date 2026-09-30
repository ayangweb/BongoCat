# ADR-0024: macOS TCC Capability Boundary

状态：已接受（2026-09-05）

> 后续修订（2026-09-23）：ADR-0054 退役了项目自有 AccessKit/AppKit 设置桥；Input Monitoring 与 Accessibility TCC 的边界仍然有效。

## 背景

BongoCat 需要在应用失焦时监听键盘和鼠标边沿，以驱动 BongoCat overlay；同时设置窗口必须向
VoiceOver 等辅助技术公开自身的可访问语义。两种能力不能因为都被 macOS 归入隐私或辅助技术领域而
混为同一项 TCC 请求。

## 决策

- 全局键盘、鼠标和修饰键监听只使用 Input Monitoring。`bongocat-platform` 仅以
  `CGPreflightListenEventAccess` 查询状态，并只在由用户发起的明确设置操作中允许调用
  `CGRequestListenEventAccess`；启动、轮询和服务恢复不得弹出请求或重复请求。
- listen-only `CGEventTap`、`CGEventSourceKeyState` 和 `CGEventSourceButtonState` 都属于上述
  Input Monitoring 用途。权限拒绝或撤销必须让输入服务进入匿名 `PermissionDenied` 状态并可靠 Reset；
  overlay、设置窗口和本地配置仍继续运行。
- 设置窗口的 AccessKit/AppKit adapter 只公开 BongoCat 自己窗口的可访问树和 action channel。它不
  读取、控制或自动化其它应用，不调用 Accessibility trust/prompt API，因此不得为此请求 Accessibility
  TCC 权限。
- `NSOpenPanel`、`NSWorkspace` URL 打开和 pasteboard wrapper 保持各自最小能力边界；它们不得借由
  Input Monitoring 或 Accessibility 状态扩大访问范围。
- Settings 必须分别显示 Input Monitoring 的当前状态和输入服务状态。状态变化只更新 snapshot/诊断，
  不把“已授权”表述为输入服务已经重启；重新启动输入服务仍必须经过显式、受控的 owner 生命周期。

## 验证

- 静态检查确认平台输入路径只使用 listen-event preflight/request API，AccessKit bridge 不链接或调用
  Accessibility trust API。
- macOS 实机矩阵覆盖未授权、拒绝、授权和撤销 Input Monitoring；确认不出现 Accessibility 提示，且
  拒绝时 overlay/settings 保持可用。
- 后续 TCC UI 刷新实现必须分别测试 permission snapshot 与 service status，确保授权变化不会被误报为
  已运行的 event tap。

## 引导流程与逐次重置（2026-09-30，ADR-0078）

ADR-0078 把 macOS 的引导动作换成 `permission-flow` 的浮动面板，并在**每次进入授权流程前**执行
`tccutil reset ListenEvent com.ayangweb.bongo-cat`。本 ADR 的 Input Monitoring 边界因此有两处修订：

- 「只在由用户发起的明确设置操作中允许调用 `CGRequestListenEventAccess`」仍然成立，且引导流程
  整体位于用户点击之后，没有引入启动或轮询路径上的 TCC 请求。
- 但「不重复请求」的语义改变了：逐次重置会主动把已有授权清回未授权，因此用户每次进入引导都要重新
  授权。这是产品明确要求的行为，不是实现偏差。
- 「权限拒绝或撤销必须让输入服务进入匿名 `PermissionDenied` 状态并可靠 Reset」中，**进入
  `PermissionDenied` 这一半已有覆盖**（tap 被禁用 → 重启 → 进程内 `CGPreflightListenEventAccess`
  复查 → `PermissionDenied`），但**重新授权之后没有自动重启输入服务的触发点**：重置使运行中的
  tap 失效，用户重新授权后输入要等应用重启才恢复。这一缺口记录在 ADR-0078 的「未完成项」。
- 能力面保持最小：`permission-flow` 虽然暴露 8 类权限，但产品只使用 `INPUT_MONITORING` 一项，
  且其类型不离开 `bongocat-platform` 的私有 adapter。
