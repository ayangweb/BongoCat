# macOS Input Permission and Tap Lifecycle Spike

状态：权限/tap 生命周期 contract、listen-only CGEventTap、可靠 callback 队列、键盘/鼠标按钮周期校正、真实 keyboard callback release 丢弃后的恢复、受控 disable 恢复、NSWorkspace 生命周期 Reset 和 callback shutdown smoke 已通过；权限矩阵、真实系统生命周期、系统自然 timeout 和物理鼠标矩阵仍待验证
日期：2026-08-29

## Contract

`spikes/input-macos/` 将 macOS 输入服务拆成三个可观察状态：

- `PermissionState`：`Unknown`、`Denied`、`Granted`；普通启动只 preflight，不主动弹权限请求；
- `TapState`：`Stopped`、`Running`、`Disabled`、`TimedOut`；tap disabled/timeout 后必须 reset pressed state，并在权限仍 granted 时安排 restart；
- `SessionReset`：锁屏、睡眠、快速用户切换、TCC 变化和服务重启都清空 pressed state。

权限撤销会停止 tap、发出 reset 并进入用户引导；重新 granted 后才允许创建 tap。状态层不把 callback 生命周期或 CoreGraphics 指针暴露给 runtime。callback 只生成不含用户内容的 `CapturedInputEvent`，由 mutex 保护的生产者为其分配单调 `u64` sequence 后送入固定容量队列；消费发生在 tap run loop 中，不能访问已析构的 runtime。满载 Reset 继承被拒边沿的 sequence，使被丢弃 backlog 形成可计数 gap；正常 cycle 的 gap 和 duplicate/out-of-order 必须为 0，且 `queued_events = consumed_events + queue_discarded_events`。

## Probe

默认运行只读取系统 preflight 状态，不请求权限：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
```

显式 `--request` 才调用 `CGRequestListenEventAccess`，应由开发者在受控 macOS 会话中运行：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --request
```

CoreGraphics binding 仅存在于 macOS target dependency；非 macOS 构建会输出 skipped，不引入跨平台 API。`--tap-ms <milliseconds>` 会在专用线程/run loop 上创建 listen-only `CGEventTap`，只统计事件类型计数和队列诊断，不记录具体键值。键盘事件在 queue 中保留 keycode/repeat；鼠标 down/up 保留 CoreGraphics 的 0–31 号 button identity，不再把左、右、中和侧键压成同一个事件。run-loop consumer 分别维护键盘和鼠标候选 pressed set，`FlagsChanged` 用 `CGEventSourceKeyState` 判定方向；每 `250 ms` 只查询当前候选 key/button，经 `CGEventSourceKeyState`/`CGEventSourceButtonState` 连续 `2` 次缺失才形成 reconciled release。`Reset`、tap shutdown 和 queue overflow 同时清空两类候选。`--cycles <count>` 可重复创建、运行、禁用并销毁 tap，0 会被拒绝；`--summary-only` 省略逐 cycle 行但保留严格校验和聚合结果。任一 cycle 创建失败、未恢复 enabled、callback panic、队列 overflow/close 后事件、observer 数量不匹配或注入语义失败都会以非零状态退出，不再只打印错误。`--key-state <macOS-keycode>` 和 `--button-state <0..31>` 分别对单个候选执行系统状态查询，只输出 checked/still-pressed/released 数量。默认仍不会自动创建 tap。

`--inject-disable timeout|user` 只能和 `--tap-ms` 一起用于受控故障验证。每个 cycle 先注入一个没有 KeyUp 的候选键，再禁用真实 tap；`user` 使用 CoreGraphics 返回的真实 user-disable callback，`timeout` 将测试动作附带的 user-disable 通知替换为 timeout 原因。两者随后走与系统 callback 相同的 Reset、权限 preflight 和 re-enable 路径。恢复信号使用原子位合并，不会像有界 `try_send` 一样在满载时静默丢失；报告只输出 disable、Reset、release 和队列数量。

`--inject-release-loss` 只能和 `--tap-ms` 一起用于 release 校正闭环。probe 会分别 preflight listen-only Input Monitoring 和 synthetic Post Event/Accessibility 权限；后者未授予时以 `PostEventPermissionDenied` 明确失败，不触发权限请求。权限齐备后通过 private `CGEventSource` 向 session event tap 投递 keycode 0 的 down/up；listen-only callback 必须收到两个真实 CoreGraphics 事件，但 callback 到 consumer 的边界会故意丢弃一次 KeyUp。consumer 保留由 KeyDown 建立的候选，随后只通过两次 `CGEventSourceKeyState` 缺失确认清除它。命令会断言投递数、callback down/up、丢弃数、校正数和 shutdown 前候选数，不输出 keycode。它验证真实 event tap callback 到校正实现的闭环，但不能代替物理键盘、系统丢事件或 TCC 变化实测。

