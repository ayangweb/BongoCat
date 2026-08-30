# Toolchain and Target Matrix

状态：Provisional；Windows 仅 x64/ARM64，ARM64 被 R5 Core 阻塞，macOS Intel 尚未冻结
记录日期：2026-08-30

## 1. Decision Rule

ADR-0010 已固定 Windows Native Rewrite 只支持 x64 与 ARM64；i686 明确退出产品矩阵。历史 `v1.1.0` 产物只用于行为和发布考古，不决定新版本的架构支持。target 仍只有同时通过以下检查后才能成为可发布支持：

target 只有同时通过以下检查后才能进入首发支持矩阵：

1. GPUI release 构建及设置窗口 smoke test；
2. 对应架构 Cubism Core 的来源、许可证、hash、加载与生命周期测试；
3. 原生 overlay、输入和 GPU renderer 实机验证；
4. 安装、代码签名、升级和卸载验证；
5. CI 或受控构建机可重复产出。

## 2. Provisional Target Tiers

| Target                    | Historical v1.1.0   | Native Rewrite status            | Required disposition                                                                                         |
| ------------------------- | ------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `x86_64-pc-windows-msvc`  | Installer published | Primary validation target        | 首发候选；需 Windows 实机完成 GPUI、Raw Input、D3D11、Cubism 和签名链                                        |
| `aarch64-apple-darwin`    | DMG/app published   | Primary validation target        | 首发候选；GPUI/Metal overlay、真实 r.5 Core ABI 与三个预置 Moc 生命周期已通过，仍需完整 renderer/输入/签名链 |
| `aarch64-pc-windows-msvc` | Installer published | Product target / release blocked | 官方 R5 仅提供 experimental UWP ARM64 DLL，没有 desktop Windows ARM64 Core；获得可用官方 artifact 前不可发布 |
| `i686-pc-windows-msvc`    | Installer published | Out of scope                     | Native Rewrite 不构建、测试、打包或更新 x86；历史安装包只作考古输入                                          |
| `x86_64-apple-darwin`     | DMG/app published   | Compatibility decision pending   | R5 Core 提供 x64 static library；仍需 Intel 实机验证 GPUI、Metal、输入、签名和发布形式                       |

Linux target 不属于首发 tier。共享 crate 的 Linux `cargo check` 仅用于防止业务层绑定平台类型，遵循 ADR-0006。

## 3. Observed macOS Toolchain

当前 GPUI spike 环境：

| Component                | Observed version/status                                         |
| ------------------------ | --------------------------------------------------------------- |
| Hardware                 | Apple M1 Pro, arm64, Metal 4                                    |
| OS                       | macOS 26.5.2 (25F84)                                            |
| Xcode                    | 26.6 (17F113)                                                   |
| macOS SDK                | 26.5                                                            |
| Optional Metal Toolchain | build 17F109, `installed`                                       |
| Rust                     | 1.97.1, host `aarch64-apple-darwin`                             |
| Cargo                    | 1.97.1                                                          |
| GPUI                     | crates.io `0.2.2`, exact version and lockfile in isolated spike |

本机已安装 Rust targets：`aarch64-apple-darwin`、`x86_64-apple-darwin`、`x86_64-pc-windows-msvc`。target 已安装只代表标准库可用，不代表能在 macOS 链接 Windows MSVC 产物，也不代表目标运行测试通过。

当前 GPUI spike 已移除公开 `runtime_shaders` feature，并使用 Metal Toolchain 17F109 通过默认预编译 shader 的 debug/release 构建和 `.app` smoke。最低 Xcode/SDK/Rust 组合仍未验证，不能由这台较新开发机反推 macOS 12 构建兼容性。

## 4. Required Windows Toolchain Evidence

Windows 工具链尚未在实机记录。`P0-GPUI-WINDOWS` 至少要固定并保存：

- Windows 10 1903+ 与一台当前 Windows 11 的 build number；
- Visual Studio Build Tools/MSVC v143 的精确版本与安装 components；
- Windows SDK 精确版本和最低 `_WIN32_WINNT`；
- Rust stable 版本、host 与所有发布 target；
- GPUI 使用的 DirectWrite/D3D shader 编译前置条件；
- D3D11、DXGI、DirectComposition/DWM 在目标 GPU/driver 的 smoke result；
- x64 与 ARM64 各自是原生构建还是交叉编译，并提供可重复构建与实机结果；不得加入 x86 target。

在这些证据补齐前，不能把“CI 能运行 `cargo check`”写成 Windows 产品支持。

## 5. Cubism Constraint

Cubism Core 是冻结首发架构矩阵的硬前置条件。`docs/phase-0/cubism-sdk-source-and-license.md` 已固定当前本地 Cubism 5 SDK for Native `5-r.5`、Core `06.00.0001`、archive/artifact hash、官方来源和许可门禁，并确认产品需要的 Windows x64 Core 与 macOS arm64/x64 Core 存在，但 Windows ARM64 desktop Core 不存在。ARM64 因缺少 `aarch64-pc-windows-msvc` 可用的官方 artifact 而保持发布阻塞，不能用 experimental UWP DLL、自制兼容层、未知来源二进制或跨架构模拟补齐；R5 即使提供 x86 Core，也不会恢复已排除的 i686 target。

macOS arm64 已用仓库外真实 binding 和 universal dylib 完成 Core 版本、三个预置 Moc
各 100 次 lifecycle、drawable/offscreen 数组与 `leaks` 0-byte 验证，详见
`cubism-core-r5-probe.md`。Windows x64 和 macOS x64 仍必须分别完成生成、真实加载和
生命周期结果；第二来源复核与发布授权也未完成，因此本矩阵仍未冻结。

## 6. Freeze Gate

在 `P0-GPUI-WINDOWS`、`P0-GPUI-PACKAGE-MAC` 和 `P0-CUBISM` 形成证据后，更新本文件并提交架构支持 ADR。该 ADR 必须逐项决定：

- Windows x64 是否首发；
- macOS arm64 是否首发；
- Windows ARM64 的官方 Core 阻塞是否已解除；x86 维持不支持；
- macOS Intel 是独立包、universal binary、实验性还是停止支持；
- 每个支持 target 的最低 OS、构建工具链和实机测试 tier。

在该 ADR Accepted 前，TODO 的“冻结首发 target triple 和 CPU 架构矩阵”不得勾选。
