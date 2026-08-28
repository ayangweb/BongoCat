# Pressed State Reliability Spike

状态：平台无关 pressed-set contract 和 issue #47 丢失 release 恢复测试通过；Raw Input/CGEventTap 实机接入待完成
日期：2026-08-28

## 目的

`spikes/input-state/` 只固定输入状态层的可靠性不变量，不接收平台 callback，也不依赖 `rdev`、`monio` 或 GPUI。平台采集器产生有序 `Down`/`Up`，并在状态校正、设备变化、锁屏或服务重启时发送 `Reconcile`/`Reset`。

状态层只保留一个 `BTreeSet<InputKey>`：重复按下不会复制状态；正常 `Up`、校正快照中缺失的 key 和 `Reset` 都能释放 pressed state。可靠事件可以包装为带单调 sequence 的 `SequencedInputEvent`；重复/乱序事件会被计数并忽略，出现跳号时先执行安全 `Reset` 再应用当前事件。所有计数器只记录事件类别和序列异常数量，不记录真实键值。

## 验证

```text
cargo fmt --manifest-path spikes/input-state/Cargo.toml -- --check
cargo test --manifest-path spikes/input-state/Cargo.toml --locked
```

测试覆盖：issue #47 中 `Ctrl+Alt+A` 的 A-up 丢失恢复、仍按住的键不被误清除、重复 down 计数、session reset、序列跳号安全 reset 以及重复/乱序序列拒绝。它证明的是状态层行为，不能替代 Windows Raw Input、`GetAsyncKeyState` 或 macOS `CGEventSourceKeyState` 实机验证。
