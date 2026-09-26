# ADR-0023: Windows Per-User Installer

状态：已接受（2026-09-05）；NSIS toolchain 与 wrapper 边界已由 ADR-0033 取代（2026-09-14）

> 注（2026-09-14）：本 ADR 关于 **per-user 安装、权限面、卸载语义与数据隔离**的决策**仍然有效**。
> 以下内容已随 ADR-0033 作废，不要再据此实现：
> - 由本项目维护 NSIS 脚本、固定官方 `nsis-3.11-setup.exe` 的 MD5、以及通过
>   `BONGOCAT_NSIS_SETUP_PATH` / `BONGOCAT_MAKENSIS_PATH` 注入本机 NSIS 路径；
>   安装器改由 `cargo-packager` 的 NSIS 模板与 `install-mode` 配置生成。
> - "x86 不构建或发布，ARM64 在 Cubism desktop Core 门禁通过前不生成可发布 artifact"：
>   Windows 现在只有 x64（ADR-0010 已更新）。
> - "Windows packaging 仅接受已 Authenticode 签名的 payload"：新流水线在未配置签名
>   身份时会生成未签名安装器并输出警告，签名验证降级为 release workflow 的显式门禁。
> - "未来 Rust update helper 只接收 ADR-0021 验证完成的本 target/arch artifact"一句已随
>   ADR-0029 作废：不再有独立 Rust update helper，替换由 `self_update 1.3.0` 在进程内完成，
>   其权限边界、原子性与失败恢复未经本项目威胁建模。ADR-0021 已标记「已被 ADR-0029 取代」。

## 背景

BongoCat 已使用当前用户 HKCU Run 启动项、环境隔离的数据根和独立的签名更新 trust boundary。
Windows 首发仍需要固定安装格式，以便明确 installer 权限、卸载语义和未来 update helper 的交接。

## 决策

- Windows 首发采用 per-user NSIS installer。每个受支持 target/arch 生成独立安装 artifact；
  当前只有 `x86_64-pc-windows-msvc`。
- 安装器以当前用户身份安装，不请求管理员权限，不安装 service、driver 或机器级注册项；
  卸载元数据写在 HKCU。
- installer 升级只在固定 product root 内替换旧的 product files，避免遗留 binary。
- 安装、升级和卸载不能读取、导入、迁移或删除 Development/Production 的 config、window state、models、
  backups、logs 或 updates 数据。默认卸载只删除 product files；删除用户数据必须是独立且明确的
  用户操作。
- installer 不联网、不解析 update manifest，也不自行选择 artifact。NSIS 只作为安装 packaging
  tool，不承载 BongoCat 业务逻辑。

## 备选方案

- MSIX 会把启动、activation 与更新交由 package model，不能作为当前 HKCU Run 与进程内
  `self_update` 路径的无附加重构替代。
- WiX / MSI 主要服务机器级部署，通常引入管理员权限与企业安装策略；首发不需要该权限面。
  `cargo-packager` 的 `wix` format 因此不进入发布矩阵。

## 验证

后续 installer job 必须在干净 Windows 10 1903+ 与 Windows 11 用户 profile 验证安装、升级、卸载、
签名、环境数据保留、x64 artifact 选择和失败 rollback。这些证据尚未取得，不能宣称 Windows
package 可发布。
