# Repository and Release Rollback Baseline

状态：Git 回滚基线已冻结；发布产物归档与 Windows 签名复核待完成
记录日期：2026-08-28

## 1. Source Baseline

以下对象通过本地 Git object database 和远程引用核对：

| 用途                           | 引用                                     | Commit                                     |
| ------------------------------ | ---------------------------------------- | ------------------------------------------ |
| 当前主分支基线                 | `master`, `origin/master`, `origin/HEAD` | `44f44bcf2b17b8e16463ad479a477a949d01cc9a` |
| 重构前保护引用                 | `pre-refactor`, `origin/pre-refactor`    | `44f44bcf2b17b8e16463ad479a477a949d01cc9a` |
| `next` 创建基点                | `merge-base(master, next)`               | `44f44bcf2b17b8e16463ad479a477a949d01cc9a` |
| 记录前的 `next` 文档提交       | `next`                                   | `f630e6fc0609fc205d885484835321e929819db0` |
| 最新已发布 tag object          | `v1.1.0`                                 | `2a4e3b51706fe9e1ed5f8e39aed02336583a8823` |
| `v1.1.0` peeled release commit | `v1.1.0^{}`                              | `84f9f4ccfb11d8a4aefb9623934637878be0e384` |

仓库包含 `v0.1.0` 至 `v1.1.0` 的连续发布 tag。回看行为或恢复旧源码时优先使用不可变 tag/commit，不依赖会继续移动的 `master` 或 `next` 名称。

## 2. Historical Build Matrix

`.github/workflows/release.yml` 在 `v1.1.0` 时代声明了以下 target：

| Platform | Target                      |
| -------- | --------------------------- |
| macOS    | `aarch64-apple-darwin`      |
| macOS    | `x86_64-apple-darwin`       |
| Windows  | `x86_64-pc-windows-msvc`    |
| Windows  | `i686-pc-windows-msvc`      |
| Windows  | `aarch64-pc-windows-msvc`   |
| Linux    | `x86_64-unknown-linux-gnu`  |
| Linux    | `aarch64-unknown-linux-gnu` |

这份矩阵只证明旧版 CI 曾为这些 target 执行 Tauri 打包。它不证明 Native Rewrite、GPUI 或 Cubism 二进制已支持同一矩阵，也不证明每个产物已在对应硬件运行。

## 3. v1.1.0 Release Artifacts

GitHub Release `v1.1.0` 于 2026-04-20 发布，目标分支为 `master`。以下 SHA-256 来自 GitHub release asset digest；带“本地复核”的项目另从 release 下载并使用系统 `shasum` 复算一致。

| Platform        | Asset                            | SHA-256                                                            | Evidence                      |
| --------------- | -------------------------------- | ------------------------------------------------------------------ | ----------------------------- |
| macOS arm64     | `BongoCat_1.1.0_aarch64.dmg`     | `ca6a890b9c1754b8f828627f2d6864b177d5cb0efca1342f7c7da88dcaf1e94e` | GitHub metadata               |
| macOS x64       | `BongoCat_1.1.0_x64.dmg`         | `7264690b9f33606ce960f274236acee111ce34ff061886ae5c2d5a154c9b4b77` | GitHub metadata               |
| macOS arm64 app | `BongoCat_aarch64.app.tar.gz`    | `7938b320b16caf1feeea497ab112a541a516774abe54f5d5449bcead90b96710` | Metadata + local verification |
| macOS x64 app   | `BongoCat_x64.app.tar.gz`        | `272c922b41394a87b57eb931d7398bce6b96ea35c8ebd6f29b0dbd0be66bb313` | Metadata + local verification |
| Windows arm64   | `BongoCat_1.1.0_arm64-setup.exe` | `19e717dd866bab18097ec4fe43c6ea9f9e7a6c873260a4be5c9d8fdb8c67f312` | GitHub metadata               |
| Windows x64     | `BongoCat_1.1.0_x64-setup.exe`   | `c83f2963cb38056273aa98731704c8da650a4bb5bebe4262b4887ba2db76a935` | Metadata + local verification |
| Windows x86     | `BongoCat_1.1.0_x86-setup.exe`   | `cf892a92cf8be8efc5bf4d3bf9057b3091e608bf0118c0c410411eb04f0728dc` | GitHub metadata               |

发布页仍是上述产物的权威下载位置。当前只将临时下载用于哈希与签名检查，尚未建立有保留策略的独立归档，因此 TODO 中“保存旧版最后可用安装包”保持未完成。

## 4. Signature Findings

在 macOS 26.5.2 上解压两个 `.app.tar.gz` 后得到：

| Artifact              | `codesign` result                                                                        | Interpretation                                   |
| --------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------ |
| arm64 `.app`          | linker-signed ad-hoc；`codesign --verify --deep --strict` 失败，报告 resources 未被 seal | 不是 Developer ID 发布签名，不能作为新版签名基线 |
| x64 `.app`            | `code object is not signed at all`                                                       | 未签名                                           |
| Windows x64 installer | 当前 macOS 环境没有 Authenticode 验证器                                                  | 签名状态未知，必须在 Windows 使用系统工具复核    |

本机 `spctl` 显示 security assessment 被关闭；即使命令返回 accepted，也不能作为 Gatekeeper、公证或首次启动成功证据。

Release 中的 `.sig` 是 Tauri updater 签名，用于旧更新协议的内容校验。它们不是 macOS code signing/notarization，也不是 Windows Authenticode 证据。Native Rewrite 必须分别建立操作系统代码签名和自身更新 manifest 签名流程。

## 5. Rollback Procedure

- 源码与行为考古：检出 `v1.1.0^{}` 或其 peeled commit `84f9f4cc...`。
- 重构前最新源码对照：使用 `pre-refactor` 的固定 commit `44f44bc...`。
- 已发布安装包恢复：从 GitHub `v1.1.0` release 获取目标架构产物，下载后先核对本文件 SHA-256。
- 不使用 release `.sig` 判断操作系统签名状态。
- 不覆盖现有用户配置做回滚测试；复制到隔离的数据目录后运行旧版或迁移器。

## 6. Remaining Evidence

- 在 Windows 实机核对三个 installer 的 Authenticode、实际 payload architecture、安装和启动结果。
- 下载并校验两个 DMG 与 Windows arm64/x86 installer，而不只依赖 GitHub metadata。
- 建立受控的长期产物归档、资源清单和恢复演练记录。
- 在旧版支持的每个目标架构上区分“CI 产出”“可安装”和“已实机运行”。
