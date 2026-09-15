# Rust Future-Incompatibility Gate

状态：macOS 输入产品路径已迁移；GPUI 开发图获准，未来工具链/stable 发布继续阻塞
日期：2026-08-31

## Findings

当前 stable Rust 对两个传递依赖报告 future-incompatibility：

| Package                   | Current path                                                   | Diagnostic                                                                | Future impact                           |
| ------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------- |
| `block 0.1.6`             | Phase 0 `core-graphics2`、GPUI/cocoa/metal、正式 Metal overlay | `static _NSConcreteStackBlock: Class` 使用不可构造类型，Rust issue #74840 | 未来 Rust 会把当前 warning 提升为错误   |
| `proc-macro-error2 2.0.1` | `gpui 0.2.2 -> stacksafe 0.1.4 -> stacksafe-macro`             | 私有 `extern crate proc_macro` 被公开 re-export，Rust issue #127909       | 未来 Rust 会把 E0365 warning 提升为错误 |

复现命令：

```text
cargo check --manifest-path spikes/input-macos/Cargo.toml --locked --future-incompat-report
cargo check --manifest-path spikes/gpui-settings/Cargo.toml --locked --future-incompat-report
cargo check --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked --future-incompat-report
```

在对应 workspace 目录执行 `cargo report future-incompatibilities --id 1 --package <name>@<version>` 可读取完整 rustc 诊断。report id 是本地 Cargo 状态，不作为长期稳定标识。

## Dependency Assessment

`block 0.1.6` 仍是 crates.io 最新版。`core-graphics2 0.4.1` 直接依赖它，2026-08-29 的最新 `core-graphics2 0.6.1` 也仍声明 `block = "0.1"`，单纯升级该 crate 不能解除风险。

macOS 输入产品边界不需要继续依赖 `core-graphics2`。正式 `bongocat-platform` 已在
2026-08-30 使用最新稳定 `objc2-core-graphics 0.3.2`/`objc2-core-foundation 0.3.2`
重建窄边界，并以同进程 CGEventTap→runtime down/up、runtime 提前停止清理和 tap 立即重建
集成测试验证。`core-graphics2` 只保留在隔离的 Phase 0 spike，不进入正式产品 workspace。

GPUI 0.2.2 自身和 macOS 图中的 cocoa/metal/core-video 仍会引入 `block 0.1.6`，因此只替换输入 binding 不能清除 GPUI 图的 warning。

`proc-macro-error2 2.0.1` 也是当前最新版。最新 `stacksafe 1.0.3` 已不再依赖它，但 GPUI 0.2.2 固定兼容 `stacksafe 0.1`，Cargo 不能把该边自动升级到 1.x。解除风险需要 GPUI 发布更新后的依赖约束，或对相关 crate 做单独审计和可复现 patch。

## Decision

- Phase 0 spike 可以继续使用已锁定依赖，因为当前 stable Rust、双平台 CI 和既有 smoke 仍通过。
- 当前 warning 不得作为 stable 发布或未来不兼容 Rust 工具链的长期接受项，也不得仅用
  `--cap-lints`、忽略 warning 或未审阅 git fork 隐藏。
- macOS 输入产品实现已迁移到 `objc2-core-graphics`；合成 tap/callback/runtime/shutdown
  闭环已通过，TCC 撤销、物理输入、系统自然 timeout 和生命周期实机矩阵继续作为发布门禁。
- ADR-0011 允许精确锁定的 GPUI 0.2.2 图进入正式 `bongocat-ui` 最小窗口，用于不公开
  分发的本地开发和 CI；正式图必须持续执行 Windows/macOS locked build、Clippy、test
  和 future-incompatibility check。若当前 stable 尚只报告 warning，可继续功能开发；若
  项目 Rust toolchain 将其提升为错误，则该 toolchain 升级与 stable 发布保持阻塞。
- 若上游版本未及时解除依赖，允许的下一步是形成独立 patch 评审：记录来源、diff、许可证、维护责任和退出版本；不得在本结论中预先批准 patch。

这项结论不解除 ADR-0009 的 GPUI accessibility P0 gate，也不表示 GPUI 已获 stable 发布
GO。开发期接受的是可观测、精确锁定且有退出条件的上游风险，不是永久接受 warning。
