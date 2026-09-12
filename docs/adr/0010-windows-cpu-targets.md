# ADR-0010: Windows CPU Targets

状态：Accepted, ARM64 release blocked by Cubism R5
日期：2026-08-29

## Context

Native Rewrite 需要明确 Windows 发布架构，避免继续为不再支持的 target
维护输入、renderer、安装和更新分支。当前固定的 Cubism 5 SDK for Native R5
提供 desktop Windows x86/x64 Core，但没有 desktop Windows ARM64 Core；其
experimental UWP ARM64 DLL 不能用于 Win32 desktop 应用。

## Decision

Windows 产品目标只包括：

- `x86_64-pc-windows-msvc`；
- `aarch64-pc-windows-msvc`。

`i686-pc-windows-msvc` 不属于 Native Rewrite 的构建、CI、安装包、更新或测试
目标。历史 x86 安装包只作为行为与发布考古证据保留，不形成兼容承诺。

Windows ARM64 是产品目标，不等于当前可发布。R5 缺少匹配的官方 desktop
Core，因此 ARM64 在 `P0-CUBISM` 保持 release blocker：不得使用 UWP DLL、
未知来源二进制、自制 Core 兼容层或模拟执行来宣称支持。

## Consequences

- 新的 target matrix、binding fixture 和发布配置不得加入 i686。
- Windows 平台依赖与 renderer 最终必须分别验证 x64 和原生 ARM64。
- 在官方可授权的 desktop ARM64 Core 存在并通过 hash、ABI、模型和 renderer
  smoke 前，Native Rewrite 不能宣称 Windows ARM64 可发布。
- 若 Cubism 在 Phase 0 结束前仍不提供 ARM64 artifact，go/no-go 必须把该项
  列为明确条件；移除 ARM64 或改变 Live2D 方案需要新的用户决策和 ADR。

## Verification

- `tools/inspect-cubism-sdk.py` 把 Windows ARM64 报告为
  `unsupported_by_r5`，把 i686 报告为 `excluded_by_product`。
- `tools/cubism-bindgen` 只为当前 R5 可用且仍在产品矩阵内的 Windows x64 和
  macOS targets 生成合成 bindings；它拒绝 Windows ARM64 和 i686。
- Phase 0 CI 不安装或编译 i686 target。
