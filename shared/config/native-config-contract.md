# Native Configuration Contract

状态：Native schema v2；JSON schema、Rust 类型与 fixtures 同步维护

## Naming

- JSON key 统一使用 `snake_case`，Rust 字段保持同名，不维护旧字段 alias。
- 名称描述当前产品语义，不沿用历史 UI 组件名、Pinia store 名或平台 API 名。
- 单位写入字段名：毫秒使用 `_ms`，百分比使用 `_percent`，帧率使用 `_fps`。
- 布尔值使用可直接判断真假的语义名称，避免 `mode`、`behavior`、`enabled2` 等含糊字段。
- 路径不直接作为资源身份；模型使用由 Native Rewrite 生成的稳定 ID，并在模型索引中解析。

## Initial Shape

配置顶层按领域分区：

```text
schema_version
application
appearance
overlay
model
shortcuts
```

当前命名基线：

| Section       | Field                             | Meaning                                |
| ------------- | --------------------------------- | -------------------------------------- |
| `application` | `launch_at_login`                 | 登录后启动                             |
| `application` | `show_taskbar_icon`               | Windows 任务栏可见性                   |
| `application` | `show_status_icon`                | 托盘/菜单栏入口可见性                  |
| `application` | `check_for_updates_automatically` | 自动检查更新                           |
| `appearance`  | `theme`                           | `system`、`light` 或 `dark`            |
| `appearance`  | `language`                        | UI locale                              |
| `overlay`     | `visible`                         | 主猫窗口可见性                         |
| `overlay`     | `click_through`                   | 指针事件是否穿透                       |
| `overlay`     | `always_on_top`                   | 是否置顶                               |
| `overlay`     | `scale_percent`                   | 模型/窗口缩放百分比                    |
| `overlay`     | `opacity_percent`                 | 窗口不透明度百分比                     |
| `overlay`     | `corner_radius_percent`           | 窗口圆角百分比                         |
| `overlay`     | `hide_on_pointer_hover`           | 指针移入时隐藏                         |
| `overlay`     | `hide_on_pointer_hover_delay_ms`  | 移入隐藏延迟                           |
| `overlay`     | `keep_inside_work_area`           | 保持在可见工作区                       |
| `model`       | `selected_model_id`               | 当前模型稳定 ID，与 origin 成对为空    |
| `model`       | `selected_model_origin`           | `preset` / `installed`，与 ID 成对为空 |
| `model`       | `mirror`                          | 水平镜像模型                           |
| `model`       | `mirror_pointer_tracking`         | 镜像指针跟随方向                       |
| `model`       | `play_motion_audio`               | 播放动作音效                           |
| `model`       | `enable_behavior_shortcuts`       | 启用模型动作/表情绑定                  |
| `model`       | `maximum_fps`                     | overlay 最大帧率                       |
| `model`       | `ignore_pointer`                  | 模型求值忽略指针位置                   |
| `model`       | `release_fallback_timeout_ms`     | 输入校正失败后的最后保险，不是主语义   |
| `shortcuts`   | `commands`                        | 应用 command 到快捷键绑定              |
| `shortcuts`   | `model_behaviors`                 | 模型动作/表情绑定                      |

窗口坐标、pressed state、权限结果、模型解析缓存和 renderer 状态不属于 `config.json`。可恢复窗口布局写入 `state.json`；其余瞬时/派生状态不持久化。

## Application State v1

- `state.json` 使用独立的 `schema_version: 1`，只保存可恢复的设置窗口布局；它不是用户配置，
  不参与 `config.json` 的 revision、backup、migration 或 recovery-only 状态机。
- `settings_window` 为空时，窗口在当前主显示器居中以 `800x600` 打开。非空值保存 GPUI
  逻辑坐标 `x`/`y`、逻辑尺寸 `width`/`height` 和 `maximized`；坐标允许负值，限制在
  `-1000000..=1000000`，尺寸限制为宽 `640..=16384`、高 `480..=16384`。
