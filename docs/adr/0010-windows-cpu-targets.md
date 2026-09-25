# ADR-0010: Windows CPU Targets

状态：已被 ADR-0033 取代（2026-09-14，Windows ARM64 不再是产品目标，只剩 x64）

## Context

BongoCat 需要明确 Windows 发布架构，避免继续为不再支持的 target
维护输入、renderer、安装和更新分支。当前固定的 Cubism 5 SDK for Native R5
提供 desktop Windows x86/x64 Core，但没有 desktop Windows ARM64 Core；其
experimental UWP ARM64 DLL 不能用于 Win32 desktop 应用。

## Decision

Windows 产品目标只包括：

- `x86_64-pc-windows-msvc`。

`i686-pc-windows-msvc` 和 `aarch64-pc-windows-msvc` 都不属于 BongoCat
的构建、CI、安装包、更新或测试目标。历史 x86 安装包只作为行为与发布考古证据
保留，不形成兼容承诺。

Windows ARM64 曾作为产品目标保留；ADR-0033 已取消该目标。原因不是"R5 缺少
Core"这一次要阻碍，而是它使产品矩阵多出一个**无法端到端验证**的原生构建：
没有官方可授权的 desktop ARM64 Core，就没有真实的 ABI、模型和 renderer 证据链。
Windows 自身提供 x64 仿真，Windows on ARM 设备运行 x64 构建在正常情况下可用，
因此维持一个不可验证的原生 ARM64 目标只增加成本、CI 面积和发布风险。

## Consequences

- 新的 target matrix、binding fixture 和发布配置不得加入 `i686-pc-windows-msvc`
  或 `aarch64-pc-windows-msvc`。
- `deny.toml` 的 `[graph].targets`、`bongocat-update::UpdateTargetTriple` 与
  发布流水线必须只包含 `x86_64-pc-windows-msvc`；契约测试强制三者一致。
- Windows 平台依赖与 renderer 只在 x64 上验证。ARM64 上的实际体验通过 Windows
  的 x64 仿真覆盖，不再有独立的原生 ARM64 证据要求。

## Verification

- `tools/inspect-cubism-sdk.py` 把 Windows ARM64 报告为 `unsupported_by_r5`，
  把 i686 报告为 `excluded_by_product`。
- `tools/cubism-bindgen` 只为当前 R5 可用且仍在产品矩阵内的 Windows x64 和
  macOS targets 生成合成 bindings；它拒绝 Windows ARM64 和 i686。
- Phase 0 CI 不安装或编译 i686，也不再安装或编译 Windows ARM64。
- `tools/tests/test_packaging_contract.py` 断言打包工具的 target 集合、
  `deny.toml` 与 `UpdateTargetTriple` 三者一致且不含 ARM64。
