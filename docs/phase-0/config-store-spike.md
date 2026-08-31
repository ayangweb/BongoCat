# Native Configuration Store Spike

状态：typed config、Development/Production 隔离、双平台 path resolver、schema validation、原子提交、expected revision、OS writer lock 和强制进程终止恢复 contract 已通过；正式产品已提升有界备份、损坏恢复和中断提交恢复
日期：2026-08-28

## 已固定的行为

`spikes/config-store/` 只实现 Native Rewrite 配置契约，不读取或转换旧 Tauri/Pinia 数据：

- Bundle ID 为 `com.ayangweb.bongo-cat`；相同 base 下使用 `development/` 与 `production/` 两个互斥根目录；
- 两个环境具有完全一致的相对结构：`config.json`、`state.json`、`models/`、`backups/`、`logs/`、`locks/`；
- `NativeConfig` 是强类型结构，当前与共享默认 fixture 对齐为 `schema_version = 2`，模型选择
  使用成对的 `selected_model_origin`/`selected_model_id`，JSON key 使用当前产品语义的
  `snake_case`；v1 -> v2 迁移、原 bytes 备份和幂等写回只由正式 `bongocat-config` 实现；
- typed parser 与 JSON schema 都拒绝未知字段，避免拼写错误或旧配置字段被静默接受；
- commit 流程为 validate -> serialize -> 同目录临时文件 -> `sync_all` -> 备份当前有效配置 -> rename -> 提交后重新打开验证；校验失败不会覆盖旧配置，成功提交会保留 `backups/config.previous.json`；
- 损坏 JSON 会返回诊断错误并保留原始文件，不静默写回默认配置；
- `BuildEnvironment` 只在构造 store 时选择。产品实现必须由构建产物固定环境，禁止 CLI、环境变量或设置项在运行时切换；`platform_layout` 不接受外部路径覆盖。
- 每个环境在持久的 `locks/config.writer.lock` 上获取标准库 OS advisory lock；Unix 使用 `flock`，Windows 使用 `LockFileEx`。锁 guard drop 或进程终止关闭 handle 时由内核释放，已持有锁的 writer 以稳定 `LockUnavailable` 错误失败；不再依赖删除 lock file 判断 owner 存活。
- `revision()` 返回由验证后 NativeConfig 稳定序列化计算的 equality token；`commit_if_revision` 在持锁后重新读取当前配置，revision 不匹配时返回 `RevisionConflict`，不得静默覆盖较新的修改。
- 启动加载前会在 writer lock 内检查 `config.json.tmp`：主配置有效时不提升临时文件，而是归档为 `backups/config.interrupted*.json`；主配置缺失或损坏且临时文件有效时才提升临时文件，并将损坏主配置归档为 `backups/config.corrupt*.json`；临时文件无效时归档为 `backups/config.interrupted.invalid*.json`，不覆盖主配置。归档使用只创建不覆盖的递增文件名，避免 Unix/Windows `rename` 语义差异。恢复操作没有临时文件时返回 `NothingToRecover`，可重复执行且不会重复修改已恢复结果。
- 普通 `commit` 的 writer lock 冲突立即返回 `LockUnavailable`；仅启动恢复路径以 10 ms 间隔、最多 1 秒重试锁获取，用于容纳 Windows 被强制终止进程已经 `wait` 完成但 file lock 尚未对新进程可见的短暂窗口。超过门限仍返回原错误，不猜测 owner、不删除 lock file。
- `load_or_default` 在同一 writer lock guard 内完成 interrupted-temp 恢复、当前文件读取以及缺失时的默认配置提交；禁止 recovery 返回后立即释放再重新获取同一 Windows byte-range lock，避免内核释放可见性窗口造成全新目录首次启动偶发失败。

## 验证

```text
cargo fmt --manifest-path spikes/config-store/Cargo.toml -- --check
cargo test --manifest-path spikes/config-store/Cargo.toml --locked
cargo run --manifest-path spikes/config-store/Cargo.toml --locked
```

当前测试覆盖环境隔离、默认配置、非法提交保留旧文件、损坏配置不被静默覆盖、成功提交保留上一份有效备份、snake_case 序列化、writer lock 冲突/释放、stale revision 拒绝、四种中断提交恢复路径、归档不覆盖和平台目录 resolver（macOS/Windows 各 17 个 Rust unit test，分别包含 target-specific resolver test）。另有 2 项 process integration test：

- 子进程持有 writer lock，写入并 flush `config.json.tmp` 后等待；父进程确认并发写被拒绝，再强制终止子进程，验证锁自动释放、临时配置被归档且当前配置不变。该测试已在 macOS 本机和 Windows push run `33251278193`、job `99097261951` 通过。
- Development/Production 子进程同时从相同 application base 启动，分别提交 `zh-CN`/`pt-BR` sentinel；两个进程退出后由新建 store 重载并验证各自值和 lock root。macOS 本机与 Windows runner 均已通过。

