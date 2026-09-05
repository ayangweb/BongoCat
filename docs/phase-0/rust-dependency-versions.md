# Native Rewrite Rust Dependency Version Audit

状态：所有直接依赖已使用 crates.io 最新稳定版；lockfile 已更新到上游约束允许的最新解析结果
日期：2026-09-05
Rust：`cargo 1.97.1`、`rustc 1.97.1`

## Scope

本次审计覆盖正式 `native/` workspace，以及 12 个 `spikes/*`/2 个 `tools/*` 独立 workspace，
当前共 15 个 workspace。仓库根 workspace、`src-tauri/` 和其插件属于历史行为对照，
不是 Native Rewrite 的依赖图；Phase 0 不借依赖升级修改历史应用。

版本来源使用 crates.io stable release：

```text
cargo search <crate> --limit 1
cargo update --manifest-path <workspace>/Cargo.toml
cargo update --manifest-path <workspace>/Cargo.toml --dry-run --verbose
cargo tree --manifest-path <workspace>/Cargo.toml --invert <crate>@<version>
```

预发布版本、yanked 版本和未固定 git branch 不属于“最新稳定版”。直接依赖精确 pin，传递依赖由 `Cargo.lock` 固定。

## Direct Dependencies

| Crate                                 | Pinned version | Result                             |
| ------------------------------------- | -------------: | ---------------------------------- |
| `accesskit`                           |       `0.25.0` | 新增时即为最新                     |
| `accesskit_macos`                     |       `0.27.0` | 新增时即为最新                     |
| `accesskit_windows`                   |       `0.35.0` | 新增时即为最新                     |
| `async-channel`                       |        `2.5.0` | 从 `1.9.0` 升级                    |
| `atomic-write-file`                   |        `0.3.1` | 配置与更新 sequence 存储新增时最新 |
| `bindgen`                             |       `0.72.1` | 新增时即为最新                     |
| `block2`                              |        `0.6.2` | 已是最新                           |
| `core-foundation`                     |       `0.10.1` | 已是最新                           |
| `core-graphics-types`                 |        `0.2.0` | 从 `0.1.3` 升级                    |
| `core-graphics2`                      |        `0.6.1` | 从 `0.4.1` 升级                    |
| `dirs`                                |        `6.0.0` | 从 `5.0.1` 升级                    |
| `embed-resource`                      |       `3.0.11` | Windows 产品图标新增时最新         |
| `futures-lite`                        |        `2.6.1` | 已是最新                           |
| `gpui`                                |        `0.2.2` | 已是最新                           |
| `libc`                                |      `0.2.189` | 新增时即为最新稳定版               |
| `metal`                               |       `0.33.0` | 从 `0.29.0` 升级                   |
| `objc2`                               |        `0.6.4` | 已是最新                           |
| `objc2`（GPUI AX）                    |        `0.5.2` | 上游 ABI 类型兼容例外              |
| `objc2-app-kit`                       |        `0.3.2` | 已是最新                           |
| `objc2-core-foundation`               |        `0.3.2` | 正式输入边界新增时最新             |
| `objc2-core-graphics`                 |        `0.3.2` | 正式输入边界新增时最新             |
| `objc2-foundation`                    |        `0.3.2` | 已是最新                           |
| `objc2-foundation`（GPUI 原生 probe） |        `0.2.2` | 上游 ABI 类型兼容例外              |
| `objc2-game-controller`               |        `0.3.2` | 新增时即为最新                     |
| `objc2-quartz-core`                   |        `0.3.2` | 已是最新                           |
| `objc2-service-management`            |        `0.3.2` | 启动项 adapter 新增时最新          |
| `serde`                               |      `1.0.229` | 从 `1.0.228` 升级                  |
| `serde_json`                          |      `1.0.151` | 从 `1.0.149` 升级                  |
| `raw-window-handle`                   |        `0.6.2` | 新增时即为最新                     |
| `rodio`                               |       `0.22.2` | motion 音效新增时最新              |
| `sha2`                                |       `0.11.0` | 新增时即为最新                     |
| `tempfile`                            |       `3.27.0` | 已是最新                           |
| `unicode-segmentation`                |       `1.13.3` | 已是最新                           |
| `url`                                 |        `2.5.8` | 外部 HTTPS URL wrapper 新增时最新  |
| `windows`                             |       `0.62.2` | 从 `0.61.3` 升级                   |

`windows 0.62.2` 删除了 `Error::from_win32()`；Win32 wrapper 已改为在失败调用后立即使用语义等价的 `Error::from_thread()`，避免清理 API 覆盖 thread last-error。

