# ADR-0034: Detached Minisign Update Trust Model

状态：已接受（2026-09-14；§3 的 manifest 形状于同日修订，见该节修订说明）
取代：ADR-0029 的更新库与签名部分（正文保留为历史记录）

## 背景

ADR-0029 把更新栈换成 `self_update 1.3.0`，信任模型为 zipsign ed25519 **归档内嵌**签名。
换实现之后暴露了两个问题，两个都是"能力缺失"而不是"实现缺陷"，且都由源码阅读确认：

1. **zipsign 只能签 `.zip` 与 `.tar.gz`。** `self_update` 的 `detect_archive()` 按扩展名判定
   归档类型，裸 `.exe` 与 `.dmg` 落入 `ArchiveKind::Plain(None)`，`verify_signature()` 直接返回
   `Error::NoSignatures`。而产品在 Windows 上发布的更新产物就是那个 NSIS 安装器 `.exe`。
   一旦 `verifying_keys` 非空，**Windows 必然验签失败**；不配置密钥则等于不验签。
2. **`self_update` 的替换语义不适用于系统安装器。** 上游 `src/lib.rs:302` 明确：
   `.deb` / `.msi` 这类系统安装器不在 replace-and-verify 语义内，需要调用方自己交给
   `dpkg -i` / `msiexec /i`。NSIS `.exe` 同理——用安装器去替换 `bongocat-app.exe` 在语义上
   就是把应用本体换成一个安装器。走 zipsign 就意味着 Windows 必须自研
   「下载 → 验签 → 静默运行安装器 → 重启」这条编排。

同时维护者给定了产物形状约束：macOS 为 `.dmg` + `.app.tar.gz`，Windows 只有 `.exe`，
两平台各只额外增加一个 `.sig`，不引入其他文件。这正是 Tauri 的 minisign 分离签名模型。

调研过程与结论的完整记录见 `docs/update-signing-guide.md`（解释性文档，非规范性）。

## 调研结论

已核实（2026-09-14，读 `cargo-packager 0.11.8`、`cargo-packager-updater 0.2.3` 与
`minisign 0.7.9` 源码，未执行任何真实签名或真实更新）：

- **签名端已经在依赖图里。** `cargo-packager 0.11.8` 公开 `pub mod sign`：
  `generate_key`、`save_keypair`、`sign_file`、`sign_file_with_secret_key`、`SigningConfig`；
  其 `minisign 0.7.9` 已由 `cargo-packager` 引入。**签名工具与产物出自同一个 crate**，
  不存在"两个工具各自演进"的漂移面。
- **`cargo-packager-updater 0.2.3` 是同一项目的消费端**，覆盖 manifest 获取 → 版本比较 →
  下载 → 验签 → 安装的完整链路，且**自带 Windows 安装步骤**：`UpdateFormat::Nsis` 会用
  PowerShell `Start-Process` 运行下载到的安装器，随后 `std::process::exit(0)`；
  `UpdateFormat::App` 则整包替换 macOS bundle。这正是 ADR-0029 缺失的那一半。
- **验签发生在安装之前。** `Update::download()` 读完 payload 后立即调
  `verify_signature(...)`，`install()` 只接受已验签的字节。失败即返回，不进入安装。
- **manifest 有两种形状**（`RemoteReleaseData`，`#[serde(untagged)]`）：*dynamic*（顶层
  `url`/`signature`/`format`，一份 manifest 描述一个 target）与 *static*（`platforms` 映射，
  键为 `<os>-<arch>`）。dynamic 形状没有 per-platform 条目，因此**它不可能报 target miss**。
- **`WindowsConfig { install_mode: Quiet }` → NSIS 参数 `["/S", "/R"]`**；`installer_args: None`
  不追加任何参数。`/S` 静默，`/R` 要求安装器结束后重启应用——Windows 路径依赖它，因为库
  自己 `exit(0)`。
- **`SigningConfig.private_key` 是 base64 文本，不是路径**（`decode_private_key` 先 base64
  解码再解析 minisign `SecretKeyBox`），所以 CI secret 可以直接整段传入，不需要先落盘。
- **minisign 与 zipsign 密码学上不互通**：minisign 用 BLAKE2b-512 预哈希并比对 8 字节 key ID，
  zipsign 用 SHA-512。换模型等于换信任模型，不是换封装。
