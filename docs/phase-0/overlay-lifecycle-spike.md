# Native Overlay Lifecycle Contract Spike

状态：生命周期契约通过；macOS 原生窗口、透明 Metal 合成和 GPUI 共存已通过；Windows D3D11 待实机验证
日期：2026-08-28

## 目的

`spikes/overlay-lifecycle/` 是一个隔离的纯 Rust contract probe。它不创建窗口、不持有 GPU handle，也不成为产品 runtime；用途是先把平台实现必须遵守的状态和 shutdown 顺序固定下来。

允许的状态迁移：

```text
New -> Visible <-> Hidden -> Closing -> Closed
```

`Closing` 后禁止重新显示。关闭阶段必须按以下顺序完成：

```text
InputStopped -> RuntimeStopped -> ConfigFlushed -> FrameSourceStopped
  -> RendererReleased -> OverlayDestroyed -> GpuiClosed
```

`OverlayDestroyed` 完成后状态变为 `Closed`；平台 wrapper 必须确保窗口、layer、swapchain/drawable 和 GPU owner 不再被使用。

## 验证

```text
cargo fmt --manifest-path spikes/overlay-lifecycle/Cargo.toml -- --check
cargo test --manifest-path spikes/overlay-lifecycle/Cargo.toml --locked
```

测试覆盖显示/隐藏/重开、乱序 shutdown、关闭后禁止重开以及 100 次创建/销毁循环。该 contract 结果不能替代平台 GPU 验证。

### macOS GPUI 共存验证

源码位于 `spikes/gpui-overlay-macos/`，使用 `gpui = 0.2.2`、`objc2`、`metal = 0.33.0` 和 `core-graphics-types = 0.2.0`。在以下环境执行：

```text
Hardware: MacBook Pro 18,1 / Apple M1 Pro / 16 GB
GPU: Apple M1 Pro / Metal 4
Display: 3456x2234 Retina
OS: macOS 26.5.2 (25F84)
Rust: rustc 1.97.1, aarch64-apple-darwin
```

通过的命令和结果：

```text
cargo fmt --manifest-path spikes/gpui-overlay-macos/Cargo.toml
cargo check --offline --manifest-path spikes/gpui-overlay-macos/Cargo.toml
cargo build --release --offline --locked --manifest-path spikes/gpui-overlay-macos/Cargo.toml
./spikes/gpui-overlay-macos/scripts/package-macos.sh
codesign --verify --deep --strict --verbose=4 "target/package/BongoCat GPUI Overlay Spike.app"
open -W "target/package/BongoCat GPUI Overlay Spike.app" --args --auto-quit-ms 1500
```

`.app` 的 Bundle ID 为 `com.ayangweb.bongo-cat`，ad-hoc 签名通过 strict bundle integrity 检查。运行日志确认：

```text
overlay shown
transparent clear/present submitted
native overlay created
overlay hidden
overlay shown
transparent clear/present submitted
```

设置窗口和独立 `NSPanel` 同时存在，overlay 使用独立 `CAMetalLayer`，鼠标穿透、跨 Space 和 Full Screen Auxiliary 行为由 AppKit wrapper 设置。此验证覆盖 macOS 当前机器的窗口/Metal/GPUI 生命周期，不包含 Live2D/Cubism、真实 frame source、输入服务或发布签名。

### 尚未验证

- Windows Win32 + D3D11 透明 clear/present、DPI 和 device lost 路径；当前 macOS 结果不能推断 Windows 行为。
- 两个平台的真实 Live2D/Cubism 绘制和模型资源兼容。
- 100 次真实原生窗口创建/销毁循环；目前 100 次仅针对无平台 contract probe。
- renderer 初始化失败、drawable/swapchain unavailable 和 GPU device lost 后的诊断 UI。

## 下一步

下一步是在 Windows 实机完成 Win32/D3D11 对等验证，并在两个平台接入真实 frame source、模型绘制和失败诊断；macOS 结果应保持为独立平台证据，不作为跨平台完成声明。
