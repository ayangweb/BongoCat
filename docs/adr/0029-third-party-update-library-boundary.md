# ADR-0029: Third-Party Update Library Boundary

状态：已被 ADR-0034 取代（2026-09-14）
取代：ADR-0021、ADR-0022、ADR-0025、ADR-0026

> 后续修订（2026-09-23）：ADR-0033/ADR-0034 已把历史“四个 target”收口为 Windows x64、macOS arm64 与 macOS x64；本文的 `self_update`、zipsign、四目标和相应验证记录只作历史。

> **本文已失效，正文保留为历史记录。**
>
> 2026-09-14 的换实现表明，本文记录的 `self_update 1.3.0` + zipsign 归档内嵌签名模型有两条
> 硬性阻塞：zipsign 只能签 `.zip` 与 `.tar.gz`，裸 `.exe` / `.dmg` 必然验签失败；且
> `self_update` 的 replace-and-verify 语义不适用于 NSIS 这类系统安装器（上游 `lib.rs:302`）。
> 更新库已改为 `cargo-packager-updater`，信任模型已改为 detached minisign 签名，见
> **ADR-0034**。本文中的库选择、签名方案、依赖清单与验证证据**均不再有效**。
>
> 仍然有效的部分——构建期 channel 隔离、签名密钥缺失时失败关闭、稳定错误码收敛、
> 第三方类型不外泄为项目公共 API——已在 ADR-0034 中重述。
>
> 文末引用的 `archive_layout_capability.rs`、`local_install_rehearsal.rs` 与
> `multi_file_install_capability.rs` 已随该库退役删除，替代品是
> `release_manifest_capability.rs`。

## 背景

ADR-0021、ADR-0022、ADR-0025 与 ADR-0026 共同建立了一套自研更新栈：严格 manifest v1 schema、
detached Ed25519 清单签名、Development/Production channel 绑定、单调 `release_sequence` 防降级、
精确固定的 `ureq 3.4.0` 传输，以及一个独立承担全部校验职责的 update helper。

维护者决定：更新属于通用机制，不重复实现。第三方库提供完整实现时直接采用，包括其验证模型，
不再并行保留自研验证层。本 ADR 记录该决定并取代上述四个 ADR；被取代的 ADR 保留正文作为历史记录。

## 调研结论

已核实（2026-09-13，`cargo info` / `cargo tree` / 源码阅读）：

- `self_update 1.3.0`（MIT，Rust 1.88+）提供下载、tar/zip 解压、`zipsign` 归档签名校验、
  单文件自身替换、macOS `.app` 整包 rename 交换（含 stash 与 best-effort 回滚）以及进程重启。
  项目 toolchain 为 `1.97.1`，满足要求；`deny.toml` 允许 MIT 且只允许 crates.io registry。
- `UpdateConfig` 与 `ReleaseUpdate` 均为 **sealed**，外部无法自行实现，只能通过后端 builder
  构造。接入自有发行源需实现其 `ReleaseSource` trait 并使用 `backends::custom`，或使用内置
  `github` / `gitlab` / `gitea` / `s3` / `manifest` 后端。
- `self_update 1.3.0` **公开导出**本地下载、解压与文件搬运原语：`Download`、`Extract`、`Move`、
  `MoveAll` 都在 crate 根导出（`src/lib.rs:2216` / `1390` / `1922` / `2045`）。其中 `MoveAll` 是
  **事务式多文件安装器**：逐个把被替换的目标 stash 走再 `rename` 新文件就位，任一移动失败则逆序
  回滚全部已应用的移动。`github::Update::update()` 自身只走「单文件」与「macOS bundle」两条安装
  路径，但这些原语可用于自行编排多文件更新。
  > 更正（2026-09-13）：本 ADR 初稿曾写「`self_update 1.3.0` 不提供公开的本地归档解压或文件搬运
  > 工具（无 `Extract`、无 `MoveAll`），这些能力只在它自己的 `update()` 编排内部使用」。该结论
  > **错误**，且是我此前一次 BSD `grep` 误用（`\|` 交替在 BSD grep 的 BRE 下不生效）造成的假阴性。
  > 上述可见性与行为均已重新按源码确认，并由
  > `crates/bongocat-update/tests/multi_file_install_capability.rs` 在本机实测锁定。
- 未启用任何 HTTP client feature 时 `self_update` **无法编译**，上游使用无条件 `compile_error!`。
  因此依赖它就必然引入其网络层。
