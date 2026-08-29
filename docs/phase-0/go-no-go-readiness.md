# Phase 0 Go/No-Go Readiness

状态：`NOT READY FOR DECISION`
评审日期：2026-08-29
目标分支：`next`

本文是 Phase 0 证据索引和剩余工作清单，不是最终 GO、GO WITH CONDITIONS 或 NO-GO 决议。只有 Technical Design 与 Implementation TODO 的 Phase 0 退出门槛全部有可复核结果后，才能改变本状态并勾选 `P0-GO-NO-GO`。

## 1. Current Assessment

纯 Rust 应用、GPUI 设置窗口、独立 D3D11/Metal overlay、可校正键鼠输入和双平台手柄 producer 已分别证明核心 API 与所有权模型可实现。当前仍不能进入产品 workspace，原因不是 scaffold 数量，而是以下 P0 事实尚未成立：

1. 没有合法取得并 hash 固定的 Cubism Native R5 SDK ZIP，因而没有真实 Core ABI、Moc/Model、drawable 或三个预置模型原生绘制结果；
2. Live2D 尚未书面确认 Expandable Application 发布授权、独立 Rust Framework 行为实现边界与 binding/attribution 要求；
3. GPUI 尚缺真实 IME、VoiceOver/Narrator、完整 focus/tooltip/dialog 和目标 DPI 矩阵；
4. Windows issue #47、macOS TCC 变化、物理键鼠/手柄、真实 session/power 和 GPU driver/device-loss 矩阵尚未完成；
5. Windows ARM64 没有官方 desktop R5 Core，macOS Intel 和最低工具链尚未冻结。

当前工程结论是：继续 Phase 0 spike 有价值，但创建完整产品 crate、批量迁移功能或删除历史实现仍违反阶段门禁。

## 2. Gate Matrix

| Gate                     | Current evidence                                                                                                                                                                                                                                                 | Missing evidence                                                                                                       | Disposition                                               |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| 行为与 fixture           | 47 项功能范围、9 组/51 事件/24 checkpoint 的 Rust reducer 与完整参数 snapshot、三个预置模型资源索引和异常 fixture 已冻结                                                                                                                                         | fixture 的旧版实机观察仍未全部人工确认；产品 runtime 尚未消费该 contract                                               | Engineering 可继续；不阻塞独立平台实测                    |
| GPUI settings            | GPUI 0.2.2 默认 shader、`.app`、主题、文本状态 contract、runtime bridge、AX/UIA 最小 tree/action 和窗口重开已通过                                                                                                                                                | 真实中文 IME、VoiceOver/Narrator、error/loading 宣读、完整 focus/tooltip/dialog、Windows DPI                           | P0 blocker；ADR-0009 保持 Proposed                        |
| Overlay                  | macOS Metal 与 Windows D3D11 合成几何、透明合成、resize、故障恢复、100-cycle owner 生命周期已通过                                                                                                                                                                | 物理拖动/显示器切换、真实 driver device loss、Windows swapchain unavailable、长期 GPU 工具采样                         | P0 blocker；不得等同于 Live2D renderer                    |
| Windows input            | Raw Input、状态校正、lifecycle Reset、可靠队列、pointer 合并和系统合成丢 release 闭环已通过；`b6bbd73` 的 runner XInput smoke 完成 124 次无错误查询并干净关闭                                                                                                    | PixPin、Win+L、PrintScreen、UAC、管理员差异、10 分钟物理压力、物理手柄；runner `peak_connected=0`                      | P0 blocker；CI 合成/无设备 API smoke 不能替代             |
| macOS input              | CGEventTap、两次缺失校正、受控 disable/lifecycle、100-cycle restart、cursor 合并和 sequence 变更后的 synthetic release-loss 20/20 cycle 已通过；`a4fab65` 的双 CI 原生 job 通过 30+5 项 GameController contract/report test，本机无设备 framework smoke 干净关闭 | TCC deny/grant/revoke、自然 timeout、真实 session/power、物理键鼠/手柄                                                 | P0 blocker；synthetic callback 不替代物理/系统丢事件      |
| Cubism source/license    | R5/Core 06.00.0001、官方来源、目标 artifact 路径和许可问题已固定                                                                                                                                                                                                 | SDK ZIP/hash、书面发布授权、Rust Framework 实现边界、notice/redistribution 结论                                        | External P0 blocker；当前最关键                           |
| Cubism ABI/model         | 三个预置 model3/cdi3、6 个 motion3、15 个 exp3、13 个本机历史 physics3 与合成 pose3 已由 Rust 静态 parser 验证；历史 physics 只保留匿名聚合，cdi3 计数和 legacy Core baseline 一致                                                                               | Native R5 binding、ABI、Core ID 对照、Moc/Model owner、drawable、Framework 求值、授权 physics/pose fixture、100 次切换 | P0 blocker；历史/合成结构验证不替代授权 fixture/Core/求值 |
| Targets/toolchain        | Windows 仅 x64/ARM64，i686 已排除；当前 Rust/GPUI/macOS 工具链和双 Windows target check 有记录                                                                                                                                                                   | Windows 实机 MSVC/SDK、最低 macOS toolchain、Intel 发布形式、Windows ARM64 desktop Core                                | P0 blocker；矩阵保持 Provisional                          |
| Config/runtime contracts | 强类型 bounded runtime、revision、shutdown、双环境 resolver/lock/atomic recovery 已通过 spike                                                                                                                                                                    | 产品 workspace 的 build-time environment、统一 runtime owner 与完整 config service                                     | Phase 1/2 工作；Phase 0 GO 前不创建产品实现               |

