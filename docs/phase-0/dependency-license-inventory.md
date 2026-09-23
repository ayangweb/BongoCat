# Phase 0 Dependency License Inventory

状态：Native workspace 与 spike 依赖许可证/来源策略已自动化；ADR-0056 的临时 git patch 使当前 `sources` check 预期失败
日期：2026-09-23

## Scope

`deny.toml` 使用 `cargo-deny 0.20.2` 扫描正式 `Cargo.toml`、所有
`spikes/*/Cargo.toml` 与 `tools/cubism-bindgen/Cargo.toml`，并以已提交的 lockfile 为输入。检查目标是：

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

扫描 package 节点数由当前 lockfile 与 target filter 动态决定，不作为需要手工维护的 golden value。2026-08-29 已先完成所有 Native Rewrite 直接依赖的最新稳定版审计和 lockfile 更新；ADR-0056 的 `gpui-kit` transition patch 和 lockfile 来源在 `rust-dependency-versions.md` 中单独记录。

## Direct dependencies

| Dependency family                | Locked version                 | License                   | Role                                     |
| -------------------------------- | ------------------------------ | ------------------------- | ---------------------------------------- |
| GPUI Kit                         | `0.6.6`                        | Apache-2.0                | Formal settings UI facade and components; root uses ADR-0056 transition patch |
| AccessKit core/macOS/Windows     | `0.25.0` / `0.27.0` / `0.35.0` | MIT OR Apache-2.0         | Direct in `spikes/gpui-settings`; formal UI receives `gpui-kit` transitive semantics after ADR-0054 |
| arboard                          | `3.6.1`                        | MIT OR Apache-2.0         | Dual-platform private text clipboard     |
| raw-window-handle                | `0.6.2`                        | MIT OR Apache-2.0 OR Zlib | GPUI/Win32 native window boundary        |
| opener                           | `0.8.5`                        | MIT OR Apache-2.0         | System default path/URL opener and reveal |
| async-channel                    | `2.5.0`                        | MIT OR Apache-2.0         | Formal typed command/reply and spike     |
| unicode-segmentation             | `1.13.3`                       | MIT OR Apache-2.0         | Grapheme-safe text editing               |
| futures-lite                     | `2.6.1`                        | MIT OR Apache-2.0         | Test-only executor bridge                |
| dirs                             | `6.0.0`                        | MIT OR Apache-2.0         | Config path spike                        |
| atomic-write-file                | `0.3.1`                        | BSD-3-Clause              | Native config atomic replacement         |
| serde / serde_json               | `1.0.229` / `1.0.151`          | MIT OR Apache-2.0         | Config, model and tool serialization     |
| tempfile                         | `3.27.0`                       | MIT OR Apache-2.0         | Isolated config/model fixture tests      |
| core-graphics2 / core-foundation | `0.6.1` / `0.10.1`             | MIT OR Apache-2.0         | macOS input boundary spike               |
| objc2-core-graphics / foundation | `0.3.2` / `0.3.2`              | Zlib OR Apache-2.0 OR MIT | formal macOS input adapter               |
| objc2 / block2 family            | `0.6.4` / `0.3.2`              | MIT OR Apache-2.0 / Zlib  | macOS overlay/input lifecycle            |
| objc2 (GPUI AX compatibility)    | `0.5.2`                        | MIT                       | Historical spike input for AccessKit macOS adapter objects |
| metal / core-graphics-types      | `0.33.0` / `0.2.0`             | MIT OR Apache-2.0         | macOS transparent present spike          |
| windows                          | `0.62.2`                       | MIT OR Apache-2.0         | Windows Raw Input boundary spike         |
| bindgen                          | `0.72.1`                       | BSD-3-Clause              | Offline Cubism raw binding generator     |
| embed-resource                   | `3.0.11`                       | MIT                       | Windows executable product icon compiler |
| sha2                             | `0.11.0`                       | MIT OR Apache-2.0         | Header/output provenance hashes          |
| tray-icon                        | `0.25.0`                       | MIT OR Apache-2.0         | Unified macOS/Windows system tray owner  |
| muda                             | `0.20.0`                       | Apache-2.0 OR MIT         | Native context menu and popup owner      |
| cargo-packager                   | `0.11.8`                       | Apache-2.0 OR MIT         | Bundle, installer and update signer      |
| cargo-packager-updater           | `0.2.3`                        | Apache-2.0 OR MIT         | Update manifest/verify/install engine    |
| minisign / minisign-verify       | `0.7.9` / `0.2.5`              | MIT                       | Update payload signer and verifier       |

## Policy

允许的 SPDX 许可证为：`0BSD`、`Apache-2.0`、`Apache-2.0 WITH LLVM-exception`、`BSD-2-Clause`、`BSD-3-Clause`、`BSL-1.0`、`CC0-1.0`、`ISC`、`MIT`、`MIT-0`、`MPL-2.0`、`Unicode-3.0`、`Unlicense` 和 `Zlib`。这些许可证可与项目 MIT 源码并存，但发布产物仍必须保留各依赖要求的 license text 和 notice。

`MPL-2.0` 是 file-level weak copyleft，当前图中来自 `option-ext`、`dwrote` 和构建期 `cbindgen`。不修改这些 crate 时不要求 BongoCat 改用 MPL；若未来 fork 或修改其 MPL 文件，必须履行对应源码提供义务。