- **`Config.pubkey` 是单个 `String`**：验签只支持**一把**公钥，没有多密钥 any-of 语义。

## 决策

### 1. 以 `cargo-packager-updater` 取代 `self_update`，保留 `bongocat-update` 的对外契约

采用 `cargo-packager-updater =0.2.3`（`default-features = false`，仅 `rustls-tls`）。
`bongocat-update` 的公开面不变：`ReleaseConfiguration`、`ReleaseChannel`、
`UpdateTargetTriple`、`UpdateRuntime::{check, install, restart}`、`UpdateError`、
`UpdateOutcome`、13 个稳定错误码与 10 项匿名计数。（错误码目录于 2026-09-15 增至 14 个：`ReleaseFetchFailed` 原本兼任「取不到发布信息」与「取到了但读不懂」，ADR-0035 把这两件事拆成两个码。新增码向后兼容，见该 ADR 的「真实端点首次运行的结果」。）

库的 `Error`（`#[non_exhaustive]`）、`Config`、`semver::Version` 与 `Url` 全部映射为项目
自有类型，不出现在 `diagnostics.rs` 或 app/UI 协议中；未识别的库变体降级为
`update_internal_failed`，不把库文本带进诊断导出。

### 2. 信任模型：detached minisign 签名，密钥缺失时失败关闭

`RELEASE_SIGNING_KEY: Option<&str>` 存放 **base64 的 minisign 公钥盒文本**（即
`cargo packager signer generate-key` 写出的 `.pub` 文件内容）。仓库现已内嵌一把发布公钥
（key ID `DF5E2C9D255DD85E`），Production 构建的 `UpdateRuntime::is_available()` 因而可以为真。
公钥为 `None`、空串或纯空白时，runtime 在**发出任何请求之前**返回
`update_signature_key_missing`；系统菜单不会在没有有效公钥时显示「检查更新」入口。

这条门禁是刻意的第二道防线。库对空 `pubkey` 的行为是解码失败（`Error::Base64` /
`Minisign`），即 fail-closed 而非 ADR-0029 记录的 `self_update` fail-open；但项目仍保留
自己的门禁，因为它产出的是稳定、无路径的错误码，而不是一个库错误。

### 3. 一份共享 manifest，static 形状

runtime 请求 `releases/latest/download/latest.json`，manifest 是库的 **static** 形状：顶层
`version` 加一个 `platforms` 映射，键为 `<os>-<arch>`，每个条目含 `url`、`signature`、`format`。

`crates/bongocat-packaging` 仍然**每个 target 写一份 fragment**（`<os>-<arch>.json`），因为每个构建
job 只能声明自己产出的载荷；发布前由同一个工具用 `--merge-manifests` 把 fragment 合并成
`latest.json`。合并放在打包工具而不是 CI 里，理由与打包本身一致：manifest 的形状与资产名必须由
一处拥有，工作流只负责调用工具。

平台键拼写必须与库的 `get_updater_target()` / `get_updater_arch()` 一致——是 `macos` 而非
`darwin`，是 `aarch64` 而非 `arm64`。fragment 的文件名就是该键，合并会校验它属于已发布 target，
拼错的文件名会让发布失败而不是产出一个没有 host 能命中的条目。

> **修订（2026-09-14，同日）：** 本节原决定是"每个 target 一份 manifest，dynamic 形状"，理由是
> 避免跨 job 合并。产品要求把检查地址固定为 `.../releases/latest/download/latest.json`，而单一
> 文件名下三个 job 会互相覆盖、最终所有平台拿到同一个载荷，因此必须改为共享 manifest + 合并。
> 平台门禁随之回到 manifest 内：`UpdateErrorCode::NoMatchingAsset` 重新变为可达（发布漏掉某个
> 平台键时会命中它）。fragment 形状不变，只是从"被 runtime 读取的 manifest"降级为"合并的输入"。

### 4. 更新载荷：macOS 用 bundle 归档，Windows 复用已发布的安装器

| target | 更新载荷 | 签名 |
| --- | --- | --- |
| `aarch64-apple-darwin` / `x86_64-apple-darwin` | `BongoCat-<version>-<triple>.app.tar.gz`，归档根为 `BongoCat.app/` | `.app.tar.gz.sig` |
| `x86_64-pc-windows-msvc` | 已发布的 NSIS 安装器 `BongoCat_<version>_x64.exe` | `.exe.sig` |

