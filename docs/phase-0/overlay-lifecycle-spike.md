# Native Overlay Lifecycle Contract Spike

状态：生命周期契约与 macOS 实机通过；Windows D3D11 实现已接入 CI，待 runner 证据
日期：2026-08-29

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

### Windows 实现与验收门禁

`spikes/overlay-windows/` 使用精确固定的 `windows = 0.62.2`，封装线程限定的
Win32 popup、D3D11 device/context、DXGI premultiplied-alpha composition
swapchain 和 DirectComposition target/visual。`spikes/gpui-overlay-macos/`
在 Windows 通过 target-specific path dependency 使用该 owner；它不访问 GPUI
renderer 内部对象。

当前实现包含：

- `WS_EX_NOREDIRECTIONBITMAP`、tool window、no-activate 和 click-through window；
- hardware device 优先、WARP fallback 只用于能力/生命周期验证；
- BGRA、flip sequential、premultiplied alpha 的双缓冲 composition swapchain；
- 透明 clear/present、device-removed 检查、显示/隐藏和一次性 topmost；
- owner-thread assertion 与 `!Send`/`!Sync` marker；
- composition detach -> D3D clear/flush -> COM release -> HWND destroy 顺序；
- renderer 初始化失败注入，GPUI 设置窗口继续显示 degraded 状态；
- 100 次窗口/GPU 创建销毁及 warm-up 后 process handle 增长门禁。

本机已对 `x86_64-pc-windows-msvc` 执行 Check/Clippy，并对
`aarch64-pc-windows-msvc` 执行 Check。macOS 不能提供 Windows 窗口/GPU 运行证据，
因此 Windows 项在 `windows-latest` 正常、故障注入和 100-cycle smoke 成功前保持
未完成。

### 尚未验证

- Windows runner 的 Win32 + D3D11 透明 clear/present、DPI、GPUI 共存和 100-cycle 结果；实现已存在但不能由 macOS 推断运行正确。
- 两个平台的真实 Live2D/Cubism 绘制和模型资源兼容。
- macOS 100 次真实原生窗口创建/销毁循环，以及双平台 GPU memory/driver resource 专项采样。
- drawable/swapchain unavailable、真实 GPU device lost 和恢复后的诊断 UI。

## 下一步

下一步是在 Windows 实机完成 Win32/D3D11 对等验证，并在两个平台接入真实 frame source、模型绘制和失败诊断；macOS 结果应保持为独立平台证据，不作为跨平台完成声明。
