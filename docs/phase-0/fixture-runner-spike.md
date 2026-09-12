# Rust Fixture Runner Spike

状态：Phase 0 共享输入 fixture 已由 Rust reducer 执行；产品 runtime 接入待 Phase 1/2
日期：2026-08-29

## Scope

`spikes/fixture-runner/` 是无平台依赖的行为 contract，不是产品 runtime。它使用强类型 Rust
事件执行 `shared/fixtures/input-sequences/` 的全部序列，并在每个 expected checkpoint 生成
规范化 input/model snapshot。当前基线为 9 组序列、51 个事件、24 个 checkpoint 和 1 个
有序 audio cue。

runner 固定以下语义：

- 相同 `atMs` 按数组顺序处理，checkpoint 在该时间的所有事件后生成；时间回退直接失败；
- keyboard/mouse/gamepad pressed state、device 生命周期、cursor、Reset、motion priority、
  expression 和 model switch 由单一 reducer owner 更新；
- gamepad button 使用共享 `0.5` 阈值，未连接设备的 button/axis 事件直接失败；
- disconnect 清除该设备的 button/axis，Reset 清除 pressed/latest input，但不伪造设备断连；
- snapshot 集合按稳定字典序输出，浮点值规范到 6 位小数；
- `model.parameters` 包含序列声明/触达的完整参数域，零值也保留，不能由 golden 中已有 key
  反向决定计算范围；
- mismatch 输出 `$.input...`/`$.model...` 字段路径、expected 和 actual，不只返回布尔失败。

旧 Python runner 继续作为实现独立的轻量 oracle，但已同步完整参数域规则。此前 Python 实现
按 expected 中出现的 parameter key 选择实际计算项，删除一个应检查的 key 仍会成功；本次
收紧 golden 后，gamepad reconnect 与 recovery fixture 会同时验证所有已声明参数的按下和清零。

## Dependencies

直接依赖只有审计日 crates.io 最新稳定版 `serde 1.0.229` 与 `serde_json 1.0.151`，许可证均为
MIT OR Apache-2.0。crate 使用 `#![forbid(unsafe_code)]`，JSON 类型不会进入平台 adapter；产品
runtime 建立后应复用强类型事件/snapshot contract，而不是依赖本 spike 的文件路径或 CLI。

## Verification

```text
cargo fmt --manifest-path spikes/fixture-runner/Cargo.toml -- --check
cargo clippy --manifest-path spikes/fixture-runner/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path spikes/fixture-runner/Cargo.toml --locked
cargo run --manifest-path spikes/fixture-runner/Cargo.toml --locked
cargo check --manifest-path spikes/fixture-runner/Cargo.toml --locked --release
cargo deny --manifest-path spikes/fixture-runner/Cargo.toml --locked --config deny.toml check licenses sources --allow license-not-encountered
python3 tools/validate-fixtures.py
python3 tools/validate-json-schema.py
python3 tools/run-input-fixtures.py
```

本机 8 项 Rust test 与完整 CLI run 已通过，报告
`sequences=9 events=51 checkpoints=24 audio_triggers=1`。Phase 0 CI matrix 已加入独立
`fixture-runner` job；push/PR runner 证据在本批提交后补录。该结果冻结平台无关产品语义，
不证明 Raw Input、CGEventTap、手柄 callback、Live2D 求值或产品 runtime 已完成。
