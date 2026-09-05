# ADR-0021: Signed Update Manifest Trust Boundary

状态：已接受（2026-09-05）

## 背景

Native Rewrite 将自动/手动更新列为首发行为，但旧版 updater 使用 HTTP endpoint、前端固定凭据和
旧 payload，不能进入新架构。下载和安装前必须先建立独立、可离线验证的信任边界；否则网络 client、
设置 UI 和高权限 installer 会各自解释版本、target、hash 或签名，形成不一致的安全判断。

Development 与 Production 构建也不能共享可变 channel。Windows 只支持 x64/ARM64，macOS 支持
Intel/Apple Silicon；Windows ARM64 在官方 desktop Cubism Core 可用前仍是发布阻塞，而不是由更新
manifest 绕过。

## 决策

- 新增平台无关且禁止 `unsafe` 的 `bongocat-update` crate。它只执行 manifest、签名、版本、target
  和 artifact 完整性验证，不联网、不写文件、不启动 installer，也不持有 runtime 或 UI 状态。
- manifest v1 使用严格 `snake_case` JSON 和 detached Ed25519 签名。客户端先对收到的原始 manifest
  bytes 执行严格验签，成功后才反序列化；未知字段、非 v1 schema、超过 1 MiB 或无效 JSON 均拒绝。
- manifest 固定包含 Development/Production channel、SemVer release、最低可升级版本、单调
  `release_sequence`、Unix 发布时间和 1 至 8 个 artifact。artifact 明确携带 target triple、arch、
  HTTPS URL、字节数和小写 SHA-256；只接受既定的四个 Windows/macOS target/arch 组合。
- verifier 由不可变构建环境、当前 target、当前版本、已安装 release sequence 和编译期信任公钥集
  构造。公钥以稳定 key ID、环境 channel 和 sequence 有效窗绑定；未知、跨环境、重复或超出轮换窗
  的 key 均拒绝。私钥不进入源码、构建产物或运行时配置。
- manifest sequence 低于已安装值，或新版本没有更高 sequence，视为降级攻击。当前版本低于
  manifest 的最低可升级版本时不尝试就地更新；相同或更旧版本只返回 up-to-date，不产生安装候选。
- 每个环境的最高已验证 sequence 由 `bongocat-update::UpdateSequenceStore` 从不可变
  `StorageLayout` 单独保存为 `updates/update-sequence.json` v1，不进入用户 config 或窗口 state 事务。
  状态固定包含 channel
  和最高 sequence；同目录锁串行化 writer，写入经同目录原子替换并回读验证。未知字段、非 v1、
  跨环境、符号链接、损坏或较低 sequence 一律失败关闭，绝不重置或覆盖已有状态。Unix 在创建或
  重开 `updates/` 时强制 `0700`；Windows 继续以用户 profile ACL 作为同等私有边界。
- 验证成功只返回字段私有、只提供只读 accessor 的项目自有不可变 `VerifiedUpdate`/`VerifiedArtifact`。
  第三方 URL、SemVer、签名或 digest 类型不进入公共 API。下载层必须把流交回 `VerifiedArtifact`
  同时验证精确长度和 SHA-256，通过前不得进入安装阶段。
- `UpdateVerificationSession` 将 verifier 与当前环境的 sequence store 成对创建：打开时读取最高
  sequence，验签和严格 manifest 验证成功后才记录新 sequence；`Available` 与 `UpToDate` 都属于
  成功验证，并立即更新 session 的 rollback 下限。验证失败不得创建或推进 state；该 session
  不联网、不下载或启动 installer。
- 公共 schema 与 accept/reject fixtures 位于 `shared/update/`。签名传输 envelope、endpoint、下载
  恢复、OS 包签名、installer 权限与回滚属于后续独立 contract。

## 依赖与替换边界

- `ed25519-dalek = 3.0.0`（BSD-3-Clause）提供纯 Rust Ed25519 严格验签；关闭默认 zeroize feature，
  只启用 verifier 所需 fast 表。维护仓库为 dalek-cryptography，Rust 1.85+。
- `sha2 = 0.11.0`、`semver = 1.0.28` 和 `url = 2.5.8` 均为当前稳定版，许可证为
  MIT OR Apache-2.0，支持项目 Rust 1.97。它们分别只位于 digest、版本和 HTTPS 解析边界。
- `bongocat-config` 是同一 workspace 的平台无关依赖，只提供不可变的 `StorageLayout` 与
  `BuildEnvironment`，用于把更新 sequence 固定在当前环境的 `updates/` 根；它不向 update
  verifier 暴露配置内容、平台 handle 或 UI 类型。
- 这些依赖均由 `bongocat-update` 封装；若停止维护，可替换实现而不改变 app/runtime/UI 公共协议。
  manifest 算法或字段变化仍需新 schema/ADR，不能在 v1 中静默替换。

## 验证

- 单元测试使用固定测试私钥覆盖有效签名、篡改、环境错配、公钥轮换窗、sequence 降级、最低版本、
  target/arch 错配、重复 target、未知字段、HTTP URL、artifact 长度/hash 和读取失败。
- Draft 2020-12 门禁验证共享 accept/reject fixtures；Rust 测试对同一 valid fixture 签名并解析，防止
  schema 与实现漂移。
- 完整 Native workspace 在 macOS、Windows 和 Ubuntu 执行 format、严格 Clippy、测试与 release
  check；dependency policy 继续覆盖许可证和 registry source。

## 后续边界

下一步建立环境绑定 endpoint 与 24 小时自动/手动检查调度，再实现有界下载、取消、临时文件清理、
平台 installer、操作系统签名验证和失败回滚。没有这些证据前不得声称更新功能或 stable 发布完成。