## Transitive Constraints

每个 workspace 都已执行完整 `cargo update`。这会升级所有满足现有依赖约束的传递包，但不能合法越过上游 crate 的 semver 或精确约束。

最新 `gpui 0.2.2` 的依赖图仍固定旧一代 `metal 0.29.0` 和 `core-graphics2 0.4.1`；overlay spike 自己使用的直接版本已分别升级到 `0.33.0`，输入 spike 自己使用 `core-graphics2 0.6.1`，因此 lockfile 中会同时存在两个 API generation。`cargo update --dry-run --verbose` 还报告以下 3 个有更新但被上游约束阻止的兼容版本：

| Locked dependency      | Available | Owner path                 |
| ---------------------- | --------- | -------------------------- |
| `cocoa 0.26.0`         | `0.26.1`  | `gpui 0.2.2`               |
| `generic-array 0.14.7` | `0.14.9`  | `gpui_http_client -> sha2` |
| `smallvec 1.15.2`      | `1.16.0`  | GPUI Kit/image/URL graphs  |

这些版本不能通过手改 lockfile、`cargo update --precise` 或本地 patch 安全升级。解除方式是 GPUI 发布兼容的新版本后升级 GPUI 并重跑双平台 UI/overlay smoke；不为追求表面版本一致而 fork 上游。

`rodio 0.22.2` 在审计日是 crates.io 最新非 yanked 稳定版（MIT OR Apache-2.0，
Rust 1.87+），但其 playback feature 约束 `cpal 0.17.x`，因此完整 `cargo update` 合法解析
为 `cpal 0.17.3`，而不是独立最新的 `0.18.2`。Native workspace 不直接依赖 CPAL；
rodio 仅在 Windows/macOS target 启用 `playback + flac`，录音及其它 codec feature 均关闭。
替换边界完全位于 `bongocat-audio` 私有 backend，不把第三方类型暴露到 runtime contract。

GPUI accessibility spike 直接固定 `objc2 0.5.2` 与 `objc2-foundation 0.2.2`，虽然 crates.io 最新稳定版分别为 `0.6.4` 与 `0.3.2`。这是类型兼容例外，不是为了回避 API 迁移：`accesskit_macos 0.27.0` 的 adapter 公共对象使用其 `objc2 0.5.x` 依赖构造，本机 AX 诊断和 native tooltip probe 必须使用同一代 Rust Objective-C/Foundation 类型。把这些对象借用为 `objc2 0.6.x` / `objc2-foundation 0.3.x` 类型会跨越两个互不兼容的 Rust 类型世代。该直接依赖只存在于 macOS spike 的平台诊断边界，不进入业务 API；当 AccessKit macOS 升级到 `objc2 0.6`，或诊断不再需要直接检查 adapter 对象时立即移除并重跑 AX/tooltip smoke。

## Verification

当前 15 个 workspace 均纳入 locked format、Clippy、test 和 dependency policy；正式 `native/` workspace 还执行三平台 release check。无依赖的 contract workspace 同样重新生成/检查 lockfile。附加平台验证包括：

- `windows 0.62.2` 同时封装 Raw Input、XInput 与原生 overlay 边界；输入和 overlay crate 均在 `x86_64-pc-windows-msvc` 完成 Check/Clippy，输入与 overlay 也对 `aarch64-pc-windows-msvc` 完成 Check；XInput 仅增加同一 package 的 `Win32_UI_Input_XboxController` feature，真实 Windows 输入与 D3D11 生命周期 smoke 由 push CI 执行；
- `core-graphics2 0.6.1` 在已授予 Input Monitoring 的 macOS 会话创建 listen-only tap，完成 lifecycle Reset 和正常 shutdown；
- `objc2-core-graphics 0.3.2` 与 `objc2-core-foundation 0.3.2` 只存在于正式 macOS
  platform adapter，取代会为输入路径引入 `block 0.1.6` 的 `core-graphics2`；窄 wrapper
  管理 callback context、CFRunLoop source 和 tap 的统一析构，项目公共 API 仅暴露自有
  permission、diagnostics 和 error 类型。替换边界是 `MacInputService` 私有实现，不影响 runtime；
- `objc2-game-controller 0.3.2` 只在 macOS 输入 spike 的窄平台边界枚举 `GCExtendedGamepad`、安装 value-change handler 并管理 background delivery；许可证为 Zlib OR Apache-2.0 OR MIT，项目公共协议只接收自有 snapshot/event 类型，停止使用 GameController 时可替换该 adapter 而不改变 producer contract；
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
- `gpui 0.2.2`（Apache-2.0）只在 Windows/macOS 正式 UI/app target 编译，Linux 共享协议
  不依赖 GPUI；替换边界位于 `bongocat-ui::window` 与 app 主循环，runtime/config/model 不导入
  GPUI 类型。macOS 正式窗口 + Cubism overlay release smoke 已通过，Windows 由 hardware CI 验证；
