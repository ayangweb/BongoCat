# Rust Dependency Version Audit

状态：所有直接依赖已使用 crates.io 最新稳定版、精确上游 revision 或已记录的 ABI/transition 例外；lockfile 已更新到上游约束允许的最新解析结果
日期：2026-09-28（全量升级审计：`dirs` `6.0.0`→`7.0.0`、`tray-icon` `0.25.0`→`0.25.1`、`rust-i18n` `4.2.2`→`4.2.3`、`accesskit` `0.25.0`→`0.25.1`、`accesskit_macos` `0.27.0`→`0.27.1`、`accesskit_windows` `0.35.0`→`0.35.1`、`bindgen` `0.72.1`→`0.73.2`；`spikes/gpui-settings` 的 `objc2`/`objc2-foundation` 直接 pin 由 `0.5.2`/`0.2.2` 升到 `0.6.4`/`0.3.2` 并重新验证；`ayangweb/gilrs` 固定 commit 仍为 `e69f1083d1a13a234513cb360c3d9d8abe5ea025`（fork `master` 无新 commit）；上次全量审计 2026-09-13）
Rust：`cargo 1.97.1`、`rustc 1.97.1`

## Scope

本次审计覆盖仓库根目录的正式 workspace，以及 `spikes/*` 和 `tools/*` 下列出的独立 workspace。
历史 Vue/Tauri workspace 已退役至远端 `pre-refactor-tauri` 分支，不属于 BongoCat 的依赖图。

版本来源使用 crates.io stable release：

```text
cargo search <crate> --limit 1
cargo update --manifest-path <workspace>/Cargo.toml
cargo update --manifest-path <workspace>/Cargo.toml --dry-run --verbose
cargo tree --manifest-path <workspace>/Cargo.toml --invert <crate>@<version>
```

预发布版本、yanked 版本和未固定 git branch 不属于“最新稳定版”。直接依赖精确 pin，传递依赖由 `Cargo.lock` 固定；`deny.toml` 的 `required-git-spec = "rev"` 进一步拒绝 branch/tag git source。直接依赖中只剩一个固定 commit 例外：`gilrs` 使用维护者 fork 统一承接手柄兼容、backend queue 与平台生命周期修复，不能以浮动 branch 跟随；`gpui-kit` 已在 2026-09-28 恢复为纯 registry 精确 pin。

## Direct Dependencies

