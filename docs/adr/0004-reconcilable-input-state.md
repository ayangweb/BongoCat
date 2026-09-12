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

平台 adapter 统一使用单调时钟调度校正：默认间隔为 `250 ms`，同一个本地 pressed key 必须连续 `2` 次系统快照缺失才生成释放。单次查询异常只增加该 key 的待确认次数；后续快照确认仍按下时清零待确认次数。正常 `KeyUp` 和生命周期 `Reset` 立即清理待确认状态，`Reset` 不等待确认阈值；时钟回退不得推进校正调度游标。该策略由平台无关状态 contract 固定，平台只负责提供候选 pressed-set。

`model.release_fallback_timeout_ms` 是捕获键盘按键的最终保险，范围为 `0..=60000`，`0`
明确禁用。runtime 只使用自己可注入的单调时钟记录收到 down/repeat 的时刻，不比较 Windows/macOS
input service 各自原点的事件时间戳；repeat down 刷新期限。到期只移除键盘 control，不自动释放鼠标
或手柄，并通过独立 `fallback_release` 聚合计数与 captured/reconciled/reset 路径区分。

## Consequences

- 单个释放事件丢失不会永久卡键。
- 队列溢出必须可观测，不能静默丢弃边沿。
- 自动释放超时只作为最后保险，不替代可靠 `KeyUp`、系统状态校正或生命周期 `Reset`。
- 校正延迟上限由平台查询周期和确认次数共同决定；实机应分别测量权限、锁屏和睡眠恢复下的延迟。
- Renderer 不直接查询系统键盘状态。

## Verification

Windows 强制覆盖 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回。双平台覆盖锁屏、睡眠、设备变化、服务 restart 和输入压力测试。
