# Native Rewrite Rust Dependency Version Audit

状态：所有直接依赖已使用 crates.io 最新稳定版；lockfile 已更新到上游约束允许的最新解析结果
日期：2026-08-29
Rust：`cargo 1.97.1`、`rustc 1.97.1`

## Scope

本次审计覆盖 Native Rewrite 的 9 个 `spikes/*` 独立 workspace 和 `tools/legacy-config-inspector`。仓库根 workspace、`src-tauri/` 和其插件属于历史行为对照，不是 Native Rewrite 的依赖图；Phase 0 不借依赖升级修改历史应用。

版本来源使用 crates.io stable release：

```text
cargo search <crate> --limit 1
cargo update --manifest-path <workspace>/Cargo.toml
cargo update --manifest-path <workspace>/Cargo.toml --dry-run --verbose
cargo tree --manifest-path <workspace>/Cargo.toml --invert <crate>@<version>
```

预发布版本、yanked 版本和未固定 git branch 不属于“最新稳定版”。直接依赖精确 pin，传递依赖由 `Cargo.lock` 固定。

## Direct Dependencies

| Crate                  | Pinned version | Result            |
| ---------------------- | -------------: | ----------------- |
| `async-channel`        |        `2.5.0` | 从 `1.9.0` 升级   |
| `block2`               |        `0.6.2` | 已是最新          |
| `core-foundation`      |       `0.10.1` | 已是最新          |
| `core-graphics-types`  |        `0.2.0` | 从 `0.1.3` 升级   |
| `core-graphics2`       |        `0.6.1` | 从 `0.4.1` 升级   |
| `dirs`                 |        `6.0.0` | 从 `5.0.1` 升级   |
| `futures-lite`         |        `2.6.1` | 已是最新          |
| `gpui`                 |        `0.2.2` | 已是最新          |
| `metal`                |       `0.33.0` | 从 `0.29.0` 升级  |
| `objc2`                |        `0.6.4` | 已是最新          |
| `objc2-app-kit`        |        `0.3.2` | 已是最新          |
| `objc2-foundation`     |        `0.3.2` | 已是最新          |
| `objc2-quartz-core`    |        `0.3.2` | 已是最新          |
| `serde`                |      `1.0.229` | 从 `1.0.228` 升级 |
| `serde_json`           |      `1.0.151` | 从 `1.0.149` 升级 |
| `tempfile`             |       `3.27.0` | 已是最新          |
| `unicode-segmentation` |       `1.13.3` | 已是最新          |
| `windows`              |       `0.62.2` | 从 `0.61.3` 升级  |

`windows 0.62.2` 删除了 `Error::from_win32()`；Win32 wrapper 已改为在失败调用后立即使用语义等价的 `Error::from_thread()`，避免清理 API 覆盖 thread last-error。

## Transitive Constraints

每个 workspace 都已执行完整 `cargo update`。这会升级所有满足现有依赖约束的传递包，但不能合法越过上游 crate 的 semver 或精确约束。

最新 `gpui 0.2.2` 的依赖图仍固定旧一代 `metal 0.29.0` 和 `core-graphics2 0.4.1`；overlay spike 自己使用的直接版本已分别升级到 `0.33.0`，输入 spike 自己使用 `core-graphics2 0.6.1`，因此 lockfile 中会同时存在两个 API generation。`cargo update --dry-run --verbose` 还报告以下 5 个有更新但被 GPUI 上游约束阻止的兼容版本：

| Locked through GPUI      | Available | Owner path                    |
| ------------------------ | --------- | ----------------------------- |
| `cocoa 0.26.0`           | `0.26.1`  | `gpui 0.2.2`                  |
| `cocoa-foundation 0.2.0` | `0.2.1`   | `gpui 0.2.2` / `cocoa`        |
| `core-foundation 0.10.0` | `0.10.1`  | `gpui` macOS dependency graph |
| `generic-array 0.14.7`   | `0.14.9`  | `gpui_http_client -> sha2`    |
| `taffy 0.9.0`            | `0.9.2`   | `gpui 0.2.2`                  |

这些版本不能通过手改 lockfile、`cargo update --precise` 或本地 patch 安全升级。解除方式是 GPUI 发布兼容的新版本后升级 GPUI 并重跑双平台 UI/overlay smoke；不为追求表面版本一致而 fork 上游。

## Verification

所有 10 个 workspace 均完成 locked format、Clippy、test 和 release check；无依赖的 contract workspace 同样重新生成/检查 lockfile。附加平台验证包括：

- `windows 0.62.2` 在 `x86_64-pc-windows-msvc` 完成 check 与 Clippy；真实 Windows 注册和生命周期 smoke 由 push CI 执行；
- `core-graphics2 0.6.1` 在已授予 Input Monitoring 的 macOS 会话创建 listen-only tap，完成 lifecycle Reset 和正常 shutdown；
- `metal 0.33.0` 创建透明 `CAMetalLayer`，完成两次 clear/present、隐藏/重显和自动退出；
- `async-channel 2.5.0` 的 GPUI 设置 spike 完成 revisioned snapshot、runtime shutdown 和自动退出；
- `dirs 6.0.0`、`serde 1.0.229` 与 `serde_json 1.0.151` 的配置/考古工具共 28 项测试通过；
- `cargo-deny 0.20.2` 对全部 10 个 workspace 的四目标 license/source policy 通过。

GPUI 图继续报告已单独建档的 `block 0.1.6` 和 `proc-macro-error2 2.0.1` future-incompatibility。两者本身已是各自当前最新版，升级直接依赖没有解除上游约束，产品 workspace 的禁止门槛保持不变。

## Future Additions

新增依赖时必须先核对当日最新稳定版并选用该版本。若最新版本与已确认 toolchain、target、许可证或安全边界冲突，提交必须同时记录实际选择、阻塞原因、上游解除条件和替换成本。新增或修改 manifest 后必须更新对应 lockfile，运行 license/source policy、format、Clippy、test 和目标平台 build。

`.github/dependabot.yml` 每周扫描这 10 个独立 workspace，并把更新目标固定为 `next`。扫描不包含根目录和 `src-tauri`，避免把历史行为对照混入 Native Rewrite 依赖 PR。自动 PR 仍必须通过双平台 CI 和人工 API/许可证评审，不能因版本号更新而自动合并。

版本最新不替代依赖审查。维护状态、许可证、unsafe 面积、平台覆盖和公共 API 泄漏仍按 `AGENTS.md` 的依赖规则独立验收。

CI 通过 Cargo 安装的 `cargo-deny` 也从 `0.18.3` 升级并精确固定到审计日最新稳定版 `0.20.2`；它不属于应用依赖图，但必须遵守相同的版本核对规则。
