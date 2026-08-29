# macOS Input Permission and Tap Lifecycle Spike

状态：权限/tap 生命周期 contract、listen-only CGEventTap、可靠 callback 队列、cursor/gamepad axis latest-value 通道、键盘/鼠标按钮周期校正、GameController owner、真实 keyboard callback release 丢弃后的恢复、受控 disable 恢复、NSWorkspace 生命周期 Reset 和 callback shutdown smoke 已通过；权限矩阵、真实系统生命周期、系统自然 timeout 和物理鼠标/手柄矩阵仍待验证
日期：2026-08-29

## Contract

`spikes/input-macos/` 将 macOS 输入服务拆成三个可观察状态：

- `PermissionState`：`Unknown`、`Denied`、`Granted`；普通启动只 preflight，不主动弹权限请求；
- `TapState`：`Stopped`、`Running`、`Disabled`、`TimedOut`；tap disabled/timeout 后必须 reset pressed state，并在权限仍 granted 时安排 restart；
- `SessionReset`：锁屏、睡眠、快速用户切换、TCC 变化和服务重启都清空 pressed state。

权限撤销会停止 tap、发出 reset 并进入用户引导；重新 granted 后才允许创建 tap。状态层不把 callback 生命周期或 CoreGraphics 指针暴露给 runtime。callback 只生成不含用户内容的 `CapturedInputEvent`，由 mutex 保护的生产者为其分配单调 `u64` sequence 后送入固定容量队列；消费发生在 tap run loop 中，不能访问已析构的 runtime。满载 Reset 继承被拒边沿的 sequence，使被丢弃 backlog 形成可计数 gap；正常 cycle 的 gap 和 duplicate/out-of-order 必须为 0，且 `queued_events = consumed_events + queue_discarded_events`。

`MouseMoved` 和 left/right/other drag 不进入上述可靠队列。callback 把 `CGEventGetLocation` 的全局坐标转换成项目自有 `MacCursorSample`，覆盖独立 latest-value slot；run-loop owner 约每 `16 ms` 最多消费一个样本，shutdown 封住 producer 后 flush 最后一个 pending sample。匿名诊断强制满足 `cursor_captured = cursor_coalesced + cursor_consumed`，并拒绝 close 后发布。cursor flood 因而不会占用 key/button/Reset 的容量或 sequence。

GameController 使用独立 owner 枚举 `GCExtendedGamepad`，不把 Objective-C 对象或 element 类型送入公共协议。每次连接分配项目内 slot 和单调 generation；按钮边沿、连接和断开进入可靠 FIFO，六个标准 axis 进入以 `{device_id, generation, axis}` 为 key 的固定容量 latest-values。断开先移除 value-change handler，再丢弃该 generation 的待消费 axis；复用 slot 必须获得新 generation，迟到 callback 只计数并拒绝。producer 按 `0.5` 生成按钮边沿，axis 钳制到 `[-1, 1]`，trigger 钳制到 `[0, 1]`，非有限值归零并进入匿名诊断。服务期显式启用 `shouldMonitorBackgroundEvents`，shutdown 在 handler 全部移除后恢复原进程值。

`objc2-game-controller = 0.3.2` 是 2026-08-29 核对到的 crates.io 最新稳定版，许可证为 `Zlib OR Apache-2.0 OR MIT`，Rust 1.71+。仅启用 Controller/ExtendedGamepad 所需 feature，生成 binding 和 `unsafe` getter 集中在本文件对应的平台模块；纯 Rust producer 只依赖项目自有类型和 `input-queue` contract。若 objc2 binding 停止维护，可替换窄平台 getter/handler owner，不改变可靠事件或 axis key 协议。

## Probe

默认运行只读取系统 preflight 状态，不请求权限：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
```

显式 `--request` 才调用 `CGRequestListenEventAccess`，应由开发者在受控 macOS 会话中运行：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --request
```

