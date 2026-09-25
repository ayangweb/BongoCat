# Native Rewrite Rust Dependency Version Audit

状态：所有直接依赖已使用 crates.io 最新稳定版、精确上游 revision 或已记录的 ABI/transition 例外；lockfile 已更新到上游约束允许的最新解析结果
日期：2026-09-25（新增 `ayangweb/gilrs` 固定 commit `fb3cc4efa8d368e19ec9c465cf2d4d1a8d9bbb4c`；`gpui-kit` 固定 revision `500852f449c05dc01920ec82f3ae2656a61d0387`；上次全量审计 2026-09-13）
Rust：`cargo 1.97.1`、`rustc 1.97.1`

## Scope

本次审计覆盖仓库根目录的正式 workspace，以及 `spikes/*` 和 `tools/*` 下列出的独立 workspace。
历史 Vue/Tauri workspace 已退役至远端 `pre-refactor-tauri` 分支，不属于 Native Rewrite 的依赖图。

版本来源使用 crates.io stable release：

```text
cargo search <crate> --limit 1
cargo update --manifest-path <workspace>/Cargo.toml
cargo update --manifest-path <workspace>/Cargo.toml --dry-run --verbose
cargo tree --manifest-path <workspace>/Cargo.toml --invert <crate>@<version>
```

预发布版本、yanked 版本和未固定 git branch 不属于“最新稳定版”。直接依赖精确 pin，传递依赖由 `Cargo.lock` 固定；`deny.toml` 的 `required-git-spec = "rev"` 进一步拒绝 branch/tag git source。两个直接依赖例外都固定完整 commit：`gpui-kit` 等待包含已合并 API 的 crates.io release；`gilrs` 使用维护者 fork 统一承接手柄兼容、backend queue 与平台生命周期修复，不能以浮动 branch 跟随。

## Direct Dependencies

| Crate                                 | Pinned version | Result                                                   |
| ------------------------------------- | -------------: | -------------------------------------------------------- |
| `accesskit`                           |       `0.25.0` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `accesskit_macos`                     |       `0.27.0` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `accesskit_windows`                   |       `0.35.0` | spike 直接依赖；正式 UI 经 `gpui-kit` 传递                    |
| `arboard`                             |        `3.6.1` | 剪贴板 adapter 新增时即为最新                            |
| `async-channel`                       |        `2.5.0` | 从 `1.9.0` 升级                                          |
| `atomic-write-file`                   |        `0.3.1` | 配置与更新 sequence 存储新增时最新                       |
| `bindgen`                             |       `0.72.1` | 新增时即为最新                                           |
| `block2`                              |        `0.6.2` | 已是最新                                                 |
| `core-foundation`                     |       `0.10.1` | 已是最新                                                 |
| `core-graphics-types`                 |        `0.2.0` | 从 `0.1.3` 升级                                          |
| `core-graphics2`                      |        `0.6.1` | 从 `0.4.1` 升级                                          |
| `dirs`                                |        `6.0.0` | 从 `5.0.1` 升级                                          |
| `embed-resource`                      |       `3.0.11` | Windows 产品图标新增时最新                               |
| `gpui-kit`                            | `0.6.5` @ `500852f` | 上游固定 revision；含 variant 与 Popover arrow                |
| `gilrs`                               | `0.11.2` @ `fb3cc4e` | 维护者 fork 固定 revision；WGI/IOHID 与后续手柄修复统一维护    |
| `futures-lite`                        |        `2.6.1` | 已是最新                                                 |
| `gpui`                                |        `0.2.2` | spike 直接依赖；正式 UI 经 `gpui-kit` suite 传递              |
| `libc`                                |      `0.2.189` | 新增时即为最新稳定版                                     |
| `metal`                               |       `0.33.0` | 从 `0.29.0` 升级                                         |
| `objc2`                               |        `0.6.4` | 已是最新                                                 |
| `objc2`（GPUI AX）                    |        `0.5.2` | spike 内 ABI 类型兼容例外                                |
| `objc2-app-kit`                       |        `0.3.2` | 已是最新                                                 |
| `objc2-core-foundation`               |        `0.3.2` | 正式输入边界新增时最新                                   |
| `objc2-core-graphics`                 |        `0.3.2` | 正式输入边界新增时最新                                   |
| `objc2-foundation`                    |        `0.3.2` | 已是最新                                                 |
| `objc2-foundation`（GPUI 原生 probe） |        `0.2.2` | 上游 ABI 类型兼容例外                                    |
| `objc2-quartz-core`                   |        `0.3.2` | 已是最新                                                 |
| `objc2-service-management`            |        `0.3.2` | 启动项 adapter 新增时最新                                |
| `serde`                               |      `1.0.229` | 从 `1.0.228` 升级                                        |
| `serde_json`                          |      `1.0.151` | 从 `1.0.149` 升级                                        |
| `raw-window-handle`                   |        `0.6.2` | 新增时即为最新                                           |
| `rfd`                                 |       `0.17.2` | 双平台目录选择迁移时最新稳定版                           |
| `rodio`                               |       `0.22.2` | motion 音效新增时最新                                    |
| `sha2`                                |       `0.11.0` | 新增时即为最新                                           |
| `tempfile`                            |       `3.27.0` | 已是最新                                                 |
| `muda`                                |       `0.20.0` | macOS/Windows 菜单与右键弹出 owner（ADR-0031）           |
| `tray-icon`                           |       `0.25.0` | macOS/Windows 统一托盘 owner 新增时最新（ADR-0031）      |
| `unicode-segmentation`                |       `1.13.3` | 已是最新                                                 |
| `url`                                 |        `2.5.8` | 外部 HTTPS URL wrapper 新增时最新                        |
| `cargo-packager-updater`              |        `0.2.3` | 更新库，取代 `self_update` 时最新（ADR-0034）            |
| `minisign`                            |        `0.7.9` | `cargo-packager` 的传递依赖，更新载荷签名                |
| `minisign-verify`                     |        `0.2.5` | `cargo-packager-updater` 的传递依赖，客户端验签          |
| `windows`                             |       `0.62.2` | 从 `0.61.3` 升级                                         |

