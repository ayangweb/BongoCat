# Windows Raw Input Scan Code Contract Spike

状态：平台无关 scan code/edge contract 已通过；Win32 `WM_INPUT`、设备热插拔和 `GetAsyncKeyState` 待 Windows 实机
日期：2026-08-28

## 范围

`spikes/input-windows/` 只冻结 Windows Raw Input keyboard packet 到稳定 `PhysicalKey` 和 pressed edge 的映射，不创建窗口、不注册 Raw Input，也不读取真实键盘。实现覆盖：

- `RI_KEY_BREAK` 到 down/up 的边沿转换；
- E0 扩展码和左右 Control/Alt/Meta、导航键、PrintScreen；
- E1 Pause 序列，避免误判为左 Control；
- 未知 scan code 保留为诊断值，不静默丢弃。

该 contract 可在 macOS/Linux 离线运行，但不能证明 Windows 消息接收、scan code 设备差异或 `GetAsyncKeyState` 校正行为。

## 验证

```text
cargo fmt --manifest-path spikes/input-windows/Cargo.toml -- --check
cargo clippy --manifest-path spikes/input-windows/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/input-windows/Cargo.toml --locked
cargo run --manifest-path spikes/input-windows/Cargo.toml --locked
```

下一步是在 Windows 实机把 `RAWINPUTHEADER`/`RAWKEYBOARD` 解包接到该 contract，验证 `WM_INPUT` 的设备句柄生命周期、E0/E1 实际序列、`RI_KEY_BREAK` 和 `GetAsyncKeyState` reconciliation；不得用本 spike 的通过结果替代平台验收。
