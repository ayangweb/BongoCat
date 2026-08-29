# Windows Raw Input Spike

状态：平台无关 mapping contract、Windows Raw Input 注册/退出路径、`GetAsyncKeyState` 查询 adapter 与周期性校正已实现；注册/退出、查询和设备通知注册 smoke 已通过，scheduler smoke、真实键盘和真实热插拔待验证
日期：2026-08-29

## 范围

`spikes/input-windows/` 冻结 Windows Raw Input keyboard packet 到稳定 `PhysicalKey` 和 pressed edge 的映射，并在仅 Windows 编译的窄 platform wrapper 中打通最小注册与消息路径。实现覆盖：

- `RI_KEY_BREAK` 到 down/up 的边沿转换；
- E0 扩展码和左右 Control/Alt/Meta、导航键、PrintScreen；
- E1 Pause 序列，避免误判为左 Control；
- 未知 scan code 保留为诊断值，不静默丢弃；
- `GetRawInputData` 返回的声明长度、输入类型、键盘 payload 偏移和截断数据在进入 mapper 前校验；
- 在专用 message-only HWND 上为鼠标和键盘注册 `RIDEV_INPUTSINK`；
- `WM_INPUT` 先查询长度，再使用对齐 buffer 读取，不向安全 mapper 泄漏 handle/pointer；
- callback panic boundary、`WM_TIMER` 自动退出、Raw Input 注销、window class 注销和清理结果诊断。
- 只对本地 pressed candidates 建立 physical-key 到 Win32 virtual-key 的查询计划，左右修饰键使用独立 virtual-key；
- `GetAsyncKeyState` 只读取当前按下高位，不把 toggle/近期按下低位当作 pressed state；
- 查询前验证当前 input desktop 可访问；失败时不生成可能错误的全释放 snapshot；
- 无法可靠映射的未知 scan code 保留在 snapshot，并显式返回 `reset_required`，禁止静默误释放或永久忽略。
- 注册 `RIDEV_DEVNOTIFY` 并处理 `WM_INPUT_DEVICE_CHANGE`；设备移除和服务停止会清空平台候选 pressed-set 并记录 Reset，不记录具体键值。
- message window 每 `250 ms` 查询一次候选 pressed-set，同一键连续 `2` 次缺失才形成 reconciled release；中间重新确认按下会取消待释放状态。
- input desktop 查询失败或候选键不可查询时立即 Reset，不把不可信的全零 snapshot 当作正常释放。

安全 contract 可在 macOS/Linux 离线运行；Win32 wrapper 使用精确锁定的 `windows = 0.61.3`，只在 Windows target 编译。wrapper 当前只输出消息、edge、decode error 和 callback panic 计数，不记录真实按键值。

## 验证

```text
cargo fmt --manifest-path spikes/input-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-windows/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --register-smoke-ms 100
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --key-state-smoke
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --reconcile-smoke-ms 600
```

当前本地验证包括 macOS host 上的 format、15 项 contract test、Clippy 和 release check，以及 `x86_64-pc-windows-msvc` 的 check/Clippy。测试覆盖左右修饰键 virtual-key、只查询 pressed candidates、未知键触发 Reset、设备移除/服务停止 Reset、重复 down/无匹配 up 诊断、连续两次缺失释放、仍按下取消待确认和零确认阈值拒绝。

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

下一步是获取真实 `RAWINPUTHEADER`/`RAWKEYBOARD` 样本，验证连续缺失能在实际 callback 后形成 reconciled release，以及设备句柄生命周期、E0/E1 实际序列、`RI_KEY_BREAK` 与热插拔。最后执行 PixPin、Win+L、PrintScreen、UAC 和管理员/非管理员矩阵；不得用无人值守 CI 的空闲查询替代这些平台验收。