`windows 0.62.2` 删除了 `Error::from_win32()`；Win32 wrapper 已改为在失败调用后立即使用语义等价的 `Error::from_thread()`，避免清理 API 覆盖 thread last-error。

`cargo search gpui-kit --limit 1` 与 `cargo info gpui-kit` 在 2026-09-24 仍显示 crates.io 最新稳定版为 `0.6.6`，但该 release 不含后来合并的 `SettingGroup::variant()` 与 `Popover::arrow()`。ADR-0056 因此删除根目录 `[patch.crates-io]` 和 `ayangweb/gpui-kit` fork，直接固定上游 `longbridge/gpui-kit` merge commit `500852f449c05dc01920ec82f3ae2656a61d0387`；该 commit 的 package 元数据为 `0.6.5`。lockfile 中五个 GPUI Kit suite package 统一从这一 git source 解析，`gpui-pre` 仍为 crates.io `0.3.6`。`deny.toml` 只放行该上游仓库；包含两项 API 的 release 发布后必须切回 registry 精确 pin。

### 已记录的上游阻塞：`tray-icon 0.25.0` 的 Windows `set_tooltip`

- 阻塞版本：`tray-icon 0.25.0`（crates.io 当日最新稳定版，无更新版本可升级）。
- 现象：Windows 上 `TrayIcon::set_tooltip` 在图标以固定 GUID 注册后必然返回错误。`set_icon` 与
  内部 `set_tray_visible` 都调用 `apply_guid`，只有 `set_tooltip` 未设置 `NIF_GUID`；shell 对以
  `guidItem` 标识的图标忽略 `uID`，并要求后续每次 `Shell_NotifyIcon` 调用携带同一 GUID
  （<https://learn.microsoft.com/windows/win32/api/shellapi/ns-shellapi-notifyicondataw#troubleshooting>）。
- 上游 owner：`tauri-apps/tray-icon`（`src/platform_impl/windows/mod.rs` 的 `TrayIcon::set_tooltip`）。
  已核对上游 `dev` 分支为同一实现，即尚未修复。
- 影响与绕行：`bongocat-platform` 的 `system_menu_native` adapter 不再在 `set_presentation` 中调用
  `set_tooltip`，把 tooltip 固定为创建期输入（ADR-0031）。产品文案 `system_menu.title` 恒为
  `BongoCat`，运行期不变，因此不产生可见行为差异。