| Crate                                 | Pinned version | Result                                                   |
| ------------------------------------- | -------------: | -------------------------------------------------------- |
| `accesskit`                           |       `0.25.1` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `accesskit_macos`                     |       `0.27.1` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `accesskit_windows`                   |       `0.35.1` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `arboard`                             |        `3.6.1` | 剪贴板 adapter 新增时即为最新                            |
| `async-channel`                       |        `2.5.0` | 从 `1.9.0` 升级                                          |
| `atomic-write-file`                   |        `0.3.1` | 配置与更新 sequence 存储新增时最新                       |
| `bindgen`                             |       `0.73.2` | 从 `0.72.1` 升级；已重生成合成 golden 与产品 raw bindings    |
| `block2`                              |        `0.6.2` | 已是最新                                                 |
| `core-foundation`                     |       `0.10.1` | 已是最新                                                 |
| `core-graphics-types`                 |        `0.2.0` | 从 `0.1.3` 升级                                          |
| `core-graphics2`                      |        `0.6.1` | 从 `0.4.1` 升级                                          |
| `dirs`                                |        `7.0.0` | 从 `6.0.0` 升级；`dirs-sys 0.5.0` 不变，平台路径解析无差异   |
| `embed-resource`                      |       `3.0.11` | Windows 产品图标新增时最新                               |
| `gpui-kit`                            |        `0.7.0` | crates.io 精确 pin；含 variant 与 Popover arrow，GPUI 快照升到 `0.3.7` |
| `gilrs`                               | `0.11.2` @ `fb3cc4e` | 维护者 fork 固定 revision；WGI/IOHID 与后续手柄修复统一维护    |
| `futures-lite`                        |        `2.6.1` | 已是最新                                                 |
| `gpui`                                |        `0.2.2` | spike 直接依赖；正式 UI 经 `gpui-kit` suite 传递              |
| `libc`                                |      `0.2.189` | 新增时即为最新稳定版                                     |
| `metal`                               |       `0.33.0` | 从 `0.29.0` 升级                                         |
| `objc2`                               |        `0.6.4` | spike 直接依赖与产品 platform adapter 共用同一代类型          |
| `objc2-app-kit`                       |        `0.3.2` | 已是最新                                                 |
| `objc2-core-foundation`               |        `0.3.2` | 正式输入边界新增时最新                                   |
| `objc2-core-graphics`                 |        `0.3.2` | 正式输入边界新增时最新                                   |
| `objc2-foundation`                    |        `0.3.2` | 已是最新                                                 |
| `objc2-foundation`（GPUI 原生 probe） |        `0.3.2` | spike 直接 pin 已与产品对齐；`accesskit_macos` 仍在图中带入 `0.2.2` 传递代 |
| `objc2-quartz-core`                   |        `0.3.2` | 已是最新                                                 |
| `objc2-service-management`            |        `0.3.2` | 启动项 adapter 新增时最新                                |
| `serde`                               |      `1.0.229` | 从 `1.0.228` 升级                                        |
| `serde_json`                          |      `1.0.151` | 从 `1.0.149` 升级                                        |
| `raw-window-handle`                   |        `0.6.2` | 新增时即为最新                                           |
| `schemars`                            |        `1.2.2` | 配置与窗口状态 JSON Schema 离线生成；MIT，Rust 1.74+       |
| `thiserror`                           |       `2.0.21` | 项目错误类型派生；MIT OR Apache-2.0，Rust 1.77+             |
| `time`                                |       `0.3.55` | 日志 UTC 日期/时间格式化；MIT OR Apache-2.0，Rust 1.88+   |
| `walkdir`                             |        `2.5.0` | Mver 源目录遍历；MIT/Unlicense，跨平台                    |
| `rfd`                                 |       `0.17.2` | 双平台目录选择迁移时最新稳定版                           |
| `rodio`                               |       `0.22.2` | motion 音效新增时最新                                    |
| `sha2`                                |       `0.11.0` | 新增时即为最新                                           |
| `tempfile`                            |       `3.27.0` | 已是最新                                                 |
| `muda`                                |       `0.20.0` | macOS/Windows 菜单与右键弹出 owner（ADR-0031）           |
| `tray-icon`                           |       `0.25.1` | macOS/Windows 统一托盘 owner；含 macOS 左键事件与 `set_title` 修复（ADR-0031） |
| `unicode-segmentation`                |       `1.13.3` | 已是最新                                                 |
| `url`                                 |        `2.5.8` | 外部 HTTPS URL wrapper 新增时最新                        |
| `cargo-packager-updater`              |        `0.2.3` | 更新库，取代 `self_update` 时最新（ADR-0034）            |
| `minisign`                            |        `0.7.9` | `cargo-packager` 的传递依赖，更新载荷签名                |
| `minisign-verify`                     |        `0.2.5` | `cargo-packager-updater` 的传递依赖，客户端验签          |
| `windows`                             |       `0.62.2` | 从 `0.61.3` 升级                                         |

`windows 0.62.2` 删除了 `Error::from_win32()`；Win32 wrapper 已改为在失败调用后立即使用语义等价的 `Error::from_thread()`，避免清理 API 覆盖 thread last-error。

`gpui-kit` 的来源经历了三步，全部记录在 ADR-0020 与 ADR-0056：`=0.6.6`（当时 crates.io 最新非 yanked 稳定版，不含 `SettingGroup::variant()` 与 `Popover::arrow()`）→ 上游固定 commit `500852f449c05dc01920ec82f3ae2656a61d0387`（package 元数据 `0.6.5`）→ 2026-09-28 恢复为 crates.io 精确 pin `=0.7.0`。v0.7.0 是首个同时发布这两项 API 的 release，因此临时 git source 及其 `deny.toml` 放行项一并删除；`deny.toml` 现在只放行 `ayangweb/gilrs`。lockfile 中五个 GPUI Kit suite package 统一解析为 `0.7.0`，`gpui-pre` 家族由 `0.3.6` 升到 `0.3.7`（v0.7.0 以 `=0.3.7` 精确 pin，`gpui-pre-reqwest` 仍为 `0.12.15`；Zed 快照由 `bcf6582ce3500df93a8a39366640173e6786cea6` 前移到 `1a28cff4b409169bac058bca40dfbfeb7621d19b`，`zed-version` 仍为 `0.2.2`）。相对上一个固定 revision，v0.7.0 对本仓库用到的 API 全部是增量：项目未使用 Chart/Plot、DatePicker、Table、Dock、Accordion、Attachment、Form、Command 或 Tree-sitter，因此本次升级不产生源码迁移。

### 已记录的上游阻塞：`tray-icon 0.25.1` 的 Windows `set_tooltip`

- 阻塞版本：`tray-icon 0.25.1`（crates.io 当日最新稳定版，无更新版本可升级）。
- 现象：Windows 上 `TrayIcon::set_tooltip` 在图标以固定 GUID 注册后必然返回错误。`set_icon` 与
  内部 `set_tray_visible` 都调用 `apply_guid`，只有 `set_tooltip` 未设置 `NIF_GUID`；shell 对以
  `guidItem` 标识的图标忽略 `uID`，并要求后续每次 `Shell_NotifyIcon` 调用携带同一 GUID
  （<https://learn.microsoft.com/windows/win32/api/shellapi/ns-shellapi-notifyicondataw#troubleshooting>）。
