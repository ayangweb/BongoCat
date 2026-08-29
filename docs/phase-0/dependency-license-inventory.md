# Phase 0 Dependency License Inventory

状态：Native spike 依赖许可证与来源策略已自动化
日期：2026-08-29

## Scope

`deny.toml` 使用 `cargo-deny 0.20.2` 扫描所有 `spikes/*/Cargo.toml` 与 `tools/legacy-config-inspector/Cargo.toml`，并以已提交的 lockfile 为输入。检查目标是：

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-pc-windows-msvc`
- `x86_64-pc-windows-msvc`

扫描 package 节点数由当前 lockfile 与 target filter 动态决定，不作为需要手工维护的 golden value。2026-08-29 已先完成所有 Native Rewrite 直接依赖的最新稳定版审计和 lockfile 更新，详见 `rust-dependency-versions.md`。

## Direct dependencies

| Dependency family                | Locked version        | License                  | Role                                 |
| -------------------------------- | --------------------- | ------------------------ | ------------------------------------ |
| GPUI                             | `0.2.2`               | Apache-2.0               | Settings UI spike                    |
| async-channel                    | `2.5.0`               | MIT OR Apache-2.0        | Typed command/reply spike            |
| unicode-segmentation             | `1.13.3`              | MIT OR Apache-2.0        | Grapheme-safe text editing           |
| futures-lite                     | `2.6.1`               | MIT OR Apache-2.0        | Test-only executor bridge            |
| dirs                             | `6.0.0`               | MIT OR Apache-2.0        | Config path spike                    |
| serde / serde_json               | `1.0.229` / `1.0.151` | MIT OR Apache-2.0        | Config, model and tool serialization |
| tempfile                         | `3.27.0`              | MIT OR Apache-2.0        | Isolated config/model fixture tests  |
| core-graphics2 / core-foundation | `0.6.1` / `0.10.1`    | MIT OR Apache-2.0        | macOS input boundary spike           |
| objc2 / block2 family            | `0.6.4` / `0.3.2`     | MIT OR Apache-2.0 / Zlib | macOS overlay/input lifecycle        |
| metal / core-graphics-types      | `0.33.0` / `0.2.0`    | MIT OR Apache-2.0        | macOS transparent present spike      |
| windows                          | `0.62.2`              | MIT OR Apache-2.0        | Windows Raw Input boundary spike     |

## Policy

允许的 SPDX 许可证为：`0BSD`、`Apache-2.0`、`Apache-2.0 WITH LLVM-exception`、`BSD-2-Clause`、`BSD-3-Clause`、`BSL-1.0`、`CC0-1.0`、`ISC`、`MIT`、`MIT-0`、`MPL-2.0`、`Unicode-3.0`、`Unlicense` 和 `Zlib`。这些许可证可与项目 MIT 源码并存，但发布产物仍必须保留各依赖要求的 license text 和 notice。

`MPL-2.0` 是 file-level weak copyleft，当前图中来自 `option-ext`、`dwrote` 和构建期 `cbindgen`。不修改这些 crate 时不要求 BongoCat 改用 MPL；若未来 fork 或修改其 MPL 文件，必须履行对应源码提供义务。

`cargo-deny list` 会为包含多选许可的 crate 展示所有标识。例如 `self_cell` 的表达式包含 `Apache-2.0 OR GPL-2.0`，`r-efi` 包含 `MIT OR Apache-2.0 OR LGPL-2.1-or-later`；策略通过允许的 Apache/MIT 分支满足表达式，没有全局允许 GPL/LGPL。

依赖来源只允许 crates.io index。unknown registry 和任何 git dependency 均会使检查失败；新的 git source 必须先形成明确评审结论，不能通过宽泛 organization allowlist 绕过。

## Reproduction

```text
cargo install cargo-deny --version 0.20.2 --locked
./tools/check-native-dependencies.sh
```

检查脚本会拒绝其他 `cargo-deny` 版本，然后对每个独立 workspace 执行 locked license/source check。GitHub Actions 的 `Check Native dependency policy` job 使用同一命令。

## Boundaries

本结论覆盖当前 Native Rust spike 的 crate graph，不包括：

- 历史 Tauri/Vue 产品的发布依赖；
- 官方 Cubism Core 二进制、SDK 资源和 attribution；
- 未来的音频、更新、打包或产品 workspace 新依赖；
- 发布阶段的 SBOM 与 notice bundle 生成。

Cubism 版本、来源、hash、再分发条款和 attribution 必须在 `P0-CUBISM` 单独形成书面结论；完成前不得制作可公开分发的 Native Rewrite 安装包。

## Future-Incompatibility

当前 stable Rust 报告 `block 0.1.6` 与 `proc-macro-error2 2.0.1` 将在未来版本成为硬错误。两者仅允许保留在 Phase 0 spike lockfile 中，不获准直接进入产品 workspace。依赖路径、rustc 诊断、替换策略和产品门禁见 `docs/phase-0/future-incompatibility.md`。
