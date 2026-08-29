# Windows Raw Input Spike

状态：平台无关 mapping contract、Windows Raw Input 注册/退出路径、可靠 callback queue、`GetAsyncKeyState` 周期校正、XInput producer 与生命周期 Reset 已实现；系统合成输入已覆盖丢 release 校正闭环和多键有序边沿压力 smoke，真实键盘/手柄、锁屏/睡眠和热插拔待验证
日期：2026-08-29

## 范围

`spikes/input-windows/` 冻结 Windows Raw Input keyboard packet 到稳定 `PhysicalKey` 和 pressed edge 的映射，并在仅 Windows 编译的窄 platform wrapper 中打通最小注册与消息路径。实现覆盖：

- `RI_KEY_BREAK` 到 down/up 的边沿转换；
- E0 扩展码和左右 Control/Alt/Meta、导航键、PrintScreen；
- E1 Pause 序列，避免误判为左 Control；
- 未知 scan code 保留为诊断值，不静默丢弃；
- `GetRawInputData` 返回的声明长度、输入类型、键盘 payload 偏移和截断数据在进入 mapper 前校验；
- `RAWMOUSE` 只在 safe decoder 中读取稳定的 `usButtonFlags` 前缀，将 left/right/middle/back/forward 的 down/up 映射为项目类型；纯移动和 wheel flag 不伪造 button edge，同包多边沿按固定顺序保留；
- 独立 safe decoder 读取 `usFlags`、`lLastX` 和 `lLastY`，保留 relative/absolute 与 virtual-desktop 语义；截断 payload 在进入 callback 状态前拒绝；
- 在永不显示的专用顶层 HWND 上为鼠标和键盘注册 `RIDEV_INPUTSINK`；顶层窗口用于接收 message-only HWND 收不到的电源广播；
- `WM_INPUT` 先查询长度，再使用对齐 buffer 读取，不向安全 mapper 泄漏 handle/pointer；
- callback 只把 keyboard/button edge、lifecycle Reset 和低频 owner tick 写入容量 `64` 的强类型 FIFO；pressed candidate 与 cursor query 只在 `DispatchMessageW` 返回后的 owner drain 中更新；
- 每个可靠事件携带单调 sequence；正常路径诊断 gap、duplicate/out-of-order，队列满载时原子丢弃不可信 backlog 并以同一 sequence 插入 `QueueOverflow` Reset；overflow、recovery Reset 和 discarded 数量全部可观测；
- 鼠标移动不进入可靠边沿队列：callback 只覆盖独立 `LatestValue<RawPointerMovement>` 槽位并记录 coalesced 数量；可靠 FIFO 只接收每 `16 ms` 一次的 owner tick，tick 在 callback 返回后调用 `GetCursorPos` 获取当前屏幕位置，坐标不写入日志；
- `WM_DESTROY` 先写入 `ServiceStopped` Reset 再关闭 producer，message owner 最终 drain 后才报告 clean shutdown；关闭后的迟到 push 会被拒绝并计数；
- callback panic boundary、`WM_TIMER` 自动退出、Raw Input 注销、window class 注销和清理结果诊断。
- 只对本地 pressed candidates 建立 physical-key 到 Win32 virtual-key 的查询计划，左右修饰键使用独立 virtual-key；
- `GetAsyncKeyState` 只读取当前按下高位，不把 toggle/近期按下低位当作 pressed state；
- 查询前验证当前 input desktop 可访问；失败时不生成可能错误的全释放 snapshot；
- 无法可靠映射的未知 scan code 保留在 snapshot，并显式返回 `reset_required`，禁止静默误释放或永久忽略。
- 注册 `RIDEV_DEVNOTIFY` 并处理 `WM_INPUT_DEVICE_CHANGE`；设备移除和服务停止会清空平台候选 pressed-set 并记录 Reset，不记录具体键值。
- message window 每 `250 ms` 查询一次候选 pressed-set，同一键连续 `2` 次缺失才形成 reconciled release；中间重新确认按下会取消待释放状态。
- input desktop 查询失败或候选键不可查询时立即 Reset，不把不可信的全零 snapshot 当作正常释放。
- 使用 `WTSRegisterSessionNotification`/`WTSUnRegisterSessionNotification` 成对管理当前会话通知；锁定、解锁、console/remote connect/disconnect 都立即 Reset。
- 处理 `WM_POWERBROADCAST` 的 suspend/standby 和各类 resume 通知；进入和离开不可观测窗口都立即 Reset。
- XInput owner 显式管理 `XInputEnable(true/false)` 服务期，并以固定 8ms 周期轮询 0–3 号 slot；连接/断开和按钮边沿进入可靠 FIFO，六个标准 axis 进入 `{device_id, connection_generation, axis}` 固定容量 latest-values。slot 断开会丢弃对应 generation 的待消费 axis，重连分配新 generation；轴值只做全范围归一化，不在 adapter 静默加入 dead-zone。
- XInput A/B/X/Y 映射为 south/east/west/north，Back/Start、shoulder、thumb、D-pad 和两个 trigger 使用项目稳定类型；trigger 同时提供 `[0, 1]` axis，并按共享 `0.5` 阈值生成按钮边沿。

