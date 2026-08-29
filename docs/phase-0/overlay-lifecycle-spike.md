# Native Overlay Lifecycle Contract Spike

状态：生命周期契约、双平台透明合成、连续帧与受控 renderer 故障恢复通过；完整 overlay 门禁仍未完成
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
non-empty premultiplied-alpha draw/present submitted
native overlay created
overlay hidden
overlay shown
non-empty premultiplied-alpha draw/present submitted
```

设置窗口和独立 `NSPanel` 同时存在，overlay 使用独立 `CAMetalLayer`，鼠标穿透、跨 Space 和 Full Screen Auxiliary 行为由 AppKit wrapper 设置。此验证覆盖 macOS 当前机器的窗口/Metal/GPUI 生命周期，不包含 Live2D/Cubism、真实 frame source、输入服务或发布签名。

同一台机器还通过以下 release smoke：

```text
cargo run --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked --release -- --macos-overlay-cycles 100
NSZombieEnabled=YES spikes/gpui-overlay-macos/target/release/bongocat-gpui-overlay-macos-spike --macos-overlay-cycles 100
leaks --atExit -- spikes/gpui-overlay-macos/target/release/bongocat-gpui-overlay-macos-spike --macos-overlay-cycle-worker 100
cargo run --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked --release -- --simulate-macos-stale-drawable-size --auto-quit-ms 900
cargo run --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked -- --simulate-macos-drawable-unavailable --auto-quit-ms 300
cargo run --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked --release -- --simulate-renderer-loss-at-frame 12 --auto-quit-ms 1600
```

每次循环都真实创建 `NSPanel`、`CAMetalLayer`、Metal render pipeline、
vertex buffer、command queue 和 drawable，清空透明背景后绘制一组预乘 alpha 三角形，
等待 command buffer 完成，并从 drawable 中回读中心像素；只有 alpha 非零且三个颜色
通道都不大于 alpha 才通过。随后循环隐藏窗口并析构 GPU/window owner。
父进程只在 worker 经 GPUI `quit()` 正常退出且输出 shutdown marker 时接受结果；
初始版本的普通与 `NSZombieEnabled` 运行均报告
`non_empty_frames=100 windows_before=0 windows_after=0 owners_before=0 owners_after=0 clean_shutdown=true`，
且没有 deallocated-object 消息。

为区分一次性系统初始化与每轮 overlay owner 增长，`leaks --atExit` 分别以
1、10 和 100 cycle 建立基线。原 100-cycle 结果为 `342 leaks / 22512 bytes`、
physical footprint `38.4M`、peak `47.1M`，其中比 1/10-cycle 基线多出 3 个
`_NSWindowTransformAnimation` retain cycle。overlay 是无动画的后台窗口，因此 wrapper
现在显式设置 `NSWindowAnimationBehavior::None`。修改后相同 100-cycle probe 为
`288 leaks / 18816 bytes`、physical footprint `16.3M`、peak `24.3M`，不再包含
`_NSWindowTransformAnimation`、overlay owner 或 Metal resource stack；剩余记录全部来自
AppIntents/LinkServices 的 3 个系统 `NSXPCConnection` 常驻 cycle。普通 release 与
`NSZombieEnabled=YES` probe 仍分别完成 100/100 个非空帧，窗口和 Rust owner 都回到 0。
这些数据证明窗口动画 retain cycle 已消除，但不把系统 XPC 基线计作应用泄漏，也不替代
Metal driver/GPU memory 和线程的专项采样。

当前 resource probe 在 10 次初始化预热后运行三个等长的 100-cycle batch。第一批让
Metal compiler/driver worker pool 完成延迟初始化，第二批结束时建立进程基线，第三批结束时
再次读取 `proc_pidinfo(PROC_PIDTASKINFO)` 的线程/RSS 和
`MTLDevice.currentAllocatedSize`。窗口或 Rust owner 不相等、线程数增加或 Metal allocation
增加都会使 worker 非零退出；RSS 受 allocator/系统 cache 影响，只输出原始值，不作为单点
泄漏判定。连续两次独立 release 运行都完成 300 个非空帧，第二/第三批均为
`threads 9 -> 9`、`metal_bytes 393216 -> 393216`、window/owner `0 -> 0`；RSS 分别约增长
230 KiB 和 304 KiB。该 probe 已覆盖应用可见的 Metal allocation 与进程线程增长，不替代
Instruments/Metal System Trace 的 driver resource 和长期趋势证据。

每次提交帧前，wrapper 通过 content view 的 `convertRectToBacking` 计算当前物理像素尺寸，
仅在它与 `CAMetalLayer.drawableSize` 不同时更新 layer。这样跨 Retina/非 Retina 显示器后
即使窗口通知丢失，下一帧也会收敛，不需要把 AppKit notification token 或 callback 生命周期
扩散给 renderer。受控 release smoke 先把 drawable 改成 `1x1`，首帧恢复为当前 Retina
`640x480`，逻辑 resize 后再恢复为 `800x600`，随后完成 49 帧和有序 shutdown。7 项单元测试
覆盖正数/有限/整数物理尺寸以及陈旧尺寸检测。该结果验证校正算法，不替代真实外接显示器
热切换、非 Retina 屏幕或 display removal 实测。

受控 drawable unavailable 路径保留 overlay owner 直到统一 shutdown，但不执行后续
显示/隐藏 smoke；设置窗口仍打开并显示 degraded 诊断，然后在 GPUI `quit()` 前显式
释放 overlay。以上计数与 process/Metal resource probe 不能替代 driver resource 的
Instruments/Metal System Trace 专项采样。GitHub `macos-latest` 已接入相关
smoke。commit `5bc82b61b12d9873fb8bddfdb0de4f1652487ac9` 的 push run
`33245147905`、job `99081224637` 与 PR run `33245149605`、job
`99081228964` 均通过；两次 runner 都记录正常透明 clear/present、显示/隐藏、
drawable unavailable 降级、GPUI 正常退出、owner 释放，以及
`windows_before=0 windows_after=0 owners_before=0 owners_after=0 clean_shutdown=true`。

连续 frame source 还支持运行中故障恢复。受控 smoke 在第 12 个成功帧注入
`device_lost` 语义故障，立即释放旧 Metal/AppKit owner，等待 3 个 frame tick 后重建
完整 owner，重新应用 `400x300` 逻辑尺寸与 `800x600` Retina drawable，并恢复提交。
本机 release 结果为 `frames=87 resize_completed=true failures=1 recoveries=1`；设置窗口
在恢复期间显示 recovering，成功后切回 recovered。重建最多尝试 3 次并使用有限退避，
耗尽后只停止 frame source、保留 GPUI 设置窗口和诊断，不进入 panic 或忙循环。

当前非空帧使用 spike 内的合成顶点和运行时编译 Metal shader，只证明 Rust 能独立
创建 pipeline、提交真实 draw call、进行预乘 alpha 合成并可靠回读。它不包含 Cubism
drawable、模型纹理、draw order、mask 或 production shader 打包；这些仍属于 1.8 的
模型 renderer 门禁，正式实现不得依赖运行时 shader 编译。

同一 commit 的 Windows push job `99081224522` 与 PR job `99081228988` 也保持
hardware D3D11、DPI 96、透明 clear/present、degraded 初始化、GPU 早于 HWND
释放，以及 `handles_before=172 handles_after=172 clean_shutdown=true`，说明共用入口
的 macOS 改动没有破坏已有 Windows smoke。

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
- Rust 提供的合成顶点、运行时编译 HLSL、vertex/pixel shader、input layout、
  vertex buffer、`CULL_NONE` rasterizer state 和 `ONE / INV_SRC_ALPHA` 预乘 alpha blend state；
- 透明 clear 后提交非空 draw，将 back buffer 复制到 staging texture 并映射中心
  BGRA 像素；alpha 必须非零且三个颜色通道不得大于 alpha；
- present、device-removed 检查、显示/隐藏和一次性 topmost；
- 将 `DXGI_ERROR_DEVICE_REMOVED/RESET/HUNG/DRIVER_INTERNAL_ERROR` 分类为 device lost，
  将 access lost/not currently available/wait timeout 分类为 surface unavailable；
- 按窗口 DPI 将逻辑尺寸换算为物理像素，释放旧 back-buffer 引用后执行
  `ResizeBuffers` 并重建 RTV 与 staging texture；
- owner-thread assertion 与 `!Send`/`!Sync` marker；
- composition detach -> D3D clear/flush -> COM release -> HWND destroy 顺序；
- renderer 初始化失败注入，GPUI 设置窗口继续显示 degraded 状态；
- 连续帧故障后释放旧 D3D11/Win32 owner、有限退避、重建并恢复绘制；
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

commit `53eec36` 的首次真实 Windows 非空帧运行暴露了默认 D3D11 背面剔除：draw
已提交但中心像素仍透明。commit `ebaea32` 将 rasterizer 明确设为 `CULL_NONE`，并由
push run `33247687689` 和 PR run `33247689437` 的 hardware D3D11 smoke 验证连续帧、
非空 readback、逻辑 resize、`172 -> 172` handle 计数与有序退出。当前新增的受控
运行中 renderer loss/recreate smoke 将在本批 push 后由同一 Windows job 验证；真实
驱动 device removal 仍不能由注入结果代替。

当前非空帧仍使用 spike 内的合成顶点和运行时编译 HLSL，只验证 D3D11 pipeline、
预乘 alpha 和可回读 draw。预置模型纹理、draw order、mask 与 production shader
打包仍属于 Cubism/Renderer 门禁，正式实现不得依赖运行时 shader 编译。

### 线程与所有权不变量

- GPUI application loop 在进程 UI 主线程启动；overlay owner 在同一线程创建、
  调用和析构。
- macOS wrapper 必须持有 `MainThreadMarker` 才能创建 `NSPanel`，AppKit 和 Metal
  layer 的窗口生命周期操作不得离开主线程。
- Windows wrapper 记录创建线程并在每次公开操作和析构时断言 owner thread；
  `Rc` marker 使完整 window/GPU owner 保持 `!Send`/`!Sync`。
- GPUI async task 只可通过 `cx.update` 回到 application thread 后访问 overlay；
  runtime 或输入 worker 不得持有 HWND、NSPanel、GPU handle 或 overlay 引用。
- 当前 spike 的 60 Hz 定时 frame source 在 GPUI executor 上等待，在 application thread
  上提交 renderer 调用；退出先请求停止并等待确认，再析构 GPU/window owner。
- 未来跨线程操作必须发送强类型 command，由 UI thread adapter 执行；shutdown
  command 必须停止生产者后，在 GPUI `quit()` 前显式取出并析构 overlay owner。

### 尚未验证

- 两个平台的真实 Live2D/Cubism 绘制和模型资源兼容。
- 两个平台的模型 texture、draw order、mask 及离线固定 shader 产物。
- Windows GPU memory/driver resource 与线程专项采样，以及 macOS Instruments/Metal
  System Trace driver resource 证据；macOS process thread 和 Metal allocated size 已接入门禁。
- Windows swapchain unavailable 与双平台真实 GPU device lost；双平台受控 owner 释放、
  有限退避、重建和诊断 UI 已实现，但注入结果不能替代真实驱动故障。
- 用户拖动、显示器/DPI 热切换和 production display-linked frame source 的完整生命周期；
  当前验证了 programmatic resize、逐帧 backing-size 校正与 GPUI 定时 frame source。

## 下一步

下一步是补充真实 swapchain unavailable/device-lost、双平台 GPU/线程专项采样、
display-linked frame source、拖动/显示器切换和模型绘制；任一平台结果都不能替代
另一平台证据。