- 解除条件：上游 `set_tooltip` 补上 `apply_guid` 后，可恢复运行期 tooltip 更新；升级时按 ADR-0031
  的替换边界复验 GUID 注册下的 `set_tooltip` 行为。

## Transitive Constraints

每个 workspace 都已执行完整 `cargo update`。这会升级所有满足现有依赖约束的传递包，但不能合法越过上游 crate 的 semver 或精确约束。

最新 `gpui 0.2.2` 的依赖图仍固定旧一代 `metal 0.29.0` 和 `core-graphics2 0.4.1`；overlay spike 自己使用的直接版本已分别升级到 `0.33.0`，输入 spike 自己使用 `core-graphics2 0.6.1`，因此 lockfile 中会同时存在两个 API generation。`cargo update --dry-run --verbose` 还报告以下 3 个有更新但被上游约束阻止的兼容版本：

| Locked dependency      | Available | Owner path                 |
| ---------------------- | --------- | -------------------------- |
| `cocoa 0.26.0`         | `0.26.1`  | `gpui 0.2.2`               |
| `generic-array 0.14.7` | `0.14.9`  | `gpui_http_client -> sha2` |
| `smallvec 1.15.2`      | `1.16.0`  | GPUI Kit/image/URL graphs  |

这些版本不能通过手改 lockfile 或 `cargo update --precise` 安全升级；ADR-0056 的上游固定 revision 只用于取得已经合并的 GPUI Kit API，不用于越过这些上游约束。解除方式是 GPUI 发布兼容的新版本后升级 GPUI 并重跑双平台 UI/overlay smoke；不为追求表面版本一致而改写上游依赖图。

`rodio 0.22.2` 在审计日是 crates.io 最新非 yanked 稳定版（MIT OR Apache-2.0，
Rust 1.87+），但其 playback feature 约束 `cpal 0.17.x`，因此完整 `cargo update` 合法解析
为 `cpal 0.17.3`，而不是独立最新的 `0.18.2`。Native workspace 不直接依赖 CPAL；
rodio 仅在 Windows/macOS target 启用 `playback + flac`，录音及其它 codec feature 均关闭。
替换边界完全位于 `bongocat-audio` 私有 backend，不把第三方类型暴露到 runtime contract。

GPUI accessibility spike 直接固定 `objc2 0.5.2` 与 `objc2-foundation 0.2.2`，虽然 crates.io 最新稳定版分别为 `0.6.4` 与 `0.3.2`。这是类型兼容例外，不是为了回避 API 迁移：`accesskit_macos 0.27.0` 的 adapter 公共对象使用其 `objc2 0.5.x` 依赖构造，本机 spike 诊断和 native tooltip probe 必须使用同一代 Rust Objective-C/Foundation 类型。把这些对象借用为 `objc2 0.6.x` / `objc2-foundation 0.3.x` 类型会跨越两个互不兼容的 Rust 类型世代。该直接依赖只存在于 macOS spike 的平台诊断边界，不进入业务 API；当 AccessKit macOS 升级到 `objc2 0.6`，或诊断不再需要直接检查 adapter 对象时立即移除并重跑 spike/tooltip smoke。

## Verification

当前 dependency-policy 脚本覆盖根 workspace、12 个 `spikes/*` workspace 与 `tools/cubism-bindgen`，共 14 个 manifest；它们均以 locked license/source check 为目标。正式根 workspace 还执行三个首发 target 的 release dependency tree check。无依赖的 contract workspace 同样重新生成/检查 lockfile。附加平台验证包括：

- `windows 0.62.2` 同时封装 Raw Input、Win32 窗口与原生 overlay 边界；输入和 overlay crate 均在
  `x86_64-pc-windows-msvc` 完成 Check/Clippy。Windows 手柄已迁入 gilrs WGI，旧
  `Win32_UI_Input_XboxController` feature 与自维护 XInput DLL 加载已删除；
- `core-graphics2 0.6.1` 在已授予 Input Monitoring 的 macOS 会话创建 listen-only tap，完成 lifecycle Reset 和正常 shutdown；
- `objc2-core-graphics 0.3.2` 与 `objc2-core-foundation 0.3.2` 只存在于正式 macOS
  platform adapter，取代会为输入路径引入 `block 0.1.6` 的 `core-graphics2`；窄 wrapper
  管理 callback context、CFRunLoop source 和 tap 的统一析构，项目公共 API 仅暴露自有
  permission、diagnostics 和 error 类型。替换边界是 `MacInputService` 私有实现，不影响 runtime；
