# 桌面应用更新签名机制入门

> **文档性质：解释性说明，非规范性事实来源。**
> 本文只用于讲解概念与记录调研结论，不定义目标架构。
> 规范性事实仍以 `docs/technical-design.md`、`docs/implementation-todo.md` 和
> `docs/adr/` 为准。
> 如果本文的结论要变成项目约定，必须先写成 ADR（见 §6.3）。
>
> ---
>
> **状态更新（2026-09-14）：本文的结论已经落地。**
>
> §0.3 推荐的方案 B（detached minisign 签名）已被采纳并实现，规范记录见
> **`docs/adr/0034-detached-minisign-update-trust-model.md`**：更新库改为
> `cargo-packager-updater 0.2.3`，签名端由 `crates/bongocat-packaging` 调用
> `cargo_packager::sign`，macOS 载荷是 `.app.tar.gz`、Windows 载荷复用 NSIS 安装器，
> 各附一个 `.sig`。检查地址是
> `https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json`：**一份共享
> manifest**（`platforms` 映射按 `<os>-<arch>` 给出每个 target 的载荷），由每个构建 job 写出的
> `<os>-<arch>.json` fragment 在发布前用 `just manifest` 合并而成。
>
> 因此，**下文凡是把 "`self_update` + zipsign 内嵌签名" 描述为"本仓库当前实现"的段落，
> 指的都是本文写作时（2026-09-14 上午）的 ADR-0029 状态，现已退役**。§2.2、§4.1、
> §4.2、§5.2、§5.3 属于该状态的技术记录，保留作为决策依据与历史对照，不再描述现状。
> §6.2 的落地步骤已执行完毕；§7 清单中已完成的项在 §6.2 中可查到对应实现。
> 仍未被验证的事项以 ADR-0034 的"待验证项"为准——本文 §8 的若干条目已由
> `release_manifest_capability.rs` 的 loopback 能力测试覆盖（真实密钥对、真实签名、真实验签）。

写作日期：2026-09-14。所有"已确认"结论均来自本机源码阅读，**没有执行过任何一次真实签名
或真实更新**；未验证事项集中列在 §8。

---

## 0. 先给结论

### 0.1 你的产物形状与仓库现状不匹配

你描述的产物是：macOS = `.dmg` + `.app.tar.gz`，Windows = 只有一个 `.exe`，两平台各加一个
`.sig`。这是 **Tauri 的模型**（minisign 分离签名）。本文写作时本仓库的实现是
**`self_update` + zipsign 内嵌签名**（ADR-0029，现已退役），两者不兼容。三个当时已核实的硬事实：

| 判断 | 依据（本机源码） |
| --- | --- |
| 裸 `.exe` 和 `.dmg` **无法**被 zipsign 签名，也无法通过验签 | `self_update` 的 `detect_archive()`：只有 `.zip` 和 `.tar.gz` 有归档类型，**其余任何扩展名（含 `.exe`/`.dmg`）落入 `ArchiveKind::Plain(None)`**；`verify_signature()` 对非 zip/tar.gz 直接返回 `Error::NoSignatures` |
| zipsign **不产生** `.sig` 文件，签名写在归档内部 | `zipsign-api` 的 tar 实现把签名块 base64 后作为**追加的 gzip 成员注释**写入；zip 实现把签名块**前置**到归档前。不存在独立签名文件 |
| `cargo-packager`（你已在用的打包工具）**自带** Tauri 式 minisign `.sig` 能力 | `pub mod sign`，公开 `generate_key` / `sign_file` / `sign_outputs` / `SigningConfig`；依赖里已有 `minisign 0.7` |

### 0.2 Windows 用 `.exe` 是当前实现的必然失败

即使先不管签名格式：`self_update` 的安装语义是**用下载物替换自己**（单文件模式替换
`bongocat-app.exe`，bundle 模式整包交换 `.app`）。用 NSIS 安装器去替换 `bongocat-app.exe`
在语义上就是把应用本体换成一个安装器。上游对此有明确声明（`self_update-1.3.0/src/lib.rs:302`）：

> `.deb` / `.msi` packages are a different shape entirely — hand the downloaded file to
> `dpkg -i` / `msiexec /i` yourself; the crate's replace-and-verify semantics do not apply
> to a system installer.

所以「Windows 只有一个 `.exe`」等价于「Windows 必须自己走『下载 → 校验 → 静默运行安装器』」，
这条路 `self_update` 不提供，必须自研编排。

### 0.3 推荐（已于 2026-09-14 采纳，见 ADR-0034）

**采用 `cargo-packager` 的 minisign 签名（生产端）+ `minisign-verify`（消费端）**，理由是它
同时满足你的产物约束和"不重复实现通用机制"的项目纪律：签名工具已经在依赖图里，`.app.tar.gz`
和 `.sig` 由现有工具生成，不需要引入新工具链。代价是要写一个新 ADR 取代 ADR-0029 的签名部分，
并在 Windows 上单独实现"运行安装器"的安装步骤。

> 落地时发现后一项代价比预期小：消费端不必自研。**`cargo-packager-updater` 与
> `cargo-packager` 同属一个项目**，它自带 `UpdateFormat::Nsis`（运行下载到的安装器）与
> `UpdateFormat::App`（整包替换 macOS bundle），因此"运行安装器"这一步不需要本项目实现。
> 最终方案是整条链路换用 `cargo-packager-updater`，而不是在 `self_update` 上挂 minisign 钩子。

如果不想动验证模型，则走方案 A（保持 zipsign）：macOS 几乎零成本，**但 Windows 必须增加一个
载荷归档文件**，与你的"不引入其他文件"约束冲突。详见 §6。

