# Native Configuration Store Spike

状态：typed config、Development/Production 隔离、schema validation、原子提交、expected revision 和 writer lock contract 已通过；Windows resolver、崩溃注入和 stale-lock 恢复待产品 crate 阶段完成
日期：2026-08-28

## 已固定的行为

`spikes/config-store/` 只实现 Native Rewrite 配置契约，不读取或转换旧 Tauri/Pinia 数据：

- Bundle ID 为 `com.ayangweb.bongo-cat`；相同 base 下使用 `development/` 与 `production/` 两个互斥根目录；
- 两个环境具有完全一致的相对结构：`config.json`、`state.json`、`models/`、`backups/`、`logs/`、`locks/`；
- `NativeConfig` 是强类型结构，显式 `schema_version = 1`，JSON key 使用当前产品语义的 `snake_case`；
- typed parser 与 JSON schema 都拒绝未知字段，避免拼写错误或旧配置字段被静默接受；
- commit 流程为 validate -> serialize -> 同目录临时文件 -> `sync_all` -> 备份当前有效配置 -> rename -> 提交后重新打开验证；校验失败不会覆盖旧配置，成功提交会保留 `backups/config.previous.json`；
- 损坏 JSON 会返回诊断错误并保留原始文件，不静默写回默认配置；
- `BuildEnvironment` 只在构造 store 时选择。产品实现必须由构建产物固定环境，禁止 CLI、环境变量或设置项在运行时切换；`platform_layout` 不接受外部路径覆盖。
- 每个环境在 `locks/config.writer.lock` 使用同目录独占创建保护提交；锁 guard drop 时释放，已持有锁的 writer 以稳定 `LockUnavailable` 错误失败。
- `revision()` 返回由验证后 NativeConfig 稳定序列化计算的 equality token；`commit_if_revision` 在持锁后重新读取当前配置，revision 不匹配时返回 `RevisionConflict`，不得静默覆盖较新的修改。

## 验证

```text
cargo fmt --manifest-path spikes/config-store/Cargo.toml -- --check
cargo test --manifest-path spikes/config-store/Cargo.toml --locked
cargo run --manifest-path spikes/config-store/Cargo.toml --locked
```

当前测试覆盖环境隔离、默认配置、非法提交保留旧文件、损坏配置不被静默覆盖、成功提交保留上一份有效备份、snake_case 序列化、writer lock 冲突/释放、stale revision 拒绝和平台目录 resolver。macOS 实机运行输出应位于 `~/Library/Application Support/com.ayangweb.bongo-cat/development/`（debug）或 `production/`（release）；Windows 对应 `%APPDATA%\\BongoCat\\<environment>\\` 仍待 Windows 实机验证。`tempfile` 只用于测试，`dirs` 仅用于该 resolver spike。

## 未完成

Windows 的真实用户目录 resolver、权限/磁盘满/目标占用、中断恢复、stale lock 清理、备份保留策略和 GPUI typed command 尚未在该 spike 中实现；这些必须在 Phase 6 配置 crate 中分别验证。当前 lock 文件没有自动超时或进程存活探测，异常终止后的恢复策略不能由本 spike 推断。
