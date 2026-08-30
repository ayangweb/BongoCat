# Phase 0 Go/No-Go Readiness

状态：`IMPLEMENTATION GO WITH RELEASE CONDITIONS`
评审日期：2026-08-30
目标分支：`next`

本文是 Phase 0 证据索引和剩余工作清单。ADR-0011 允许正式 workspace 和不依赖外部证据的模块渐进实现；本状态不是完整功能或 stable 发布的 GO，`P0-GO-NO-GO` 在全部退出门槛可复核前保持未勾选。

## 1. Current Assessment

纯 Rust 应用、GPUI 设置窗口、独立 D3D11/Metal overlay、可校正键鼠输入和双平台手柄 producer 已分别证明核心 API 与所有权模型可实现。维护者决定不再让以下外部证据阻止正式 workspace，但它们继续阻止对应功能完成声明和 stable 发布：

1. 已取得并 hash 固定 Cubism Native `5-r.4.1` SDK ZIP，且本机加载 macOS universal Core 得到版本 `05.01.0000`；真实 binding、Moc/Model、drawable 和三个预置模型原生绘制仍未完成；
2. Live2D 尚未书面确认 Expandable Application 发布授权、独立 Rust Framework 行为实现边界与 binding/attribution 要求；维护者将此工作延后到公开发布前；
3. GPUI 的 macOS WeType 拼音组合链路已通过，但仍缺 Apple 拼音、Windows IME、VoiceOver/Narrator、物理 keyboard/pointer、tooltip 朗读和目标 DPI 矩阵；
4. Windows issue #47、macOS TCC 变化、物理键鼠/手柄、真实 session/power 和 GPU driver/device-loss 矩阵尚未完成；
5. Windows ARM64 没有官方 desktop R5 Core，macOS Intel 和最低工具链尚未冻结。

当前工程结论是：可以创建正式产品 crate 并提升已验证 contract；完整 UI、批量迁移、删除历史实现、Cubism artifact 提交或公开分发仍违反阶段门禁。

## 2. Gate Matrix

| Gate                     | Current evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | Missing evidence                                                                                                                                                    | Disposition                                               |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| 行为与 fixture           | 47 项功能范围、9 组/51 事件/24 checkpoint 的 Rust reducer 与完整参数 snapshot、三个预置模型资源索引和异常 fixture 已冻结                                                                                                                                                                                                                                                                                                                                                                                                             | fixture 的旧版实机观察仍未全部人工确认；产品 runtime 尚未消费该 contract                                                                                            | Engineering 可继续；不阻塞独立平台实测                    |
| GPUI settings            | GPUI 0.2.2 默认 shader、`.app`、主题、文本状态 contract、runtime bridge、AX/UIA 最小 tree/action 和窗口重开已通过；macOS WeType 拼音 2.2.3 已通过真实 marked-text update/commit 与已有中文前缀后的再次组合；双平台 modal dialog/焦点/语义子树、原生合成 mouse-move -> tooltip delay/build/exit 与 macOS Application/Edit/Window 原生菜单动作通过；macOS AX 与 Windows UIA 均可观察 loading/error/retry，其中 Windows push run `33291750411`、job `99204478369` 与 PR run `33291751558`、job `99204481348` 已通过 revision 2 恢复门禁 | runner 托管 UIA client 缺少 AriaProperties 标识，`busy=true` 投影未验证；Apple 拼音、Windows IME、VoiceOver/Narrator、物理 keyboard/pointer、tooltip 朗读和目标 DPI | P0 blocker；ADR-0009 保持 Proposed                        |
| Overlay                  | macOS Metal 与 Windows D3D11 合成几何、透明合成、resize、故障恢复、100-cycle owner 生命周期已通过                                                                                                                                                                                                                                                                                                                                                                                                                                    | 物理拖动/显示器切换、真实 driver device loss、Windows swapchain unavailable、长期 GPU 工具采样                                                                      | P0 blocker；不得等同于 Live2D renderer                    |
| Windows input            | Raw Input、状态校正、lifecycle Reset、可靠队列、pointer 合并和系统合成丢 release 闭环已通过；`b6bbd73` 的 runner XInput smoke 完成 124 次无错误查询并干净关闭                                                                                                                                                                                                                                                                                                                                                                        | PixPin、Win+L、PrintScreen、UAC、管理员差异、10 分钟物理压力、物理手柄；runner `peak_connected=0`                                                                   | P0 blocker；CI 合成/无设备 API smoke 不能替代             |
| macOS input              | CGEventTap、两次缺失校正、受控 disable/lifecycle、100-cycle restart、cursor 合并已通过；keyboard、modifier 与 mouse synthetic release-loss 均进入真实 callback 并恢复，modifier/mouse 各通过 20-cycle；`a4fab65` 的双 CI 原生 job 通过 30+5 项 GameController contract/report test，本机无设备 framework smoke 干净关闭                                                                                                                                                                                                              | TCC deny/grant/revoke、自然 timeout、真实 session/power、物理键鼠/手柄                                                                                              | P0 blocker；synthetic callback 不替代物理/系统丢事件      |
| Cubism source/license    | Native `5-r.4.1`/Core `05.01.0000`、archive SHA-256、官方来源和目标 artifact 路径已固定；SDK 保持仓库外                                                                                                                                                                                                                                                                                                                                                                                                                              | 书面发布授权、Rust Framework 实现边界、notice/redistribution 结论                                                                                                   | stable 发布 blocker；不阻止本地实现                       |
| Cubism ABI/model         | 三个预置 model3/cdi3、6 个 motion3、15 个 exp3、13 个本机历史 physics3 与合成 pose3/userdata3 已由 Rust 静态 parser 验证；历史 physics 只保留匿名聚合，cdi3 计数和 legacy Core baseline 一致                                                                                                                                                                                                                                                                                                                                         | Native R5 binding、ABI、Core ID 对照、Moc/Model owner、drawable、Framework 求值、授权 physics/pose fixture、100 次切换                                              | P0 blocker；历史/合成结构验证不替代授权 fixture/Core/求值 |
| Targets/toolchain        | Windows 仅 x64/ARM64，i686 已排除；当前 Rust/GPUI/macOS 工具链和双 Windows target check 有记录                                                                                                                                                                                                                                                                                                                                                                                                                                       | Windows 实机 MSVC/SDK、最低 macOS toolchain、Intel 发布形式、Windows ARM64 desktop Core                                                                             | P0 blocker；矩阵保持 Provisional                          |
| Config/runtime contracts | 强类型 bounded runtime、revision、shutdown、双环境 resolver/lock/atomic recovery 已通过 spike                                                                                                                                                                                                                                                                                                                                                                                                                                        | 产品 workspace 的 build-time environment、统一 runtime owner 与完整 config service                                                                                  | ADR-0011 授权立即进入 Phase 1 实现                        |

