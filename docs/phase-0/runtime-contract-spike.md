# Runtime Contract Spike

状态：平台无关 runtime 生命周期与 shutdown contract 已通过；产品 runtime owner、线程、channel 和真实工作预算待 Phase 1/2
日期：2026-08-28

`spikes/runtime-contract/` 固定产品 runtime 必须满足的最小边界：

- `Starting -> Ready/Degraded -> Stopping -> Stopped` 是显式生命周期；startup 可以被取消并直接进入 shutdown，未进入可运行状态不能 tick 或提交 operation；
- tick 使用单调的相对毫秒值，时间回退返回 `NonMonotonicTick`，不得驱动墙上时间动画；
- 有副作用的 operation 使用 `u64` operation id 去重，重复提交返回 `Duplicate` 且不增加待处理工作；
- shutdown 进入 `Stopping` 后必须先 drain pending work；重复 completion 是显式错误，超时可以停止但必须报告被丢弃的工作数量；
- contract 只保存计数和状态，不持有 GPUI、平台句柄、模型或 GPU 资源。

## Verification

```text
cargo fmt --manifest-path spikes/runtime-contract/Cargo.toml -- --check
cargo clippy --manifest-path spikes/runtime-contract/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path spikes/runtime-contract/Cargo.toml --locked
cargo run --manifest-path spikes/runtime-contract/Cargo.toml --locked
```

当前 7 个单元测试覆盖 degraded/recovery、startup 取消、tick 回退、operation 去重、重复 completion、shutdown drain 和超时报告。该 spike 不证明跨线程唤醒、队列容量、runtime panic 隔离、模型求值或平台输入行为；产品 crate 必须以相同 contract 重新实现并补充 worker/join 验收。
