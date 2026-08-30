# ADR-0011: Progressive Implementation With Release Gates

状态：Accepted
日期：2026-08-30

## Context

Phase 0 已通过独立 spike 验证 runtime、配置隔离、GPUI 设置窗口、原生 overlay
和可校正输入的核心所有权模型。剩余证据主要依赖厂商授权、标准 Cubism SDK、
物理设备、辅助技术、目标系统和签名环境。把这些外部证据全部作为创建产品
workspace 的前置条件，会阻止已经具备自动化契约的业务模块进入正式实现。

维护者已提供仓库外的 `CubismSdkForNative-5-r.5.zip`，并明确授权本地技术
验证和正式产品代码开发；公开发布授权和实机验收延后到产品完成后处理。

## Decision

项目采用渐进式实现，并把开发门禁与发布门禁分离：

- 允许建立正式 Rust workspace，并逐步实现 runtime、配置、模型、UI、平台和
  renderer；每项仍必须满足自己的自动化测试和代码边界。
- Phase 0 未完成项继续保持未勾选。缺失的 Cubism 行为、物理设备、辅助功能、
  GPU、签名和目标系统证据不能由编译或合成测试替代。
- Cubism 发布授权、最终 SDK/二进制清单、Windows ARM64 desktop Core、实机矩阵、
  签名、notarization、更新回滚和 soak 结果全部是 stable 发布门禁，不再阻止
  不公开分发的本地开发。
- 在获得书面许可前，不提交或公开分发 SDK ZIP、Core/Framework 源文件、由厂商
  header 生成的 bindings、厂商二进制或包含这些内容的安装包。仓库只记录版本、
  文件名、hash、目标能力和本地验证结果。
- 产品构建默认不联网，也不从未固定来源下载 SDK。缺少本地 SDK 时，与 Cubism
  无关的 workspace、测试和 CI 必须保持可用。
- 完整设置 UI 仍受 ADR-0009 的辅助功能条件约束；允许建立 UI crate 和最小产品
  窗口，但不得把未通过的交互或辅助功能宣称为可发布功能。

## Supplied SDK Disposition

当前本地基线是 Cubism SDK for Native `5-r.5`，archive SHA-256 为
`7ff3a4bbc19c0a8728965aa522ab77eb11b252916453e68a8a78d3b71188bb12`，Core
版本为 `06.00.0001`。它包含 Windows x64 和 macOS arm64/x64 Core，不包含
desktop Windows ARM64 Core。

此前提供的 `CubismSdkMotionSyncPluginForNative-5-r.2.1.zip` 是可选 MotionSync
插件，SHA-256 为
`26baaf30b0ebb26bc6253884aa61b010666e2ecc2491e3a7fff6d43eb64d6548`。它不是
标准 Native SDK，不能加载 `.moc3`，当前不进入产品范围。

## Consequences

正式实现可以从已验证的 runtime/config 契约开始，不再等待外部测试人员或法律
流程。发布状态必须单独判断；“功能已实现”不表示“可以公开分发”。如果后续授权、
SDK ABI 或实机验证失败，受影响的发布目标保持阻塞，并通过后续 ADR 调整能力或
目标矩阵，不能用未经授权的 artifact 或模拟结果绕过。