---

## 1. 密码学基础

### 1.1 自动更新要防的是什么

用户装的是你发布的二进制。攻击者有几个现实的入手点：

1. **网络中间人**：篡改下载内容（虽然 HTTPS 已经防了大部分，但 CDN、镜像、企业代理、
   证书被误信任都可能出问题）。
2. **分发渠道被投毒**：GitHub Release 资产被替换、账号被盗后发布恶意版本。
3. **降级攻击**：强迫客户端回退到有已知漏洞的旧版本。
4. **本地文件被替换**：用户机器上已下载的临时文件被改。

签名机制主要针对 1 和 2 的"内容换掉了但看起来一样"这一面。它**不能**防：你的私钥被偷、
操作系统代码签名被绕过、或用户主动安装了你没有签名的东西。

### 1.2 哈希（摘要）

哈希函数（如 SHA-256、SHA-512、BLAKE2b）把任意长度的数据压成固定长度的指纹：

- **单向**：从指纹推不回原文。
- **抗碰撞**：极难找到两段不同的数据得到同一个指纹。
- **雪崩**：改一个字节，指纹面目全非。

哈希只解决 **完整性**（数据有没有被改动），**不解决真实性**。原因很直接：攻击者改完文件，
顺手把指纹文件一起改成新的就行。所以"我只公布 SHA-256"不足以证明东西是你发的。

### 1.3 为什么必须用签名而不是哈希

数字签名把"完整性"和"身份"绑在一起：用**只有你能持有的私钥**产生签名，**任何人都能用公开的
公钥**验证。攻击者能改文件，但改不出一个新签名——因为他没有私钥。

这正是哈希做不到的那一半。

### 1.4 非对称密钥与 Ed25519

- 一对密钥：**私钥**（保密，用来签名）+ **公钥**（公开，用来验签）。
- 公钥**不是**从私钥"反推"出来的，而是数学上成对生成；所以即使公钥满天飞，私钥也不会泄漏。
- **Ed25519** 是目前桌面应用签名的默认选择：签名 64 字节、公钥 32 字节、私钥 32 字节种子，
  实现小、速度快、没有 RSA/ECDSA 那类参数选择陷阱（选错曲线、随机数复用导致私钥泄露等）。

Ed25519 的私钥在实践里常存成 **64 字节的 keypair**（32 字节种子 + 32 字节公钥）。本仓库用到的
zipsign 就是读 64 字节 keypair（`SigningKey::from_keypair_bytes`），公钥文件则是裸 32 字节。

### 1.5 预哈希：为什么出现 `Ed25519ph` 和 `BLAKE2b`

Ed25519 原始定义是"对整条消息签名"。文件可能有几百 MB，而且流式读取时无法把整条消息留在内存里。
RFC 8032 因此定义了 **`Ed25519ph`**：先对消息做一次哈希，再对哈希值签名（`ph` = prehash），并用
一个域分离前缀防止与其他用法混淆。

不同项目选取的预哈希算法不同，**这是两套签名格式互不兼容的根因**：

| 方案 | 预哈希 | 说明 |
| --- | --- | --- |
| zipsign（本仓库当前） | **SHA-512** | 签名字节里写死魔数 `\x0c\x04\x01` + `ed25519ph` + `\x00\x00` |
| minisign（Tauri / cargo-packager） | **BLAKE2b-512** | 签名结构里带 `sig_alg = prehashed` 标记 |

两者都是"Ed25519 + 预哈希"，但预哈希不同、封装格式不同，**签名文件不能互相识别**。

### 1.6 信任根：公钥必须随二进制发布

这是整个机制里最容易被忽略、却最关键的一点：

> **验证用的公钥必须内嵌在客户端二进制里（编译期常量），绝不能从网络获取。**

如果你从网络拿公钥，攻击者只需把公钥一起换掉，签名验证就变成自证清白。本仓库的公钥就是编译期
常量：`crates/bongocat-update/src/runtime.rs` 的 `RELEASE_SIGNING_KEY`。

推论：**换钥匙必须先发一个"同时认识新旧两把钥匙"的版本**。否则老版本用户会验不过新签名，更新链
从此断掉。这是 §3.3 密钥轮换的约束来源。

### 1.7 密钥 ID、多密钥与"any-of"语义

真实项目会轮换密钥，所以验签要能同时接受多把公钥。两套方案的差异：

- **zipsign**：`verifying_keys(&[key1, key2])`，语义是 **any-of**——签名在归档里可以有多条，
  命中任意一条即通过。这天然支持轮换窗。
- **minisign**：签名结构里带 8 字节 **key ID**；验签**先比对 key ID**，不匹配直接
  `UnexpectedKeyId` 失败（`minisign-verify-0.2.5` 源码确认）。所以传错公钥是**明确报错**，
  不会"悄悄通过"。

key ID 的价值是错误定位：日志里能区分"钥匙不对"和"内容被改".

### 1.8 上下文绑定：签名可以绑到文件名上

zipsign 的签名计算把**文件名**作为 Ed25519ph 的 `context` 一起签（`self_update` 传的是下载文件的
`file_name()`；`zipsign` CLI 的 `-c/--context` 默认也是文件名）。

后果很实际：**zipsign 签名过的归档改名之后就验不过了**。这条约束直接决定 CI 里"命名"和"签名"
的先后顺序（§5.1）。