安全 contract 可在 macOS/Linux 离线运行；Win32 wrapper 使用精确锁定的 `windows = 0.62.2`，只在 Windows target 编译。wrapper 当前只输出消息、edge、decode error 和 callback panic 计数，不记录真实按键值。

## 验证

```text
cargo fmt --manifest-path spikes/input-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-windows/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --register-smoke-ms 100
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --key-state-smoke
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --mouse-button-state-smoke
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --reconcile-smoke-ms 600
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --synthetic-release-recovery-ms 800
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --queue-overflow-smoke-ms 100
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked --release -- --synthetic-edge-pressure-cycles 128
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked --release -- --synthetic-pointer-flood-cycles 128
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --lifecycle-smoke-ms 100
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --xinput-ms 250
```

当前本地验证包括 macOS host 上的 format、27 项 contract test 和 Clippy，以及 `x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc` 的 check/Clippy。测试覆盖左右修饰键 virtual-key、只查询 pressed candidates、未知键触发 Reset、设备移除/服务停止/session/power Reset、Reset 释放数量、重复 down/无匹配 up 诊断、连续两次缺失释放、仍按下取消待确认、零确认阈值拒绝、RAWMOUSE 五个规范 button、相对/绝对/virtual-desktop movement 与截断拒绝，以及可靠队列的 FIFO overflow Reset、关闭 drain 和 sequence gap/duplicate 诊断。

Windows runner 验收证据：

- 实现 commit：`0672a00b3d278505a1345f8e749b814bd9391b3c`；
- push run：`33232314822`，job `99047139749`；
- pull request run：`33232316402`，job `99047143693`；
- 两个 `windows-latest` job 均通过 Windows build、Clippy、test，以及 `cargo run -- --register-smoke-ms 100`；
- smoke 结果为 `registered=true`、`clean_shutdown=true`、`decode_errors=0`、`callback_panics=0`。

该证据只确认 Windows runner 上可以注册、运行和有序清理 Raw Input message window。无人值守 runner 没有提供受控真实键盘输入，因此不能证明 `WM_INPUT` callback 能覆盖实际 down/up、设备变化和 issue #47 场景。

`GetAsyncKeyState` 查询 adapter 的 Windows runner 证据：

- 实现 commit：`a338686d0af9461f5d1997dac2b67c64591b333f`；
- push run：`33232636952`，job `99048003185`；
- pull request run：`33232638253`，job `99048006545`；
- 两个 `windows-latest` job 均通过 build、Clippy、该版本的 9 项 contract test、注册/退出 smoke 和 `GetAsyncKeyState` 查询 smoke。

该 smoke 证明 runner 可打开 input desktop 并执行候选键查询，但没有注入按键或丢失 release。该 commit 尚未包含后续的 `RIDEV_DEVNOTIFY` 与生命周期 Reset；对应证据记录在下一段。

设备通知注册与服务停止 Reset 的 Windows runner 证据：

- 实现 commit：`1c1947f3e80a7d5adb8caca48d5b3ee17ee27b07`；
- push run：`33232844193`，job `99048557823`；
- pull request run：`33232845935`，job `99048562213`；
- 两个 `windows-latest` job 均通过 build、Clippy、11 项 contract test、带 `RIDEV_DEVNOTIFY` 的注册/退出 smoke、服务停止 Reset 断言和 key-state 查询 smoke。

该证据没有产生真实 `WM_INPUT_DEVICE_CHANGE` removal 消息，只证明设备通知注册与 shutdown Reset 路径可在 runner 执行。真实键鼠拔插、设备 handle 生命周期和移除期间的 pressed edge 仍需交互式 Windows 验收。

周期校正 scheduler 的 Windows runner 证据：