macOS 的归档由 `crates/bongocat-packaging` 用 `tar` + `flate2` 从已完成的 `.app` 生成，
`follow_symlinks(false)` 保留 bundle 内部链接。库在 `UpdateFormat::App` 下丢弃归档的根条目
再把其余内容装到 bundle 路径，所以归档根**必须**是 `BongoCat.app/`。

`.dmg` **不是**更新载荷：它是人工安装路径（拖入 `/Applications`），库没有安装它的能力。

### 5. 签名在 `crates/bongocat-packaging` 内完成，且是最后一步

签名密钥通过 `SIGNING_PRIVATE_KEY` / `SIGNING_PRIVATE_KEY_PASSWORD`
注入，与既有的 `BONGOCAT_MACOS_SIGNING_IDENTITY` 同一形状——**凭据注入点，不是隐藏构建步骤**。
未设置即跳过签名，本地构建因此只产出 bundle 与安装器，不产出更新资产。

密钥对由**同一个入口**一次性生成：`just keygen <file>`（`crates/bongocat-packaging` 的
`--generate-signing-key`）。它用同一份精确 pin 的 `cargo-packager` 调 `generate_key` /
`save_keypair`，因此不需要任何全局 `cargo install`；生成后显式把私钥收紧到 `0600`，并打印公钥
那一行供填入 `RELEASE_SIGNING_KEY`。生成是离线的运维动作，不在 CI 中执行。

release workflow 反过来**断言**发布构建一定签过名：未配置 secret 时 job 直接失败，
并在打包后逐项校验载荷、`.sig` 与 manifest 存在且形状正确。这消除了"发布一批无人能更新的
产物"这一类静默失败。

顺序上签名必须最后发生：minisign 签名覆盖发布时的确切字节，签名之后再改名、重压缩或
`strip` 都会让签名失效。manifest 的 URL 因此在签名之后才写入（载荷文件名已经确定）。

### 6. 退役 `self_update` 专属的能力测试，改为钉住真实的签名/验签对

`archive_layout_capability.rs`、`local_install_rehearsal.rs` 与
`multi_file_install_capability.rs` 锁定的是 `self_update` 的 `Extract` / `MoveAll` /
`backends::custom` 原语，随该库一并退役。

替代品 `release_manifest_capability.rs` 用 loopback HTTP 服务把
**`cargo_packager::sign::sign_file`（生产端用的签名器）与 `cargo-packager-updater`（应用真正
运行的验签器）**放在一起跑，覆盖：共享 manifest 与它的 `<os>-<arch>` 平台键查找、被篡改的载荷、
由未知密钥签名的载荷、空公钥、以及（macOS）整包替换。

manifest 形状的跨 crate 一致性不靠正则匹配源码，而是由 `crates/bongocat-packaging` 的合并测试
把**自己写出的 `latest.json` 喂给更新库自己的读取类型**（`cargo-packager-updater` 作为 dev 依赖），
因此形状不匹配会在这里失败而不是在用户机器上。`tools/tests/test_update_release_contract.py`
覆盖另一半——资产名与平台键必须与 runtime 声明的一致。

### 7. 更新源的 GitHub 代理回退（2026-09-15 增补）

国内直连 GitHub 不稳定，一次更新运行在请求共享 manifest 时按固定顺序逐个尝试代理源
（`<proxy>/https://github.com/.../releases/latest/download/latest.json`）：

1. `https://cdn.gh-proxy.org`
2. `https://v6.gh-proxy.org`
3. `https://axisnow.gh-proxy.org`
4. `https://v4.gh-proxy.org`
5. `https://gh-proxy.org`
6. GitHub 官方源（兜底，不加前缀）

一个源**可用**的判定与库对 endpoint 的判定一致：请求返回成功状态**且** body 能被更新库
作为本发布管线的 manifest 反序列化（§3 的 static 形状）。超时、网络错误、HTTP 错误或
内容不可解析都跳到下一个源；全部失败时报最后一个错误，错误码目录不变。
`NoMatchingAsset`（manifest 可读但不含本平台条目）不触发回退——所有源服务的是同一份
release 资产，换源不会改变答案，该诊断必须原样到达用户。