minisign 把文件名放在 `trusted comment`（`timestamp:...\tfile:<名字>`）里，并且对这段注释有第二
个"全局签名"。但**客户端是否比对文件名取决于验证实现**：`minisign-verify 0.2.5` 的
`PublicKey::verify()` 只校验 key ID + 内容预哈希 + Ed25519 签名，**不比对** `file:` 字段。所以
minisign 改名后技术上仍可验证通过（代价是丢掉了文件名绑定这一层弱保护）。即便如此，仍然应当
"先命名、后签名"，避免给未来留坑。

### 1.9 更新签名 ≠ 操作系统代码签名

这是新手最容易混淆的一点。它们是两套独立机制，各防各的：

| | 更新签名（本文主题） | 操作系统代码签名 |
| --- | --- | --- |
| macOS | zipsign / minisign | `codesign` + Gatekeeper 公证（notarization） |
| Windows | zipsign / minisign | Authenticode（`cargo-packager` 通过 `sign_command` / 证书触发） |
| 防的是 | 更新内容不是你发的 | 二进制来源可信、没被篡改、能过 SmartScreen/Gatekeeper |
| 谁验证 | 你自己的应用 | 操作系统 |

本仓库两者都还是缺口：打包只做 ad-hoc 签名（`crates/bongocat-packaging/src/main.rs:97` 的
`ADHOC_SIGNING_IDENTITY = "-"`），`release.yml` 里 Windows Authenticode 目前只打 warning。

---

## 2. 两套现成方案

### 2.1 对比

| | zipsign（ADR-0029 时的实现，已退役） | minisign（Tauri / cargo-packager） |
| --- | --- | --- |
| 库 | `zipsign-api 0.2.1`（经 `self_update` 的 `signatures` feature 引入） | 生产端 `minisign 0.7.9`；消费端 `minisign-verify 0.2.5` |
| 预哈希 | SHA-512 | BLAKE2b-512 |
| 签名位置 | **归档内部**（内嵌） | **独立 `.sig` 文件**（分离） |
| 支持的载体 | 只有 `.zip` 与 `.tar.gz` | **任意文件**（裸 `.exe`、`.dmg` 都行） |
| 是否产生额外文件 | **否** | **是**，每个被签文件 +1 个 `.sig` |
| 文件名绑定 | **强绑定**（作为签名 context） | 弱绑定（在 trusted comment 里，客户端可不比对） |
| 多密钥语义 | any-of | key ID 匹配 + 单密钥验证 |
| 许可证 | MIT OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | minisign: MIT；minisign-verify: MIT |

### 2.2 「哪些产物能被签名」速查

`self_update` 用扩展名判定归档类型，所以能不能被 zipsign 签名完全由文件名决定：

| 产物 | `detect_archive()` 结果 | 能否 zipsign 签名 |
| --- | --- | --- |
| `BongoCat-1.1.0-aarch64-apple-darwin.app.tar.gz` | `Tar(Gz)` | ✅ |
| `BongoCat-1.1.0-...app.zip` | `Zip` | ✅ |
| `BongoCat-1.1.0-arm64.dmg` | `Plain(None)` | ❌ `NoSignatures` |
| `BongoCat_1.1.0_x64.exe` | `Plain(None)` | ❌ `NoSignatures` |

再叠加一条：**`.dmg` 本来也不该作为更新产物**。它是人工安装路径（拖进 `/Applications`），
`self_update` 没有安装 dmg 的能力。Tauri 同样不使用 dmg 做更新。更新产物必须是"应用本体"：
macOS 是 `.app` 归档，Windows 是安装器或应用本体。

### 2.3 cargo-packager 已经能做的事（已读源码确认）

`cargo-packager 0.11.8` 公开导出 `pub mod sign`，关键 API：

```rust
// 生成密钥（返回 base64 编码的公钥与私钥字符串）
cargo_packager::sign::generate_key(Some(String::new()))?;   // 空密码 = 不提示

// 把密钥对落盘：<path> 与 <path>.pub
cargo_packager::sign::save_keypair(&keypair, path, force)?;

// 签名单个文件，产出 <file>.sig
cargo_packager::sign::sign_file(&SigningConfig { private_key, password }, path)?;

// 签名整批产物；目录会先被打成 <dir>.tar.gz 再签名
cargo_packager::sign_outputs(&signing_config, &mut packages)?;

// 打包 + 签名一步完成
cargo_packager::package_and_sign(&config, &signing_config)?;
```

两个已核实的细节，正好对上你的需求：

1. **目录会被自动打成 `BongoCat.app.tar.gz`**（`with_additional_extension("tar.gz")`），且
   归档**根目录就是 `BongoCat.app/`**——因为内部用 `append_dir_all(filename, src_dir)` 而不是
   展开内容。这正是更新库 bundle 模式要求的布局。该布局在落地后由
   `crates/bongocat-packaging` 的 `the_bundle_archive_roots_at_the_bundle_directory` 与
   `crates/bongocat-update/tests/release_manifest_capability.rs` 的
   `installing_replaces_the_configured_bundle` 共同锁定（原先锁定它的
   `archive_layout_capability.rs` 已随 `self_update` 退役删除）。
2. **签名产出的 `.sig` 内容**是 base64 编码的 minisign 签名盒，`sig_alg` 固定为
   `prehashed`，trusted comment 形如 `timestamp:<unix>\tfile:<文件名>`，
   untrusted comment 是 `signature from cargo-packager secret key`。

---

## 3. 需求 1：签名密钥的生成方式与安全存储管理

### 3.1 生成

**zipsign（当前方案）** — 需要 `cargo install zipsign`：

```sh
zipsign gen-key priv.key pub.key      # 生成密钥对
zipsign gen-key -e priv.key pub.key   # 只从已有私钥导出公钥
```

产物形态（已读 `zipsign-api` 源码确认）：

