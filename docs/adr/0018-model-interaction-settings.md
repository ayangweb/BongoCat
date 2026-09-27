# ADR-0018: Typed Model Interaction Settings

状态：已接受（2026-09-01；2026-09-25 增加键盘/手柄模型输入门禁；2026-09-27 指针跟随按轴拆分）

## 背景

BongoCat 的 配置已经包含水平镜像、镜像指针跟随和忽略指针字段，但此前
这些字段没有穿过 runtime 边界。直接让 renderer 读取配置会违反 renderer 只消费不可变
`RenderSnapshot` 的所有权规则，也会让模型求值与显示变换在不同线程拥有不一致的设置。

## 决策

- `bongocat-runtime::ModelSettings` 是六项模型交互设置的唯一 runtime 类型，并通过
  `RuntimeCommand::SetModelSettings` 写入、`RuntimeSnapshot::model_settings` 读回。
- `mirror` 由 runtime 写入 `RenderSnapshot::mirror_horizontal`；Windows D3D11 和 macOS
  Metal 只依据该不可变字段对模型中心执行水平变换，不读取配置或 UI 状态。
- 指针跟随按轴拆成 `mirror_pointer_tracking_horizontal` 和
  `mirror_pointer_tracking_vertical`，两项互相独立、默认均为 `false`：水平项在产品参数覆盖阶段
  反转指针的 X/Z 值（Z 与 X 是同一水平扫视的两个方向），垂直项只反转 Y 值。拆成两个布尔值而不是
  一个方向枚举，是因为「左右相反」和「上下相反」可以只出现其一、也可以同时出现，枚举无法在不增加
  第三种取值的前提下表达这四种组合；`ignore_pointer` 仍然跳过所有指针参数覆盖，使 Core 默认值或
  前序动画值保持有效。
- `ignore_keyboard` 和 `ignore_gamepad` 是模型输入投影门禁：分别屏蔽键盘的键图/手部贡献，
  以及手柄的按钮/手部/摇杆/扳机贡献。它们不停止平台采集、pressed-state 校正与复位、
  匿名诊断或独立的快捷键注册；原始 pressed state 始终保留，解除门禁后仍能正确处理释放。
  三个模型输入忽略开关各自有一个可选的 application shortcut target，触发后仍经 settings
  service 的 typed command 和配置 revision 原子持久化。
- 应用启动在配置验证后发送一次 typed settings command。动态设置页编辑、revision-checked
  持久化 command 和完整多显示器/实机输入证据由后续 TODO 继续跟踪。

## 验证

runtime 测试验证 settings command 的 revision、snapshot 和 shutdown 保留，分别验证键盘/手柄
输入门禁只改变模型投影而不丢失另一来源或原始 pressed state，并验证两个指针翻转开关各自只影响
所属轴：单开水平项时 X/Z 反号而 Y 不变，单开垂直项时只有 Y 反号，两项同开等于两次单开结果的组合。
app service 测试验证三个输入门禁快捷键会切换并持久化对应设置，并验证两个翻转字段经 typed command
写入配置文件后重启仍被恢复。macOS Cubism 测试验证指针镜像/忽略规则及 render snapshot 镜像
字段；overlay 变换测试验证普通与水平镜像在中心点保持相同且 X scale 符号相反。Windows 使用
相同纯 Rust 变换逻辑，其真实 GPU/DPI 矩阵仍属于平台门禁。

## 后续边界

本 ADR 不改变 physics/pose 的授权 fixture 和行为求值门禁，不声称完整 Live2D 兼容；
也不定义快捷键冲突、用户编辑或平台全局 hotkey 注册协议。