- 上游 owner：`tauri-apps/tray-icon`（`src/platform_impl/windows/mod.rs` 的 `TrayIcon::set_tooltip`）。
  2026-09-28 升级到 `0.25.1` 时已逐行核对 `tray-icon-v0.25.1` tag 的实现：`set_tooltip` 仍然只设置
  `NIF_TIP`、`hWnd` 与 `uID`，没有写入 `guidItem`，即该缺陷在 `0.25.1` 仍未修复。
- 影响与绕行：`bongocat-platform` 的 `system_menu_native` adapter 不再在 `set_presentation` 中调用
  `set_tooltip`，把 tooltip 固定为创建期输入（ADR-0031）。产品文案 `system_menu.title` 恒为
  `BongoCat`，运行期不变，因此不产生可见行为差异。
- 解除条件：上游 `set_tooltip` 补上 `apply_guid` 后，可恢复运行期 tooltip 更新；升级时按 ADR-0031
  的替换边界复验 GUID 注册下的 `set_tooltip` 行为。

`tray-icon 0.25.1` 相对 `0.25.0` 的其余三项变更与本项目相关且均为修复：改用 `dirs 7`（与本仓库直接
依赖同代）、macOS 27 上左键事件不再被吞掉（菜单改为仅在弹出期间附着到 status item，匹配本项目
`with_menu_on_left_click(cfg!(target_os = "macos"))` 的用法）、macOS `set_title(None)` 正确清除托盘标题。
本项目不使用 `set_title`，因此不产生行为变化；`just dev-smoke` 已在 macOS 上完成托盘创建与设置窗口
关闭/重开循环。

## Transitive Constraints

每个 workspace 都已执行完整 `cargo update`。这会升级所有满足现有依赖约束的传递包，但不能合法越过上游 crate 的 semver 或精确约束。

最新 `gpui 0.2.2` 的依赖图仍固定旧一代 `metal 0.29.0` 和 `core-graphics2 0.4.1`；overlay spike 自己使用的直接版本已分别升级到 `0.33.0`，输入 spike 自己使用 `core-graphics2 0.6.1`，因此 lockfile 中会同时存在两个 API generation。`cargo update --dry-run --verbose` 在根 workspace 仍只报告 1 个有更新但被上游约束阻止的兼容版本：

| Locked dependency      | Available | Owner path                 |
| ---------------------- | --------- | -------------------------- |
| `generic-array 0.14.7` | `0.14.9`  | `gpui_http_client -> sha2` |

这些版本不能通过手改 lockfile 或 `cargo update --precise` 安全升级；ADR-0056 的上游固定 revision 只用于取得已经合并的 GPUI Kit API，不用于越过这些上游约束。解除方式是 GPUI 发布兼容的新版本后升级 GPUI 并重跑双平台 UI/overlay smoke；不为追求表面版本一致而改写上游依赖图。

两个直接锁定 `gpui 0.2.2` 的历史 spike（`spikes/gpui-settings`、`spikes/gpui-overlay-macos`）在完整
`cargo update` 后还有 5 个被上游约束阻止的版本，全部由 `gpui 0.2.2` 自己的依赖图决定：

| Locked dependency          | Available | Owner path     |
| -------------------------- | --------- | -------------- |
| `cocoa 0.26.0`             | `0.26.1`  | `gpui 0.2.2`   |
| `cocoa-foundation 0.2.0`   | `0.2.1`   | `gpui 0.2.2`   |
| `core-foundation 0.10.0`   | `0.10.1`  | `gpui 0.2.2`   |
| `taffy 0.9.0`              | `0.9.2`   | `gpui 0.2.2`   |
| `generic-array 0.14.7`     | `0.14.9`  | `gpui -> sha2` |

注意 `spikes/input-macos` 自己直接使用 `core-foundation 0.10.1`（已是最新），因此该 spike 的
lockfile 里 0.10.0 与 0.10.1 并存；这是两个直接依赖世代的正常结果，不是未同步。

`dirs` 直接依赖升到 `7.0.0` 后，根 lockfile 内合法并存三代 `dirs`，全部由上游 semver 约束决定，没有任何手改 lockfile：

| Locked `dirs` | Owner path                                            |
| ------------- | ----------------------------------------------------- |
| `7.0.0`       | `bongocat-config`（项目直接依赖）                      |
| `6.0.0`       | `auto-launch 0.6.0`、`cargo-packager 0.11.8`          |
| `5.0.1`       | `cargo-packager-updater 0.2.3`                        |