- `priv.key` = **64 字节二进制**（Ed25519 keypair = seed‖public）
- `pub.key` = **32 字节二进制**

注意：它是**裸二进制**，不是 base64 文本。这意味着往 GitHub Secrets 里放之前必须自己 base64
编码，CI 里再解码。多一层手工步骤，也是出错点。

**minisign（已采纳）** — 已经在依赖的工具里，且**不需要全局安装**：

```sh
# 本项目入口（推荐）：用精确 pin 的 cargo-packager 生成，产物已 chmod 600
just keygen ~/.bongocat/release.key
```

它等价于 `crates/bongocat-packaging --generate-signing-key <file>`，也等价于直接调
`cargo_packager::sign::{generate_key, save_keypair}`。口令通过
`SIGNING_PRIVATE_KEY_PASSWORD` 提供（与签名时解锁私钥用的是同一个变量）；不设口令时
会生成未加密的私钥并打印警告。

`just keygen` **没有 password 参数**，这是刻意设计：把口令放上命令行会进入 shell history，
也可能出现在进程列表中。交互式生成带口令的 key pair 时，在当前 `zsh` 中执行：

```sh
read -rs "SIGNING_PRIVATE_KEY_PASSWORD?Signing key password: "
echo
export SIGNING_PRIVATE_KEY_PASSWORD

just keygen ~/.bongocat/release.key

unset SIGNING_PRIVATE_KEY_PASSWORD
```

`read -s` 不回显输入；`-r` 保留反斜杠字面值。若 shell 是 bash，等价写法是：

```bash
read -rsp 'Signing key password: ' SIGNING_PRIVATE_KEY_PASSWORD
echo
export SIGNING_PRIVATE_KEY_PASSWORD

just keygen ~/.bongocat/release.key

unset SIGNING_PRIVATE_KEY_PASSWORD
```

不要写成 `SIGNING_PRIVATE_KEY_PASSWORD='...' just keygen ...`：虽然功能上可用，但口令会进入
shell history。生成完成后把私钥文件内容放进 `SIGNING_PRIVATE_KEY` secret，把同一口令放进
`SIGNING_PRIVATE_KEY_PASSWORD` secret；两者必须同时存在。

产物形态（**本机实测**，2026-09-14）：

- `<file>`：私钥，**base64 单行文本**，348 字节，权限 `0600`（工具显式收紧；`save_keypair`
  自身按 umask 落盘，默认账号下会是 644）
- `<file>.pub`：公钥，**base64 单行文本**，152 字节，`0644`
- 工具同时把公钥那一行打印出来，可直接粘进 `RELEASE_SIGNING_KEY`

因为是纯文本，可以直接整段塞进 CI secret，不需要额外编码步骤。

> 若坚持用上游 CLI：`cargo install cargo-packager --version 0.11.8 --locked` 后
> `cargo packager signer generate-key --path <file>`。参数为 `--path`、`--password`（或环境变量
> `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`）、`--force`、`--ci`；不传 `--path` 时它只把密钥对
> 打印到日志，**在 CI 里会直接把私钥写进日志**。本项目不采用这条路，因为它要求全局安装与
> lockfile 版本一致的二进制，而 `just keygen` 用的是同一份 pin。
>
> Tauri 的等价命令是 `tauri signer generate -w ~/.tauri/myapp.key`，产出同一个格式
> （base64 minisign 密钥），并支持给私钥设密码。

### 3.2 怎么进 CI（这是本需求真正的难点）

**私钥绝不能进仓库。** 三条铁律：

1. **不进源码、不进构建产物、不进日志。** 检查清单：`git log -S` 搜不到；构建输出里 grep 不到；
   错误信息不打印私钥内容。
2. **只通过 CI secret 注入。** GitHub Actions 里用
   `secrets.SIGNING_PRIVATE_KEY`（口令用 `secrets.SIGNING_PRIVATE_KEY_PASSWORD`）
   传给打包步骤的环境变量。注意 Tauri 文档特别强调的一点：**`.env` 文件不生效**，必须是真的
   环境变量。变量名已在 `crates/bongocat-packaging/src/main.rs` 中固定为
   `SIGNING_PRIVATE_KEY` / `SIGNING_PRIVATE_KEY_PASSWORD`。
3. **本地开发一律不签名。** 签名只在发布流水线里发生；本地 `just build` 不设私钥时应当
   **跳过签名**（而不是用一个空密钥签名成功——那是最坏的情况，见 §4.4）。

具体形式（已落地在 `.github/workflows/release.yml`）：

```yaml
# .github/workflows/release.yml 里新增
- name: Sign the update artifacts
  env:
    SIGNING_PRIVATE_KEY: ${{ secrets.SIGNING_PRIVATE_KEY }}
  run: just build            # 打包工具读到该变量才签名
```

对应到 `crates/bongocat-packaging/src/main.rs` 的现有模式：它已经有一个凭据注入点的先例——
`BONGOCAT_MACOS_SIGNING_IDENTITY`（第 103 行）。更新签名密钥应该照同样的方式设计：
**一个环境变量注入点，不设就跳过，且必须让 CI 能断言"发布构建一定签过名"**。

### 3.3 权限、备份、轮换、丢失