成功的代理贯穿本次更新：`cargo-packager-updater 0.2.3` 的 `Update.download_url` 是公开
字段，runtime 在下载前把 manifest 里的官方 GitHub URL 统一转换为
`<proxy>/<official-url>`；转换只作用于 scheme 为 HTTPS 且 host 为 `github.com` 的 URL，
因此已代理的 URL（host 是代理本身）不会被二次加前缀，非 GitHub URL 不被改动。官方源
成功的运行不做任何转换。`release_page_url`（人工查看 changelog 的页面）不属于传输路径，
不转换。

单次 manifest 请求有独立的 30 秒上界（`UPDATE_MANIFEST_REQUEST_TIMEOUT`），低于载荷
传输的 1800 秒上界（`UPDATE_REQUEST_TIMEOUT`）：源是串行尝试的，一个黑洞连接若占用
传输级超时会把后续每个源都堵死 30 分钟。成功的源确定后，超时在 `Update.timeout` 上恢复
为传输级上界再下载。

信任模型不受影响（§2）：代理只是 URL 前缀转发，能迟滞一次运行、返回过期或伪造的
manifest、把下载指去别处，但无法伪造 minisign 签名，任何被篡改的内容到不了安装步骤；
"旧但签名有效"的降级风险即 §待验证项 5 的既有风险，代理化不改变它。该回退由
`runtime.rs` 的单元测试钉住（代理列表的顺序与字面量、代理 endpoint 的精确形态、
逐源尝试/首个可用即停/官方兜底/全失败报最后错误、`NoMatchingAsset` 短路、下载 URL
转换的幂等性与范围、超时上界），Windows 与 macOS 走同一段平台无关代码。

## 被取代的能力与损失

ADR-0021 / ADR-0022 / ADR-0025 / ADR-0026 的退役能力沿用 ADR-0029 的记录（detached 清单
签名、manifest v1 严格 schema 与 1 MiB 上限、`https_only` / 禁止 redirect / 15s 截止 /
仅接受 200 / 禁止透明压缩、独立 update helper、`updates/` 目录 0700 与原子写入）。
相对 ADR-0029，本次换实现又新增了以下损失，记录在此以免日后被误认为仍然有效：

| 能力 | ADR-0029 时的状态 | 现状 |
| --- | --- | --- |
| 归档完整性校验（`checksums` feature，用 GitHub 公布的 per-asset sha256） | 存在，但上游明确它只是完整性检查、不能替代签名 | **无替代**。`UpdateErrorCode::ChecksumMismatch` 已无产出路径（`diagnostics.rs` 已注明） |
| per-platform 资产匹配（`asset_for` 按 triple 匹配资产名） | 存在 | **无替代**。平台选择改为 manifest 内的 `<os>-<arch>` 键，而不是按资产名匹配；`UpdateErrorCode::NoMatchingAsset` 因此仍然可达（发布漏掉某个平台键时命中） |
| 公钥轮换窗 | **无替代**（zipsign 至少是 any-of 多密钥语义） | **更弱**。`Config.pubkey` 是单把公钥，无 key ID 白名单、无有效期概念 |
| 单调 `release_sequence` 防降级 | **无替代**，仅按 semver 比较 | 同左，未改变 |
| 归档内嵌签名（签名随产物走，不需要额外资产） | 存在 | 改为**分离** `.sig`：每个被签产物 +1 个文件（这正是维护者要求的形状） |

另外两条由库决定、不是项目选择的行为，一并记录：

- 库调用 `minisign-verify` 的 `verify(data, signature, allow_legacy = true)`，因此**同时接受**
  minisign 的 legacy（非预哈希）签名，而不只是 `cargo-packager` 产出的 prehashed 签名。
  这是比 `allow_legacy = false` 更宽的接受面。
- 验签前把整个 payload `read_to_end` 进内存。当前载荷规模（约百 MB 量级）可接受，但它是
  一条随产物增长而增长的常驻内存路径。

## 已接受的残余风险

- **Windows 安装路径未在本机验证。** 本机是 macOS。`install_mode = Quiet` 对应的
  `/S` 静默开关、`/R` 重启语义、以及"安装器替换被占用文件"的可行性均来自源码阅读，
  未在真实 Windows 上跑通。per-user NSIS 安装不需要提权，这一点与 `Quiet` 的前提一致。