`dirs 6.0.0` 与 `dirs 7.0.0` 依赖同一个 `dirs-sys 0.5.0`，因此本项目使用的 `dirs::data_dir()` 在
两个版本上解析到完全相同的平台路径（Windows Known Folder、macOS `~/Library/Application Support`），
不存在存储根迁移风险；`spikes/config-store` 的 `dirs` 同步升到 `7.0.0`。macOS 上 `just dev-smoke`
后复核 `~/Library/Application Support/com.ayangweb.bongo-cat/development/config.json` 仍被原地读写。
旧的两代只由 `auto-launch` 和打包/更新库自身使用，不经过本项目的存储层。

`rodio 0.22.2` 在审计日是 crates.io 最新非 yanked 稳定版（MIT OR Apache-2.0，
Rust 1.87+），但其 playback feature 约束 `cpal 0.17.x`，因此完整 `cargo update` 合法解析
为 `cpal 0.17.3`，而不是独立最新的 `0.18.2`。workspace 不直接依赖 CPAL；
rodio 仅在 Windows/macOS target 启用 `playback + flac`，录音及其它 codec feature 均关闭。
替换边界完全位于 `bongocat-audio` 私有 backend，不把第三方类型暴露到 runtime contract。

GPUI accessibility spike 的 `objc2 0.5.2` / `objc2-foundation 0.2.2` 直接 pin 已于 2026-09-28 移除，
改为与产品 platform adapter 相同的 `objc2 0.6.4` / `objc2-foundation 0.3.2`。原记录把这组 pin 描述为
“类型兼容例外”，理由是 `accesskit_macos` 的 adapter 公共对象由其 `objc2 0.5.x` 依赖构造，spike 必须
使用同一代 Rust Objective-C/Foundation 类型。重新核对 spike 源码后确认该理由对本 spike 不成立：

- `accesskit_macos::SubclassingAdapter::new` 的第一个参数是原始 `*mut c_void` NSView，
  `spikes/gpui-settings/src/accessibility.rs` 传入的是 `handle.ns_view.as_ptr()`；spike 与
  `accesskit_macos` 之间没有任何 objc2 类型跨界，只交换原始指针和 accesskit 自己的 trait 实现；
- spike 自身的 `objc2` 用法（`macos_menu.rs`、`accessibility.rs` 的 `msg_send!`/`AnyObject`，以及
  `platform_ui_probe.rs` 的 `class!`、`Retained`、`autoreleasepool`、`NSPoint::new`）全部是独立的
  原始指针代码，不引用 `accesskit_macos` 构造的 typed object。

升级后的验证方式是在仓库外建立一次性 probe crate，用与 spike 完全相同的 `accesskit_macos 0.27.1`
feature unification 和上述全部调用形状，分别以 `objc2 0.5.2`/`objc2-foundation 0.2.2` 与
`objc2 0.6.4`/`objc2-foundation 0.3.2` 编译，并跑 `cargo clippy --all-targets -- -D warnings`。
两代都零错误、零 warning，证明升级不引入类型世代不匹配。

`accesskit_macos 0.27.1` 仍声明 `objc2 ^0.5.1` / `objc2-foundation ^0.2.0`（0.27.1 相对 0.27.0 只同步
`accesskit`/`accesskit_consumer` patch 版本，未迁移 generation），因此 `spikes/gpui-settings` 的
lockfile 中旧一代 `objc2 0.5.2`、`objc2-foundation 0.2.2` 及配套 `objc2-app-kit`/`objc2-quartz-core`
等包作为纯传递依赖保留；项目代码不再直接依赖它们。该 spike 的完整 macOS 编译仍需完整 Xcode
提供 `metal` 编译器（历史 spike 直接锁定 `gpui 0.2.2`，其 build script 现场编译 shader），本次只能在
CI 或装有完整 Xcode 的 macOS 上完成，见「Verification」的未运行项。

## Verification

当前 dependency-policy 脚本覆盖根 workspace、12 个 `spikes/*` workspace 与 `tools/cubism-bindgen`，共 14 个 manifest；它们均以 locked license/source check 为目标。正式根 workspace 还执行三个首发 target 的 release dependency tree check。无依赖的 contract workspace 同样重新生成/检查 lockfile。

2026-09-28 本次升级实际执行的验证与未运行项：

- 已通过：`just check`（`cargo fmt --all -- --check`、三组 `cargo clippy -D warnings`、
  `cargo test --workspace`、`cargo check --workspace --release`）；`sh tools/check-dependencies.sh`
  （三个首发 target 的 release dependency tree + 14 个 manifest 的 `cargo deny check licenses sources`）；
  `just schema` 重新生成后无漂移；`tools/validate-fixtures.py`、`validate-json-schema.py`、
  `validate-locales.py`、`run-input-fixtures.py` 与 `python -m unittest discover -s tools/tests`
  （72 tests）；`tools/cubism-bindgen` 的 fmt/clippy/test/`check-fixtures`/release check，三个
  target 的 fixture 以 `rustc --emit metadata -D warnings` 单独编译；`tools/cubism-core-probe`
  对合成 bindings 的 clippy/release check；`bongocat-input`、`bongocat-platform`、`bongocat-overlay`
  在 `x86_64-pc-windows-msvc` 的 Clippy；macOS `just preview standard`（`frames=357`、
  `gpu_bytes_before == gpu_bytes_after`、`Live2D Cubism SDK Core Version 6.0.1`）与
  `just dev-smoke`（托盘创建 + 设置窗口关闭/重开）。
