# Toolchain and Target Matrix

状态：Provisional；R5 Core 已排除 Windows ARM64，其他首发 CPU 架构尚未冻结
记录日期：2026-08-29

## 1. Decision Rule

旧版 `v1.1.0` 发布过五个 Windows/macOS target，因此 Native Rewrite 不能在没有证据和迁移说明的情况下静默移除其中任何一个。另一方面，旧版产物存在并不证明 GPUI、Cubism Core、原生 renderer 和安装签名链已在相同架构可用。

target 只有同时通过以下检查后才能进入首发支持矩阵：

1. GPUI release 构建及设置窗口 smoke test；
2. 对应架构 Cubism Core 的来源、许可证、hash、加载与生命周期测试；
3. 原生 overlay、输入和 GPU renderer 实机验证；
4. 安装、代码签名、升级和卸载验证；
5. CI 或受控构建机可重复产出。

## 2. Provisional Target Tiers

| Target                    | Historical v1.1.0   | Native Rewrite status          | Required disposition                                                                    |
| ------------------------- | ------------------- | ------------------------------ | --------------------------------------------------------------------------------------- |
| `x86_64-pc-windows-msvc`  | Installer published | Primary validation target      | 首发候选；需 Windows 实机完成 GPUI、Raw Input、D3D11、Cubism 和签名链                   |
| `aarch64-apple-darwin`    | DMG/app published   | Primary validation target      | 首发候选；当前仅 GPUI runtime-shader 生命周期 spike 通过                                |
| `aarch64-pc-windows-msvc` | Installer published | R5 Core unavailable / NO-GO    | 官方 R5 仅提供 experimental UWP ARM64 DLL，没有 desktop Windows ARM64 Core；首发不支持  |
| `i686-pc-windows-msvc`    | Installer published | Compatibility decision pending | R5 Core 支持 x86；继续验证 GPUI/依赖/renderer/installer，若移除须记录用户影响和迁移说明 |
| `x86_64-apple-darwin`     | DMG/app published   | Compatibility decision pending | R5 Core 提供 x64 static library；仍需 Intel 实机验证 GPUI、Metal、输入、签名和发布形式  |

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
- x64、ARM64、x86 各自是原生构建、交叉编译还是不支持，并提供原因。

在这些证据补齐前，不能把“CI 能运行 `cargo check`”写成 Windows 产品支持。

## 5. Cubism Constraint

Cubism Core 是冻结首发架构矩阵的硬前置条件。`docs/phase-0/cubism-sdk-source-and-license.md` 已固定 Cubism 5 SDK for Native R5、Core `06.00.0001`、官方来源和许可门禁，并确认标准 Windows Core 只有 x86/x86_64，macOS Core 有 arm64/x86_64。Windows ARM64 因缺少 `aarch64-pc-windows-msvc` 可用的官方 desktop Core 而在 R5 下 NO-GO，不能用 experimental UWP DLL、自制兼容层、未知来源二进制或跨架构模拟补齐。

其他候选 target 仍必须记录官方 ZIP SHA-256、目标文件 hash、真实加载和生命周期结果。尚未由维护者合法下载并检查 ZIP，因此本矩阵仍未冻结。

## 6. Freeze Gate

在 `P0-GPUI-WINDOWS`、`P0-GPUI-PACKAGE-MAC` 和 `P0-CUBISM` 形成证据后，更新本文件并提交架构支持 ADR。该 ADR 必须逐项决定：

- Windows x64 是否首发；
- macOS arm64 是否首发；
- Windows ARM64 停止首发支持的用户影响与迁移说明，以及 x86 是首发、实验性还是停止支持；
- macOS Intel 是独立包、universal binary、实验性还是停止支持；
- 每个支持 target 的最低 OS、构建工具链和实机测试 tier。

在该 ADR Accepted 前，TODO 的“冻结首发 target triple 和 CPU 架构矩阵”不得勾选。
