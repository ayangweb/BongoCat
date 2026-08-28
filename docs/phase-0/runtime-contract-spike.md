# Runtime Contract Spike

状态：平台无关 runtime 生命周期、bounded worker、snapshot 和 shutdown contract 已通过；产品 runtime owner、真实输入/模型工作预算待 Phase 1/2
日期：2026-08-28

`spikes/runtime-contract/` 固定产品 runtime 必须满足的最小边界：

- `Starting -> Ready/Degraded -> Stopping -> Stopped` 是显式生命周期；startup 可以被取消并直接进入 shutdown，未进入可运行状态不能 tick 或提交 operation；
- tick 使用单调的相对毫秒值，时间回退返回 `NonMonotonicTick`，不得驱动墙上时间动画；
- 有副作用的 operation 使用 `u64` operation id 去重，重复提交返回 `Duplicate` 且不增加待处理工作；
- shutdown 进入 `Stopping` 后必须先 drain pending work；重复 completion 是显式错误，超时可以停止但必须报告被丢弃的工作数量；
- contract 只保存计数和状态，不持有 GPUI、平台句柄、模型或 GPU 资源。

worker contract 进一步固定：

- `CommandQueue` 使用固定容量、FIFO 和 `Condvar` 唤醒；满载时清空无法证明顺序的命令、计数并注入 `Reset { reason: QueueOverflow }`，原失败命令通过错误返回给 producer；
- worker 独占 `RuntimeContract`，每次处理后发布递增 `RuntimeSnapshot.revision`；snapshot 只包含状态、计数和队列诊断，不包含平台对象或用户内容；
- `RuntimeWorker::shutdown` 先关闭 producer，再 drain 已入队命令；队列排空但仍有未完成工作时返回 `TimedOut { discarded_work }`，不伪装为成功完成；
- worker 通过 `catch_unwind` 隔离 panic，关闭 queue 并返回 `WorkerExit::Panicked`；正常退出和 panic 都有明确的 join report。

## Verification

```text
cargo fmt --manifest-path spikes/runtime-contract/Cargo.toml -- --check
cargo clippy --manifest-path spikes/runtime-contract/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path spikes/runtime-contract/Cargo.toml --locked
cargo run --manifest-path spikes/runtime-contract/Cargo.toml --locked
```

当前 13 个单元测试覆盖 degraded/recovery、startup 取消、tick 回退、operation 去重、重复 completion、reset、bounded queue 溢出恢复、worker revision snapshot、shutdown drain/timeout、command error 和 panic/join 报告。该 spike 不证明模型求值、平台输入行为、实时工作预算或产品级 channel 选型；产品 crate 必须以相同 contract 重新实现并补充 runtime/input/model 集成验收。