- **发布私钥的 CI secret 与真实发布链路尚未验证。** 客户端已内嵌公钥
  (`DF5E2C9D255DD85E`)，但仓库无法得知 GitHub 上 `SIGNING_PRIVATE_KEY` /
  `SIGNING_PRIVATE_KEY_PASSWORD` 是否已设置。未设置时 release workflow 会在
  「Require the update signing key」一步主动失败；这是刻意的：在没有可用密钥的情况下发布，
  比发布失败更糟。真实签名、上传、下载与安装仍需一次完整 release 才能确认。
- **minisign 私钥默认无口令保护。** `generate_key(Some(String::new()))` 产出的私钥是明文
  base64；`SigningConfig.password` 只在生成时设了口令才有意义。密钥的离线备份、权限
  （`0600`）与轮换流程仍是运维职责，不是代码保证。
- **签名/验签从未在真实发布上执行过。** 全部证据来自 loopback fixture 与源码阅读。
- **manifest 合并是本项目新增的自有步骤。** 更新库只消费一份 manifest，不提供合并；共享
  `latest.json` 由 `crates/bongocat-packaging --merge-manifests` 生成。它是本项目代码，因此它的
  正确性（版本一致性、平台键合法性、重复键拒绝）由本项目测试保证，不由上游保证。

## 验证

已完成（2026-09-14，本机 macOS 26.5 / aarch64）：

- `cargo fmt --all --check` 通过。
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` 零警告。
- `cargo test --locked -p bongocat-update`：17 个单元测试 + 8 个 `release_manifest_capability`
  测试通过，含共享 manifest 的名称、平台键查找与 target 无关的 endpoint 断言。
- `cargo test --locked -p bongocat-packaging`：12 个单元测试通过，含合并产物被更新库自身读取
  类型反序列化成功、版本不一致/非法平台键/空输入被拒绝。
- `python3 -m unittest discover -s tools/tests` 全部通过，含 manifest 资产名一致性契约与
  依赖策略目标一致性契约。
- 端到端冒烟：用三份 fragment 跑 `just manifest`，产出的 `latest.json` 含 `version` 与三个
  `platforms` 键，Windows 条目为 `nsis`、macOS 条目为 `app`。
- 依赖图：`self_update`、`zipsign-api`、`ureq`、`sha2`、`semver` 已从 workspace 依赖与
  `Cargo.lock` 中移除；`ed25519-dalek` 的失效声明一并删除。`cargo update` 无待解析变更。
- 签名器与验签器的互操作由 `release_manifest_capability.rs` 实测锁定（**生成真实密钥对、
  真实签名、真实验签**，只是走 loopback 而非 GitHub）。

## 待验证项（不得当作已确认）

1. **真实发布链路**：从真实 GitHub Release 下载、验签并安装，未执行。
   - 补充（2026-09-15）：**manifest 获取这一步已在真实端点上执行**。结论是端点被旧产物占用——
     `releases/latest/download/latest.json` 返回 200，但内容是旧 Tauri 版本的 updater manifest
     （平台键 `darwin-*`、条目缺本库必需的 `format`、签名为 Tauri 私钥），因此客户端在平台查找前
     反序列化失败。下载、验签与安装仍未执行，线上尚无可用的新流程 release。排查细节、由此修正的
     诊断粒度与新增的能力测试见 ADR-0035 的"真实端点首次运行的结果"。
2. **Windows**：安装器静默安装、`/R` 重启、被占用可执行文件的替换、安装失败回滚，均未实测。
3. **`.app` 的 codesign + notarize**：当前打包只做 ad-hoc 或注入身份的 `codesign`，
   公证仍是独立发布门禁。归档必须在公证**之后**生成，否则签名对象不是最终分发的 bundle。
4. **密钥轮换**：单公钥模型下，换钥匙必须先发布一个"认识新钥匙"的版本，否则已安装用户
   的更新链直接断裂。轮换流程尚未设计。
5. **降级攻击**：仍只按 semver 比较。manifest 由 HTTPS 保护，但能够替换 manifest 的攻击者
   可以把客户端指向一个**旧但签名有效**的载荷。
6. **内存占用**：库把整个载荷读进内存后验签，大产物下的峰值内存未测量。

## 后续边界

本 ADR 不实现 endpoint 配置界面、公钥注入的自动化、update worker、更新 UI、操作系统包签名
验证或失败启动恢复。在待验证项 1 与 2 完成前，不得声称更新功能或 stable 发布完成。
