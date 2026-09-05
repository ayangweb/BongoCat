# ADR-0002: GPUI for Settings UI

状态：Accepted, pending Phase 0 validation
日期：2026-08-28

## Context

BongoCat 需要一套跨 Windows/macOS、可主题化、支持复杂表单和模型管理的 Rust 设置界面。设置 UI 不是实时 Live2D 渲染表面。

## Decision

使用 GPUI 构建设置、模型管理、快捷键、权限、更新和诊断界面。首个 spike 固定 `gpui = "=0.2.2"` 并提交 `Cargo.lock`。

GPUI `Entity` 只拥有临时视图状态。运行时配置和业务状态通过强类型 command/snapshot 边界访问。

## Consequences

- 项目维护一套设置 UI 和 design system。
- 不依赖 Zed 应用内部 UI crate 或 GPUI renderer 私有接口。
- GPUI 升级必须独立验证，不能无约束跟随上游。
- GPUI 设置窗口关闭后，输入、动画和 overlay 必须继续运行。
- GPUI 0.2.2 的 Windows `WM_DESTROY` 回调会同步重入已借用的 `AsyncApp`；在固定上游修复前，
  普通 close 只隐藏并保留唯一窗口，显式 Quit 必须先完成全部 BongoCat owner 的 shutdown/join，
  再由平台适配器跳过有缺陷的最终 GPUI 析构。不得用进程退出代替或提前截断业务 shutdown。

## Verification

Phase 0 验证双平台字体、中文输入法、焦点、键盘导航、辅助功能、DPI/Retina、窗口重建、后台生命周期和 shutdown。

2026-08-31 的 Windows CI run `33328391234`、job `99302481796` 证明保留窗口后的真实
`WM_DESTROY` 仍以 `0xC0000409` fast-fail；同期检查确认 Zed commit
`399258feeaf90ad8a3a208c99221ee87b6452f38` 的 `main` close callback 仍执行同步
`handle.update`。该证据限定上述兼容措施，并不解除未来恢复正常 GPUI 析构的门禁。
