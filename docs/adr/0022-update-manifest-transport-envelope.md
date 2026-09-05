# ADR-0022: Update Manifest Transport Envelope

状态：已接受（2026-09-05）

## 背景

ADR-0021 已固定签名 manifest 的离线验证边界，但尚未定义网络响应如何无损携带 key ID、detached
signature 和原始 body。让 HTTP client、设置服务或发布脚本各自选择 JSON wrapper、base64 或 redirect
策略会使待验签 bytes 产生歧义，也会把传输库类型带入信任判断。

## 决策

- manifest endpoint 只能经 HTTPS `GET` 获取，客户端不跟随 redirect，只接受 `200` 响应。endpoint
  自身继续由不可变环境发布配置提供，不能来自用户 config、CLI 或运行时输入。
- 响应 body 是不经 JSON 解析、字符转换、解压或重编码的 manifest 原始 bytes，最大为 1 MiB。
- `bongocat-update-key-id` header 携带最多 64 字节的 ASCII key ID，只允许字母、数字、`-`、`_` 和 `.`。
  `bongocat-update-signature-ed25519` header 携带恰好 128 个小写十六进制字符，代表 64-byte detached
  Ed25519 signature。缺失、过长或语法无效的 header 以稳定 update error code 失败关闭。
- transport adapter 只把有限 header 和精确 body 转为 `UpdateManifestEnvelope`。它不验证 manifest、写入
  sequence state、下载 artifact 或调用 installer；envelope 只可交给 ADR-0021 的 verifier/session。
- HTTP、TLS、代理、状态文本和 response 类型不离开 transport adapter。自动/手动调度、取消、下载与
  安装另行实现。

## 验证

`bongocat-update` 单元测试覆盖 envelope 的 body 上限、key ID/signature 编码拒绝、exact-byte 保留以及
通过 envelope 的验签路径。后续网络 adapter 必须补充真实 HTTPS、redirect、代理、非-200、截断和超限
body 测试。

## 后续边界

本 ADR 不选择 HTTP crate、Production endpoint 或公钥注入方式，也不授权下载、包签名、installer 权限或
回滚行为。
