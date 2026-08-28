# Native Configuration Store Spike

状态：typed config、Development/Production 隔离、schema validation、原子提交和当前 macOS 真实 path resolver 已通过；Windows resolver、文件锁并发和崩溃注入待产品 crate 阶段完成
日期：2026-08-28

## 已固定的行为

`spikes/config-store/` 只实现 Native Rewrite 配置契约，不读取或转换旧 Tauri/Pinia 数据：

- Bundle ID 为 `com.ayangweb.bongo-cat`；相同 base 下使用 `development/` 与 `production/` 两个互斥根目录；
- 两个环境具有完全一致的相对结构：`config.json`、`state.json`、`models/`、`backups/`、`logs/`、`locks/`；
- `NativeConfig` 是强类型结构，显式 `schema_version = 1`，JSON key 使用当前产品语义的 `snake_case`；
- typed parser 与 JSON schema 都拒绝未知字段，避免拼写错误或旧配置字段被静默接受；
- commit 流程为 validate -> serialize -> 同目录临时文件 -> `sync_all` -> rename -> 提交后重新打开验证；校验失败不会覆盖旧配置；
- 损坏 JSON 会返回诊断错误并保留原始文件，不静默写回默认配置；
- `BuildEnvironment` 只在构造 store 时选择。产品实现必须由构建产物固定环境，禁止 CLI、环境变量或设置项在运行时切换；`platform_layout` 不接受外部路径覆盖。

## 验证

```text
cargo fmt --manifest-path spikes/config-store/Cargo.toml -- --check
cargo test --manifest-path spikes/config-store/Cargo.toml --locked
cargo run --manifest-path spikes/config-store/Cargo.toml --locked
```

当前测试覆盖环境隔离、默认配置、非法提交保留旧文件、损坏配置不被静默覆盖、snake_case 序列化和平台目录 resolver。macOS 实机运行输出应位于 `~/Library/Application Support/com.ayangweb.bongo-cat/development/`（debug）或 `production/`（release）；Windows 对应 `%APPDATA%\\BongoCat\\<environment>\\` 仍待 Windows 实机验证。`tempfile` 只用于测试，`dirs` 仅用于该 resolver spike。

## 未完成

Windows 的真实用户目录 resolver、跨进程 writer lock、权限/磁盘满/目标占用、中断恢复、备份保留策略、expected revision 和 GPUI typed command 尚未在该 spike 中实现；这些必须在 Phase 6 配置 crate 中分别验证。
