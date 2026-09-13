# ADR-0029: Third-Party Update Library Boundary

状态：已接受（2026-09-13）

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
- `self_update 1.3.0` **不提供**公开的本地归档解压或文件搬运工具（无 `Extract`、无 `MoveAll`）。
  这些能力只在它自己的 `update()` 编排内部使用。
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

## 待验证项（不得当作已确认）

1. 未在真实 Windows / macOS 上执行替换与重启。`.app` 整包交换、Windows 运行中 exe 改名、
   替换失败后的启动恢复均**未实测**。
2. 未配置真实 endpoint 与签名密钥。`RELEASE_SIGNING_KEY` 为 `None`，因此当前构建
   `update_check_available()` 恒为 `false`，更新入口在 UI 中不显示。
3. 发行资产的命名约定尚未与 `self_update` 的 target 匹配规则对齐；打包脚本
   （`scripts/package-macos.sh`、`windows/installer/BongoCat.nsi`）未修改。
4. 断电残留的 `.` 前缀临时文件清理策略未决定。上游明确不建议启动时自动清理。
5. CI 覆盖面：`native-rewrite-phase0.yml` 的 `dependency-policy` 作业会运行
   `./tools/check-native-dependencies.sh`，`native-workspace` 作业在 Windows/macOS/Ubuntu 三平台
   运行 fmt / Clippy / test / release check / production 环境构建。因此本 ADR 的门禁已在 CI 覆盖，
   但仍未在**真实发行流程**中验证：发行资产命名、签名与安装包产物未与本 ADR 的 target 匹配规则
   对齐（见待验证项 3）。
6. `bongocat-config::StorageLayout` 仍保留 `update_staging`（`<env>/updates/staging/`）字段，
   但 `staging` 模块已删除，**当前没有任何写入方**。该字段属于 `bongocat-config` 的公开布局契约
   与环境目录形状测试，超出本次 `bongocat-update` 替换范围，故未一并移除。若要清理，需在
   `bongocat-config` 内单独变更并同步目录形状断言。

## 后续边界

本 ADR 不实现 endpoint、公钥注入、update worker、更新 UI、OS 包签名验证或启动恢复。
在待验证项 1 与 2 完成前，不得声称更新功能或 stable 发布完成。
