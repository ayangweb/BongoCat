# ADR-0033: Build, Packaging and Release Toolchain

状态：已接受（2026-09-14）
取代：ADR-0010 的 Windows target 集合、ADR-0023 的 NSIS toolchain 与 wrapper 边界

## 背景

Native Rewrite 之前的构建体系是自己维护的：

| 组件 | 之前的状态 |
| --- | --- |
| `justfile` | 通过 `os()` 分支调用两个平台脚本 |
| `scripts/` | `build-macos.sh`、`package-macos.sh`、`build-windows.ps1`、`package-windows.ps1` |
| `windows/installer/BongoCat.nsi` | 自维护的 56 行 NSIS 脚本 |
| GitHub Actions | 只有 `native-rewrite-phase0.yml`（PR 触发）；**没有任何 release workflow** |
| 版本号 | 只有 `[workspace.package].version` 一个来源，但 macOS / Windows 各自重新解析一次 |

这套体系的问题不是"实现得不好"，而是它同时承担了三件事：编译、打包、发布。
平台判断、架构判断、目录布局、`Info.plist` 注入、图标校验、provenance、NSIS 调用
全部由项目自己实现，任何一个平台细节变化都要改项目代码。Windows 侧还要求发布机器
预置 `nsis-3.11-setup.exe` 与 `makensis.exe` 并通过 `BONGOCAT_NSIS_SETUP_PATH` /
`BONGOCAT_MAKENSIS_PATH` 注入——这是"依赖个人机器状态"的构建，不是项目级构建。

本次重构的目标由用户明确给出：三个发布目标、项目级配置、成熟开源工具、薄 `just`
入口、删除 `scripts/`、本地与 CI 同一套体系、版本号唯一来源。用户同时明确：现有 ADR
是参考资料而不是约束，与最终目标冲突时以目标为准。

### 发布目标

- `x86_64-pc-windows-msvc` → NSIS 安装器（`.exe`）
- `x86_64-apple-darwin` → `.app` + `.dmg`
- `aarch64-apple-darwin` → `.app` + `.dmg`

Windows ARM64 **不再是产品目标**。Cubism Native R5 没有 desktop ARM64 Core，而
Windows 自身的 x64 仿真可以运行 x64 应用，因此维护一个无法端到端验证的原生
ARM64 目标只增加成本和发布风险。ADR-0010 的 ARM64 结论作废。

## 决策

### 1. `cargo-packager` 作为唯一打包工具，并以库形式集成

采用 `cargo-packager =0.11.8`（`default-features = false`，仅启用 `rustls-tls`，
Apache-2.0 OR MIT），由 workspace 成员 `crates/bongocat-packaging` 以**库**方式调用
`cargo_packager::package()`。

- 打包配置是仓库内的 Rust 源码，编译期检查，不存在弱类型配置文件。
- 版本由 `Cargo.lock` 精确固定，不要求任何开发者 `cargo install` 全局工具。
- 本地与 CI 执行同一个 crate，接口天然一致。
- 打包工作流完全归 `cargo-packager` 所有：bundle 目录布局、`Info.plist` 生成、
  资源复制、图标转换（`.icns` / `.ico`）、NSIS payload 组装与 installer 生成。

`cargo-packager` 的关键能力与 BongoCat 的对应关系：

| 需求 | `cargo-packager` 能力 |
| --- | --- |
| Windows x64 安装器 | `nsis`，`install_mode = currentUser` 是默认值 |
| macOS `.app` | `app`，自动生成 `Info.plist` 并合并仓库 overlay |
| macOS `.dmg` | `dmg`（本项目按下方第 4 条处理） |
| 桌面 GUI 应用 | 本身就是 Tauri/Wry/Dioxus/egui 等桌面框架使用的打包器 |
| 资源与图标 | `resources`（支持 `src`/`target` 映射）、`icons` |
| 签名 | macOS `signing_identity`、公证凭据；Windows `signtool` / 自定义签名命令 |
| Cargo workspace | 直接消费 cargo target 目录与二进制名 |

### 2. 删除自研构建与打包脚本

- 删除 `scripts/`（4 个文件，约 14 KB shell / PowerShell）。
- 删除 `windows/installer/BongoCat.nsi`（由 `cargo-packager` 的模板与
  `install-mode` 配置取代）。
- `justfile` 只保留统一入口，不含平台判断、复制、打包或版本处理逻辑：
  `just build [--target ...] [--environment ...] [--formats ...]`。
