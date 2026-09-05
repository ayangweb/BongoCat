# ADR-0025: Signed Manifest HTTPS Transport

状态：已接受（2026-09-05）

## 背景

ADR-0021 和 ADR-0022 已定义离线验签及 HTTP envelope 字节协议，但没有实际 HTTPS client 时，
endpoint、redirect、压缩和 response-body 限制仍可能在后续 app 接线时被各自实现。尤其是 transparent
compression 会改变 detached Ed25519 signature 必须覆盖的原始 bytes。

## 决策

- `bongocat-update` 使用精确固定的 `ureq 3.4.0` 作为唯一 signed-manifest HTTPS transport。它只启用
  纯 Rust `rustls` feature，显式关闭默认 features，因而不启用 gzip、charset、cookie 或 proxy feature。
- `UreqUpdateManifestSource` 只接受 `UpdateManifestEndpoint`。endpoint 最多 2 KiB，必须是无 credentials、
  无 fragment 且具有 host 的 HTTPS URL；它来自后续不可变的构建发布配置，绝不来自用户 config、CLI 或
  运行时输入。
- 每次 fetch 只发送一次 GET，限制为 15 秒 global deadline，不跟随 redirect，且只接受 HTTP `200`。
  response body 以 `Read::take(1 MiB + 1)` 有界读取，不进行 JSON/字符集/压缩处理；超过上限和空 body
  均通过既有 `manifest_too_large` verifier code 失败关闭。
- transport 只构造 `UpdateManifestEnvelope`，由其继续校验 key-ID/signature header。endpoint、status、
  transport 和 body-read 错误只公开固定匿名 transport code；底层 URL、HTTP status text、TLS 或 client
  类型不离开 update crate。调用方必须在专用 update worker 运行 blocking fetch，禁止在 GPUI executor
  执行。
- `UpdateCheckCoordinator` 只能以 `UpdateManifestSource` 取得 envelope，再调用既有
  `UpdateVerificationSession`。其顺序固定为 fetch -> signature/schema/target verification -> sequence
  store commit；任何 fetch 或 verification failure 都不能改变当前环境的 anti-rollback state。
- 同一受限 agent 还提供 artifact source，但它只可打开 `VerifiedArtifact` 已验证的 URL，固定 HTTPS、
  无 redirect、`200`，并只返回 raw reader 或既有 `HttpStatus`/`Transport` retry 分类。下载 coordinator
  继续独占重试、取消与 staging，artifact verifier 继续独占长度和 hash 校验。

## 依赖与替换边界

`ureq 3.4.0` 为 MIT OR Apache-2.0、Rust 1.85+ 的公开维护 crate。它是纯 Rust blocking client；只在
`UreqUpdateManifestSource` 私有字段出现。若维护或安全边界不再满足要求，可替换该 source，不改变
`UpdateManifestEndpoint`、`UpdateManifestEnvelope`、verifier、runtime 或 UI contract。

## 验证

- 单元测试锁定 HTTPS/credential/fragment/长度 endpoint 边界、HTTPS-only、no-redirect agent config 和
  transport code 唯一性。
- 注入式 source 回归验证通过时仅写入验证后的 release sequence，而 source failure 后 sequence 保持不变。
- Native workspace 的 format、strict Clippy、test、release check 与 dependency policy 覆盖新 crate graph。
- 后续 app-owned worker 必须在真实 HTTPS endpoint 验证断网、代理、redirect、non-200、截断、超限、
  签名错误和降级攻击，且不影响本地 overlay。

## 后续

本 ADR 不选择 Production endpoint 或公钥，不实现 UI command、自动/手动 dispatch、installer 或 rollback。
它们仍需要各自的发布基础设施与安全验收。