- 实现 commit：`09773f0066f526799eb702fb1759049d0de9732f`；
- push run：`33233023879`，job `99049028541`；
- pull request run：`33233025942`，job `99049034643`；
- 两个 `windows-latest` job 均通过 15 项 contract test 和 600 ms scheduler smoke，至少执行两次 `GetAsyncKeyState` reconciliation，`reconciliation_query_errors=0`。

该 smoke 的候选 pressed-set 为空，只证明 timer、input desktop 查询、连续确认实现和 shutdown 可以共存。真实 Raw Input down 后丢失 up 的恢复延迟与正确性仍须受控输入验证。

新增的 `--synthetic-release-recovery-ms 800` 使用 Windows `SendInput` 发送 scan code
`0x1e` 的 down/up 对。隐藏窗口必须从系统输入流收到至少两个真实 `WM_INPUT` keyboard
edge；consumer 随后只故意忽略一次已捕获的 release，保留 pressed candidate。两次
`GetAsyncKeyState` 快照确认系统已释放后，candidate 必须以 `reconciled_releases=1` 清除，
最终 candidate 数必须为 0。该命令已接入 `windows-latest`，结果待包含本批实现的 push
run 验证。它覆盖真实 Win32 callback、解码、pressed set 和校正调度的进程内闭环，但
仍属于系统合成输入，不能替代物理键盘、PixPin、安全桌面或不同完整性级别实测。

`--synthetic-edge-pressure-cycles 128` 为 A、S、Space、左 Shift、左 Control 和 E0
右 Control 依次发送 128 轮 down/up，共 768 对、1536 个 scan-code 边沿。消费端保存预期
物理边沿队列，逐条验证 `WM_INPUT` 解码后的顺序，并分别断言 injected/seen/down/up、
duplicate down、unmatched up、decode error、callback panic 和最终 candidate 数量。命令最长
运行 3 秒，cycles 上限为 256，防止 CI 参数失控；输出只包含聚合计数，不包含实际按键值。
该 smoke 已接入 `windows-latest`。commit `f68b46f` 的 push run `33255823160`、job
`99109219820` 与 PR run `33255825363`、job `99109225114` 均已通过，证明 1536 个预期
键盘边沿完整、有序到达，duplicate/unmatched/decode/panic/残留均为 0；完整聚合输出待
workflow 结束后归档。

这项压力 smoke 证明 `SendInput -> WM_INPUT -> raw decode -> candidate state` 在无人值守
runner 上对一组已知 scan code 保持有序，不等于 10 分钟高速物理键鼠测试，也不能覆盖
PixPin、Win+L、PrintScreen、UAC、输入桌面切换或不同完整性级别的事件投递。

`--synthetic-pointer-flood-cycles 128` 在同一组 1536 个键盘边沿之间插入 3072 个带
`MOUSEEVENTF_MOVE_NOCOALESCE` 的相对鼠标移动，正负移动成对以恢复初始位置。注入以 256
项为一批，避免一次调用无界扩张；Raw Input 端分别统计 mouse message 和 keyboard edge，
只要求收到足以证明洪峰存在的 mouse message，不要求高频位置样本逐个可靠送达，但仍要求
全部键盘 down/up 保持原顺序且没有 duplicate、unmatched 或残留 candidate。命令最长运行
5 秒、cycles 上限 256。commit `64dd9d3` 的 push run `33256121099`、job `99110014206`
和 PR run `33256122578`、job `99110018813` 均已通过；断言保证实际收到至少 128 个 mouse
message，1536 个 keyboard edge 仍完整、有序，且 duplicate/unmatched/decode/panic/残留为 0。

会话与电源 Reset 的 Windows runner 证据：

- 实现 commit：`32bc9a37efd201a788511ee86e7350c6a5058ab3`；
- push run：`33234259414`，job `99052333561`；
- `windows-latest` 通过 build、Clippy、16 项 contract test 和 `--lifecycle-smoke-ms 100`；
- 报告为 `session_notifications_registered=true session_notifications_unregistered=true clean_shutdown=true resets=5 reset_releases=4 session_change_resets=2 power_change_resets=2 service_stopped_resets=1 callback_panics=0`。

该 smoke 在每条受控 `WM_WTSSESSION_CHANGE`/`WM_POWERBROADCAST` 前注入一个无 KeyUp 候选，并验证消息 dispatch 实际清空它；它不等于操作系统真实锁屏或睡眠。Win+L、快速用户切换、睡眠/唤醒和 UAC 返回仍需交互式 Windows 验收。