- 恢复前再次检查窗口与当前显示器是否相交；显示器移除或完全离屏时回到居中默认布局，
  并以实际可见 bounds 更新内存 state。fullscreen 不作为可恢复设置窗口状态。
- 缺失、损坏、未知字段、越界值或读取失败只忽略 state 并使用默认布局，不阻塞
  `config.json`、runtime 或 recovery-only 窗口。未来 state schema 同样回退默认布局，但
  当前版本拒绝覆盖未来文件，避免降级破坏。
- state 使用环境内独立的 `locks/state.writer.lock` 和原子替换；提交后重新读取并比较 typed
  state，验证失败恢复替换前 bytes。它不创建 config backup/quarantine，也不读取另一环境。
- GPUI bounds observer 只更新共享 typed 内存 tracker，不执行文件 I/O。macOS Entity 重建和
  Windows隐藏/重显都读取该 tracker；settings service worker 在正常 shutdown 前 flush，失败
  形成稳定匿名错误但仍继续停止 runtime/audio 等 owner。

## Schema Evolution

- v1 只有 `selected_model_id`，产品启动时将非空值解释为预置模型。
- v2 增加 `selected_model_origin`；v1 的非空 ID 顺序迁移为 `preset`，空 ID 迁移为
  空 origin。迁移在当前环境 writer lock 内先生成配置备份，再原子写回。
- v2 要求 ID 与 origin 同时有值或同时为空；不探测旧 Tauri/Pinia 字段，不接受 alias。

## Backup Retention

- 每份备份是内部 JSON envelope，包含 `backup_format_version`、`created_at_unix_ms`、
  `source_schema_version`、16 位小写十六进制 `source_revision` 和未迁移的原始 `config` 值。
- 文件名使用 `config-<20-digit-order>-<5-digit-sequence>.json` 自有命名空间。排序键不会因
  系统时钟回退而倒退；envelope 时间仍记录真实持久化墙上时间。
- 每个环境最多保留最新 8 份、总计最多 8 MiB，最旧优先清理。其他文件不参与计数或删除。
- 备份创建或 retention 收敛失败会中止配置替换，继续保留当前 `config.json`。

## Write Failure Contract

- 项目稳定区分 `PermissionDenied`、`StorageFull` 和 `TargetOccupied`。前两类分别覆盖操作系统的
  权限/只读文件系统与空间/配额不足结果；固定 `config.json.tmp` 已存在，或原子写入返回等价
  的文件/目录占用结果时归为 `TargetOccupied`。其他 I/O 失败保持通用错误。
- settings service 将三类原因映射为匿名且可操作的 typed error code，不公开配置路径、临时路径或
  操作系统原始错误文本；失败 command 不推进 settings revision，也不修改 runtime snapshot。
- atomic writer 只删除本次调用已成功创建的 temp。创建前已存在的文件、目录或符号链接，以及在
  检查与 `create_new` 之间并发出现的条目都不得删除；写入失败继续逐字节保留 current。
- 测试在 temp 创建前注入权限失败、创建后注入空间不足，并使用真实文件/目录占用 temp target；每种情况
  都验证 current 保留、partial temp 清理边界和稳定 settings error。

## Corruption Recovery

- 当前 `config.json` 无法 parse、迁移或 validate 时，在当前环境 writer lock 内按文件名从新到旧
  检查自有备份；未知文件、目录和符号链接不作为候选，也不得读取另一环境。高于当前版本的
  schema 直接报告不支持并逐字节保留，禁止把较新配置自动降级为旧备份。
- 候选必须同时通过 envelope 格式版本、非零创建时间、源 schema 与内嵌 config 一致性、
  16 位规范化 source revision 以及完整 Native typed validation。未来格式、未来 schema、revision
  不匹配和损坏候选会被跳过；只恢复第一个完全有效的候选，并原子写回当前 v2 规范化 JSON。
- 替换前把损坏的当前文件逐字节保存为
  `config-corrupt-<20-digit-order>-<5-digit-sequence>.bin`。该自有 quarantine 每环境最多保留
  最新 4 份、总计最多 8 MiB；其他文件不参与计数或删除。
