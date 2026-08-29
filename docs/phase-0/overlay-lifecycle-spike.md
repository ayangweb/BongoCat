# Native Overlay Lifecycle Contract Spike

状态：生命周期契约、双平台透明 clear/present 与 Windows 生命周期门禁通过；完整 overlay 门禁仍未完成
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
`aarch64-pc-windows-msvc` 执行 Check。commit
`d0ce206ffc56ef83acf6f18c7aa330910bb5543f` 的 push run `33243568461`、job
`99076961942` 与 PR run `33243569993`、job `99076967070` 均在
`windows-latest` 通过。两次运行都确认 hardware D3D11、DPI 96、两次透明
clear/present、隐藏/重显示、GPUI 共存和自动退出；退出日志中
`Windows overlay GPU released` 严格早于 `Windows overlay window destroyed`。

受控 renderer 初始化失败后，GPUI 设置窗口仍然打开并走完自动退出。100 次
完整 Win32/D3D11/DirectComposition owner 重建报告
`handles_before=172 handles_after=172 clean_shutdown=true`。该计数证明本次 runner
进程的 process handle 未增长，不替代 GPU memory/driver resource 专项采样。

### 线程与所有权不变量

- GPUI application loop 在进程 UI 主线程启动；overlay owner 在同一线程创建、
  调用和析构。
- macOS wrapper 必须持有 `MainThreadMarker` 才能创建 `NSPanel`，AppKit 和 Metal
  layer 的窗口生命周期操作不得离开主线程。
- Windows wrapper 记录创建线程并在每次公开操作和析构时断言 owner thread；
  `Rc` marker 使完整 window/GPU owner 保持 `!Send`/`!Sync`。
- GPUI async task 只可通过 `cx.update` 回到 application thread 后访问 overlay；
  runtime 或输入 worker 不得持有 HWND、NSPanel、GPU handle 或 overlay 引用。
- 未来跨线程操作必须发送强类型 command，由 UI thread adapter 执行；shutdown
  command 必须停止生产者后，在 GPUI `quit()` 前显式取出并析构 overlay owner。

### 尚未验证

- 两个平台的真实 Live2D/Cubism 绘制和模型资源兼容。
- macOS 100 次真实原生窗口创建/销毁循环，以及双平台 GPU memory/driver resource 专项采样。
- drawable/swapchain unavailable、真实 GPU device lost 和恢复后的诊断 UI。
- 拖动、缩放、显示器/DPI 切换和真实 frame source 的完整生命周期。

## 下一步

下一步是在 macOS 补齐 100 次原生窗口/GPU owner 重建与 drawable unavailable
注入，并为 Windows 增加 swapchain unavailable/device-lost 恢复验证。随后才能在
两个平台接入真实 frame source、模型绘制和 GPU 专项采样；任一平台结果都不能
替代另一平台证据。
