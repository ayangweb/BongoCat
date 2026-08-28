# Input Queue Contract Spike

状态：平台无关 bounded reliable queue、溢出恢复和 latest-value channel contract 已通过；runtime worker、实际 producer 和 shutdown coordinator 待产品 crate
日期：2026-08-28

## Contract

`spikes/input-queue/` 固定输入传输的容量和关闭语义，不依赖平台 API、GPUI 或第三方 channel：

- `ReliableQueue<T>` 只承载 key/button edge、设备生命周期事件和 command；容量必须为正且固定；
- 满载时返回原始 item 并增加 `overflow_count`，调用方必须触发安全恢复（通常是 `Reset`），不得静默丢弃；
- `push_with_overflow_reset` 将溢出变成显式恢复：保留原始失败 item、清空无法证明顺序的已缓存 item、计数丢弃数量，并把调用方提供的 `Reset` 标记放到队首；关闭队列不再注入恢复标记；
- 关闭后 push 返回原始 item，消费者仍可 drain 已入队事件；
- `LatestValue<T>` 只承载 cursor/axis 等高频值，更新会替换旧值，不得用于可靠边沿。

## Verification

```text
cargo fmt --manifest-path spikes/input-queue/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-queue/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-queue/Cargo.toml --locked
```

测试还证明恢复标记优先于旧边沿、恢复归档不会静默成功，以及关闭状态不会伪造恢复事件。该 spike 证明的是容器契约，不证明 runtime 的容量选择、跨线程唤醒、背压策略或退出时限；产品 producer 必须在捕获队列溢出时调用 recovery API，并把诊断计数送入 runtime snapshot。