| 关注点 | 做法 |
| --- | --- |
| 文件权限 | 生成后立刻 `chmod 600`（`zipsign gen-key` 与 `cargo-packager` 的 `save_keypair` 都按默认 umask 落盘，可能是 644） |
| 密码保护 | minisign 支持给私钥加密（`generate_encrypted_keypair`），密码单独存一个 secret；`SigningConfig.password`。zipsign 的私钥**没有**口令保护 |
| 备份 | 至少离线备份一份。**私钥丢失 = 已安装用户永远收不到更新**（因为验证公钥烧在旧二进制里） |
| 轮换 | 先把新旧两把公钥**同时**写进客户端并发布，确认用户升上来之后，再用新私钥签名。zipsign 的 any-of 天然支持；minisign 需要客户端支持多 key 列表 |
| 泄露响应 | 换钥匙 + 发一个"吊销公告版本"。没有别的办法——这正是"必须先发双钥匙版本"的代价 |
| 一把还是多把 | 建议**一把 release key 覆盖两平台**（同一个发布通道，威胁模型相同），减少管理面。分平台只在两平台的发布权限真的分属不同人时才有意义 |

### 3.4 本仓库的落点

公钥的位置是 `crates/bongocat-update/src/runtime.rs` 的 `RELEASE_SIGNING_KEY`。

**已落地的形态**（ADR-0034）：base64 的 minisign 公钥盒文本，也就是
`cargo packager signer generate-key` 写出的 `.pub` 文件内容。仓库现已内嵌发布公钥，
key ID 为 `DF5E2C9D255DD85E`，因此 Production runtime 的
`UpdateRuntime::is_available()` 为真，系统菜单可以显示更新入口：

```rust
pub const RELEASE_SIGNING_KEY: Option<&str> = Some("<minisign public key>");
```

**公钥不是秘密**，可以正常提交到仓库；只有私钥和私钥口令必须留在仓库之外。运行时会拒绝
`None`、空串和纯空白值，并在构造更新库实例之前返回 `update_signature_key_missing`；
`runtime.rs` 的定向测试同时锁定已配置公钥和空白值失败关闭两条路径。真实 GitHub 发布链路尚未
验证，不能据此宣称发布签名已经端到端可用（见 ADR-0034 待验证项 1）。

---

## 4. 需求 2：更新校验时签名验证的完整流程

### 4.1 `self_update` 的完整校验链（已读源码确认顺序）

```
取发布列表 → 选资产 → 下载 → ┌ 1. checksum（显式传入的摘要）
                             ├ 2. release digest（GitHub API 为该资产公布的 sha256）
                             ├ 3. signature（zipsign，用内置公钥集）
                             ├ 4. verify_archive 钩子（你注入的闭包）
                             ├ 解压
                             ├ 5. verify_binary 钩子（跑一下新二进制做冒烟测试）
                             └ 替换安装
```

第 2 项值得单独说：GitHub 会为每个 Release 资产公布一个 `sha256:<hex>`，`self_update` 在有
`checksums` feature 时**默认自动校验**。但上游明确写了这是**纯完整性检查**——攻击者替换资产后
GitHub 会重新计算这个摘要，所以它**不能替代签名**。

第 4、5 项是两个**公开可用的钩子**，签名类型是普通闭包：

```rust
pub fn verify_archive(&mut self, verify: impl Fn(&std::path::Path) -> crate::Result<()> + Send + Sync + 'static)
pub fn verify_binary(&mut self,  verify: impl Fn(&std::path::Path) -> crate::Result<()> + Send + Sync + 'static)
```

上游文档把"crate 自身不支持的**分离签名校验**"明确列为 `verify_archive` 的用途。**这是把
minisign 接进现有流水线的关键入口**——不需要抛弃 `self_update` 的下载/解压/替换引擎。

### 4.2 zipsign 的验签步骤（当前实现）

1. 按扩展名判定归档类型；不是 `.zip` / `.tar.gz` → `Error::NoSignatures`。
2. 从归档里定位签名块：tar.gz 读**追加的 gzip 成员注释**；zip 读**前置数据**。
3. 校验魔数头 `\x0c\x04\x01` + `ed25519ph` + `\x00\x00`，读出签名条数（u16 LE）。
4. 用**文件名**作为 context，对归档中"签名块之前"的内容算 SHA-512 预哈希。
5. 对每把公钥 × 每条签名做 `verify_prehashed_strict`；**任意一条通过即通过**（any-of）。
6. 全部不匹配 → `Error::Signature` → 映射为 `update_signature_invalid`。

### 4.3 minisign 的验签步骤（推荐方案）

1. 读取 `<产物>.sig`，base64 解码得到 minisign 签名结构。
2. 比对签名里的 **key ID** 与内置公钥的 key ID；不匹配 → `UnexpectedKeyId`。
3. 对产物内容算 **BLAKE2b-512** 预哈希。
4. 用公钥对预哈希值做 Ed25519 验签。
5. 通过则继续解压/安装；否则拒绝。

（`minisign-verify 0.2.5`：`PublicKey::verify(data, &signature, allow_legacy=false)`。
`allow_legacy=false` 是正确值——它拒绝 minisign 老版本的非预哈希签名，也正好和
`cargo-packager` 的 `prehashed` 输出匹配。）

### 4.4 必须在设计里消掉的陷阱：fail-open

这是本文最重要的一条实操警告。

`self_update::verify_signature()` 的第一行逻辑是：

```rust
if keys.is_empty() {
    return Ok(());      // ← 静默跳过验签
}
```

**公钥集为空 = 验签直接通过 = 会安装未签名产物，且不报任何错。** 任何"把公钥列表原样转发下去"
的封装都会踩这个坑。

本仓库已经做了正确补偿：`RELEASE_SIGNING_KEY` 缺失、为空或纯空白时，runtime 在**发出任何网络
请求之前**就返回 `update_signature_key_missing`（门禁位于 `runtime.rs` 的 `updater()`，并由
定向测试锁定）。仓库当前内嵌发布公钥，因此这条拒绝路径只会在未配置的构建或测试输入上命中。

