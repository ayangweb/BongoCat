# Toolchain and Target Matrix

状态：Provisional；首发 CPU 架构尚未冻结
记录日期：2026-08-28

## 1. Decision Rule

旧版 `v1.1.0` 发布过五个 Windows/macOS target，因此 Native Rewrite 不能在没有证据和迁移说明的情况下静默移除其中任何一个。另一方面，旧版产物存在并不证明 GPUI、Cubism Core、原生 renderer 和安装签名链已在相同架构可用。

target 只有同时通过以下检查后才能进入首发支持矩阵：

1. GPUI release 构建及设置窗口 smoke test；
2. 对应架构 Cubism Core 的来源、许可证、hash、加载与生命周期测试；
3. 原生 overlay、输入和 GPU renderer 实机验证；
4. 安装、代码签名、升级和卸载验证；
5. CI 或受控构建机可重复产出。

## 2. Provisional Target Tiers

| Target                    | Historical v1.1.0   | Native Rewrite status          | Required disposition                                                                          |
| ------------------------- | ------------------- | ------------------------------ | --------------------------------------------------------------------------------------------- |
| `x86_64-pc-windows-msvc`  | Installer published | Primary validation target      | 首发候选；需 Windows 实机完成 GPUI、Raw Input、D3D11、Cubism 和签名链                         |
| `aarch64-apple-darwin`    | DMG/app published   | Primary validation target      | 首发候选；当前仅 GPUI runtime-shader 生命周期 spike 通过                                      |
| `aarch64-pc-windows-msvc` | Installer published | Compatibility decision pending | 验证 GPUI、D3D11/DirectComposition、输入、Cubism binary 和 installer 后决定发布或明确停止支持 |
| `i686-pc-windows-msvc`    | Installer published | Compatibility decision pending | 验证 GPUI/Cubism/依赖是否仍支持 32-bit；若移除必须记录用户影响和迁移说明                      |
| `x86_64-apple-darwin`     | DMG/app published   | Compatibility decision pending | 在 Intel Mac 或受控设备验证 GPUI、Metal、输入、Cubism 和签名；决定独立包或 universal binary   |

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

Cubism Core 是冻结首发架构矩阵的硬前置条件。每个候选 target 必须记录官方二进制文件名、SDK/Core 版本、下载来源、SHA-256、再分发条款和真实加载结果。缺少官方目标二进制时，不通过自制兼容层、未知来源二进制或跨架构模拟静默补齐。

## 6. Freeze Gate

在 `P0-GPUI-WINDOWS`、`P0-GPUI-PACKAGE-MAC` 和 `P0-CUBISM` 形成证据后，更新本文件并提交架构支持 ADR。该 ADR 必须逐项决定：

- Windows x64 是否首发；
- macOS arm64 是否首发；
- Windows ARM64 与 x86 是首发、实验性还是停止支持；
- macOS Intel 是独立包、universal binary、实验性还是停止支持；
- 每个支持 target 的最低 OS、构建工具链和实机测试 tier。

在该 ADR Accepted 前，TODO 的“冻结首发 target triple 和 CPU 架构矩阵”不得勾选。
