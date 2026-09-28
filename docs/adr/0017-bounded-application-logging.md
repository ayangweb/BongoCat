# ADR-0017: Bounded Application Logging

状态：已接受（2026-09-01）；2026-09-24 由 ADR-0064 修订文本格式、过滤与统一 retention

## 背景

BongoCat 需要可诊断的应用生命周期记录，但不能把按键、路径、剪贴板、配置/模型正文、URL、密钥/凭据或用户文件
内容写入日志。Cubism Core callback 只覆盖厂商边界，不能替代应用级日志；无界文件增长也会破坏环境
隔离和稳定性验收。早期实现使用 JSONL 与两个独立 writer，ADR-0064 已把输出改为普通文本，并让
application/Core 共用一套 writer policy。

## 决策

- `bongocat-app` 持有 app-owned `ApplicationLogHandle`，业务调用只能提交闭合 event catalog 中的
  module、stable code、固定 message 和有界安全上下文。路径、原始按键、剪贴板、配置/模型正文、
  动态 OS 错误文本和密钥不进入日志协议。
- application 与 Cubism Core 使用 `bongocat-log` 的同一套 UTF-8 单行文本 writer、级别过滤、按 UTC
  日切换、单文件大小保护、目录总量/文件数预算与 retention；两类 stream 仍分离，便于支持人员区分
  产品事件和厂商 callback message。
- 文件名固定为 `application-YYYY-MM-DD.log` / `cubism-core-YYYY-MM-DD.log`；轮转段使用
  `.<generation>.log`。单个文件达到 1 MiB 后继续切分，两类日志合计最多 8 MiB/32 个文件，当前
  UTC 日活动文件始终保留，过期或超预算的最旧已轮转文件优先删除。
- 当前 v1 `logging.level` 接受 `error`、`warn`、`info`、`debug`、`trace`，默认 `info`；
  `logging.retention_days` 接受 `1..=30`，默认 `7`。设置先原子持久化，再立即更新共享 controller
  并收敛允许删除的旧文件；不提供 rotation mode、文件大小或目录容量配置。
- Cubism Core callback 只复制至多 512 bytes 到容量 128 的 non-blocking queue；专用 Rust worker
  才调用共享 writer。callback 不执行格式化或文件 I/O；callback-slot contention、queue full 和 stop
  后迟到记录只增加匿名 dropped 计数。shutdown 先注销 callback，再拒绝新记录、排空队列并 join
  worker。Core callback 没有可信 severity，原始 message 固定按 `debug` 过滤；实际 Core/renderer
  失败由 app owner 以稳定 `error`/`warn` event 记录。
- Application startup creates `application-running.marker` inside the environment's log directory.
  The v1 marker contains only a schema version and one fixed phase: `running`, `shutting_down`, or
  `panicked`; it is flushed before services start and is removed only after runtime/audio shutdown
  and the `shutdown_completed` event. The next startup classifies a leftover marker anonymously as
  forced/unknown, shutdown interrupted, or panic, then immediately overwrites it with a new
  `running` marker. This preserves diagnosis without recording payloads or paths and prevents a
  stale marker from causing a repeated recovery loop.
- Diagnostics export includes aggregate counts for the fixed application event codes and a saturated
  application-plus-Core retained-byte/file summary, alongside separate anonymous source counters, as
  part of its current format version 1. Consumers must reject non-v1 data rather than guessing. The
  export never embeds raw log records, panic payloads, paths, key values, or user file content.
- Diagnostics preview 另由 ADR-0027/0064 约束：只从严格命名且可解析的 application `.log` 来源重写
  闭合 code，不复制原日志行；Core 原始 message 永远不进入 preview。

## 验证

`bongocat-log` 单元测试覆盖级别顺序、共享 controller、UTC 日切换、1 MiB 轮转、配置化保留、目录级
8 MiB/32 文件预算、未知文件/符号链接隔离、严格文本 parser 与 owner-only 权限。`bongocat-app` 覆盖
固定 event catalog、panic 脱敏、运行标记分类、共享过滤/retention 和脱敏 preview；`bongocat-live2d`
覆盖 Core callback contention、queue saturation、shutdown drain、文本写入和跨 writer prune 统计。
macOS Development release smoke 已验证 diagnostics-export 固定 entries、panic=abort 恢复和
settings-window lifecycle；独立 bounded normal run 在本机最新尝试中超时并留下 unclean marker，仍需复核。
Windows release smoke 与完整双平台证据仍待发布门禁补齐。

## 后续边界

远程上传、Core message 导出和第三方稳定日志 schema 不在本 ADR 范围。`.log` 面向用户与支持人员，
需要机器处理时使用匿名 `diagnostics.json` 或严格重写后的 preview entry。