换实现时必须保留这条性质。设计原则一句话：**"验签不可能成功"必须等于"拒绝更新"，而不是
"跳过验签"。**

> 落地结果（2026-09-14）：新库在这一点上已经是 fail-closed——`verify_signature` 先 base64 解码
> 公钥再 `PublicKey::decode`，空字符串解不出公钥，直接得到 `Error::Minisign`，不会静默通过。
> 但**它发生在下载之后**：`check()` 只解析 manifest，因此空公钥仍会先得到一个更新offer，
> 失败落在 `download()`（这正是 `release_manifest_capability.rs` 的
> `an_empty_public_key_cannot_verify_a_payload` 断言的形状）。
>
> 项目**仍然保留了自己的门禁**，理由不变：它在构造更新库实例之前就返回稳定、无路径的
> `update_signature_key_missing`，而不是一个需要映射的库错误，也不会先发出请求。两者是纵深
> 防御关系，不是重复实现。

### 4.5 其余校验原则

- **先验签，后使用**：签名校验必须发生在解压和替换**之前**（`self_update` 的顺序已经满足）。
- **失败一律 fail-closed**：任何一步失败都不留下"半个已安装的新版本"。
- **错误码稳定**：`crates/bongocat-update/src/diagnostics.rs` 的 14 个错误码是诊断导出契约
  （ADR-0016 / ADR-0027），换实现时只能收敛到既有码，不能泄漏第三方错误文本。
- **降级防护**：ADR-0029 记录了 zipsign 模型**没有**单调 `release_sequence`，降级攻击不再被
  检测，只按 semver 比较。这是换实现时应当一并考虑补回来的东西。

---

## 5. 需求 3：更新分发环节如何接入并校验签名

### 5.1 顺序：命名 → 签名 → 合并 → 上传

关键约束来自 §1.8：签名与内容（含文件名）绑定。所以流水线上的正确顺序是：

```
编译 → 打包（.app / .dmg / .exe）
     → macOS: codesign + notarize（.app 必须先真正签名并公证）
     → 生成更新归档（macOS: .app.tar.gz；Windows: 复用 .exe）
     → 用正式名字命名
     → 签名（产出 .sig）
     → 写本 target 的 manifest fragment（<os>-<arch>.json）
     → 每个 target 各自上传为构建产物
     → 发布前合并 fragment 成共享 latest.json（just manifest）
     → 上传 Release
```

三条硬规则：

1. **签名必须是最后一步。** 签名之后任何字节改动（重新压缩、改名、`strip`）都会让签名失效。
   对 zipsign 而言改名**一定**失效（文件名是签名 context）。
2. **归档内的 `.app` 必须先完成 codesign + notarize。** 一旦打成 tar.gz，再想签名里面的 `.app`
   就晚了——解压、签名、重新打包会直接让归档签名失效。实际上"先打包再公证"在流程上根本走不通。
3. **命名和签名严格分先后。** 先改好名字，再签名。

> 现状（2026-09-14）：合并发生在签名**之后**，但只作用于 manifest——它把各 target 已经签好的
> `url`/`signature`/`format` 组装成一份文档，不触碰任何已签名的载荷字节，因此不破坏上面三条规则。
> 合并由 `crates/bongocat-packaging --merge-manifests` 承担（`just manifest`），CI 只调用工具。

### 5.2 资产命名要求（决定客户端能不能找到）

`self_update` 的 `Release::asset_for` 先用**完整 target triple**匹配资产名，失败后退化为
`arch` + `os` 标记（`arch` = triple 首段，`os` ∈ `darwin`/`windows`/…）。注意
**`bin_name` 不参与资产名匹配**。

所以资产名**必须包含完整 target triple**，例如：

```
BongoCat-1.1.0-aarch64-apple-darwin.app.tar.gz
BongoCat-1.1.0-x86_64-apple-darwin.app.tar.gz
BongoCat_1.1.0_x64.exe                      ← 现有名字，不含 triple
```

⚠️ 最后一行是个真问题：现有 Windows 安装器名 `BongoCat_<version>_x64.exe` 里只有 `x64`，
退化匹配需要 `arch` + `os` 两个标记同时命中才算。这一点在换方案时必须实测确认，不能靠推断。

### 5.3 归档内布局要求

| 平台 | 模式 | 归档根必须是什么 | 依据 |
| --- | --- | --- | --- |
| macOS | bundle 模式 | `BongoCat.app/` 目录 | `crates/bongocat-packaging` 的 `write_bundle_archive` 回归 |
| Windows | 安装器模式 | 不适用（载荷是裸 `.exe`，库直接运行它） | `UpdateFormat::Nsis` |

**多套一层目录会直接报错**（不会静默取错文件）。

> 现状（2026-09-14）：Windows 已不再是"单文件替换"模式。落地后 Windows 载荷就是那个 NSIS
> 安装器，由库的 `UpdateFormat::Nsis` 路径运行，因此 `resources/` 会随安装器一起更新——
> 原先需要 `MoveAll` 自行编排的理由（单文件替换不更新 `resources/`）随 `self_update` 一并
> 退役。**这一条是结构推论，未在 Windows 实机验证**（本机是 macOS），见 ADR-0034 待验证项 2。

### 5.4 `.dmg` 与 `.sig` 的取舍（直接回答你的约束）

你的约束是"若两平台均需 `.sig`，则仅额外增加该文件，不引入其他文件"。逐条对照：

**方案 A（保持 zipsign）**

