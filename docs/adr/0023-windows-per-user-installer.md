# ADR-0023: Windows Per-User Installer

状态：已接受（2026-09-05）

## 背景

Native Rewrite 已使用当前用户 HKCU Run 启动项、环境隔离的数据根和独立的签名更新 trust boundary。
Windows 首发仍需要固定安装格式，以便明确 installer 权限、卸载语义和未来 update helper 的交接。

## 决策

- Windows 首发采用 NSIS per-user installer。每个受支持 target/arch 生成独立安装 artifact；x86 不构建或
  发布，ARM64 在 Cubism desktop Core 的 ABI 与模型发布门禁通过前不生成可发布 artifact。
- installer 使用无 plugin 的 NSIS v3.11 toolchain（官方 nsis-3.11-setup.exe，MD5
  700dc40097d4cd226b13212dda1d33ac，SourceForge release；source 为 zlib/libpng license，压缩
  components 例外），在当前用户 local application directory 安装已 Authenticode 签名的 product files。
  packaging wrapper 同时核验该 acquisition artifact 的 MD5 和本地 makensis /VERSION。脚本以 user
  execution level 运行，不请求管理员权限，不安装 service、driver 或机器级注册项。
- Windows packaging 仅接受已经构建和签名的 x86_64-pc-windows-msvc release payload；wrapper 读取
  path-free build provenance，核验 target、环境、三个预置模型、所有 PE 签名和无 reparse point，拒绝既有
  output file。它不执行 Cargo、签名、下载或网络访问。installer 升级只在固定 product root 内移除旧 product
  files，避免遗留 binary；payload 不得携带 uninstaller。
- 安装、升级和卸载不能读取、导入、迁移或删除 Development/Production 的 config、state、models、backups、
  logs 或 updates 数据。默认卸载只删除 product files；删除用户数据必须是独立且明确的用户操作。
- installer 不联网、不解析 update manifest，也不自行选择 artifact。未来 Rust update helper 只接收
  ADR-0021 验证完成的本 target/arch artifact，在父进程按 shutdown coordinator 停止后执行原子替换；失败
  必须恢复上一已知可运行版本。helper 权限、替换算法与 rollback 测试仍需单独威胁建模。
- NSIS 只作为安装 packaging tool，不承载 BongoCat 业务逻辑。新增 plug-in、compression mode 或 toolchain
  升级必须记录精确版本、hash、许可证与再分发影响。

## 备选方案

- MSIX 会把启动、activation 与更新交由 package model，不能作为当前 HKCU Run 和独立 signed update helper
  路径的无附加重构替代。
- WiX 主要服务机器级 MSI 部署，通常引入管理员权限与企业安装策略；首发不需要该权限面。

## 验证

后续 installer job 必须在干净 Windows 10 1903+ 与 Windows 11 用户 profile 验证安装、升级、卸载、签名、
环境数据保留、x64 artifact 选择和失败 rollback。ARM64 在 Cubism desktop Core 门禁解除后另行验证，不能由
x64 installer 推断。NSIS toolchain 的 license/source/hash 进入 release provenance 与 SBOM；未完成这些
证据前不得宣称 Windows package 可发布。