- 未运行（环境限制，非本次改动引入）：`spikes/gpui-settings` 与 `spikes/gpui-overlay-macos` 的
  macOS 编译。两者的历史锁定 `gpui 0.2.2` build script 需要完整 Xcode 的 `metal` 编译器，本机只有
  Command Line Tools，`xcrun --find metal` 失败；已在未改动的 `master` 上复现同一失败。这两个
  workspace 的 `cargo deny`、`cargo metadata --locked` 与 `objc2` 双代 probe 均已通过，但仍需 CI
  或装有完整 Xcode 的 macOS 补一次真实编译。
- 未运行：`spikes/input-macos`、`spikes/input-windows`、`spikes/overlay-windows` 的实机
  输入/GPU smoke，Windows 硬件 CI，以及 `just build` 的打包产物（需签名密钥与真实目标环境）。
  完整 workspace 的 `x86_64-pc-windows-msvc` Clippy 同样因本机缺少 Windows C 交叉工具链
  （`ring 0.17.14` 的 `cc` 步骤）无法运行，已在 `master` 上复现为既有失败。

附加平台验证包括：

- `windows 0.62.2` 同时封装 Raw Input、Win32 窗口与原生 overlay 边界；输入和 overlay crate 均在
  `x86_64-pc-windows-msvc` 完成 Check/Clippy。Windows 手柄已迁入 gilrs WGI，旧
  `Win32_UI_Input_XboxController` feature 与自维护 XInput DLL 加载已删除；
- `core-graphics2 0.6.1` 在已授予 Input Monitoring 的 macOS 会话创建 listen-only tap，完成 lifecycle Reset 和正常 shutdown；
- `objc2-core-graphics 0.3.2` 与 `objc2-core-foundation 0.3.2` 只存在于正式 macOS
  platform adapter，取代会为输入路径引入 `block 0.1.6` 的 `core-graphics2`；窄 wrapper
  管理 callback context、CFRunLoop source 和 tap 的统一析构，项目公共 API 仅暴露自有
  permission、diagnostics 和 error 类型。替换边界是 `MacInputService` 私有实现，不影响 runtime；
- `gilrs 0.11.2` / `gilrs-core 0.6.8` 固定 `ayangweb/gilrs` commit
  `e69f1083d1a13a234513cb360c3d9d8abe5ea025`，许可证 `Apache-2.0 OR MIT`，MSRV `1.84`。
  Windows `wgi` backend、macOS IOHID backend 和 fork 内置 SDL mapping 都只存在于
  `bongocat-platform` 私有 adapter；gilrs id/type/error 不进入 runtime/UI 公共 API，环境 mapping
  与 force feedback 关闭。fork 的 SDL_GameControllerDB submodule 固定为
  `15b5e9f4abfb1c5c691c468799816755a91a2e11`。该 commit 已提供 backend/high-level pending bounded queue/epoch、authoritative
  reset、WGI/XInput bounded shutdown acknowledgement、macOS callback ownership/close 和 xinput
  extreme-axis regression 修复；workspace `gilrs` dependency 保持 featureless，只有 Windows target table
  启用 `wgi`，macOS target 不再启用临时 `wgi`。BongoCat adapter 将 gilrs overflow/reset/shutdown
  结果映射为项目自有诊断；物理设备、焦点和长期生命周期证据仍是 ADR-0066 发布门禁。
- `objc2-service-management 0.3.2`（Zlib OR Apache-2.0 OR MIT，Rust 1.71+）来自持续维护
  `objc2` binding 集，只在 macOS platform adapter 以最小 `SMAppService`/Foundation feature
  调用 macOS 13+ main-app login item；运行时先检查 class availability，macOS 12 与 Development
  不触发 mutation。Objective-C/NSError 不离开 wrapper，替换边界是未来系统 API 或 binding
  变化时重写该 adapter，不影响 settings/runtime/config contract；
- 系统语言初始化只扩展现有平台 binding 的 feature：Windows `windows 0.62.2` 增加
  `Win32_Globalization` 并调用 `GetUserPreferredUILanguages`，macOS `objc2-foundation 0.3.2`
  增加 `NSLocale` 并调用 `preferredLanguages`。没有新增直接依赖，平台字符串立即规范化为项目
  `Language` 枚举，不向上泄漏 Win32/Foundation 类型。按规则执行完整 `cargo update` 后，
  `cc 1.5.1`、`find-msvc-tools 0.1.14`、`tinyvec 1.13.2` 和 `tokio-rustls 0.26.5` 在现有上游
  约束内更新；其余直接依赖版本不变；
