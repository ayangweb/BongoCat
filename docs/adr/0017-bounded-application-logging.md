# ADR-0017: Bounded Application Logging

状态：已接受（2026-09-01）

## 背景

Native Rewrite 需要可诊断的应用生命周期记录，但不能把按键、路径、剪贴板或用户文件内容写入
日志。Cubism Core callback 只覆盖厂商边界，不能替代应用级日志；无界文件增长也会破坏环境隔离
和稳定性验收。

## 决策

- `bongocat-app` 持有唯一 app-owned `ApplicationLogHandle`，业务调用只能提交固定的组件、级别和
  code，writer 不接受自由格式消息。
- 文件名使用 `application-<utc_day>.jsonl`，每行只包含 `component`、`level` 和 `code` 三个稳定
  字段。单文件最多 1 MiB；轮转文件使用 `.1` 到 `.8` 后缀。
- writer 最多保留 8 个应用日志文件、总计不超过 8 MiB，并在启动和日期切换时删除超过 7 个 UTC
  日的文件。清理顺序按日期和 generation 确定排序，当前文件不会因总量清理被删除。
- 写入、轮转或清理失败不得传播动态 I/O 文本；writer 只增加 `dropped`/`pruned` 等匿名统计。
  诊断导出只读取这些统计，不复制原始日志。
- Cubism Core 日志仍由其 FFI callback 专用 sink 管理；两类日志不共享 callback 或裸 handle，
  也不在本 ADR 中宣称 Core 历史文件已经统一清理。
- Application startup creates `application-running.marker` inside the environment's log directory.
  The marker contains only a schema version, is flushed before services start, and is removed only
  after runtime/audio shutdown and the `shutdown_completed` event. A leftover marker causes the next
  startup to record `previous_run_unclean`; panic, forced termination, and failed shutdown therefore
  remain diagnosable without recording payloads or paths.

## 验证

`bongocat-app` 单元测试覆盖固定字段、日期切换和过期清理、1 MiB 轮转、文件数量上限、无效
目录失败路径以及运行标记的异常保留/正常清理；settings 测试验证导出包含匿名应用日志统计
且不泄漏路径或模型身份。

## 后续边界

跨域原始日志预览、Core/application 历史文件合并和更新系统诊断 manifest 仍需独立设计；在这些
证据完成前，日志导出和 Phase 7 日志门禁保持未勾选。