- 依赖 `ureq` feature 时，`self_update` 声明
  `ureq = { version = "3.0.6", default-features = false, features = ["gzip", "json", "socks-proxy", "charset"] }`。
  Cargo feature 在整个构建图中取并集，调用方的 `default-features = false` 无法阻止这些 feature 被启用。
  该冲突在 ADR-0025 下不可接受，在本 ADR 下随 ADR-0025 一并作废。
- **fail-open 行为**：`self_update::verify_signature` 在公钥集为空时**直接返回 `Ok(())`**，
  即静默跳过验签。任何转发空公钥列表的封装都会安装未签名产物而不自知。
- macOS 上 `bundle_install_path` 会从运行中的可执行文件自动推导其所属 `.app`，
  并对 Gatekeeper translocation（隔离属性导致运行于只读临时挂载）返回专门错误，
  而不是在中途 swap 时报出只读文件系统错误。

## 决策

- 删除 `bongocat-update` 的全部现有实现（9 个模块、4615 行），改为基于 `self_update 1.3.0`
  的薄封装。保留 crate 名称与诊断契约，其余全部替换。
- 采用 `self_update` 的**完整信任模型**：`zipsign` ed25519 归档签名。不再有 detached 清单签名、
  不再有先验签后解析、不再有 manifest v1 schema 与 1 MiB 上限。
- 采用内置 **`github` 后端**，仓库 `ayangweb/BongoCat`。发行资产须按目标 triple 命名以供匹配。
- 传输使用 `self_update` 的 **`ureq` feature**（而非 `reqwest`）。理由：项目本就固定 `ureq 3.4.0`，
  该选择避免引入 `reqwest` / `hyper` / `h2` / `tower-http` / `cookie_store` /
  `rustls-platform-verifier` / `aws-lc-rs` / `cmake` 整套依赖。实测真实编译图增量为
  **+17 个包**（462 → 479），`reqwest` 方案为 +31。
- 启用 features：`ureq`、`rustls`、`github`、`archive-tar`、`archive-zip`、
  `compression-tar-gz`、`compression-zip-deflate`、`checksums`、`signatures`。
  不启用 `progress-bar`（GPUI 应用无需终端进度条）。
- **channel 隔离保留**：`AGENTS.md` §10 要求更新 channel 按环境隔离，这是本 ADR 之外的约束。
  Development 构建的 `ReleaseChannel` 不允许联网或安装，`check()` / `install()` 在发出任何请求前
  即失败关闭。
- **签名密钥缺失时失败关闭**：由于上游空公钥集是 fail-open，`RELEASE_SIGNING_KEY` 为 `None` 时
  runtime 拒绝安装并返回 `update_signature_key_missing`。这是本项目对该 fail-open 的补偿。
- **稳定错误码保留**：原 32 个错误码收敛为 13 个，覆盖新实现的实际失败面。其中
  `update_download_transport_failed` 沿用原名，因为它是既有诊断导出契约的一部分
  （`ADR-0016` / `ADR-0027`）且被测试固定。
- 第三方错误、配置与平台类型不得扩散为项目公共 API。`self_update::Error` 在本 crate 边界内
  被映射为自有稳定码；`UpdateDiagnostics` 不含任何库类型。
- 依赖按 `AGENTS.md` §9 精确 pin（`=1.3.0`）并提交 `Cargo.lock`。
- 随本 ADR 一并移除因验证层退役而失效的 workspace 依赖：`ed25519-dalek`、`ureq`、`sha2`、`semver`。

## 被取代的 ADR 与其损失

以下能力随 ADR-0021 / ADR-0022 / ADR-0025 / ADR-0026 一并退役，`self_update` **没有**对应实现。
记录在此以免日后被误认为仍然有效：

| 退役能力 | 原依据 | 现状 |
| --- | --- | --- |
| detached Ed25519 清单签名、先验签后解析 | ADR-0021 | 由 zipsign 归档签名替代，信任模型不同 |
| 单调 `release_sequence` 防降级 | ADR-0021 | **无替代**。现仅按 semver 比较，降级攻击不再被检测 |
| manifest 层 Development/Production channel 绑定 | ADR-0021 | 改为构建期 `ReleaseChannel` 门禁，弱于原方案 |
| manifest v1 严格 schema、未知字段拒绝、1 MiB 上限 | ADR-0021 | **无替代** |
| 公钥轮换窗（key ID + channel + sequence 有效期） | ADR-0021 | **无替代**。zipsign 为 any-of 多密钥语义 |
| 4 个 target/arch 白名单在清单层强制 | ADR-0021 | 改为资产名匹配 + 构建期 target 常量 |
| 32 个稳定错误码作为公共契约 | ADR-0021 | 收敛为 13 个，语义不同 |
| `https_only`、禁止 redirect、15s 截止、仅接受 200、禁止透明压缩 | ADR-0025 | **无替代**。改由库的传输策略决定 |
| 独立且承担全部校验的 update helper | ADR-0026 | 由库内部编排替代 |
| `updates/` 目录 0700 与原子写入 | ADR-0021 | 随 sequence store 一并退役 |