## 3. External Actions

以下工作不能由仓库内代码或无人值守 CI 代替：

| Owner              | Required action                                                                                                                   | Evidence to return                                              | Failure decision                                         |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------- |
| Maintainer         | 已在仓库外提供 `CubismSdkForNative-5-r.4.1.zip`；后续独立复核并在发布选型冻结时重新确认是否升级                                   | 当前 archive/header/Core hashes；第二人或第二机器复核           | 不阻止本地实现；未复核或版本未冻结则阻止发布             |
| Maintainer / legal | 向 Live2D 书面确认 Expandable Application、MIT Rust 调用 Core、Framework 行为依据、binding 和分发/attribution                     | 可归档的书面答复与适用协议                                      | 禁止发布或禁止可行 Rust 实现时，对当前方案给出 NO-GO     |
| Windows tester     | 在 Windows 10/11 实机完成 PixPin、Win+L、PrintScreen、UAC、管理员/非管理员、DPI/display/GPU 和物理 XInput 矩阵                    | commit、OS/build、硬件/driver、运行命令、匿名计数和原始工具输出 | issue #47 残留或 renderer 不可恢复时保持 NO-GO candidate |
| macOS tester       | 在目标 macOS 版本补齐 Apple 拼音/物理键盘、TCC deny/grant/revoke、VoiceOver、锁屏/睡眠/用户切换、显示器和物理 GameController 矩阵 | commit、OS、CPU/GPU、权限前后状态、匿名计数和辅助技术结果       | 权限/输入不可恢复或 AX 不可用时触发 GPUI/input 选型复评  |
| Intel Mac owner    | 验证 `x86_64-apple-darwin` 的 GPUI、Metal、CGEventTap、Core、签名和打包                                                           | 原生 Intel 构建/运行记录                                        | 无设备或失败时明确停止 Intel 首发，不以 Rosetta 冒充     |

## 4. Engineering Queue

外部动作等待期间，仓库内工作按以下顺序继续：

1. 建立正式 runtime/config/app 最小闭环，并将已验证的 contract 从 spike 提升为产品测试；
2. 对已提供 SDK 运行 offline inspector、raw binding 和 macOS arm64/Windows x64 ABI smoke，所有生成物保持仓库外；
3. 建立最小 Moc/Model safe owner，读取三个预置 moc 的 drawable 数据并验证 100 次析构；
4. 以同一不可变 RenderSnapshot 在 D3D11/Metal 绘制 texture/order/alpha/mask，完成至少一个输入到绘制闭环；
5. 取得可分发授权的 physics3/pose3 fixture，并完成 GPUI/TCC/issue #47/物理设备矩阵；
6. 冻结 target/toolchain 和发布授权，随后发布最终 Phase 0/发布决议。

## 5. Decision Rules

最终评审只能使用以下三种结论：

- `IMPLEMENTATION GO WITH RELEASE CONDITIONS`：允许按 TODO Phase 1 建立产品 workspace；未完成条件必须有 owner、最迟阶段和失败回退，且不能据此宣称可发布；
- `GO`：所有 P0 和发布 blocker 已解除，可以进入 stable 发布准备；
- `GO WITH CONDITIONS`：仅剩不影响目标发布的受控条件，每项必须有 owner、最迟阶段和失败回退；
- `NO-GO`：Cubism 授权/独立 Rust 实现不可行、GPUI 无法满足双平台基础输入/辅助功能、输入最终一致性或原生 overlay 无法达到门槛。

在当前状态下，不允许把“合成测试通过”“CI 编译通过”“无设备 API smoke”或“旧 Web Core baseline”替代对应实机、厂商二进制与书面授权证据；这些缺口不再阻止无关产品模块的实现。
