# Input Queue Contract Spike

状态：平台无关 bounded reliable queue、溢出恢复、单槽与 keyed latest-value channel contract 已通过；runtime worker、实际 producer 和 shutdown coordinator 待产品 crate
日期：2026-08-29

## Contract

`spikes/input-queue/` 固定输入传输的容量和关闭语义，不依赖平台 API、GPUI 或第三方 channel：

- `ReliableQueue<T>` 只承载 key/button edge、设备生命周期事件和 command；容量必须为正且固定；
- 满载时返回原始 item 并增加 `overflow_count`，调用方必须触发安全恢复（通常是 `Reset`），不得静默丢弃；
- `push_with_overflow_reset` 将溢出变成显式恢复：保留原始失败 item、清空无法证明顺序的已缓存 item、计数丢弃数量，并把调用方提供的 `Reset` 标记放到队首；关闭队列不再注入恢复标记；
- 关闭后 push 返回原始 item，消费者仍可 drain 已入队事件；
- `LatestValue<T>` 只承载单一 cursor 等高频值，更新会替换旧值，不得用于可靠边沿；
- `LatestValues<K, V>` 为多设备/多轴提供固定 key 容量；已有 key 更新可合并，新 key 满载会返回原 key/value 并计数，不允许异常 profile 扩成无界 map；
- keyed channel 记录 captured、coalesced、consumed、disconnect discarded、overflow 和 close 后拒绝，并校验每个 accepted sample 均被解释；
- 手柄 key 包含连接 generation。断开丢弃旧 generation，重连分配新 generation，使迟到 callback 不会把旧 axis 应用到新连接。

## Verification

```text
cargo fmt --manifest-path spikes/input-queue/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-queue/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path spikes/input-queue/Cargo.toml --locked
cargo check --manifest-path spikes/input-queue/Cargo.toml --locked --release
```

11 项测试还证明恢复标记优先于旧边沿、恢复归档不会静默成功，以及关闭状态不会伪造恢复事件。gamepad contract 将同一轴 10,000 次更新合并为最终样本，独立保留不同轴；六轴容量边界拒绝第七个未知 key；断开 generation 41 后只允许 generation 42 的重连样本；shutdown flush pending 并拒绝迟到 publish。commit `16a51bb` 的 push run `33259120950`、job `99117907732` 已通过 format、Clippy 和全部测试。

该 spike 证明的是容器与 generation 契约，不证明 XInput/GameController producer、runtime 的实际容量选择、跨线程唤醒、背压策略或退出时限；产品 producer 必须把可靠连接/断开/按钮边沿与 keyed axis channel 按该协议接入，并把匿名诊断送入 runtime snapshot。
