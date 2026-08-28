# ADR-0004: Reconcilable Input State

状态：Accepted, pending platform validation
日期：2026-08-28

## Context

系统快捷键、锁屏、安全桌面、权限变化或输入队列异常可能导致应用收到按下边沿却收不到释放边沿。只依赖事件配对会产生永久 pressed state。

## Decision

输入状态由三类信号共同维护：

1. 低延迟的按下/释放事件。
2. 对当前 pressed set 的系统状态校正。
3. 锁屏、睡眠、设备移除和服务重启等生命周期 `Reset`。

Key/button edge 使用可靠有序队列。鼠标移动和手柄轴使用 latest-value 合并，不得阻塞释放边沿。

Windows 以 Raw Input 为主路径，并用 `GetAsyncKeyState` 校正。macOS 使用 CGEventTap，并用 `CGEventSourceKeyState` 校正。

## Consequences

- 单个释放事件丢失不会永久卡键。
- 队列溢出必须可观测，不能静默丢弃边沿。
- 自动释放超时只作为最后保险，不是正常输入语义。
- Renderer 不直接查询系统键盘状态。

## Verification

Windows 强制覆盖 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回。双平台覆盖锁屏、睡眠、设备变化、服务 restart 和输入压力测试。