CoreGraphics binding 仅存在于 macOS target dependency；非 macOS 构建会输出 skipped，不引入跨平台 API。`--tap-ms <milliseconds>` 会在专用线程/run loop 上创建 listen-only `CGEventTap`，只统计事件类型、可靠队列和 cursor 合并诊断，不记录具体键值或坐标。键盘事件在 queue 中保留 keycode/repeat；鼠标 down/up 保留 CoreGraphics 的 0–31 号 button identity，不再把左、右、中和侧键压成同一个事件。run-loop consumer 分别维护键盘和鼠标候选 pressed set，`FlagsChanged` 用 `CGEventSourceKeyState` 判定方向；每 `250 ms` 只查询当前候选 key/button，经 `CGEventSourceKeyState`/`CGEventSourceButtonState` 连续 `2` 次缺失才形成 reconciled release。`Reset`、tap shutdown 和 queue overflow 同时清空两类候选。`--cycles <count>` 可重复创建、运行、禁用并销毁 tap，0 会被拒绝；`--summary-only` 省略逐 cycle 行但保留严格校验和聚合结果。任一 cycle 创建失败、未恢复 enabled、callback panic、队列 overflow/close 后事件、cursor accounting 不完整、observer 数量不匹配或注入语义失败都会以非零状态退出，不再只打印错误。`--key-state <macOS-keycode>` 和 `--button-state <0..31>` 分别对单个候选执行系统状态查询，只输出 checked/still-pressed/released 数量。默认仍不会自动创建 tap。

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
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked --release -- --gamepad-ms 1000
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

2026-08-29 在 commit `500a956` 上将 `MouseMoved` 与三类 drag 接入独立 cursor latest-value slot。23 项 library test 中的 10,000-sample flood 将 9,999 个中间位置合并、只消费最终样本，同时两项容量的可靠队列完整保留 MouseDown/MouseUp 且 overflow 为 0；关闭测试证明 pending sample 会 flush，迟到 publish 会被拒绝并计数。当前 Apple M1 Pro、macOS 26.5.2 上执行 `--tap-ms 600 --cycles 3`，三轮真实 tap 均 `started=true finished_enabled=true`，cursor accounting 为 `0 = 0 + 0`、close 后拒绝为 0；测试期间没有物理移动鼠标，因此该结果不冒充物理 cursor callback 证据。PR run `33258718745` 的原生 macOS job `99116842307` 已通过 input check、format、Clippy、23 项 library test、4 项报告 test 和 release build，独立 contract job `99116842405` 也通过。

2026-08-29 当前 Apple M1 Pro、macOS 26.5.2 上新增 GameController producer 后，30 项 library test 与 5 项报告 test 通过。contract 覆盖连接/断开、按钮阈值、六轴合并、10,000 次 axis flood 不阻塞 release、可靠队列 overflow Reset + 原边沿重放、slot generation 复用、断开丢弃、非有限值、容量边界和 shutdown 迟到 callback。release `--gamepad-ms 1000` 完成 37 次真实 framework 枚举，`background_monitoring_enabled=true background_monitoring_restored=true callback_panics=0 clean_shutdown=true`；本机没有连接手柄，报告为 `observed_controllers=0`，因此它只证明 framework API、进程全局策略和 owner shutdown，不证明物理 controller profile、按钮、axis 或热插拔。

同一工作批次重跑 `--tap-ms 800 --inject-release-loss` 时，两项 TCC preflight 仍为 true 且投递计数为 2，但 session callback 收到 `0/2`；严格 validator 继续非零退出，并把错误精确区分为“未到达 event-tap callback”，未将其误报成校正失败或成功。sequence 变更后的 release-loss 实机回归仍需在可接收 synthetic callback 的交互式会话重跑。

实现约束：特殊的 `kCGEventTapDisabledByTimeout`/`kCGEventTapDisabledByUserInput` 值不能放入第三方事件 mask（其高位值会导致 `1 << type` 溢出）；callback 仍对这两类通知分支处理，收到后通过有界 channel 请求在 run loop 内 re-enable。tap 创建阶段使用 panic boundary，避免 binding 异常杀死输入线程。

目前已覆盖 denied/granted、tap timeout/disable、permission revocation 和 session reset 的状态测试，以及真实 tap 创建/运行/停止、严格 100 次 tap wrapper restart 与 malloc/NSZombie 检查、受控 timeout/user-disable 恢复、公开 NSWorkspace observer 的生命周期 Reset、真实 callback release 在 consumer 边界丢弃后的候选校正、callback panic boundary 和 queue 的 FIFO/overflow/close contract。系统自然触发的 timeout、TCC 拒绝/撤销、带真实 modifier 的 `FlagsChanged` 字段、物理输入或系统自然丢失 release 后的校正、runtime pressed state 接入和真实锁屏/睡眠/快速用户切换恢复仍必须在受控 macOS 实机完成；100 次循环尚未覆盖 timeout/权限故障或 Instruments 级 allocation/port 采样。纯函数和 target gating 已通过本机 `x86_64-unknown-linux-gnu` `cargo check --all-targets`，Ubuntu CI 继续提供原生 Linux contract test。