macOS 实机运行输出应位于 `~/Library/Application Support/com.ayangweb.bongo-cat/development/`（debug）或 `production/`（release）；Windows test 在 push run `33250708023` 已通过 `dirs::data_dir()/BongoCat/<environment>/` 精确断言，也就是 `%APPDATA%\\BongoCat\\<environment>\\`。同一 job 随后暴露 Windows `FlushFileBuffers` 不接受只读 handle：11 个写入/恢复测试返回 `AccessDenied`；改用可写 handle 后，push run `33251112463`、job `99096826978` 已通过全部配置测试，后续 process recovery run 继续通过。`tempfile` 已限制为 dev-dependency，`dirs` 仅用于该 resolver spike；writer lock 使用 Rust 1.89 起的标准库 API，没有新增第三方依赖。

commit `cf16291e8cee027b6983abcf919a32fb5a0278a5` 的 push run `33251410654`、
Windows job `99097619545` 在 Windows Server 2025 / `windows-2025-vs2026` runner 上通过
17 项 unit test 和 2 项 process integration test；后者明确包含
`development_and_production_processes_commit_and_restart_independently`，验证两个环境并发
提交、退出后重建 store 及 sentinel/lock root 隔离。该证据只覆盖 config store，不替代未来
state、模型、日志、单实例和更新 channel 的环境隔离测试。

后续 push run `33255204781` 的独立 Windows config-store job 在强杀子进程并 `wait` 后立即
恢复时偶发一次 `LockUnavailable`，而同 commit 的 Windows input/config job、对应 PR job及
本地重复测试均通过，说明 ready-file 握手与进程终止已完成，但内核锁释放存在短暂可见性
窗口。第一版恢复重试在 commit `7d92daa` 的 push run `33255952549`、job `99109567157`
和 PR input/config job `99109571372` 通过；同一 PR 的独立 config-store job `99109571503`
随后在尚未启动 crash 子进程的首次 `load_or_default` 捕获第二个竞态：恢复锁刚释放就为默认
提交重新加锁。当前实现改为整个 load/recover/create-default 共用单个 guard，新的 Windows
runner 已在 commit `e776867` 的 push run `33256593886` 验证：独立 config-store job
`99111304933` 通过 18 项 unit test（含 100 次全新目录首次加载）和 2 项 process integration
test；input/config job `99111304790` 随后再次执行完整 config-store tests 也通过。

## 未完成

状态（2026-08-31）：正式 `bongocat-config` 已把产品提交固定为同目录
`config.json.tmp` -> flush -> 跨平台原子替换 -> 重新读取验证，并把上述 current/temp 状态机、
1 秒 crash-release lock 重试、强杀子进程回归、匿名 app action，以及每环境 4 份/8 MiB 的
stale/invalid interrupted archive 提升到产品实现。正式 store 的普通备份和损坏 current
quarantine 也已各自具有独立有界保留策略。

状态（2026-08-31）：正式 app/settings service 已增加无有效备份的 `RecoveryRequired` 安全模式、
recovery-only GPUI 窗口和显式 typed 恢复默认 command；恢复前禁止业务写入，恢复后要求重启，原
损坏字节继续进入有界 quarantine。commit `e2ced51` 的 run `33374202985` 已通过 Windows/macOS/
Ubuntu Native workspace、双平台 GPUI smoke 和 Windows input/config job，但尚未专门启动损坏
current 且无备份的 recovery-only 产品窗口。正式产品现已增加只使用独立临时存储根的受控
`--configuration-recovery-smoke`，本机 macOS 已验证 recovery snapshot、真实窗口、service join
和临时数据清理；commit `175e7a4` 的 run `33376471972` 随后在 Windows/macOS Native jobs
`99438972370`/`99438972328` 实际通过同一 recovery window smoke，`P6-CONFIG-SAFE-RECOVERY`
退出条件满足。正式 config crate 随后增加权限/只读、空间/配额不足和目标占用的稳定匿名
分类；单元测试在 temp 创建前后注入权限与磁盘满，并用真实文件/目录占用固定 temp，验证 current 与
非自有占用条目保留。settings service 已投影独立可操作错误并验证失败不推进 snapshot；commit
`0549f33` 的 run `33378437342` 已通过三平台 Native workspace、Windows input/config 和独立
config-store jobs，`P6-CONFIG-WRITE-FAILURES` 退出条件满足。进程存活不通过 PID、时间戳
或删除 lock file 猜测，异常退出后的释放由 OS file lock 生命周期保证。

状态（2026-08-31）：正式 settings 协议已增加无路径参数的 `OpenConfigBackupLocation` typed
command；Application 只把当前环境 `StorageLayout.backups` 交给 platform adapter，macOS/Windows
分别以独立进程参数启动 Finder/Explorer，不使用 shell。Diagnostics 的 Backups 按钮覆盖 pending、
匿名 error、Tab 与 Enter/Space 以及 accessibility button 语义；成功不推进 snapshot revision，
recovery-only 状态仍可调用。本机 platform/ui/app 定向测试和严格 Clippy 已通过，三平台完整门禁
由 `P6-CONFIG-BACKUP-LOCATION` 当前提交的 CI 跟踪。
