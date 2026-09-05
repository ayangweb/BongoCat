# ADR-0024: macOS TCC Capability Boundary

状态：已接受（2026-09-05）

## 背景

Native Rewrite 需要在应用失焦时监听键盘和鼠标边沿，以驱动 BongoCat overlay；同时设置窗口必须向
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
