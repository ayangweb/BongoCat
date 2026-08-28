# macOS Input Permission and Tap Lifecycle Spike

状态：权限/tap 生命周期 contract 和 macOS preflight probe 已建立；真实 CGEventTap callback、权限恢复和 100 次 restart 待验证
日期：2026-08-28

## Contract

`spikes/input-macos/` 将 macOS 输入服务拆成三个可观察状态：

- `PermissionState`：`Unknown`、`Denied`、`Granted`；普通启动只 preflight，不主动弹权限请求；
- `TapState`：`Stopped`、`Running`、`Disabled`、`TimedOut`；tap disabled/timeout 后必须 reset pressed state，并在权限仍 granted 时安排 restart；
- `SessionReset`：锁屏、睡眠、快速用户切换、TCC 变化和服务重启都清空 pressed state。

权限撤销会停止 tap、发出 reset 并进入用户引导；重新 granted 后才允许创建 tap。状态层不把 callback 生命周期或 CoreGraphics 指针暴露给 runtime。

## Probe

默认运行只读取系统 preflight 状态，不请求权限：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
```

显式 `--request` 才调用 `CGRequestListenEventAccess`，应由开发者在受控 macOS 会话中运行：

```text
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked -- --request
```

CoreGraphics binding 仅存在于 macOS target dependency；非 macOS 构建会输出 skipped，不引入跨平台 API。该 probe 不会自动创建 tap，也不会记录真实按键或权限敏感信息。

## Verification

```text
cargo fmt --manifest-path spikes/input-macos/Cargo.toml -- --check
cargo test --manifest-path spikes/input-macos/Cargo.toml --locked
cargo run --manifest-path spikes/input-macos/Cargo.toml --locked
```

目前已覆盖 denied/granted、tap timeout/disable、permission revocation 和 session reset 的 4 个状态测试。真实 callback、TCC 授权/拒绝/撤销、`CGEventSourceKeyState` 校正和 100 次 restart 仍必须在 macOS 实机完成。
