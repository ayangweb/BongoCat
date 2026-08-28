# ADR-0002: GPUI for Settings UI

状态：Accepted, pending Phase 0 validation
日期：2026-08-28

## Context

BongoCat 需要一套跨 Windows/macOS、可主题化、支持复杂表单和模型管理的 Rust 设置界面。设置 UI 不是实时 Live2D 渲染表面。

## Decision

使用 GPUI 构建设置、模型管理、快捷键、权限、更新和诊断界面。首个 spike 固定 `gpui = "=0.2.2"` 并提交 `Cargo.lock`。

GPUI `Entity` 只拥有临时视图状态。运行时配置和业务状态通过强类型 command/snapshot 边界访问。

## Consequences

- 项目维护一套设置 UI 和 design system。
- 不依赖 Zed 应用内部 UI crate 或 GPUI renderer 私有接口。
- GPUI 升级必须独立验证，不能无约束跟随上游。
- GPUI 设置窗口关闭后，输入、动画和 overlay 必须继续运行。

## Verification

Phase 0 验证双平台字体、中文输入法、焦点、键盘导航、辅助功能、DPI/Retina、窗口重建、后台生命周期和 shutdown。