| 平台 | 发布产物 | 相对现状新增 |
| --- | --- | --- |
| macOS | `.dmg`（人工安装）、`BongoCat-<ver>-<triple>.app.tar.gz`（更新，**签名内嵌**） | 把现在的 `.app.zip` 换成 `.tar.gz`，**不加任何文件** |
| Windows | `.exe` 无法签名 → **必须**再加一个含 `bongocat-app.exe`（+`resources/`）的 `.zip`/`.tar.gz` | **+1 个归档文件**，与你的约束冲突 |

结论：**zipsign 满足 macOS 的零新增文件，但满足不了 Windows。**

**方案 B（改用 minisign，推荐）**

| 平台 | 发布产物 | 相对现状新增 |
| --- | --- | --- |
| macOS | `.dmg`、`BongoCat-<ver>-<triple>.app.tar.gz`、`.app.tar.gz.sig` | +1（`.app.tar.gz` 替代原 `.app.zip`）+ 1 个 `.sig` |
| Windows | `BongoCat_<ver>_x64.exe`、`.exe.sig` | **仅 +1 个 `.sig`** |

结论：**完全符合你的描述。** 注意 `cargo-packager` 的 `sign_outputs` 会连 `.dmg` 也一起签名
（产出 `.dmg.sig`）；如果不想发布这个文件，就只对更新产物调 `sign_file`，或者把
`PackageOutput` 列表过滤后再交给 `sign_outputs`。

---

## 6. 落地步骤

### 6.1 方案 A：保持 zipsign（macOS 可用，Windows 阻塞）

1. **生成并托管密钥**
   ```sh
   cargo install zipsign
   zipsign gen-key release.key release.pub && chmod 600 release.key
   base64 -i release.key | pbcopy        # 存进 GitHub Secret
   ```
2. **把公钥编译进去**：`release.pub` 是 32 字节裸数据 → 填进
   `runtime.rs` 的 `RELEASE_SIGNING_KEY`。
3. **改 CI 的 macOS 归档步骤**（`.github/workflows/release.yml` 的
   `Package the bundle for publication`）：现在用 `ditto -c -k` 产的是 `.zip`，改成产
   `BongoCat-$version-${{ matrix.triple }}.app.tar.gz`（归档根必须是 `BongoCat.app/`）。
4. **加签名步骤**：在 upload 之前，解码私钥、`zipsign sign tar <归档> release.key`。
5. **Windows**：需要额外决策——要么接受新增一个载荷归档并实现 `MoveAll` 多文件编排，
   要么放弃 Windows 自更新。

### 6.2 方案 B：改用 minisign（推荐）

**生产端（`crates/bongocat-packaging`）**

1. 复用已在依赖里的 `cargo_packager::sign`，不引入新工具。
2. 在 `packaging_config` 之后、`verify_artifacts` 之前插入签名步骤：
   - 从环境变量读私钥（照 `BONGOCAT_MACOS_SIGNING_IDENTITY` 的先例）；
   - 未设置则跳过签名，并让发布流水线能断言"发布构建必须签过名"；
   - 把 `.app` 打成 `BongoCat-<ver>-<triple>.app.tar.gz`（根目录 `BongoCat.app/`），
     把 Windows 安装器确认为 `BongoCat_<ver>_x64.exe`；
   - 对这两个文件调 `sign_file`，产出 `.sig`。
3. 更新 `tools/tests/test_update_release_contract.py`，把"资产名必须含完整 triple"这条
   编译期不可见的耦合固定下来。

**消费端（`crates/bongocat-update`）**

4. 新增依赖 `minisign-verify = "=0.2.5"`（MIT，**零依赖**，本机源码确认支持 prehashed 模式
   与 key ID 校验）。按 §9 的依赖纪律，先用 `cargo info minisign-verify` 核对最新稳定版。
5. 把 `RELEASE_SIGNING_KEY` 换成 minisign 公钥，并把
   `builder.verifying_keys([key])` 换成 `builder.verify_archive(|path| ...)`：
   在闭包里读 `<path>.sig`、调 `PublicKey::verify`，失败时返回
   `self_update::Error::archive_verification_rejected(...)`。
6. **保留 fail-closed 门禁**：公钥未配置时，在发请求前就返回 `update_signature_key_missing`
   （等价于现在的 `runtime.rs:202`）。
7. **新增 ADR**，取代 ADR-0029 的签名部分，并显式记录丢失的能力（§4.5 的降级防护）。

**需要正视的两个未解决问题**

- **`.sig` 从哪来？** `verify_archive` 钩子只拿到下载文件的路径，而 `.sig` 是另一个 Release
  资产。`self_update` 只提供 `checksum_from_asset()`，**没有** `signature_from_asset()`。
  可行做法是在闭包内自行请求一次 Release API 找到同名 `.sig` 资产并下载（`self_update` 已
  re-export `ureq`）。这带来额外一次网络请求和一段自研胶水，必须写进 ADR 的成本栏。
- **Windows 的安装步骤要自研。** `.exe` 是系统安装器，`self_update` 明确不支持（§0.2）。
  需要自己实现「下载 → 验签 → 静默运行安装器 → 重启」，并处理安装器与正在运行的进程的冲突。
  自签名发布前还要确认 per-user NSIS 安装器的静默开关（`cargo-packager` 的 NSIS 模板是否
  支持 `/S` 需实测）。

### 6.3 对"我该先做什么"的建议顺序

1. **先不要动代码。** 先写一个 ADR 决定签名方案（zipsign 内嵌 vs minisign 分离），因为这两条
   路对产物、依赖、Windows 安装语义的影响都是不可逆的。
2. 决定 Windows 的更新语义（自替换 vs 运行安装器）。**这是真正的分水岭**，比签名算法重要得多。
3. 再改打包工具产归档 + 签名，最后才是客户端验签。

