# macOS Input Permission and Tap Lifecycle Spike

状态：权限/tap 生命周期 contract、listen-only CGEventTap、可靠 callback 队列和 run-loop smoke 已通过；真实按键回调、权限矩阵和长期 restart 仍待验证
日期：2026-08-28

## Contract

`spikes/input-macos/` 将 macOS 输入服务拆成三个可观察状态：

- `PermissionState`：`Unknown`、`Denied`、`Granted`；普通启动只 preflight，不主动弹权限请求；
- `TapState`：`Stopped`、`Running`、`Disabled`、`TimedOut`；tap disabled/timeout 后必须 reset pressed state，并在权限仍 granted 时安排 restart；
- `SessionReset`：锁屏、睡眠、快速用户切换、TCC 变化和服务重启都清空 pressed state。

权限撤销会停止 tap、发出 reset 并进入用户引导；重新 granted 后才允许创建 tap。状态层不把 callback 生命周期或 CoreGraphics 指针暴露给 runtime。callback 只生成不含用户内容的 `CapturedInputEvent`，送入固定容量队列；消费发生在 tap run loop 中，不能访问已析构的 runtime。

## Probe

默认运行只读取系统 preflight 状态，不请求权限：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
```

显式 `--request` 才调用 `CGRequestListenEventAccess`，应由开发者在受控 macOS 会话中运行：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --request
```

CoreGraphics binding 仅存在于 macOS target dependency；非 macOS 构建会输出 skipped，不引入跨平台 API。`--tap-ms <milliseconds>` 会在专用线程/run loop 上创建 listen-only `CGEventTap`，只统计事件类型计数和队列诊断，不记录具体键值；键盘事件在 queue 中保留 keycode/repeat 供后续 mapper 使用；`--cycles <count>` 可重复创建、运行、禁用并销毁 tap；`--key-state <macOS-keycode>` 将该 keycode 作为 runtime 当前 pressed-set 候选，经 `CGEventSourceKeyState` 生成仍按下快照，只输出 checked/still-pressed/released 数量。默认仍不会自动创建 tap。

## Verification

```text
cargo fmt --manifest-path spikes/input-macos/Cargo.toml -- --check
cargo test --manifest-path spikes/input-macos/Cargo.toml --locked
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --tap-ms 3000 --cycles 3
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --key-state 0
```

2026-08-28 在 macOS 26.5.2、Apple M1 Pro、`aarch64-apple-darwin` 上执行 `--tap-ms 150 --cycles 3`、`--tap-ms 3000 --cycles 1` 和 `--tap-ms 20 --cycles 100`：所有 104 次 tap 均成功创建、进入 run loop、保持 enabled、正常停止，`callback_panics=0`；最新 100-cycle 结果进一步确认 100 次均无 `error`、`finished_enabled=false` 或非零 panic 计数。3 秒 tap 期间向前台应用发送两次普通按键，报告 `key_down=2 key_up=2`，证明 listen-only callback 能同时收到按下和释放。最新短 tap 报告还包含 `queued_events=0 consumed_events=0 queue_overflows=0 queue_recovery_resets=0 queue_discarded_events=0 queue_closed_events=0`，确认无事件时队列可正常关闭和 drain。`--key-state 0` 实机得到 `checked=1 still_pressed=0 released=1`，验证候选 pressed set 通过 `CGEventSourceKeyState(CombinedSessionState, key_code)` 生成校正快照。纯函数测试覆盖多键保留/释放、队列 FIFO、溢出恢复和关闭竞态，并确认不会查询 pressed set 之外的 keycode。

实现约束：特殊的 `kCGEventTapDisabledByTimeout`/`kCGEventTapDisabledByUserInput` 值不能放入第三方事件 mask（其高位值会导致 `1 << type` 溢出）；callback 仍对这两类通知分支处理，收到后通过有界 channel 请求在 run loop 内 re-enable。tap 创建阶段使用 panic boundary，避免 binding 异常杀死输入线程。

目前已覆盖 denied/granted、tap timeout/disable、permission revocation 和 session reset 的状态测试，以及真实 tap 创建/运行/停止、100 次 tap wrapper restart smoke、候选 pressed-set 校正边界和 callback queue 的 FIFO/overflow/close contract。真实按键/鼠标 callback 在 queue 中的事件字段、系统主动 timeout/disable、TCC 授权/拒绝/撤销、周期性校正调度、runtime pressed state 接入和锁屏/睡眠恢复仍必须在受控 macOS 实机完成；100 次循环尚未包含专门的泄漏工具采样或系统故障注入。本机未安装 `x86_64-unknown-linux-gnu` 标准库，因此新增纯函数的 Linux 交叉测试只由 Ubuntu CI 覆盖。