- 写回后必须重新读取并验证 config 与 revision。应用只保留恢复源 schema 和跳过的较新候选数，
  不把路径、原始配置或底层 I/O 文本放入 snapshot。
- 没有有效候选、损坏原文超过 quarantine 上限、归档/收敛失败或写回验证失败时明确报错；不得
  静默创建默认配置。写回验证失败会尝试把原始损坏字节恢复到 `config.json`，quarantine 仍保留。

## Safe Recovery Mode

- 当 current 损坏且所有自有 backup 候选均无效时，`ConfigStore::load_or_default` 返回
  `NoValidRecoveryBackup`，不得创建默认配置或覆盖原文件。Application 将该结果转换为匿名的
  `RecoveryRequired { checked_backups }`，启动 recovery-only settings 窗口，不创建 overlay/GPU、
  不激活模型；所有业务写入、模型、启动项和 overlay command 在此状态被拒绝。
- 用户必须显式发送 typed `RestoreDefaultConfiguration`。store 在同一 writer lock 内重新检查
  current，拒绝未来 schema、有效 current 或已恢复状态；确认仍损坏后把原字节写入现有有界
  `config-corrupt-*` quarantine，再原子写入、flush、读取并验证 v2 默认配置。成功后返回
  `DefaultsRestoredRestartRequired`，保留 recovery-only runtime，必须重启才恢复正常业务状态。
- 恢复 command 失败不得覆盖 current；quarantine、默认写入或验证失败均返回稳定错误。Diagnostics
  只显示状态和候选计数，不显示路径、原始 bytes、时间戳或底层 I/O 文本。
- Diagnostics 始终提供 typed `OpenConfigBackupLocation` command，包括 `RecoveryRequired` 状态。
  settings 协议不携带目录参数：Application 从当前环境 `StorageLayout.backups` 派生路径并交给 platform
  adapter；UI/snapshot/error 不得接收绝对路径或原始 OS 错误。成功打开目录不改变业务状态或 settings
  revision，失败映射为稳定的 `BackupLocationOpenFailed`。

## Interrupted Commit Recovery

- 正式提交先以 `create_new` 在 `config.json` 同目录写入固定的 `config.json.tmp`，完整写入并
  `sync_all` 后才执行跨平台原子替换。替换后重新读取并验证 typed config/revision，再删除 temp；
  可处理的写入、替换、清理或验证失败会尝试恢复旧 current 并清理 partial temp。进程被强制终止
  时，已经 flush 的 temp 可保留到下一次启动。
- 启动恢复与后续 load/default creation 共用一个 writer lock guard。只有该路径以 10 ms 间隔、
  最多 1 秒等待异常退出后的 OS lock 释放；普通 commit 冲突立即返回 `LockUnavailable`。
- temp 有效且 current 有效或使用未来 schema 时，current 优先，temp 归档为 stale；current 缺失时
  提升 temp；current 损坏时先逐字节 quarantine current，再提升 temp。temp 无法 parse、迁移或
  validate 时归档为 invalid，不覆盖 current；若 current 也缺失，归档后按正常首次启动创建默认值。
  temp 使用未来 schema 时逐字节原样保留并返回 `UnsupportedSchema`，不得归档或降级。
- interrupted archive 使用
  `config-interrupted-{stale|invalid}-<20-digit-order>-<5-digit-sequence>.bin` 自有命名空间；两类
  合计每环境最多保留最新 4 份、总计最多 8 MiB，未知文件不计数、不读取且不删除。恢复重启必须
  幂等，Development/Production 不得共享 candidate、archive 或 lock。
- app 只保留 `ArchivedStaleTemp`、`ArchivedInvalidTemp` 或是否替换损坏 current 的
  `PromotedTemp` 匿名动作；不得向 runtime/settings 暴露路径、配置内容、时间戳或底层 I/O 文本。

## Environment Isolation

`Development` 与 `Production` 使用同一 schema、默认值和相对目录结构，只由数据根目录区分。配置内容不保存环境字段，也不能引用另一环境的绝对路径。

路径与构建约束见 ADR-0008。
