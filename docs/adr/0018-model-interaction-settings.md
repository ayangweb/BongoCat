# ADR-0018: Typed Model Interaction Settings

状态：已接受（2026-09-01）

## 背景

Native Rewrite 的 Native 配置已经包含水平镜像、镜像指针跟随和忽略指针字段，但此前
这些字段没有穿过 runtime 边界。直接让 renderer 读取配置会违反 renderer 只消费不可变
`RenderSnapshot` 的所有权规则，也会让模型求值与显示变换在不同线程拥有不一致的设置。

## 决策

- `bongocat-runtime::ModelSettings` 是三项模型交互设置的唯一 runtime 类型，并通过
  `RuntimeCommand::SetModelSettings` 写入、`RuntimeSnapshot::model_settings` 读回。
- `mirror` 由 runtime 写入 `RenderSnapshot::mirror_horizontal`；Windows D3D11 和 macOS
  Metal 只依据该不可变字段对模型中心执行水平变换，不读取配置或 UI 状态。
- `mirror_pointer_tracking` 在产品参数覆盖阶段反转指针 X/Z 值，Y 值保持不变；
  `ignore_pointer` 跳过所有指针参数覆盖，使 Core 默认值或前序动画值保持有效。
- 应用启动在配置验证后发送一次 typed settings command。动态设置页编辑、revision-checked
  持久化 command 和完整多显示器/实机输入证据由后续 TODO 继续跟踪。

## 验证

runtime 测试验证 settings command 的 revision、snapshot 和 shutdown 保留；macOS
Cubism 测试验证指针镜像/忽略规则及 render snapshot 镜像字段；overlay 变换测试验证
普通与水平镜像在中心点保持相同且 X scale 符号相反。Windows 使用相同纯 Rust 变换逻辑，
其真实 GPU/DPI 矩阵仍属于平台门禁。

## 后续边界

本 ADR 不改变 physics/pose 的授权 fixture 和行为求值门禁，不声称完整 Live2D 兼容；
也不定义快捷键冲突、用户编辑或平台全局 hotkey 注册协议。