- `accesskit 0.25.0`、`accesskit_macos 0.27.0`、`accesskit_windows 0.35.0` 与 ABI generation 匹配的 `objc2-foundation 0.2.2` 已用于正式 `bongocat-platform` 的设置语义树和 action bridge，并由 `bongocat-ui` 在 GPUI render 生命周期更新；平台 adapter 只接收项目自有 tree、action 和 raw window handle，action 以容量 32 的有界通道回到 GPUI 主线程。替换边界是 GPUI 提供等价稳定 element-level accessibility 和输入测试 API，届时删除 adapter 依赖而不改变 runtime/UI command contract；
- `raw-window-handle 0.6.2`（MIT OR Apache-2.0 OR Zlib）除 spike 外也由正式 Windows
  platform adapter 直接使用，只把 GPUI 的公开 handle 转为短期借用的 HWND 以隐藏/重显设置
  窗口；裸 handle 不离开 adapter，GPUI 修复原生 close 生命周期后可移除这段正式依赖；
- `url 2.5.8`（MIT OR Apache-2.0，Rust 1.63+，Servo `rust-url` 维护）只在
  `bongocat-platform` 私有 external URL parser 中规范化并限制 HTTPS URL；公共 API 只接收字符串、
  返回项目自有错误，不泄漏 `Url`。macOS/Windows launcher 均以单一参数启动系统 opener，绝不经 shell；
  替换边界是同等严格的 WHATWG URL parser，不影响 config/runtime/UI 协议；
- `atomic-write-file 0.3.1`（BSD-3-Clause）只在正式配置 crate 的 `ConfigStore` 与 update crate 的环境内 sequence store 提供同目录跨平台原子替换；两者都只暴露项目自有的配置或更新状态类型，不泄漏库类型。替换边界分别是各自私有 commit helper；`dirs 6.0.0`、`serde 1.0.229` 与 `serde_json 1.0.151` 继续提供路径解析和严格序列化；
- `rodio 0.22.2`（MIT OR Apache-2.0）只在 `bongocat-audio` 私有 backend 打开系统输出并
  解码现有 FLAC；固定容量的项目 command/diagnostics API 隔离第三方类型，Linux contract
  build 不链接 ALSA。真实预置 FLAC header/首样本、资源/解码失败、抢占、overflow 恢复和
  shutdown 均有 Rust 测试；默认设备热切换与长期资源测量留给后续平台验收；
- `bindgen 0.72.1` 与 `sha2 0.11.0` 只存在于离线 Cubism raw binding 工具；三个当前可绑定 target 的合成 header golden、外部路径/hash/不可覆盖/provenance 测试和 release check 通过；
- `cargo-deny 0.20.2` 对全部 15 个 workspace 的四目标 license/source policy执行检查。

GPUI 图继续报告已单独建档的 `block 0.1.6` 和 `proc-macro-error2 2.0.1`
future-incompatibility。两者本身已是各自当前最新版，升级直接依赖没有解除上游约束；
ADR-0011 允许精确锁定图用于正式最小窗口的本地开发/CI，但受影响的未来 Rust 工具链与
stable 发布保持阻塞，详见 `future-incompatibility.md`。

## Future Additions

新增依赖时必须先核对当日最新稳定版并选用该版本。若最新版本与已确认 toolchain、target、许可证或安全边界冲突，提交必须同时记录实际选择、阻塞原因、上游解除条件和替换成本。新增或修改 manifest 后必须更新对应 lockfile，运行 license/source policy、format、Clippy、test 和目标平台 build。

`.github/dependabot.yml` 每周扫描这 15 个独立 workspace，并把更新目标固定为 `next`。扫描不包含历史根 workspace 和 `src-tauri`，避免把行为对照混入 Native Rewrite 依赖 PR。自动 PR 仍必须通过双平台 CI 和人工 API/许可证评审，不能因版本号更新而自动合并。

版本最新不替代依赖审查。维护状态、许可证、unsafe 面积、平台覆盖和公共 API 泄漏仍按 `AGENTS.md` 的依赖规则独立验收。

CI 通过 Cargo 安装的 `cargo-deny` 也从 `0.18.3` 升级并精确固定到审计日最新稳定版 `0.20.2`；它不属于应用依赖图，但必须遵守相同的版本核对规则。
