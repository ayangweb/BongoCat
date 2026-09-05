# Pressed State Reliability Spike

状态：平台无关 pressed-set contract、校正调度/误判保护和 issue #47 丢失 release 恢复测试通过；Raw Input/CGEventTap 实机接入待完成
日期：2026-08-28

## 目的

`spikes/input-state/` 只固定输入状态层的可靠性不变量，不接收平台 callback，也不依赖 `rdev`、`monio` 或 GPUI。平台采集器产生有序 `Down`/`Up`，并在状态校正、设备变化、锁屏或服务重启时发送 `Reconcile`/`Reset`。

状态层只保留一个 `BTreeSet<InputKey>`：重复按下不会复制状态；正常 `Up`、校正快照中缺失的 key 和 `Reset` 都能释放 pressed state。可靠事件可以包装为带单调 sequence 的 `SequencedInputEvent`；重复/乱序事件会被计数并忽略，出现跳号时先执行安全 `Reset` 再应用当前事件。所有计数器只记录事件类别和序列异常数量，不记录真实键值。

`ReconciliationScheduler` 使用单调毫秒时钟，首次 poll 立即到期，之后默认每 `250 ms` 允许一次查询；时间回退返回错误且不移动调度游标。`PressedState::reconcile_with_policy` 默认要求同一个本地 pressed key 连续 `2` 次校正快照缺失才释放，单次异常的系统查询不会造成误清除；看到 key 仍按下会清除该 key 的待确认次数。`Down`、`Up` 和 `Reset` 会清理待确认状态，生命周期 `Reset` 不等待确认阈值。平台 adapter 应在调度到期时提供候选 pressed-set，不能把该策略替换为固定自动释放 timer。

## 验证

```text
cargo fmt --manifest-path spikes/input-state/Cargo.toml -- --check
cargo test --manifest-path spikes/input-state/Cargo.toml --locked
```

测试覆盖：issue #47 中 `Ctrl+Alt+A` 的 A-up 丢失恢复、仍按住的键不被误清除、重复 down 计数、session reset、序列跳号安全 reset、重复/乱序序列拒绝、校正周期/单调时钟、零值策略拒绝、瞬时缺失保护和连续缺失释放。它证明的是状态层行为，不能替代 Windows Raw Input、`GetAsyncKeyState` 或 macOS `CGEventSourceKeyState` 实机验证；平台尚未接入该 scheduler，也未证明实机校正延迟或 TCC/锁屏恢复。