`--inject-lifecycle session|sleep|wake|all` 使用 `NSWorkspace.notificationCenter` 和公开的 `NSWorkspaceWillSleepNotification`、`NSWorkspaceDidWakeNotification`、`NSWorkspaceSessionDidResignActiveNotification`、`NSWorkspaceSessionDidBecomeActiveNotification` 做受控 callback smoke。observer callback 只写原子信号与匿名计数；输入 run-loop owner 消费合并信号并 Reset 候选 pressed-set。observer token 由 RAII owner 保存，退出时先关闭 callback gate 和输入队列，再停止 tap/source、消费末尾信号并成对注销 token。受控模式会在 gate 关闭后、注销前额外 post 一次通知，证明迟到 callback 只能增加 `workspace_callbacks_ignored_after_close`，不能再访问 queue、候选状态或 runtime owner。

这项注入只证明公开通知 API、callback、Reset 和注销链路，不等同于系统真实锁屏、睡眠、唤醒或快速用户切换。实现不订阅 undocumented 的 `com.apple.screenIsLocked` distributed notification。

## Verification

```text
cargo fmt --manifest-path spikes/input-macos/Cargo.toml -- --check
cargo test --manifest-path spikes/input-macos/Cargo.toml --locked
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --tap-ms 3000 --cycles 3
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --key-state 0
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --button-state 0
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --tap-ms 300 --inject-disable timeout
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --tap-ms 300 --inject-disable user
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --tap-ms 300 --inject-lifecycle all
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked --release -- --tap-ms 800 --inject-release-loss
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked --release -- --tap-ms 600 --cycles 20 --inject-release-loss
MallocStackLogging=1 /usr/bin/leaks --atExit -- spikes/input-macos/target/release/bongocat-input-macos-spike --tap-ms 20 --cycles 100 --summary-only
NSZombieEnabled=YES spikes/input-macos/target/release/bongocat-input-macos-spike --tap-ms 20 --cycles 100 --summary-only
```

2026-08-28 在 macOS 26.5.2、Apple M1 Pro、`aarch64-apple-darwin` 上执行 `--tap-ms 150 --cycles 3`、`--tap-ms 3000 --cycles 1` 和 `--tap-ms 20 --cycles 100`：所有 104 次 tap 均成功创建、进入 run loop、保持 enabled、正常停止，`callback_panics=0`；最新 100-cycle 结果进一步确认 100 次均无 `error`、`finished_enabled=false` 或非零 panic 计数。3 秒 tap 期间向前台应用发送两次普通按键，报告 `key_down=2 key_up=2`，证明 listen-only callback 能同时收到按下和释放。最新短 tap 报告还包含 `queued_events=0 consumed_events=0 queue_overflows=0 queue_recovery_resets=0 queue_discarded_events=0 queue_closed_events=0`，确认无事件时队列可正常关闭和 drain。`--key-state 0` 实机得到 `checked=1 still_pressed=0 released=1`，验证候选 pressed set 通过 `CGEventSourceKeyState(CombinedSessionState, key_code)` 生成校正快照。纯函数测试覆盖多键保留/释放、队列 FIFO、溢出恢复和关闭竞态，并确认不会查询 pressed set 之外的 keycode。

2026-08-29 在同一设备与已授予 Input Monitoring 的当前进程上执行 `--tap-ms 600 --cycles 1 --key-state 0`：preflight 为 granted，tap 报告 `started=true finished_enabled=true reconciliation_runs=2 reconciled_releases=0 candidate_resets=1`，所有 callback、queue、duplicate 和 unmatched 诊断为 0。13 项 contract test 覆盖 KeyDown/Up、`FlagsChanged` 方向查询、连续两次缺失释放、仍按下取消确认、Reset 清理和零阈值拒绝。

2026-08-29 使用 commit `c271ceb2449b48f37569c6746fbe7b7170dbe0d3` 在同一设备、系统和权限条件下执行 timeout/user 两种受控注入。单次 `--tap-ms 300` 均得到对应 disable 计数 1、`injected_disables=1 reenabled=1 finished_enabled=true candidate_reset_releases=1`；callback panic、queue overflow、discard 和 closed-event 计数均为 0。随后两种模式分别执行 `--tap-ms 30 --cycles 20`，40 个 cycle 全部重新启用，且每次至少由 Reset 释放 1 个候选键；测试期间若同时收到真实输入，release 数量允许大于 1。14 项 contract test 另验证两种 disable 信号可合并而不丢恢复工作。macOS CI job 已增加该 crate 的原生 target check、Clippy、test 和 release check，但 CI 不绕过 TCC 创建 event tap。

2026-08-29 在同一设备、系统和权限条件下执行 `--tap-ms 300 --inject-lifecycle all`。真实 listen-only tap 正常创建和停止，4 个公开 NSWorkspace observer 均完成注册/注销；四类受控通知计数各为 1，并合并形成 1 次 lifecycle Reset，释放 1 个注入的缺失 KeyUp 候选。shutdown 前受控触发的迟到 callback 被关闭 gate 忽略 1 次，`workspace_callback_panics=0 queue_closed_events=0`，最终报告 `workspace_observers_registered=4 workspace_observers_removed=4 candidate_resets=2 candidate_reset_releases=1`。17 项测试还以故意 panic 验证共享的 autorelease/panic boundary 会吞住 unwind 并增加匿名计数；event-tap 与 workspace callback 都使用该边界。

