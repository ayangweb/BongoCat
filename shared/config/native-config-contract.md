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

## Environment Isolation

`Development` 与 `Production` 使用同一 schema、默认值和相对目录结构，只由数据根目录区分。配置内容不保存环境字段，也不能引用另一环境的绝对路径。

路径与构建约束见 ADR-0008。