- `metal 0.33.0` 创建透明 `CAMetalLayer`，完成两次 clear/present、隐藏/重显和自动退出；
- `libc 0.2.189` 只在 macOS overlay spike 的平台边界调用 `proc_pidinfo`，用于 100-cycle 线程/RSS 资源快照；许可证为 MIT OR Apache-2.0，停止使用该系统指标后可直接移除，不进入项目公共 API；
- `async-channel 2.5.0`（MIT OR Apache-2.0）已进入正式 `bongocat-ui`，只封装容量 16 的
  typed command/reply；第三方 sender/receiver 不进入 runtime/config API。正式 app service
  已验证 FIFO、receiver close、revisioned snapshot、配置持久化与 shutdown acknowledgement；
- `embed-resource 3.0.11`（MIT，Rust 1.76+）只在 `bongocat-app` build script 中调用系统
  resource compiler，把固定 `.ico` 编译进 Windows executable；它不进入运行时或公共 API，
  替换边界是未来安装器构建系统直接生成等价 `.res`。上游仓库默认分支持续维护 3.x，且该版本
  已作为 `gpui-pre` 的传递 build dependency 存在于 lockfile；本次将其精确固定为产品直接依赖。
- `gpui 0.2.2`（Apache-2.0）在 GPUI Kit suite 与 `spikes/gpui-settings` 中使用；正式 UI 只通过
  `gpui-kit` 根导出类型，Linux 共享协议不依赖 GPUI。替换边界位于 `bongocat-ui::window` 与 app
  主循环，runtime/config/model 不导入 GPUI 类型。macOS 正式窗口 + Cubism overlay release smoke
  已通过，Windows 由 hardware CI 验证；
- `accesskit 0.25.1`、`accesskit_macos 0.27.1`、`accesskit_windows 0.35.1` 与 spike 直接 pin 的 `objc2 0.6.4`/`objc2-foundation 0.3.2` 只在 `spikes/gpui-settings` 中被直接验证；ADR-0054 后正式 `bongocat-ui`/`bongocat-platform` 不再维护项目自有语义树、native bridge 或 action channel，设置 UI 只接受 `gpui-kit` 传递语义。spike 退役后删除对应直接依赖，不影响 runtime/UI command contract；
- `raw-window-handle 0.6.2`（MIT OR Apache-2.0 OR Zlib）除 spike 外也由正式 Windows
  platform adapter 直接使用，只把 GPUI 的公开 handle 转为短期借用的 HWND 以隐藏/重显设置
  窗口；裸 handle 不离开 adapter，GPUI 修复原生 close 生命周期后可移除这段正式依赖；
- `rfd 0.17.2`（MIT，macOS/Windows target-only）在 `bongocat-platform` 的私有 model
  directory picker adapter 中替换手写 `NSOpenPanel` 和 `IFileOpenDialog` UI；以
  `default-features = false` 关闭 Linux 的 XDG portal、Wayland 与 GTK feature，不改变共享
  `Selected(PathBuf)`/`Cancelled` 和稳定匿名错误契约，也不向 UI/runtime 泄漏 `rfd` 类型。
  macOS 在 AppKit 主线程、`NSApplication` 已运行且存在 sheet parent 时调用
  `AsyncFileDialog`，以避免同步 `runModal` 重入 GPUI；Windows 在专用 worker 的 STA 中调用
  `FileDialog`。选择结果仍在 Rust 侧复验、canonicalize。`rfd` 的 `None` 同时表示取消和后端
  失败，当前按取消映射；替换边界是该私有 adapter。Windows 选择器不再额外设置
  `FOS_FORCEFILESYSTEM`、`FOS_PATHMUSTEXIST`、`FOS_NOCHANGEDIR`、`FOS_DONTADDTORECENT`，
  接受该系统对话框的默认行为；`rfd` 的 Windows 后端只设置 `FOS_PICKFOLDERS`；
- `dispatch2 0.3.1`（Zlib OR Apache-2.0 OR MIT）是 `objc2` 官方维护的 Grand Central Dispatch
  binding，仅作为 macOS smoke example 的 dev dependency，用于从验证 worker 回到 main queue
  调用 `NSApplication::stop`；不进入产品依赖图或公共 API；
- `url 2.5.8`（MIT OR Apache-2.0，Rust 1.63+，Servo `rust-url` 维护）只在
  `bongocat-platform` 私有 external URL parser 中规范化并限制 HTTPS URL；公共 API 只接收字符串、
  返回项目自有错误，不泄漏 `Url`。替换边界是同等严格的 WHATWG URL parser，不影响
  config/runtime/UI 协议；