- `gilrs 0.11.2` / `gilrs-core 0.6.8` 固定 `ayangweb/gilrs` commit
  `fb3cc4efa8d368e19ec9c465cf2d4d1a8d9bbb4c`，许可证 `Apache-2.0 OR MIT`，MSRV `1.84`。
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
  `cc 1.4.5`、`find-msvc-tools 0.1.12`、`tinyvec 1.13.2` 和 `tokio-rustls 0.26.5` 在现有上游
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
- `accesskit 0.25.0`、`accesskit_macos 0.27.0`、`accesskit_windows 0.35.0` 与 ABI generation 匹配的 `objc2-foundation 0.2.2` 只在 `spikes/gpui-settings` 中被直接验证；ADR-0054 后正式 `bongocat-ui`/`bongocat-platform` 不再维护项目自有语义树、native bridge 或 action channel，设置 UI 只接受 `gpui-kit` 传递语义。spike 退役后删除对应直接依赖，不影响 runtime/UI command contract；
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
- `tray-icon 0.25.0`（MIT OR Apache-2.0，Rust 1.90+，Tauri 项目维护）是 macOS/Windows 状态图标
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
- `atomic-write-file 0.3.1`（BSD-3-Clause）只在 `bongocat-storage` 提供同目录跨平台原子替换；配置、模型、日志与诊断导出都经该 crate 落盘，它只暴露 `&[u8]` 与 `io::Result`，不泄漏库类型。替换边界是私有 commit helper；`dirs 6.0.0`、`serde 1.0.229` 与 `serde_json 1.0.151` 继续提供路径解析和严格序列化；
- `rodio 0.22.2`（MIT OR Apache-2.0）只在 `bongocat-audio` 私有 backend 打开系统输出并
  解码现有 FLAC；固定容量的项目 command/diagnostics API 隔离第三方类型，Linux contract
  build 不链接 ALSA。真实预置 FLAC header/首样本、资源/解码失败、抢占、overflow 恢复和
  shutdown 均有 Rust 测试；默认设备热切换与长期资源测量留给后续平台验收；
- `bindgen 0.72.1` 与 `sha2 0.11.0` 只存在于离线 Cubism raw binding 工具；三个当前可绑定 target 的合成 header golden、外部路径/hash/不可覆盖/provenance 测试和 release check 通过；
- `cargo-deny 0.20.2` 驱动 14 个 manifest 的 locked license/source policy，目标矩阵为三个首发 target。`allow-git` 只放行固定上游 `https://github.com/longbridge/gpui-kit` 与固定维护者 fork `https://github.com/ayangweb/gilrs`，其它未知 git source 继续失败。

GPUI 图继续报告已单独建档的 `block 0.1.6` 和 `proc-macro-error2 2.0.1`
future-incompatibility。两者本身已是各自当前最新版，升级直接依赖没有解除上游约束；
ADR-0011 允许精确锁定图用于正式最小窗口的本地开发/CI，但受影响的未来 Rust 工具链与
stable 发布保持阻塞，详见 `future-incompatibility.md`。

## Future Additions

新增依赖时必须先核对当日最新稳定版并选用该版本。若最新版本与已确认 toolchain、target、许可证或安全边界冲突，提交必须同时记录实际选择、阻塞原因、上游解除条件和替换成本。新增或修改 manifest 后必须更新对应 lockfile，运行 license/source policy、format、Clippy、test 和目标平台 build。

`.github/dependabot.yml` 每周扫描当前 Native 和独立 spike/tool workspace，并把更新目标固定为 `next`。自动 PR 仍必须通过双平台 CI 和人工 API/许可证评审，不能因版本号更新而自动合并。

版本最新不替代依赖审查。维护状态、许可证、unsafe 面积、平台覆盖和公共 API 泄漏仍按 `AGENTS.md` 的依赖规则独立验收。

CI 通过 Cargo 安装的 `cargo-deny` 也从 `0.18.3` 升级并精确固定到审计日最新稳定版 `0.20.2`；它不属于应用依赖图，但必须遵守相同的版本核对规则。