`AGENTS.md` §10「更新只允许 HTTPS，校验版本、target、arch、hash 和签名，并提供失败回滚」
仍然成立：库分别以 HTTPS-only 传输、资产 target 匹配、构建期 target 常量、`checksums`
feature 与 `signatures` feature、install 阶段的 stash 回滚覆盖这些点。

## 验证

已完成（2026-09-13）：

- 依赖解析与编译：`self_update 1.3.0` 在 Rust 1.97.1 下与项目精确 pin 无冲突，
  `cargo check -p bongocat-update` 通过。
- 真实编译图增量实测为 +17 个包，且 `reqwest` / `aws-lc-rs` / `cmake` / `tower-http` /
  `cookie_store` / `rustls-platform-verifier` 均不在图中。
- `cargo fmt --all --check` 通过。
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` 零警告。
- `cargo test --locked --workspace` 全部通过：497 passed / 0 failed / 5 ignored。新增 13 个单元
  测试覆盖 channel 门禁、签名密钥失败关闭、错误码稳定性与诊断计数；既有 `bongocat-app` 诊断
  测试保持通过。
- `cargo check --locked --workspace --release` 通过。
- `./tools/check-native-dependencies.sh` 通过：四个首发 target 的 release 依赖树均无
  `tauri` / `wry` / `webview2` / `nodejs-sys` / `neon` / `deno_core` / `quickjs` /
  `javascriptcore`；14 个 manifest 全部 `licenses ok, sources ok`。新增的 `self_update` 及其
  传递依赖未触发任何许可证或来源违规，`deny.toml` 无需为它新增例外。
- `python3 tools/validate-json-schema.py` 通过（9 input / 9 expected / 10 config / 6 state），
  已不含 update 校验入口。
- 三平台 CI（run `34737269808`）首次运行暴露一个本机不可见的问题：Linux runner 不属于四个首发
  target，`ReleaseConfiguration::for_current_build` 在那里返回 `None`，导致两个直接用它构造
  runtime 的测试断言了错误的期望值（`left: None, right: Some("development")` 与
  `left: NotConfigured, right: SignatureKeyMissing`），`Test Native workspace (ubuntu-latest)`
  因此失败 2 项。已把这两个测试改为从显式 `ReleaseConfiguration` 构造 runtime，使其在任意宿主上
  都真正执行 channel 与签名密钥门禁，而不是被静默跳过；并新增
  `a_host_outside_the_shipped_targets_has_no_release_configuration` 覆盖 `None` 配置路径。
  本地复检 14 项测试通过、workspace 498 passed / 0 failed。
- 修复后三平台 CI（run `34738189816`，commit `b2b1f51`）**全绿：23/23 作业成功**，包括
  `Test Native workspace (ubuntu-latest)` / `(macos-latest)` / `(windows-latest)`、
  `Check Native dependency policy` 与全部平台 smoke。

- `bongocat-config::StorageLayout` 的 `update_staging` 字段（`<env>/updates/staging/`）已移除：
  `staging` 模块删除后该字段无任何写入方，`create_directories`、环境形状断言与 0700 权限断言
  已同步更新。
- `RELEASE_BINARY_NAME` 原先声明为 `BongoCat`，与实际发布的可执行文件 `bongocat-app` 不一致。
  由于 `self_update` 用它派生 Windows 归档内的提取路径，这个偏差会让第一次真实更新在解压阶段
  失败。已改为 `bongocat-app`，并新增 `tools/tests/test_update_release_contract.py` 把这层
  编译期不可见的耦合固定下来：二进制名 ↔ `bongocat-app` 包名 ↔ `build-windows.ps1` 的 exe 名、
  bundle 名 ↔ `package-macos.sh` 的 `.app` 名、仓库 owner/name ↔ workspace manifest 的
  `repository`、四个 target ↔ `deny.toml` 的 `[graph] targets`。
- 上述最后两项（`update_staging` 移除、`RELEASE_BINARY_NAME` 对齐与契约测试）由 run `34740872994`
  （commit `fe5a5de`）覆盖，同为 **23/23 作业全绿**；其中 `Validate shared fixtures` 作业运行
  `python3 -m unittest discover -s tools/tests`，即新增的契约测试已在 CI 上执行并通过。
- `MoveAll` 的事务语义已在本机实测，不依赖文档：`crates/bongocat-update/tests/multi_file_install_capability.rs`
  验证「可执行文件 + 资源文件」两条移动全部生效，以及第二条源缺失时第一条已应用的移动被回滚、
  未被触及的目标保持原内容。该测试**不覆盖**本项目尚未实现的多文件编排路径，只锁定库原语语义。
- 两种发行资产布局已在本机实测：`crates/bongocat-update/tests/archive_layout_capability.rs`
  用库自身的 `Extract` 验证 macOS 归档整包解压后 `BongoCat.app/` 位于归档根并携带
  `Contents/Resources/`、Windows 归档根级的 `bongocat-app.exe` 可被取出、以及多包一层目录时
  失败且目标目录保持为空。这三项均不依赖网络或签名密钥。
- 完整安装链路已在本机端到端演练（`crates/bongocat-update/tests/local_install_rehearsal.rs`）：
  用 `backends::custom`（未门控，可接任意 `ReleaseSource`）配合一个只回放固定字节的 loopback
  HTTP 服务，跑通**资产按 target triple 选择 → 下载 → 按扩展名识别归档 → 解压配置路径 → 安装**
  全链路，且使用的就是生产常量 `RELEASE_BINARY_NAME` / `RELEASE_BUNDLE_NAME`。两项覆盖：
  单文件替换落到配置的安装路径；bundle 模式整棵 `.app` 被替换、`Contents/Resources/` 随包到达、
  旧包遗留文件被清除。**不含**签名校验、GitHub 后端与进程重启。

## 待验证项（不得当作已确认）

1. 替换机制只在本机以「假安装目标」验证过，**未在真实发行物上验证**。已实测（见上一条）：
   单文件替换、`.app` 整包交换与旧包清理。**未实测**：Windows 上运行中的 exe 被改名替换、
   安装阶段失败后的回滚、替换后的进程重启、以及从真实 GitHub 发行下载。真实 `.app` 还需
   签名与公证，当前打包脚本只做 ad-hoc 签名。
2. 未配置真实 endpoint 与签名密钥。`RELEASE_SIGNING_KEY` 为 `None`，因此当前构建
   `update_check_available()` 恒为 `false`，更新入口在 UI 中不显示。
3. **发行流程尚未产出 `self_update` 可消费的归档。** 读 `self_update 1.3.0` 源码确认的硬性要求：
   - **资产名匹配**：`Release::asset_for` 先用**完整 target triple** 匹配资产名，失败后退化为
     `arch` + `os` 标记（`arch` 取 triple 首段，`os` ∈ `linux`/`darwin`/`windows`/…）。
     **`bin_name` 不参与资产名匹配**。因此资产名必须包含 target triple，例如
     `BongoCat-<version>-aarch64-apple-darwin.tar.gz`。
   - **归档类型按扩展名判定**（`detect_archive`）：`.zip` / `.tar` / `.tar.gz` / `.tar.xz` / `.gz` /
     `.xz` 走对应解压器，**其余任何扩展名（含 `.exe`）落入 `ArchiveKind::Plain(None)`，即当作裸
     单文件直接使用**。因此 Windows 资产**不必是压缩包**，一个裸 `BongoCat.exe` 就能被消费；macOS
     bundle 模式则必须用归档。
   - **归档内布局**：bundle 模式走 `install_bundle`，它用 `Extract::extract_into` **解压整个归档**
     再从解压根取 `bundle_path_in_archive`，因此归档根必须是 `BongoCat.app/` 目录
     （注意 `Extract::extract_file` 只能取单个文件，取目录会失败——这就是 bundle 模式用
     `extract_into` 的原因）。单文件模式下 `bin_path_in_archive` 由 `bin_name` 派生为
     `bongocat-app.exe`，裸文件或归档内的该路径都必须是**根级**，多套一层目录会直接报错而非
     静默取错文件。
   - **现状**：`scripts/package-macos.sh` 只产出 `target/package/BongoCat.app`，**不产归档**；
     `scripts/build-windows.ps1` 只产出 NSIS 安装器 `BongoCat-$version-x64-setup.exe`。
     两者都还需要新增发行步骤，本次未实现。
     > 注（2026-09-14）：上面两个脚本已随 ADR-0033 删除。当前打包入口只产出 `.app`、`.dmg` 与
     > Windows NSIS 安装器，**仍然不产可更新归档**，结论与上面一致：更新资产是一项未完成的
     > 发布门禁。ADR-0033 记录了新的打包链路与这条缺口。
   - 上述布局要求已在本机实测（不依赖网络）：
     `crates/bongocat-update/tests/archive_layout_capability.rs` 用库自身的 `Extract` 验证
     macOS 归档整包解压后 `BongoCat.app/` 位于根部且携带 `Resources/`、Windows 归档根级的
     `bongocat-app.exe` 可被取出、以及被多包一层目录时会失败且不留残留。
   - **Windows 多文件更新需要自行编排**：`github::Update::update()` 的单文件模式只替换可执行文件，
     **不会更新 `resources/`**，预置模型或 `resources/` 内容的变更无法经由此路径下发。库为此提供了
     公开原语：用 `Download` + `Extract` 取得文件，再用 `MoveAll` 完成「二进制 + 附属资源」的事务式
     替换（全成或全回滚）。该语义已在本机实测确认，见
     `crates/bongocat-update/tests/multi_file_install_capability.rs`。代价是这条路径**不走**
     `update()` 编排，需自行保证暂存目录与目标同文件系统、zipsign 校验仍然执行、以及失败后的
     启动恢复。**本项目尚未实现这条路径。**
   - macOS 归档需携带已签名并公证的 `.app`；`package-macos.sh` 目前只做 ad-hoc 签名，
     分发签名与公证仍是独立发布门禁。
4. **断电/强杀残留的清理策略仍未决定，但残留形态已查实**（读 `self_update 1.3.0`、
   `self-replace 1.5.0` 与 `tempfile` 源码）：
   - 下载与解压暂存使用 `tempfile::TempDir`（默认前缀 `.tmp`）：单文件模式建在**系统临时目录**，
     macOS bundle 模式建在**安装目标的父目录**（即 `.app` 旁边）。正常返回或 panic 时 `Drop` 会删除，
     **只有硬断电或 SIGKILL 才会留下 `.tmp*` 目录**。
   - Windows 的单文件替换由 `self-replace` 完成：它先把当前 exe **改名挪开**，再复制自身为
     `*.__selfdelete__.exe`、以 `FILE_FLAG_DELETE_ON_CLOSE` 打开并 spawn 该副本，等父进程退出后
     删除被挪开的旧 exe。硬断电可能同时留下被挪开的旧 exe 与 `.__selfdelete__.exe` 副本。
   - 本项目**不提供**启动时清理。任何清理都必须 (a) 不在更新路径内运行、(b) 绝不删除可能属于
     进行中替换的 `.__selfdelete__.exe`、(c) 不与并发更新争用同一暂存目录。
   - 附带约束：自有可执行文件**不得**以 `.__selfdelete__.exe` 结尾——`self-replace` 的删除胶水按该
     后缀判定，误命名会触发非预期行为。
5. CI 覆盖面：`native-rewrite-phase0.yml` 的 `dependency-policy` 作业会运行
   `./tools/check-native-dependencies.sh`，`native-workspace` 作业在 Windows/macOS/Ubuntu 三平台
   运行 fmt / Clippy / test / release check / production 环境构建。因此本 ADR 的门禁已在 CI 覆盖，
   但仍未在**真实发行流程**中验证：发行资产命名、签名与安装包产物未与本 ADR 的 target 匹配规则
   对齐（见待验证项 3）。
6. `StorageLayout::updates`（`<env>/updates/`）当前**无写入方**：`self_update` 把解压暂存目录建在
   可执行文件旁边，不落在环境根下。该目录被保留为环境私有的保留命名空间，仍由环境形状断言与
   0700 权限断言覆盖。若确认不需要，可另行移除；本次未动，以免改变已文档化的环境目录形状。

## 后续边界

本 ADR 不实现 endpoint、公钥注入、update worker、更新 UI、OS 包签名验证或启动恢复。
在待验证项 1 与 2 完成前，不得声称更新功能或 stable 发布完成。