- `opener 0.8.5`（MIT OR Apache-2.0，未声明 MSRV，`Seeker14491/opener` 维护）是
  `bongocat-platform` 私有 `directory_opener`/`url_opener` adapter 的系统打开与 reveal 实现。启用
  `reveal` feature 后，
  `opener::open` 统一处理目录、普通文件和 URL，`opener::reveal` 使用平台文件管理器定位并选中路径；
  两者语义保持分离。macOS 使用系统 `open`/`open -R`，Windows 使用 `ShellExecuteW`/
  `SHOpenFolderAndSelectItems`；当前公共 API 不暴露 reveal，也没有 reveal 业务调用点。项目只保留
  绝对目录 canonicalize、目录类型检查、HTTPS/credentials/长度校验及稳定匿名错误映射，不向
  app/runtime/UI 泄漏 `opener::OpenError` 或平台命令类型。替换边界是这两个私有 adapter；升级时须
  复验目录/URL 打开以及未来 reveal 的定位选中行为；
- `tray-icon 0.25.1`（MIT OR Apache-2.0，Rust 1.90+，Tauri 项目维护）是 macOS/Windows 状态图标
  的唯一 native owner；精确固定并关闭默认 features，避免 Linux 的 GTK/libappindicator 系统依赖。
  它自身依赖 `png 0.18.1` 解码图标，adapter 另通过 workspace `image 0.25.10` 在进入
  `Icon::from_rgba` 前完成 PNG 解码；
- `muda 0.20.0`（Apache-2.0 OR MIT，Rust 1.90+，Tauri 项目维护）是共享菜单对象、菜单事件和
  overlay 右键弹出的直接依赖；精确固定并关闭默认 features，避免 Linux `gtk3`/`libxdo`。它必须与
  `tray-icon` 依赖的同一个 package 版本共同解析；第三方 tray/menu 类型、句柄和错误只存在于
  `bongocat-platform` 私有 adapter，不进入 runtime/UI 公共 API。overlay 通过 `HasWindowHandle`
  提供真实 HWND/`NSView`，替换边界与双平台实机验收入口见 ADR-0031；
- `cargo-packager-updater 0.2.3`（Apache-2.0 OR MIT，与 `cargo-packager` 同属 CrabNebula/Tauri
  生态）承担更新的 manifest 获取、版本比较、下载、验签与安装，由 ADR-0034 引入，取代
  `self_update 1.3.0`（ADR-0029）。只启用 `rustls-tls`（`default-features = false`），因此不引入
  `reqwest` 之外的额外后端。它只在 `bongocat-update` 私有模块内出现，库的 `Error`、`Config`、
  `semver::Version` 与 `Url` 均被映射为项目自有稳定码，未识别变体降级为 `update_internal_failed`，
  不进入 app/runtime/UI 协议。替换边界是 `UpdateRuntime`，不影响诊断导出契约。
  换库的两条硬性理由：zipsign 只能签 `.zip`/`.tar.gz`（裸 `.exe` 必然验签失败），且 `self_update`
  的 replace-and-verify 语义不适用于 NSIS 系统安装器。能力损失见 ADR-0034 的损失表；
- `minisign 0.7.9`（MIT，jedisct1）与 `minisign-verify 0.2.5`（MIT，零依赖）分别由 `cargo-packager`
  与 `cargo-packager-updater` 传递引入，是更新载荷的**签名端与验签端**。两端都只在本项目的打包工具
  与 `bongocat-update` 私有模块内出现，库类型不进入公共 API。预哈希为 BLAKE2b-512，与已退役的
  zipsign（SHA-512 预哈希）**密码学上不互通**；
- `arboard 3.6.1`（MIT OR Apache-2.0，Rust 1.71+，1Password 维护）只在
  `bongocat-platform` 的私有 clipboard adapter 中处理纯文本；关闭默认 `image-data` feature，
  避免引入图像、Core Graphics 与 Windows GDI 能力。库类型和错误被映射为项目自有的
  `Option<String>` 与稳定无文本错误码，不进入 config/runtime/UI 公共协议。替换边界是该
  adapter；底层文本读取会先物化系统内容，之后才执行项目的 1 MiB 校验，这是当前 API 的
  已知替换成本；