2026-08-29 在同一设备、系统和权限条件下执行 release-loss 闭环。单次 `--tap-ms 800 --inject-release-loss` 得到 `key_down=1 key_up=1 reconciliation_runs=3 reconciled_releases=1 synthetic_events_posted=2 intentionally_dropped_releases=1 pressed_candidates_before_shutdown=0 callback_panics=0`。随后 release 构建执行 `--tap-ms 600 --cycles 20 --inject-release-loss`，20/20 cycle 均捕获 down/up、故意丢弃一次 release、经两次状态缺失确认释放候选，且每次 `queue_overflows=0 callback_panics=0 pressed_candidates_before_shutdown=0`。该结果证明不是依靠 shutdown Reset 清除残留键；物理输入和系统自然丢失 release 仍需单独实测。

2026-08-29 对 release 二进制增加严格 cycle validator 后，再执行两个 100-cycle 资源验证。`leaks --atExit` 报告 `completed_cycles=100 candidate_resets=100 queue_overflows=0 callback_panics=0 clean_shutdown=true`、physical footprint `5232K`、`0 leaks for 0 total leaked bytes`；当前系统同时提示受限进程的只读内存检查限制，因此该结果只作为可见 malloc leak 证据。独立的 `NSZombieEnabled=YES` 运行也完成 100/100，无 over-release crash。每个 `run_listen_only_tap` 都创建并 join 专用线程，且每 cycle 的 4 个 NSWorkspace observer 必须注册/注销数量相等才会通过 validator。

2026-08-29 将 mouse button identity 接入 callback queue、pressed candidates、reconciliation 和 lifecycle Reset；0–31 号按钮可独立统计 duplicate、unmatched、reconciled 和 reset release。19 项 library contract test 新增侧键身份保留、只查询候选按钮、两次缺失释放和 keyboard/mouse 同步 Reset。`--button-state 0` 在同一设备调用 `CGEventSourceButtonState` 得到 `checked=1 still_pressed=0 released=1`，证明窄平台边界可用。Computer Use 的 AX window/coordinate click 没有进入 session event tap，因此没有作为物理鼠标证据；真实设备 down/up 和丢 release 仍待手工矩阵。

2026-08-29 为 callback queue 的每个 edge/Reset 增加单调 sequence，并把 gap、duplicate/out-of-order 与完整 drain accounting 加入严格 cycle validator。20 项 library test 和 4 项报告 test 通过；macOS 本机普通 3-cycle、timeout disable、user disable 与四类 lifecycle 通知均为 `sequence_gaps=0 sequence_duplicates_or_out_of_order=0`，受控队列分别满足 `0=0`、`2=2`、`2=2` 与 `1=1` 的 queued/consumed 关系。当前 Codex 会话重跑 release-loss 时 listen/post 两项 preflight 均为 true，但 session tap 收到 `0/2` 个已投递 synthetic event，严格门禁按预期非零失败；本批没有把该失败改写为成功，也没有覆盖此前已记录的成功结果。需要在交互式会话确认前台 session/TCC 状态后重跑该命令，才能为 sequence 变更补充 release-loss 回归证据。

实现 commit `d7501dc` 的 push run `33257871184` 中，contract job `99114627795` 已通过；原生 macOS job `99114627654` 也已通过 input spike 的 check、format、Clippy、20 项 library test、4 项报告 test 和 release build。CI 没有绕过 TCC 创建 tap，因此这份证据只覆盖编译与纯 contract，真实 tap 结果仍以上述本机命令为准。

实现约束：特殊的 `kCGEventTapDisabledByTimeout`/`kCGEventTapDisabledByUserInput` 值不能放入第三方事件 mask（其高位值会导致 `1 << type` 溢出）；callback 仍对这两类通知分支处理，收到后通过有界 channel 请求在 run loop 内 re-enable。tap 创建阶段使用 panic boundary，避免 binding 异常杀死输入线程。

目前已覆盖 denied/granted、tap timeout/disable、permission revocation 和 session reset 的状态测试，以及真实 tap 创建/运行/停止、严格 100 次 tap wrapper restart 与 malloc/NSZombie 检查、受控 timeout/user-disable 恢复、公开 NSWorkspace observer 的生命周期 Reset、真实 callback release 在 consumer 边界丢弃后的候选校正、callback panic boundary 和 queue 的 FIFO/overflow/close contract。系统自然触发的 timeout、TCC 拒绝/撤销、带真实 modifier 的 `FlagsChanged` 字段、物理输入或系统自然丢失 release 后的校正、runtime pressed state 接入和真实锁屏/睡眠/快速用户切换恢复仍必须在受控 macOS 实机完成；100 次循环尚未覆盖 timeout/权限故障或 Instruments 级 allocation/port 采样。纯函数和 target gating 已通过本机 `x86_64-unknown-linux-gnu` `cargo check --all-targets`，Ubuntu CI 继续提供原生 Linux contract test。
