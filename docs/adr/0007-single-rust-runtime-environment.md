# ADR-0007: Single Rust Runtime Environment

状态：Accepted
日期：2026-08-28

## Context

BongoCat 的输入、pressed state、动画、当前模型和配置协调需要单一事实来源。如果生产应用同时保留多个业务 runtime，或通过 IPC 在 UI、输入和模型状态之间复制事实，会扩大时序、恢复、版本兼容和 shutdown 风险。

历史实现仍用于行为观察和模型资源兼容证据，但不应进入 Native Rewrite 的生产执行链路。旧配置只作为已归档考古样本，不属于产品输入。

## Decision

生产应用使用一个 Rust 进程和一个业务 runtime owner。GPUI 设置界面、平台输入服务、Live2D 求值、原生 overlay 和系统集成都属于同一 Rust 应用生命周期，并通过强类型 Rust command、event 和 snapshot 协作。

不得引入第二套常驻业务运行环境、跨语言共享业务 core、sidecar 业务进程或用于同步核心状态的 IPC 边界。历史应用只作为行为与资源对照，不随 Native Rewrite 生产应用启动，也不作为功能 fallback。

官方 Cubism Core 平台二进制仍是 ADR-0005 定义的窄 FFI 例外。它只提供模型计算能力，不成为独立业务 runtime，不拥有应用状态。

安装或更新若因操作系统权限必须使用短生命周期 helper，需要在系统集成阶段单独威胁建模和 ADR；helper 必须使用 Rust、不得持有持续业务状态，也不得改变本 ADR 的单一应用 runtime 所有权。

## Consequences

- pressed state、动画、当前模型和配置协调只存在一个可变 owner。
- GPUI 关闭或重建不会创建第二份业务状态；UI 只发送 command 并读取带 revision 的 snapshot。
- 输入 callback、renderer 和平台服务不能绕过 runtime 直接维护产品状态。
- 发布切换前保留历史源码、tag 和样本；生产包不包含历史 runtime、Web 资产或旧配置读取器。

## Verification

Phase 0 验证 GPUI、输入服务、runtime、Cubism wrapper 和独立 overlay 可以在同一进程中按既定顺序启动、停止和重建。Phase 1 对 release dependency tree 与产物内容执行检查，确认不存在第二套业务 runtime、WebView 或 JavaScript runtime。
