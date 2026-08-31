# ADR-0015: Separate UI Snapshot and Configuration Revisions

状态：已接受

## 背景

设置 snapshot 需要在 runtime、平台状态、输入诊断和模型目录变化时刷新。配置写入同时需要
compare-and-swap，防止窗口中的旧编辑覆盖后台配置。若两者共用一个 revision，纯诊断变化会
错误拒绝仍然有效的配置编辑。

## 决策

- `SettingsSnapshot.revision` 是 UI 可观察内容的单调版本，只用于丢弃过期异步结果和刷新视图。
- `SettingsSnapshot.config_revision` 是当前环境持久化配置的版本；恢复模式没有可编辑版本时为
  `None`。
- 所有已接入的直接配置 command（overlay visible/settings、motion audio）携带
  `expected_config_revision`，settings worker 在调用 Application 写入前比较它。
- 比较失败返回匿名 `SnapshotOutdated`，不改变 runtime、配置文件或任何 revision；UI 读取最新
  snapshot 并保留可操作错误。

## 后续边界

模型选择、启动项和其他配置域接入 optimistic concurrency 时继续使用 `config_revision`；纯
平台/诊断状态不得改变该 token。`revision` 仍可因这些状态变化推进。
