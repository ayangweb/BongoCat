# Windows Raw Input Spike

状态：平台无关 mapping contract、Windows Raw Input 注册/退出路径与 `GetAsyncKeyState` 查询 adapter 已实现；注册/退出 smoke 已通过，查询 smoke、真实键盘、热插拔和周期性状态校正待验证
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

安全 contract 可在 macOS/Linux 离线运行；Win32 wrapper 使用精确锁定的 `windows = 0.61.3`，只在 Windows target 编译。wrapper 当前只输出消息、edge、decode error 和 callback panic 计数，不记录真实按键值。

## 验证

```text
cargo fmt --manifest-path spikes/input-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-windows/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --register-smoke-ms 100
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --key-state-smoke
```

当前本地验证包括 macOS host 上的 format、9 项 contract test、Clippy 和 release check，以及 `x86_64-pc-windows-msvc` 的 check/Clippy。测试覆盖左右修饰键 virtual-key、只查询 pressed candidates、释放 snapshot，以及未知键触发 Reset contract。

Windows runner 验收证据：

- 实现 commit：`0672a00b3d278505a1345f8e749b814bd9391b3c`；
- push run：`33232314822`，job `99047139749`；
- pull request run：`33232316402`，job `99047143693`；
- 两个 `windows-latest` job 均通过 Windows build、Clippy、test，以及 `cargo run -- --register-smoke-ms 100`；
- smoke 结果为 `registered=true`、`clean_shutdown=true`、`decode_errors=0`、`callback_panics=0`。

该证据只确认 Windows runner 上可以注册、运行和有序清理 Raw Input message window。新增 `GetAsyncKeyState` 查询 smoke 必须由下一次 Windows CI 验证；即使通过，无人值守 runner 也没有提供受控真实键盘输入，因此仍不能证明 `WM_INPUT` callback 能覆盖实际 down/up、设备变化和 issue #47 场景。

下一步是在 Windows runner 验证 input desktop guard 与 `GetAsyncKeyState` API smoke；随后获取真实 `RAWINPUTHEADER`/`RAWKEYBOARD` 样本，将查询 adapter 接入 `250 ms` scheduler 和连续两次缺失确认策略，并验证设备句柄生命周期、E0/E1 实际序列、`RI_KEY_BREAK` 与热插拔。最后执行 PixPin、Win+L、PrintScreen、UAC 和管理员/非管理员矩阵；不得用无人值守 CI 的空闲查询替代这些平台验收。