- `BONGOCAT_NSIS_SETUP_PATH` / `BONGOCAT_MAKENSIS_PATH` 与 NSIS 3.11 的 MD5 固定
  要求一并作废：NSIS toolchain 由 `cargo-packager` 自动获取并缓存。

### 3. 版本号唯一来源

`[workspace.package].version` 是唯一来源。`bongocat-packaging` 继承该版本，
因此 Cargo 在打包进程启动前就解析完成；`--print-version` 把同一个值暴露给发布流水线，
release workflow 用它校验 tag 与版本一致，不再出现"脚本一个版本、CI 一个版本"的局面。

### 4. `.dmg` 由操作系统磁盘映像工具产出（有明确退出条件的偏离）

`cargo-packager` 0.11.8 的 DMG 实现依赖 `create-dmg` 在 2022 年的固定提交
`28867ba`，该脚本在当前 macOS 上**无法完成 DMG 构建**。它用

```text
hdiutil attach ... | grep -E '^/dev/' | sed 1q | awk '{print $1}'
```

解析挂载结果，`sed 1q` 在读到第一行后关闭管道，`hdiutil` 收到 SIGPIPE 后挂载事务
没有完成，脚本随后的 `hdiutil detach <device>` 报
`detach failed - No such file or directory`。

本机实测（macOS 26.5，使用与 `cargo-packager` 完全相同的参数调用该脚本）：

- 原样调用 3/3 失败；
- 把 `hdiutil attach` 的完整输出读完再解析 3/3 成功。

`create-dmg` 的 URL 与 revision 是 `cargo-packager` 的编译期常量，配置无法覆盖；
其 `main` 分支仍然固定同一个 revision，因此没有"换一个受支持的版本"这条路。

最终做法：`.app` 仍然由 `cargo-packager` 产出（bundle 布局、`Info.plist`、资源、
图标、签名全部不变），随后由 `bongocat-packaging` 用 macOS 自带工具把它封装成
DMG：`ditto` 暂存 bundle、加入 `/Applications` 拖放入口、`hdiutil create -format UDZO`
压缩、`codesign` 签名。这恰好是 `create-dmg` 自己封装的东西，只少了它的脚本层。

**退出条件**：`cargo-packager` 换用可在当前 macOS 工作的 `create-dmg` revision 后，
改回 `PackageFormat::Dmg` 并删除这段代码。

### 5. Release workflow 由项目自己描述，原生 runner 构建

新增 `.github/workflows/release.yml`，在 tag 推送时于原生 runner 上运行同一套
`just build`：

- `windows-latest` → `just build`（NSIS 安装器）
- `macos-latest` → `just build --target aarch64-apple-darwin` 与
  `just build --target x86_64-apple-darwin`（两种架构的 `.app` + `.dmg`）

macOS 两种架构在同一个 runner 上构建，保证 SDK、工具链与配置完全一致；两个架构都用
`--target` 显式指定，因此产物集合不依赖 runner 恰好是哪一种架构。

## 备选方案

- **`cargo-dist` 0.32.0**：官方 GitHub Actions 生成、archives 与包管理器集成都很强，
  但它的产物模型里**没有 `.app` 或 `.dmg`**——macOS 只产出 `tar.xz` 归档，installer
  是 shell/PowerShell/Homebrew/NPM。BongoCat 明确要求 `.app` 与 `.dmg`，这是硬性阻塞，
  不是偏好差异。它的设计中心是"CI 发布归档"，也不适合"开发者本地一次命令拿到 `.dmg`"。
- **直接使用 `cargo-packager` CLI**：需要 `cargo install cargo-packager`，版本无法随仓库
  固定，且把配置放到 `Packager.toml` 会削弱编译期检查。库形式严格更好。
- **保留自研脚本**：正是本次要删除的东西；它已导致 Windows 打包依赖发布机器预置的
  NSIS 路径。
- **其他 Rust DMG 构建方案**：crates.io 上不存在成熟的 DMG 安装器构建 crate
  （`apple-dmg` / `udif` 是读写库，不是安装器构建器）。
- **`tauri-bundler`**：`cargo-packager` 的前身，桌面框架耦合更深，没有额外收益。

## 影响

- 打包行为的一部分由第三方工具决定，项目不再逐行控制 `Info.plist` 与安装器脚本。
  对应地，仓库保留 `macos/Info.plist` 作为**overlay**，只声明 `cargo-packager`
  不生成的键（`LSMultipleInstancesProhibited`、`NSPrincipalClass`）；其余键由工具生成，
  避免两处定义同一个值。
