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

经过的时间不是第四条路径。早期的 `input.keyboard.release_fallback_timeout_ms` 按键释放超时已被移除：
用户仍按住的按键与丢失的松开事件在时长上无法区分，用时长区分等于把一帧变慢翻译成一个猫不再
相信的按键。捕获键盘按键因此没有超时配置，runtime 也不维护按键期限或与之相关的匿名计数。

唯一例外是 macOS 的 CapsLock。该键是锁存键，平台实测只投递「锁存翻转」事件而完全不投递物理松开
（见 Technical Design 的 FlagsChanged 条目），它的物理释放在平台层不存在，因此没有「用户仍按着」
这一状态需要区分。平台按产品既定的「短暂触发」语义在固定 `100 ms` 后合成释放边沿。该时长是按键
行为本身，不是超时配置、不进 schema、不暴露设置项，也不得推广到任何其它键。

## Consequences

- 单个释放事件丢失不会永久卡键。
- 队列溢出必须可观测，不能静默丢弃边沿。
- 丢失 `KeyUp` 的按键只能由状态校正或生命周期 `Reset` 释放；校正延迟上限由平台查询周期和确认
  次数共同决定，实机应分别测量权限、锁屏和睡眠恢复下的延迟。
- macOS CapsLock 的释放由 adapter 合成，`CGEventSourceKeyState` 反映锁存状态、无法替代它；该
  合成边沿走 `InputSource::Capture` 通道，runtime 侧因此把它计入 `captured_up`。这是已知且接受的
  精度损失，平台侧计数器不声称该边沿来自系统。
- Renderer 不直接查询系统键盘状态。

## Verification

Windows 强制覆盖 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回。双平台覆盖锁屏、睡眠、设备变化、服务 restart 和输入压力测试。