下一步先确认 Windows runner 的系统合成 release-recovery 与 edge-pressure 闭环，再获取物理设备的
`RAWINPUTHEADER`/`RAWKEYBOARD` 样本，验证设备句柄生命周期、E0/E1 实际序列、
`RI_KEY_BREAK` 与热插拔。最后执行 PixPin、Win+L、睡眠/唤醒、PrintScreen、UAC 和
管理员/非管理员矩阵；不得用无人值守 CI 的合成输入或受控生命周期消息替代这些平台验收。

Windows mouse button 已完成 raw byte decode、canonical mapping、callback candidate、五个稳定
virtual-key 的 `GetAsyncKeyState` 查询、连续两次缺失释放，以及 device/session/power/shutdown
Reset；诊断按 keyboard/mouse 分别输出 captured、duplicate、unmatched、reconciled 和残留数。
无注入的 `--mouse-button-state-smoke` 已接入 Windows runner。合成/物理 button release 与
wheel/cursor latest-value 分流仍待验证，不能由 pointer-move flood 结果替代。commit
`e776867` 的 push run `33256593886`、job `99111304790` 已通过 22 项 Windows contract test、
五个 button VK 查询、keyboard release recovery、两项压力 smoke、lifecycle Reset 和完整
config-store tests。

`--queue-overflow-smoke-ms 100` 先通过正常 producer/consumer 路径建立一个 A pressed
candidate，再在单次受控 window callback 内写满 `64` 项 FIFO 并追加第 65 项。满载策略必须
清除 64 项不可信 backlog、把被拒绝边沿替换为带同一 sequence 的 `QueueOverflow` Reset，
owner drain 后释放 A。命令断言 `overflows=1 recovery_resets=1 discarded=64 gaps=64`、无
duplicate/out-of-order、无残留 candidate，随后 `WM_DESTROY` 的 final Reset 必须成功入队，
producer 关闭且队列完全 drain。commit `98b27f2` 的 push run `33257310771`、job
`99113185410` 已通过 25 项 contract test 和该命令；同一 job 也重跑并通过 release recovery、
1536 个 keyboard edge、pointer flood、lifecycle Reset 与 config-store 回归。该受控 overflow
证明恢复策略在真实 Win32 callback/dispatch 路径执行，但不代表正常物理压力下允许 overflow；
产品门槛仍要求正常压力计数始终为 0。

pointer flood 现在进一步断言 decoded movement、latest-value consumption 和 cursor query：每个
非零 RAWMOUSE movement 都计入 captured sample；slot 已有未消费值时只增加 coalesced 计数，
不占用可靠 FIFO。16ms tick 消费 sample 后查询一次 `GetCursorPos`，shutdown 前再通过 final tick
清空，因此必须满足 `movement_samples = coalesced + consumed`、`cursor_queries = consumed`、
query error 为 0，同时原有 1536 keyboard edge、queue gap/overflow 和 pressed candidate 门禁
保持不变。commit `098d532` 的 push run `33258305541`、job `99115756881` 已通过 27 项
contract test 和强化后的 pointer flood；同一 job 也重跑通过 release recovery、queue overflow、
edge pressure、lifecycle 与 config 回归。该结果证明合成 RAWMOUSE 洪峰在 Windows runner 上
确实发生合并且未阻塞 release，不替代 10 分钟物理键鼠压力测试。

XInput producer 的 33 项 library contract test 覆盖 i16/u8 全范围归一化、连接/断开、标准按钮、六轴 latest-value、10,000 次 axis flood 不阻塞 release、可靠队列 overflow Reset + 原边沿重放、slot generation 复用、断开丢弃和 shutdown。`windows = 0.62.2` 只新增 `Win32_UI_Input_XboxController` feature，没有新增 package；唯一 `unsafe` 调用位于 binary platform wrapper，安全库继续 `forbid(unsafe_code)`。x64/ARM64 MSVC check 已通过。

实现 commit `b6bbd73` 的 push run `33260707799`、job `99122041439` 与 PR run
`33260709475`、job `99122046077` 均通过 33 项 Windows test 和
`--xinput-ms 250` API smoke。push job 的报告为
`service_enabled=true service_disabled=true api_calls=124 query_errors=0`
`reliable_overflows=0 axis_overflows=0 clean_shutdown=true`。runner 未连接手柄，
`peak_connected=0`，所以该结果只证明真实 `XInputEnable`/`XInputGetState` 调用、四 slot
轮询和 owner shutdown，不证明物理 controller/profile、按钮、axis 或热插拔。