- Windows 安装目录由 `$LOCALAPPDATA\Programs\BongoCat` 变为 `$LOCALAPPDATA\BongoCat`
  （`cargo-packager` 的 currentUser 布局），注册表仍写在 HKCU。`next` 尚未发布，
  不存在需要迁移的已安装实例。
- `cargo-packager` 硬编码 Windows 安装器文件名为
  `{主二进制名}_{version}_{arch}-setup.exe`，且没有可配置项。发布资产名是产品决策，
  因此 `bongocat-packaging` 在打包完成后把已产出的安装器重命名为
  `BongoCat_<version>_x64.exe`（`<version>` 取自 `CARGO_PKG_VERSION`，架构取自
  `ReleaseTarget::architecture`，两者都随构建参数自动解析）。重命名只作用于文件本身，
  安装器内容、`Info.plist` 与 bundle 布局仍完全由 `cargo-packager` 拥有。
- 不再有 NSIS 许可证页面与 DMG EULA 页面（未配置 `license-file`）。
- **没有**产出可更新资产：`bongocat-update` 的 `self_update` 需要按 target triple 命名、
  macOS 根为 `BongoCat.app/`、Windows 根为 `bongocat-app.exe` 的归档。当前
  `RELEASE_SIGNING_KEY` 仍为 `None`（fail-closed），更新功能本身尚未可发布，因此这不是
  回归，而是一项**仍未完成、明确记录的发布门禁**。

## 已接受的残余风险

1. **`cargo-packager` 的 NSIS plugin 获取**：`nsis-3.09.zip` 与 `nsis_tauri_utils.dll`
   有 SHA-1 校验，但 `NSIS-ApplicationID.zip` 只下载、不校验，且来源是
   `tauri-apps/binary-releases` 而不是官方 SourceForge。ADR-0023 原先要求的"官方
   acquisition artifact + MD5 + `makensis /VERSION`"边界被替换成对
   `cargo-packager` 自身供应链的信任。这是明确接受的降级，不能声称等价。
2. **DMG 外观布局**：`cargo-packager` 的 `create-dmg` 路径会在挂载后调用 Finder
   AppleScript 排列图标，需要一次"自动化控制 Finder"授权；`CI=true` 时它会传
   `--skip-jenkins` 跳过。本项目已不经过该路径，因此不受影响；如将来恢复该路径，
   必须重新评估无人值守环境。
3. **`x86_64-apple-darwin` 产物在 CI 中不被原生执行**：CI 交叉编译并校验两种架构的
   bundle 结构，但只在宿主架构上运行应用。原生 Intel 机器上的运行验证仍是独立门禁。
4. **Windows 端到端未在本机验证**：Windows 安装器只能在 Windows runner 上验证，
   本次没有在本机执行过 Windows 构建。

## 验证

- `just build` 在本机（macOS 26.5 / aarch64）产出 `BongoCat.app` 与
  `BongoCat-<version>-arm64.dmg`；`plutil` 断言 bundle id、`LSMinimumSystemVersion`、
  `LSMultipleInstancesProhibited`、版本号；`codesign --verify --deep --strict` 通过；
  `.app` 内含 `Contents/Resources/models/{standard,keyboard,gamepad}` 与
  `Contents/Resources/build-provenance.json`。
- `just build --target x86_64-apple-darwin` 产出 x86_64 的 `.app` 与 `.dmg`。
- `tools/tests/` 下的契约测试改为断言新的打包配置：target 集合、产物集合、
  Windows 安装器发布名、bundle id、最低系统版本、版本号来源、与 `bongocat-update` 的
  target/命名一致性。
- release workflow 用 `just version` 校验 tag。

## 后续

1. 为更新流程产出按 target triple 命名的归档（macOS 根 `BongoCat.app/`，
   Windows 根 `bongocat-app.exe`），并接入 `zipsign` 签名与 `RELEASE_SIGNING_KEY`。
2. `cargo-packager` 修复 DMG 后删除本项目第 4 条的偏离。
3. 原生 Intel 机器与干净 Windows 10 1903+ / Windows 11 profile 上的安装、升级、
   卸载与回滚验证。
4. 上报 `create-dmg` / `cargo-packager` 的 `hdiutil` 解析缺陷。