GPUI Kit 的 HTTP/TLS 传递图引入 `libbz2-rs-sys 0.2.5`（`bzip2-1.0.6`）和
`webpki-roots 1.0.9`（`CDLA-Permissive-2.0`）。`cargo-packager` 的 `ureq` 图还通过
`webpki-roots 0.26.11` 兼容转发到同一 `1.0.9` 根证书包，因此两个精确版本都建立
`CDLA-Permissive-2.0` 例外。`libbz2-rs-sys` 允许源代码和二进制再分发并要求保留版权、
条件与免责声明；`webpki-roots` 只携带 Mozilla CA 根证书数据，允许使用、修改和分享，
并要求分享数据时附带许可证文本。`deny.toml` 不把这两种许可证加入全局 allowlist。
这些依赖只服务于 GPUI Kit 与打包工具的私有 HTTP/compression/TLS 实现，不进入 BongoCat
业务 API；对应工具链停止携带这些版本后即可移除精确例外。

`cargo-deny list` 会为包含多选许可的 crate 展示所有标识。例如 `self_cell` 的表达式包含 `Apache-2.0 OR GPL-2.0`，`r-efi` 包含 `MIT OR Apache-2.0 OR LGPL-2.1-or-later`；策略通过允许的 Apache/MIT 分支满足表达式，没有全局允许 GPL/LGPL。

依赖来源基线仍是 crates.io index；unknown registry 会使检查失败。ADR-0056 为 `SettingGroup::variant()` 增加根目录临时 `[patch.crates-io]`，把 `gpui-kit` suite 固定到 `ayangweb/gpui-kit` rev `720aeef7b33f95fdefcf5e7dc6257308bc51aa22`。`deny.toml` 的 `allow-git = []` 尚未随该过渡 patch 放开，所以当前 `cargo deny check sources` 会对 GPUI Kit suite 报 `source-not-allowed`；待上游 release 包含 override 后删除 patch，恢复纯 registry 来源。

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
- 未来的更新、打包或尚未加入 workspace 的新依赖；
- 发布阶段的 SBOM 与 notice bundle 生成。

Cubism 版本、来源、hash、再分发条款和 attribution 必须在 `P0-CUBISM` 单独形成书面结论；完成前不得制作可公开分发的 Native Rewrite 安装包。

`cargo-packager 0.11.8`（Apache-2.0 OR MIT）是打包、bundle、installer 与更新载荷签名的唯一实现，
由 ADR-0033 引入、ADR-0034 扩展到签名。`cargo-packager-updater 0.2.3`（同一许可证）是它的消费端，
承担更新 manifest 获取、版本比较、下载、验签与安装，由 ADR-0034 引入并取代 `self_update 1.3.0`
（ADR-0029）。两者都以 `default-features = false` 精确 pin，只启用 `rustls-tls`。`minisign 0.7.9`
与 `minisign-verify 0.2.5`（均为 MIT）分别作为它们的传递依赖提供签名与验签；`minisign-verify`
是零依赖 crate。三个第三方库的类型、错误与配置都不进入 BongoCat runtime/UI 公共 API，替换边界
分别是 `crates/bongocat-packaging` 与 `bongocat-update` 的 `UpdateRuntime`。换库带来的能力损失
见 ADR-0034 的损失表。

AccessKit 由同一上游仓库维护。当前 `spikes/gpui-settings` 直接依赖 core 与双平台 adapter，正式 `bongocat-platform` 不再维护项目自有语义树、native bridge 或 action channel；ADR-0054 后设置 UI 只把 `gpui-kit` 传递提供的默认语义作为内部实现，不作为 BongoCat contract。`objc2 0.5.2` 是 spike 中 `accesskit_macos 0.27.0` 的 ABI 类型世代兼容例外；正式依赖只通过 `gpui-kit` suite 传递，AccessKit 切换代或 spike 退役后不再保留旧版本。

`arboard 3.6.1`（MIT OR Apache-2.0）由 1Password 维护，仅作为 `bongocat-platform` 私有文本剪贴板 adapter 的双平台实现，并关闭默认 `image-data` feature。其 `Clipboard` 与 `Error` 类型和系统错误文本不进入公共 API；项目仍只暴露自有 `Option<String>`/稳定错误码，并保留 macOS AppKit 主线程与 autorelease-pool 约束。

`tray-icon 0.25.0`（MIT OR Apache-2.0，Rust 1.90+）由 Tauri 项目维护，是 macOS/Windows
状态图标的 native owner；仅在 macOS/Windows 启用并关闭默认 features，避免 Linux
GTK/libappindicator 系统依赖。`muda 0.20.0`（Apache-2.0 OR MIT，Rust 1.90+）由同一项目维护，
直接负责共享菜单对象、菜单事件以及 overlay 窗口的右键弹出，并关闭 `gtk3`/`libxdo` 默认 features。
`tray-icon` 与 `muda` 必须由 Cargo 解析为同一个 `muda` package，避免菜单类型版本分裂。第三方
tray/menu 类型、平台句柄和错误只存在于 `bongocat-platform` 私有 adapter，不进入 runtime/UI 公共 API；
Windows 隐藏为库内 `NIS_HIDDEN`、销毁为 `NIM_DELETE`，macOS `NSStatusItem` 生命周期由库拥有。
overlay 右键弹出使用 overlay session 的真实 HWND/`NSView`，不借用托盘隐藏窗口。

## Future-Incompatibility

当前 stable Rust 报告 `block 0.1.6` 与 `proc-macro-error2 2.0.1` 将在未来版本成为硬错误。
ADR-0011 允许当前精确锁定 GPUI 图进入最小正式窗口用于本地开发和 CI；这不解除未来
Rust 工具链与 stable 发布门禁。依赖路径、rustc 诊断和替换策略见
`docs/phase-0/future-incompatibility.md`。
