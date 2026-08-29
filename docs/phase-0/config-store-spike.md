# Native Configuration Store Spike

状态：typed config、Development/Production 隔离、双平台 path resolver、schema validation、原子提交、expected revision、OS writer lock 和强制进程终止恢复 contract 已通过；产品级备份策略待后续阶段完成
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
- 每个环境在持久的 `locks/config.writer.lock` 上获取标准库 OS advisory lock；Unix 使用 `flock`，Windows 使用 `LockFileEx`。锁 guard drop 或进程终止关闭 handle 时由内核释放，已持有锁的 writer 以稳定 `LockUnavailable` 错误失败；不再依赖删除 lock file 判断 owner 存活。
- `revision()` 返回由验证后 NativeConfig 稳定序列化计算的 equality token；`commit_if_revision` 在持锁后重新读取当前配置，revision 不匹配时返回 `RevisionConflict`，不得静默覆盖较新的修改。
- 启动加载前会在 writer lock 内检查 `config.json.tmp`：主配置有效时不提升临时文件，而是归档为 `backups/config.interrupted*.json`；主配置缺失或损坏且临时文件有效时才提升临时文件，并将损坏主配置归档为 `backups/config.corrupt*.json`；临时文件无效时归档为 `backups/config.interrupted.invalid*.json`，不覆盖主配置。归档使用只创建不覆盖的递增文件名，避免 Unix/Windows `rename` 语义差异。恢复操作没有临时文件时返回 `NothingToRecover`，可重复执行且不会重复修改已恢复结果。

## 验证

```text
cargo fmt --manifest-path spikes/config-store/Cargo.toml -- --check
cargo test --manifest-path spikes/config-store/Cargo.toml --locked
cargo run --manifest-path spikes/config-store/Cargo.toml --locked
```

当前测试覆盖环境隔离、默认配置、非法提交保留旧文件、损坏配置不被静默覆盖、成功提交保留上一份有效备份、snake_case 序列化、writer lock 冲突/释放、stale revision 拒绝、四种中断提交恢复路径、归档不覆盖和平台目录 resolver（macOS/Windows 各 17 个 Rust unit test，分别包含 target-specific resolver test）。另有 1 项 process integration test：子进程持有 writer lock，写入并 flush `config.json.tmp` 后等待；父进程确认并发写被拒绝，再强制终止子进程，验证锁自动释放、临时配置被归档且当前配置不变。该测试已在 macOS 本机和 Windows push run `33251278193`、job `99097261951` 通过。

macOS 实机运行输出应位于 `~/Library/Application Support/com.ayangweb.bongo-cat/development/`（debug）或 `production/`（release）；Windows test 在 push run `33250708023` 已通过 `dirs::data_dir()/BongoCat/<environment>/` 精确断言，也就是 `%APPDATA%\\BongoCat\\<environment>\\`。同一 job 随后暴露 Windows `FlushFileBuffers` 不接受只读 handle：11 个写入/恢复测试返回 `AccessDenied`；改用可写 handle 后，push run `33251112463`、job `99096826978` 已通过全部配置测试，后续 process recovery run 继续通过。`tempfile` 已限制为 dev-dependency，`dirs` 仅用于该 resolver spike；writer lock 使用 Rust 1.89 起的标准库 API，没有新增第三方依赖。

## 未完成

权限/磁盘满/目标占用、备份保留策略和 GPUI typed command 尚未完成；这些必须在 Phase 6 配置 crate 中分别验证。进程存活不再通过 PID、时间戳或删除 lock file 猜测，异常退出后的释放由 OS file lock 生命周期保证。
