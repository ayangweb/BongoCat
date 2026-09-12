# ADR-0001: Single Rust Application

状态：Accepted
日期：2026-08-28

## Context

BongoCat 需要在 Windows 和 macOS 上统一实现设置 UI、实时输入、动画、模型管理、窗口、渲染和系统集成，同时保持平台 API 的直接控制能力。

## Decision

BongoCat 自有应用代码统一使用 Rust 2024 edition，并在一个 Cargo workspace 中组织。业务模块通过强类型 Rust 接口协作；平台差异进入明确的 platform adapter。

官方 Cubism Core 平台二进制是唯一允许的厂商 FFI 例外。该例外只提供模型计算能力，不承载 BongoCat 业务逻辑。

## Consequences

- 状态、配置、模型和行为只实现一次。
- Windows/macOS 的窗口、输入和 GPU backend 仍分别实现。
- 平台 handle、裸指针和 `unsafe` 不能进入共享业务模块。
- Linux 可在首发后通过新增 platform/renderer backend 评估。

## Verification

Phase 0 必须证明 GPUI、独立 overlay、输入服务和 Cubism wrapper 可以在同一应用生命周期内可靠启动和停止。
