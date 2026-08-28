# Native Overlay Lifecycle Contract Spike

状态：生命周期契约和 100 次创建/销毁模型测试通过；平台窗口、透明合成和 GPUI 共存待完成
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

测试覆盖显示/隐藏/重开、乱序 shutdown、关闭后禁止重开以及 100 次创建/销毁循环。该结果不能替代 Windows D3D11 或 macOS Metal 的透明窗口验证。

## 下一步

平台 overlay spike 需要在各自主线程创建原生窗口，接入真实 frame source，并将创建、隐藏、重建和 shutdown 事件映射到本契约；同时证明 GPUI settings 窗口可并存且不共享 renderer 私有对象。
