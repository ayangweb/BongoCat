# ADR-0003: Native Overlay Renderers

状态：Accepted, pending Phase 0 validation
日期：2026-08-28

## Context

主猫窗口需要透明、置顶、穿透、低延迟、高 DPI/Retina 和可控 GPU 生命周期。GPUI 没有为双平台 Live2D 外部纹理提供稳定的公共合成接口。

## Decision

主猫使用独立原生 overlay：

- Windows：Win32 + D3D11 + DXGI + DirectComposition/DWM。
- macOS：AppKit `NSPanel` + Metal + `CAMetalLayer`。

Renderer 只消费不可变 `RenderSnapshot`，不访问 GPUI、配置或输入服务。

## Consequences

- 设置 UI 与实时渲染拥有独立的窗口和 frame loop。
- 两个平台分别实现 GPU backend，但共享模型求值和 render contract。
- 首发不为了后续 Linux 强行统一 GPU API。
- 窗口、renderer 和 GPU object 必须有明确的 owner 与析构顺序。

## Verification

Phase 0 完成双平台透明 clear/present，并验证 overlay 与 GPUI 设置窗口反复创建、销毁和退出无资源增长。