- `atomic-write-file 0.3.1`（BSD-3-Clause）只在 `bongocat-storage` 提供同目录跨平台原子替换；配置、模型、日志与诊断导出都经该 crate 落盘，它只暴露 `&[u8]` 与 `io::Result`，不泄漏库类型。替换边界是私有 commit helper；`dirs 7.0.0`、`serde 1.0.229` 与 `serde_json 1.0.151` 继续提供路径解析和严格序列化；
- `schemars 1.2.2`（MIT，Rust 1.74+）只在 `bongocat-config` 的 `schema-generation` feature 和离线 schema 生成入口使用，生成 `shared/config` 中的 Draft 2020-12 文档；默认产品构建不启用其 derive 或写入 API。Python `jsonschema` 验证和 Rust `validate()` 仍是独立门禁。它已经存在于 GPUI 传递图，因此没有新增 package 节点；Schema trait 不进入 app/runtime/UI 业务协议，替换边界是配置类型的编译期 schema 派生；
- `thiserror 2.0.21`（MIT OR Apache-2.0，Rust 1.77+）只替代机械性的 `Display`/`Error` 派生，错误文本、stable code 和 source 语义由项目测试与 wrapper 保持；库类型不进入公共 API。它已存在于现有传递图，替换边界是各产品 crate 的错误实现；
- `time 0.3.55`（MIT OR Apache-2.0，Rust 1.88+）只在 `bongocat-log` 负责 UTC 日期计算、固定时间格式和闰年校验，替换手写日历算法；不进入日志协议或用户配置，替换边界是 logger 内部日期 helper。它已存在于打包/更新传递图；
- `walkdir 2.5.0`（MIT/Unlicense，跨平台）只在 `bongocat-model-store` 的 Mver 源目录遍历中使用；`follow_links(false)`、UTF-8、深度、排序和符号链接拒绝仍由项目适配层执行，库不进入模型公共 API。它已存在于打包/i18n 传递图，最近一次稳定 release 为 2024-03-01，替换边界是该局部遍历；
- `rodio 0.22.2`（MIT OR Apache-2.0）只在 `bongocat-audio` 私有 backend 打开系统输出并
  解码现有 FLAC；固定容量的项目 command/diagnostics API 隔离第三方类型，Linux contract
  build 不链接 ALSA。真实预置 FLAC header/首样本、资源/解码失败、抢占、overflow 恢复和
  shutdown 均有 Rust 测试；默认设备热切换与长期资源测量留给后续平台验收；
- `bindgen 0.73.2` 与 `sha2 0.11.0` 只存在于离线 Cubism raw binding 工具；三个当前可绑定 target 的
  合成 header golden、外部路径/hash/不可覆盖/provenance 测试和 release check 通过。`bindgen 0.73`
  移除 `< 1.51` 的 `RustTarget`、把 `Bindings::write` 改为接收 `impl Write`、改用 `syn 3`/`shlex 2`
  并删除 `itertools`；本工具只用 `Builder`、`RustTarget::stable(85, 0)`、`RustEdition::Edition2024`
  与 `Bindings::to_string`，未触及任何被移除或改签名的 API，因此不产生源码迁移。它同时把
  零尺寸 opaque handle（`csmMoc`/`csmModel`）的默认 derive 从 `Copy, Clone` 改为 `Debug`
  （"Prevent default derives for forward-declared types"）。产品只持有 `NonNull<csmModel>`/
  `*mut csmModel` 原始指针，从不复制 handle，因此该 derive 变化对业务无影响，并且移除了对
  不透明 handle 隐式复制的可能。完整重生成与验收见 `cubism-binding-generation.md`；
- `cargo-deny 0.20.2` 驱动 14 个 manifest 的 locked license/source policy，目标矩阵为三个首发 target。2026-09-28 起 `allow-git` 只放行固定维护者 fork `https://github.com/ayangweb/gilrs`，其它未知 git source 继续失败。

GPUI 图继续报告已单独建档的 `block 0.1.6` 和 `proc-macro-error2 2.0.1`
future-incompatibility。两者本身已是各自当前最新版，升级直接依赖没有解除上游约束；
ADR-0011 允许精确锁定图用于正式最小窗口的本地开发/CI，但受影响的未来 Rust 工具链与
stable 发布保持阻塞，详见 `future-incompatibility.md`。

## Future Additions

新增依赖时必须先核对当日最新稳定版并选用该版本。若最新版本与已确认 toolchain、target、许可证或安全边界冲突，提交必须同时记录实际选择、阻塞原因、上游解除条件和替换成本。新增或修改 manifest 后必须更新对应 lockfile，运行 license/source policy、format、Clippy、test 和目标平台 build。

`.github/dependabot.yml` 每周扫描当前 workspace 和独立 spike/tool workspace，更新 PR 面向默认分支。自动 PR 仍必须通过双平台 CI 和人工 API/许可证评审，不能因版本号更新而自动合并。

版本最新不替代依赖审查。维护状态、许可证、unsafe 面积、平台覆盖和公共 API 泄漏仍按 `AGENTS.md` 的依赖规则独立验收。

CI 通过 Cargo 安装的 `cargo-deny` 也从 `0.18.3` 升级并精确固定到审计日最新稳定版 `0.20.2`；它不属于应用依赖图，但必须遵守相同的版本核对规则。