> 该顺序已被遵循：决策记录在 ADR-0034，随后才动代码。§6.2 的"消费端"步骤 4–5 在执行时被
> 简化——`self_update` 的 `verify_archive` 钩子方案没有采用，因为整条链路换成了
> `cargo-packager-updater`，它自己取 manifest 里的 `.sig`，不需要在钩子里再请求一次 Release
> API。§6.2 列出的两个"未解决问题"因此都不成立：`.sig` 由 manifest 携带，Windows 安装步骤由
> 库提供。
>
> §6.2 的**生产端**步骤 1–3 与"签名必须是最后一步"的约束按原样执行，落在
> `crates/bongocat-packaging` 的 `publish_update_assets`。

---

## 7. 注意事项清单

> 下列清单是**决策前**的检查表。落地后各项的归属见括号内的说明；打勾表示已由代码或契约
> 强制，而不是靠人记住。

- [x] 私钥不进仓库、不进构建产物、不进日志；只从 CI secret 注入（`.env` 不生效）
      （`SIGNING_PRIVATE_KEY` 环境变量注入，代码里没有任何密钥字面量）
- [x] 生成后 `chmod 600` 私钥文件（`just keygen` 已显式收紧到 `0600`；实测确认）
- [x] 公钥**编译期**内嵌，绝不从网络取（`RELEASE_SIGNING_KEY` 常量）
- [x] 公钥**未配置时在发请求前就失败**（`updater()` 的门禁，有定向测试锁定）
- [x] 本地构建默认跳过签名，发布构建**断言**签过名
      （release workflow 的 "Require the update signing key" 与 "Verify the signed update assets"）
- [x] 签名是所有步骤的**最后一步**；签名后不改名、不重新压缩、不 strip
      （`publish_update_assets` 在产物确定后才签名，manifest 的 URL 在签名之后写入）
- [ ] macOS 的 `.app` 必须在**打 tar.gz 之前**完成 codesign + notarize（公证仍是独立门禁）
- [x] 更新载荷名不必再含**完整 target triple**：载荷位置由共享 manifest 的 `platforms` 条目
      给出，不再按资产名匹配（这正是 `asset_for` 退役后的变化）
- [x] macOS 归档根必须是 `BongoCat.app/`（`write_bundle_archive` 回归 + release workflow 断言）
- [x] `.dmg` 不作为更新产物（`update_payload` 只取 bundle 或 `.exe`）
- [x] Windows 的 `resources/` 随安装器一起更新（结构推论，**未在 Windows 实机验证**）
- [x] `.sig` 只在方案 B 存在；方案 B 已采纳，两平台各一个 `.sig`
- [ ] 更新签名 ≠ Authenticode / 公证，两件事都要做（Authenticode 仍只打 warning）
- [ ] 一把钥匙泄露 = 换钥匙 = 必须先发"双钥匙版本"；务必备份私钥
      （**单公钥模型下这条约束更硬**，见 ADR-0034 待验证项 4）
- [ ] ADR-0029 已丢失的降级防护（`release_sequence`）值得在换实现时补回（**本次未补**）
- [ ] 任何签名/安装路径的改动都要同步更新 ADR、契约测试和诊断错误码

---

## 8. 未验证事项（按 `AGENTS.md` §3.4 诚实标注）

下列条目是**本文写作时**（决策前）的状态。落地后其中一部分已由
`crates/bongocat-update/tests/release_manifest_capability.rs` 覆盖，逐条标注如下；
**仍未验证的事项以 ADR-0034 的"待验证项"为准**。

1. ~~没有生成过任何密钥~~ → **已覆盖**：能力测试调用 `cargo_packager::sign::generate_key` 生成
   真实密钥对并落盘；`just keygen` 也已用于生成并验证发布私钥/公钥。**发布私钥的 GitHub secrets
   托管与真实发布链路仍未验证**。
2. ~~没有跑通一次"签名 → 客户端验签"~~ → **部分覆盖**：签名与验签都在 loopback 上真实执行过
   （真实密钥、真实 `.sig`、真实 `PublicKey::verify`），但**真实 GitHub 发布链路仍未跑通**。
3. ~~`minisign-verify 0.2.5` 与 `minisign 0.7.9` 的互操作性未实测~~ → **已覆盖**：两者由能力测试
   交叉验证通过，`cargo-packager` 的 prehashed 输出可被 `cargo-packager-updater` 验签。
4. ~~GitHub 上的资产名退化匹配未实测~~ → **已失效**：换库后不再按资产名匹配，载荷位置来自
   manifest。新风险是 manifest 本身没有防降级保护（见 ADR-0034 待验证项 5）。
5. **Windows 静默安装**：`install_mode = Quiet` 映射到 NSIS `/S` `/R` 已由源码确认
   （`WindowsUpdateInstallMode::nsis_args`），但**能否在被占用的可执行文件上完成替换仍未验证**
   ——本机是 macOS。
6. ~~`.dmg` 是否被 `sign_outputs` 一并签名~~ → **已失效**：本项目不调用 `sign_outputs`，只对
   更新载荷调 `sign_file`，因此不会产出 `.dmg.sig`。
7. ~~`cargo-packager` 的 `signer generate-key` CLI 子命令的确切参数名未核实~~ → **已核实**：
   `cargo packager signer generate-key [--path <p>] [--password <p>] [--force] [--ci]`
   （`src/cli/signer/mod.rs` 的 `Commands::Generate`，参数定义在 `generate.rs`）。
   产出的 `.key` / `.key.pub` 是 base64 文本，可直接整段放进 CI secret。