## 3. External Actions

以下工作不能由仓库内代码或无人值守 CI 代替：

| Owner              | Required action                                                                                                        | Evidence to return                                                                 | Failure decision                                         |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Maintainer         | 在 Live2D 官方页面接受当时协议并合法下载 `CubismSdkForNative-5-r.5.zip`                                                | 仓库外 ZIP 路径；inspector 生成的 archive/header/Core hashes；第二人或第二机器复核 | 无 artifact 则 `P0-CUBISM` 保持阻塞                      |
| Maintainer / legal | 向 Live2D 书面确认 Expandable Application、MIT Rust 调用 Core、Framework 行为依据、binding 和分发/attribution          | 可归档的书面答复与适用协议                                                         | 禁止发布或禁止可行 Rust 实现时，对当前方案给出 NO-GO     |
| Windows tester     | 在 Windows 10/11 实机完成 PixPin、Win+L、PrintScreen、UAC、管理员/非管理员、DPI/display/GPU 和物理 XInput 矩阵         | commit、OS/build、硬件/driver、运行命令、匿名计数和原始工具输出                    | issue #47 残留或 renderer 不可恢复时保持 NO-GO candidate |
| macOS tester       | 在目标 macOS 版本完成 TCC deny/grant/revoke、真实 IME、VoiceOver、锁屏/睡眠/用户切换、显示器和物理 GameController 矩阵 | commit、OS、CPU/GPU、权限前后状态、匿名计数和辅助技术结果                          | 权限/输入不可恢复或 AX 不可用时触发 GPUI/input 选型复评  |
| Intel Mac owner    | 验证 `x86_64-apple-darwin` 的 GPUI、Metal、CGEventTap、Core、签名和打包                                                | 原生 Intel 构建/运行记录                                                           | 无设备或失败时明确停止 Intel 首发，不以 Rosetta 冒充     |

## 4. Engineering Queue

外部动作等待期间，仓库内工作按以下顺序继续：

1. 取得可分发授权的 physics3/pose3 fixture；三个预置模型不含这两类资源，合成结构样本不能替代兼容与求值证据；
2. 获得合法 SDK 后立即运行 offline inspector、raw binding 双生成和 macOS arm64/Windows x64 ABI smoke；
3. 建立最小 Moc/Model safe owner，读取三个预置 moc 的 drawable 数据并验证 100 次析构；
4. 以同一不可变 RenderSnapshot 在 D3D11/Metal 绘制 texture/order/alpha/mask，完成至少一个输入到绘制闭环；
5. 完成 GPUI/TCC/issue #47/物理设备矩阵并更新对应 spike 文档；
6. 冻结 target/toolchain ADR，随后才发布最终 Phase 0 决议。

## 5. Decision Rules

最终评审只能使用以下三种结论：

- `GO`：所有 P0 blocker 已解除，产品 workspace 可以按 TODO Phase 1 建立；
- `GO WITH CONDITIONS`：仅剩不影响 Phase 1 架构正确性的受控条件，每项必须有 owner、最迟阶段和失败回退；Cubism 发布授权不能被降级为普通发布前 TODO；
- `NO-GO`：Cubism 授权/独立 Rust 实现不可行、GPUI 无法满足双平台基础输入/辅助功能、输入最终一致性或原生 overlay 无法达到门槛。

在当前 `NOT READY FOR DECISION` 状态下，不允许把“合成测试通过”“CI 编译通过”“无设备 API smoke”或“旧 Web Core baseline”替代对应实机、厂商二进制与书面授权证据。
