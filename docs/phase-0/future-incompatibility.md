# Rust Future-Incompatibility Gate

状态：Phase 0 spike 可继续；产品 workspace 不接受当前依赖图
日期：2026-08-29

## Findings

当前 stable Rust 对两个传递依赖报告 future-incompatibility：

| Package                   | Current path                                       | Diagnostic                                                                | Future impact                           |
| ------------------------- | -------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------- |
| `block 0.1.6`             | `core-graphics2`, GPUI/cocoa/metal                 | `static _NSConcreteStackBlock: Class` 使用不可构造类型，Rust issue #74840 | 未来 Rust 会把当前 warning 提升为错误   |
| `proc-macro-error2 2.0.1` | `gpui 0.2.2 -> stacksafe 0.1.4 -> stacksafe-macro` | 私有 `extern crate proc_macro` 被公开 re-export，Rust issue #127909       | 未来 Rust 会把 E0365 warning 提升为错误 |

复现命令：

```text
cargo check --manifest-path spikes/input-macos/Cargo.toml --locked --future-incompat-report
cargo check --manifest-path spikes/gpui-settings/Cargo.toml --locked --future-incompat-report
cargo check --manifest-path spikes/gpui-overlay-macos/Cargo.toml --locked --future-incompat-report
```

在对应 workspace 目录执行 `cargo report future-incompatibilities --id 1 --package <name>@<version>` 可读取完整 rustc 诊断。report id 是本地 Cargo 状态，不作为长期稳定标识。

## Dependency Assessment

`block 0.1.6` 仍是 crates.io 最新版。`core-graphics2 0.4.1` 直接依赖它，2026-08-29 的最新 `core-graphics2 0.6.1` 也仍声明 `block = "0.1"`，单纯升级该 crate 不能解除风险。

macOS 输入产品边界不需要继续依赖 `core-graphics2`。当前依赖图已有 `objc2-core-graphics 0.3.2`，其公开绑定包含 `CGEventTapCreate`、`CGEventTapEnable`、`CGPreflightListenEventAccess`、`CGRequestListenEventAccess` 和 `CGEventSourceKeyState`。Phase 1 平台模块应使用 `objc2-core-graphics`/`objc2-core-foundation` 重建当前 spike 的窄 safe wrapper。

GPUI 0.2.2 自身和 macOS 图中的 cocoa/metal/core-video 仍会引入 `block 0.1.6`，因此只替换输入 binding 不能清除 GPUI 图的 warning。

`proc-macro-error2 2.0.1` 也是当前最新版。最新 `stacksafe 1.0.3` 已不再依赖它，但 GPUI 0.2.2 固定兼容 `stacksafe 0.1`，Cargo 不能把该边自动升级到 1.x。解除风险需要 GPUI 发布更新后的依赖约束，或对相关 crate 做单独审计和可复现 patch。

## Decision

- Phase 0 spike 可以继续使用已锁定依赖，因为当前 stable Rust、双平台 CI 和既有 smoke 仍通过。
- 当前 warning 不得作为产品 workspace 的长期接受项，也不得仅用 `--cap-lints`、忽略 warning 或未审阅 git fork 隐藏。
- macOS 输入产品实现必须迁移到 `objc2-core-graphics`；迁移后重新执行权限、tap、callback、校正和 shutdown 实机测试。
- GPUI 保持 provisional。进入产品 workspace 前，选定图必须消除 `block 0.1.6` 与 `proc-macro-error2 2.0.1`，并通过 Windows/macOS locked build、Clippy、test 和 future-incompatibility check。
- 若上游版本未及时解除依赖，允许的下一步是形成独立 patch 评审：记录来源、diff、许可证、维护责任和退出版本；不得在本结论中预先批准 patch。

这项结论关闭的是“是否接受当前风险”的调研任务，答案为“不接受进入产品”。它不解除 ADR-0009 的 GPUI accessibility P0 gate，也不表示 GPUI 已获最终 GO。
