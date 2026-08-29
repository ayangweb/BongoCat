# Windows Raw Input Spike

状态：平台无关 mapping contract 与 Windows Raw Input 注册/退出路径已实现；Windows runner smoke、真实键盘、热插拔和状态校正待验证
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

安全 contract 可在 macOS/Linux 离线运行；Win32 wrapper 使用精确锁定的 `windows = 0.61.3`，只在 Windows target 编译。wrapper 当前只输出消息、edge、decode error 和 callback panic 计数，不记录真实按键值。

## 验证

```text
cargo fmt --manifest-path spikes/input-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-windows/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked -- --register-smoke-ms 100
```

当前本地验证包括 macOS host 上的 format、6 项 contract test、Clippy 和 release check，以及 `x86_64-pc-windows-msvc` 的 check/Clippy。Windows CI smoke 在本次提交后才能形成验收证据，因此当前不宣称 Raw Input 平台路径已通过。

下一步是在 Windows 获取真实 `RAWINPUTHEADER`/`RAWKEYBOARD` 样本，验证设备句柄生命周期、E0/E1 实际序列、`RI_KEY_BREAK`、热插拔和 `GetAsyncKeyState` reconciliation，再执行 PixPin、Win+L、PrintScreen、UAC 和管理员/非管理员矩阵；不得用无人值守 CI 的空闲注册成功替代这些平台验收。
