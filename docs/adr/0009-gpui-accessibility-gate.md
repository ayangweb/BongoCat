# ADR-0009: GPUI Accessibility Gate

状态：Proposed, P0 blocker
日期：2026-08-28

## Context

设置 UI 必须向 macOS 辅助功能 API 暴露可识别的 label、value、role、错误和进度。`spikes/gpui-settings/` 在 macOS 26.5.2 上的人工 smoke 能识别应用、标准窗口、标题栏按钮和菜单，但不能识别 GPUI 绘制的 `Appearance`、`Theme`、模型名称输入框或 `Refresh` 控件。可见截图不能替代辅助功能树证据。

GPUI 0.2.2 的公开源码和 feature 列表没有为普通 element 提供 role/label/value 的通用 accessibility API。直接依赖私有 Zed UI crate、修改 GPUI 私有 renderer 或以隐藏原生控件伪造整棵表单，都会破坏已冻结的依赖和 UI 边界。

## Decision

GPUI 继续作为当前首选设置 UI，但在辅助功能 gate 通过前保持 provisional：

- 不建立完整产品 UI workspace，不把当前 spike 宣称为可发布设置界面；
- 不引入 GPUI 私有 API、未维护 fork 或隐藏控件 workaround；
- 使用 GPUI 公开的 raw window handle 安装项目自有 AccessKit semantic tree；macOS/Windows adapter 只负责系统协议，语义 snapshot 由 UI crate 维护；
- 辅助技术 action 必须通过有界强类型 channel 回到 GPUI 主线程，不允许平台 callback 直接修改 Entity 或 runtime；
- 可见控件和语义节点使用同一份 UI 状态，必须覆盖 role、label/title、value、focus、selected、loading/error 和支持的 action；
- 若 GPUI 仍无法提供基础语义，才启动 Iced 替代 spike。Iced 只作为此门槛的 fallback，不与 GPUI 并行进入产品。

截至本 ADR 日期，Iced 0.14.0 的公开 crate feature/source 中也未发现可直接满足 role/label/value 的通用 accessibility API，因此它是待验证候选，不是当前 go 结论。不能因为换 UI 框架名称就视为问题已解决。

## Gate

只有满足以下条件才能将本 ADR 标记 Accepted 并解除 P0 阻塞：

1. 选定版本的公开 API 能为设置表单提供稳定 role、label、value、错误和进度节点；
2. macOS AX tree 能读取主题选择、模型名称输入框、Refresh、错误和 loading 状态；
3. Windows UI Automation 能读取同等语义；
4. 键盘导航、真实 IME、剪贴板、缩放和窗口重建回归仍通过；
5. 方案不依赖私有 crate、未审阅 patch 或平台特定的业务状态副本。

若以上条件在进入 Phase 1 前仍不满足，形成明确的 `NO-GO` 或 `GO WITH CONDITIONS` 决策，并为 Iced 或其他后续方案指定独立 owner、截止阶段和回退条件。

## Evidence

- GPUI spike 的 AX 观察、环境和截图限制：`docs/phase-0/gpui-settings-spike.md`；
- GPUI 版本固定为 crates.io `0.2.2`，见 `spikes/gpui-settings/Cargo.toml`；
- macOS 本机通过 AppKit AX API 读取 8 个项目语义节点，并通过 `accessibilityPerformPress` 将 Dark radio action 送回 GPUI；
- commit `fd9ad85` 的 push run `33255204781`、job `99107586036` 使用进程外 UI Automation client 读取同等节点，并通过 `SelectionItem.Select` 验证 Dark radio 的 selected/action 闭环；
- Iced 候选版本与 feature 清单通过 `cargo info iced@0.14.0 --verbose` 获取，源码检索未发现通用 accessibility surface。
