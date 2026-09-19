# Native Configuration Contract

状态：Native schema v1；JSON schema、Rust 类型与 fixtures 同步维护

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

| Section       | Field                                 | Meaning                                |
| ------------- | ------------------------------------- | -------------------------------------- |
| `application` | `show_taskbar_icon`                   | Windows 任务栏可见性                   |
| `application` | `show_status_icon`                    | 托盘/菜单栏入口可见性                  |
| `application` | `check_for_updates_automatically`     | 自动检查更新                           |
| `appearance`  | `theme`                               | `system`、`light` 或 `dark`            |
| `appearance`  | `language`                            | UI locale                              |
| `overlay`     | `visible`                             | 主猫窗口可见性                         |
| `overlay`     | `click_through`                       | 指针事件是否穿透                       |
| `overlay`     | `always_on_top`                       | 是否置顶                               |
| `overlay`     | `scale_percent`                       | 模型/窗口缩放百分比                    |
| `overlay`     | `opacity_percent`                     | 窗口不透明度百分比                     |
| `overlay`     | `corner_radius_percent`               | 窗口圆角百分比，`[0, 50]`              |
| `overlay`     | `hide_on_pointer_hover`               | 指针悬停在窗口上时隐藏内容并临时穿透   |
| `overlay`     | `hide_on_pointer_hover_delay_seconds` | 悬停隐藏前的等待秒数，`[0, 60]`        |
| `overlay`     | `keep_inside_screen`                  | 保持在所有屏幕范围内，允许覆盖任务栏等区域 |
| `input`       | `gamepad_stick_dead_zone`             | 左/右摇杆死区，`[0, 1)`                |
| `input`       | `gamepad_trigger_dead_zone`           | 扳机死区，`[0, 1)`                     |
| `model`       | `selected_model_id`                   | 当前模型稳定 ID，与 origin 成对为空    |
| `model`       | `selected_model_origin`               | `preset` / `installed`，与 ID 成对为空 |
| `model`       | `installed_models`                    | 用户导入模型的元数据列表（id + title） |
| `model`       | `mirror`                              | 水平镜像模型                           |
| `model`       | `mirror_pointer_tracking`             | 镜像指针跟随方向                       |
| `model`       | `play_motion_audio`                   | 播放动作音效                           |
| `model`       | `enable_behavior_shortcuts`           | 启用模型动作/表情绑定                  |
| `model`       | `maximum_fps`                         | overlay 最大帧率                       |
| `model`       | `ignore_pointer`                      | 模型求值忽略指针位置                   |
| `model`       | `release_fallback_timeout_ms`         | 输入校正失败后的最后保险，不是主语义   |
| `shortcuts`   | `commands`                            | 应用 command 到快捷键绑定              |
| `shortcuts`   | `model_behaviors`                     | 模型动作/表情绑定                      |

首次启动创建当前 v1 配置时，`overlay.click_through` 默认为 `false`。用户后续通过
typed settings command 修改该值后，仍按配置 revision 原子提交并在重启时从当前环境恢复。

`overlay.visible` 是会话内状态，不跨重启保留。每次启动确认配置可用（`Ready`）后，若当前 v1
值为 `false`，应用先在内存里把它规整回 `true`，再尽力按原子提交写回 `config.json`；写入失败不
阻塞启动，本次会话窗口仍然可见。恢复模式（`RecoveryRequired`）不创建 overlay，也不执行该规整。
这与旧版行为一致：旧版 `cat` store 的 init 每次启动都把 `window.visible` 置回 `true`，隐藏主猫
只是在当前会话生效，重启后必定重新显示。

`overlay.hide_on_pointer_hover` 默认 `false`，`overlay.hide_on_pointer_hover_delay_seconds` 默认
`0`（立即隐藏）。两者只在窗口呈现层生效：开启后指针进入 overlay 窗口矩形并停留满延迟时间，
窗口渲染 alpha 在 300ms 内降到 `0`，同时指针事件立即穿透；指针离开后 alpha 在 300ms 内恢复，
穿透状态回到 `overlay.click_through`。窗口本身不隐藏、不销毁，也不改变
`overlay.visible`。延迟值在 `next` 首版收窄为 `0..=60` 秒，理由见
`bongocat-config` 中 `OverlayConfig::hide_on_pointer_hover_delay_seconds` 的文档注释。该字段以
整秒存储，与设置页显示和输入的单位一致；overlay frame loop 仍以毫秒计时，只在
`OverlaySessionOptions` 边界换算一次。

`application.show_status_icon` 控制 Windows 托盘或 macOS 菜单栏状态图标，不销毁系统菜单的
唯一事件 owner。修改时先通过有界主线程 bridge 应用平台显隐，成功后才按 expected revision
原子提交配置；平台失败不提交，配置失败则恢复旧的平台可见性。启动直接应用当前 v1 值，隐藏后
仍可通过设置窗口、Windows 单实例唤醒或 macOS application reopen 恢复入口。

`application.show_taskbar_icon` 只控制 Windows 设置窗口的任务栏按钮，不隐藏或销毁窗口，也不
影响 overlay；macOS 不把该字段解释为 Dock 图标。修改时先在 GPUI owner 线程切换并回读 HWND
扩展样式，成功后才按 expected revision 原子提交配置；平台失败不提交，配置失败恢复旧样式。
启动和设置窗口重建都在窗口显示前应用当前 v1 值。

登录启动不属于配置字段。它是可被系统设置或其它进程改变的平台能力，settings service 只读取
typed platform snapshot，并仅在显式用户 command 时调用平台 adapter；不得持久化第二份布尔值。

`model.enable_behavior_shortcuts` 默认 `false`，只决定配置中的 `shortcuts.model_behaviors` 是否
进入活动的 `CompiledShortcuts`：关闭时不改写、不删除这些绑定，只把它们排除在平台匹配表之外，
`shortcuts.commands` 里的应用级快捷键不受影响；重新打开时无需重录即可恢复全部已校验绑定。

默认关闭是对旧版行为的刻意收窄。旧版在模型加载完成后无条件为每个 motion 和 expression 自动分配
一整层 `primary + [Shift/Alt] + 数字/字母` 组合键（`pre-refactor:src/composables/useModel.ts` 的
`getBehaviorShortcut`），用户即使从未打开行为列表也已经背上一批全局快捷键。Native 版本按维护者
决定移植这套自动分配（`docs/phase-0/behavior-inventory.md` 中该项已上调为 `P0 首发`），但把"是否
让这些组合键真正生效"交给用户：分配照常写入 v1 配置、在快捷键页面可见且可逐项修改，只有开关打开
后才进入平台匹配表。这与旧版不同——旧版没有这个开关。

自动分配的契约与旧版一致：`primary` 在 macOS 是 `Command`、其它平台是 `Control`；分层顺序为
`[primary]`、`[primary, Shift]`、`[primary, Alt]`、`[primary, Shift, Alt]`；每层先走数字
`1234567890` 再走字母 `QWERTYUIOPASDFGHJKLZXCVBNM`，共 `4 × (10 + 26) = 144` 个名额
（`bongocat_config::BEHAVIOR_SHORTCUT_CAPACITY`）。模型的行为顺序是声明顺序：先按 motion group
的声明顺序遍历组内 motion，再遍历 expression；行为 id 沿用 `motion:{group}:{index}` 与
`expression:{name}`。超过 144 个的行为保持未绑定，与旧版返回空字符串一致。

两处有意的行为差异，都是为了让分配结果一定是合法配置：

- **已被占用的组合键跳过，不复用。** 旧版按位置索引取组合键，用户改动过某一项之后，后续行为可能
  被分到一个已被占用的组合键；Native 的 `shortcuts.conflict` 校验会因此判定整份配置无效，而不是
  仅仅产生歧义。
- **应用级 command 绑定同样计入占用。** 旧版把窗口快捷键与行为快捷键放在两个 store 里，尽管两者
  都是全局注册，因此可以互相撞车。

分配是幂等的：已有绑定的行为永不重写，所以模型每次激活都可以安全地重复执行，只会补上用户尚未
录制的那部分。因此快捷键页面的每一项都同时具备"有默认值"和"可单独修改"两种性质——修改过的项在
后续激活与"恢复默认"之间保持用户的值，未被修改的项则回到默认分层。

⚠️ 由此产生一个用户可见的不对称，需要明确记录：快捷键页面的"清除全部快捷键"送出的是
`SettingsShortcuts::default()`，它对 `shortcuts.commands` 是**持久**的，对
`shortcuts.model_behaviors` **不是**——下一次模型激活（重启、切换模型，或再次激活同一个模型）会把
该模型未绑定的行为重新补上默认组合键。这是旧版行为的直接结果（旧版每次模型加载都无条件重分配、
没有开关），所以"让这些组合键不生效"的正确手段是 `model.enable_behavior_shortcuts` 开关，而不是
靠清空。`bongocat-app` 的
`clearing_all_shortcuts_does_not_survive_the_next_activation` 把这个不对称钉住：改动任一侧都必须
是显式决定，而不是顺手改掉。

"恢复默认"（`restore_default_shortcuts`）清空应用级 command 绑定后，会为**当前已激活的模型**重新
执行一次分配，而不是把行为列表清空。注意"已激活"不等于配置里的 `selected_model_id`：全新配置没有
选中任何模型，启动时仍会激活 standard 预置，此时按 `selected_model_id` 判断会得到空列表。

窗口圆角（`overlay.corner_radius_percent`）与指针悬停隐藏（`overlay.hide_on_pointer_hover`、
`overlay.hide_on_pointer_hover_delay_seconds`）原本属于 `P1 首发后` 范围，现按维护者决定上调为
`P0 首发`，作为当前 v1 字段进入 `next` 初始版本。这不引入迁移或兼容逻辑：`next` 仍是全新首版，
三个字段直接写在当前 v1 schema、默认值、fixture 与实现中。

`appearance.language` 是严格的三值枚举：`system`、`zh-CN` 和 `en-US`，默认 `system`；未知值
由当前 v1 解析入口直接拒绝，不提供 alias、迁移或 fallback。`system` 在每次启动时读取平台首选
locale：简体中文 locale 映射为 `zh-CN`，英语和所有其它 locale 都映射为 `en-US`。其中
`zh-Hant` 与 `TW`/`HK`/`MO` 不属于当前支持的简体中文，按统一规则回退英文。系统解析结果只决定
实际显示语言，不覆写持久化偏好；显式选择 `zh-CN` 或 `en-US` 时不受系统 locale 影响。

`model.release_fallback_timeout_ms` 接受 `0..=60000`，默认 `500`；`0` 明确禁用。该值只控制
runtime 对 captured keyboard control 的最终保险，不替代可靠 KeyUp、平台 pressed-set 校正或
生命周期 Reset，也不作用于鼠标和手柄。runtime 以自己的可注入单调时钟记录 down/repeat 的观察
时刻，repeat 刷新期限；不得把 Windows/macOS input producer 的事件时间戳跨时钟原点比较。
设置通过携带 `expected_config_revision` 的 typed command 原子持久化并即时更新 runtime。

## Shortcut Chords

快捷键配置仍以字符串持久化，但在写入前必须通过平台无关的 chord 校验。每个 chord
包含零个或多个修饰键和恰好一个 key，使用 `+` 分隔；修饰键接受
`Control`/`Ctrl`、`Alt`/`Option`、`Shift` 和 `Meta`/`Command`/`Cmd`/`Win` 别名。
key 必须属于稳定的物理键闭合集合：`A`-`Z`、`0`-`9`、`F1`-`F12`、legacy 设置页
支持的标点、编辑键和导航键；`KeyA`/`Digit1` DOM code 作为输入 alias 接受，持久化仍分别
规范化为 `A`/`1`。每个 key 在编译后携带平台无关的 USB HID usage，未知 token 在配置
提交前拒绝。校验器以固定顺序
`Control+Alt+Shift+Meta+Key` 规范化别名和输入顺序，并拒绝重复修饰键、多 key、空
片段及非法 key。

规范化后的 chord 在 `shortcuts.commands` 与 `shortcuts.model_behaviors` 之间共享唯一
命名空间；同一 chord 不能绑定多个 command 或模型行为。该规则只冻结持久化和冲突
检测语义，物理 key 映射、系统注册和捕获仍由后续平台 adapter 负责。

应用 command 使用闭合集合：`toggle_overlay`、`open_settings`、`toggle_mirror`、
`toggle_click_through` 和 `toggle_always_on_top`。未知 command 在配置提交前拒绝。
模型行为使用 `motion:<group>:<index>` 或 `expression:<name>` 形式；`model_id` 仍由
绑定单独提供，runtime 在动作进入队列前接收解析后的强类型 motion/expression identity。
行为 ID 不接受旧版的复合模型路径或任意未定义 kind。

Settings service 提供 `SetShortcuts` 与 `RestoreDefaultShortcuts` 两个 typed command。
两者都携带 `expected_config_revision`，在当前环境 writer lock 内执行原子提交；revision
过期或绑定校验失败时保留当前 config、runtime 和 snapshot。恢复默认使用当前
`ShortcutConfig::default()`（目前为空绑定集合），不读取旧配置，也不触发平台注册、按键
捕获或 runtime 动作；清除绑定可通过提交空的 `commands`/`model_behaviors` 集合完成。
提交前会把已接受的 application command、model behavior 和 chord 写成 canonical spelling，
因此别名、输入顺序和多余空白不会在重启后改变 snapshot 表示。

平台层在启动或快捷键配置成功提交后，可调用 `ShortcutConfig::compile()` 将这些持久化绑定
编译为 `CompiledShortcuts`。编译结果只包含闭合的 application command 或带 model id 的
typed model action，并拒绝非法 action、非法 chord 和跨域重复 chord。平台 adapter 负责把
原始事件映射为 USB HID usage 与四位 modifier mask，再通过
`CompiledShortcuts::resolve_hid_usage()` 匹配；原始平台 keycode、窗口句柄和 callback 数据
不会进入配置或编译结果。正式平台 crate 的 `ShortcutMatcher` 复用该解析结果，聚合左右
modifier、抑制重复 key down；binding replace 保留 pressed set 以避免 held-key repeat 误触发，
明确 reset 清空、reconcile 以平台状态覆盖 transient pressed state。系统级注册、
事件捕获、清除/恢复默认 UI 和匹配后的 command 执行仍属于后续平台/UI/runtime 接线工作。

窗口坐标、pressed state、权限结果、模型解析缓存和 renderer 状态不属于 `config.json`。可恢复窗口布局写入 `state.json`；其余瞬时/派生状态不持久化。

## Application State v1

- `state.json` 使用独立的 `schema_version: 1`，只保存可恢复的 settings 与 overlay 窗口布局。
  它不是用户配置，不参与 `config.json` 的 revision、backup 或 recovery-only 状态机。
- `settings_window` 为空时，窗口在鼠标当前所在显示器居中以 `800x600` 打开。非空值保存 GPUI
  逻辑坐标 `x`/`y`、逻辑尺寸 `width`/`height` 和 `maximized`；坐标允许负值，限制在
  `-1000000..=1000000`，尺寸限制为宽 `640..=16384`、高 `480..=16384`。
- `overlay_window` 为空时，以 `350px` 作为 `100%` 的默认逻辑宽度，高度按当前模型
  Canvas 宽高比自适应，两者应用当前缩放后在鼠标所在显示器居中；非空值优先
  保存并恢复完整坐标和尺寸，不重新应用默认宽度，尺寸限制为
  `64x64..16384x16384`。模型切换和非尺寸配置更新沿用完整 bounds；显式修改缩放时按比例更新
  尺寸并保存新结果。
- 恢复前再次检查窗口与当前显示器是否相交；显示器移除或完全离屏时回到居中默认布局，
  并以实际可见 bounds 更新内存 state。fullscreen 不作为可恢复设置窗口状态。
- 缺失、损坏、未知字段、越界值或读取失败只忽略 state 并使用默认布局，不阻塞
  `config.json`、runtime 或 recovery-only 窗口。非 v1 state 同样回退默认布局，且当前版本
  拒绝覆盖该文件。
- state 使用环境内独立的 `locks/state.writer.lock` 和原子替换；提交后重新读取并比较 typed
  state，验证失败恢复替换前 bytes。它不创建 config backup/quarantine，也不读取另一环境。
- GPUI bounds observer 只更新共享 typed 内存 tracker，不执行文件 I/O，并在连续变化停止 150 ms
  后通知 settings service worker。overlay frame source 只在完整 bounds 变化时通知同一 worker；
  worker 及时原子提交，正常 shutdown 前仍强制 flush。macOS Entity 重建、Windows 隐藏/重显、
  配置更新和模型切换都读取最新状态；失败形成稳定匿名错误但仍继续停止 runtime/audio 等 owner。

## Initial Version Boundary

- `next` 是全新的初始版本。当前完整配置直接定义为 `schema_version: 1`，包括
  `selected_model_origin`、`input` 和本文件列出的全部现有字段。
- `next` 开发期间新增字段直接修改 v1 的 Rust 类型、JSON Schema、默认值和 fixture；不保留开发
  中间结构，不实现 migration、字段 alias、旧数据转换或历史版本判断。
- 解析入口只接受完整 v1，并明确拒绝其他版本且不改写原文件。该入口为首次正式发布后的迁移机制
  保留边界；发布前不包含任何迁移实现，发布后再以实际发布的 v1 为唯一迁移基线。
- `selected_model_id` 与 `selected_model_origin` 必须同时有值或同时为空。
- `model.installed_models` 只记录用户导入模型的元数据：`id` 是稳定唯一、跨平台可移植的
  存储 key（与安装目录名一致，标题修改不影响它），`title` 是可编辑的显示名称，首次导入
  默认取来源文件夹名。列表内 `id` 不得重复；`title` 去除首尾空白后不得为空且不超过 128
  个字符。预置模型是 product files，不出现在该列表中。

## Backup Retention

- 每份备份是内部 JSON envelope，包含 `backup_format_version`、`created_at_unix_ms`、
  `source_schema_version`、16 位小写十六进制 `source_revision` 和原始 `config` 值。
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

- 当前 `config.json` 无法 parse 或 validate 时，在当前环境 writer lock 内按文件名从新到旧
  检查自有备份；未知文件、目录和符号链接不作为候选，也不得读取另一环境。非 v1 schema
  直接报告不支持并逐字节保留，不尝试转换或恢复为其他结构。
- 候选必须同时通过 envelope 格式版本、非零创建时间、源 schema 与内嵌 config 一致性、
  16 位规范化 source revision 以及完整 Native typed validation。未知格式、非 v1 schema、revision
  不匹配和损坏候选会被跳过；只恢复第一个完全有效的 v1 候选，并原子写回规范化 JSON。
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
  current，拒绝非 v1 schema、有效 current 或已恢复状态；确认仍损坏后把原字节写入现有有界
  `config-corrupt-*` quarantine，再原子写入、flush、读取并验证 v1 默认配置。成功后返回
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
- temp 有效且 current 有效或使用非 v1 schema 时，current 优先，temp 归档为 stale；current 缺失时
  提升 temp；current 损坏时先逐字节 quarantine current，再提升 temp。temp 无法 parse 或 validate
  时归档为 invalid，不覆盖 current；若 current 也缺失，归档后按正常首次启动创建默认值。
  temp 使用非 v1 schema 时逐字节原样保留并返回 `UnsupportedSchema`，不得归档或转换。
- interrupted archive 使用
  `config-interrupted-{stale|invalid}-<20-digit-order>-<5-digit-sequence>.bin` 自有命名空间；两类
  合计每环境最多保留最新 4 份、总计最多 8 MiB，未知文件不计数、不读取且不删除。恢复重启必须
  幂等，Development/Production 不得共享 candidate、archive 或 lock。
- app 只保留 `ArchivedStaleTemp`、`ArchivedInvalidTemp` 或是否替换损坏 current 的
  `PromotedTemp` 匿名动作；不得向 runtime/settings 暴露路径、配置内容、时间戳或底层 I/O 文本。

## Environment Isolation

`Development` 与 `Production` 使用同一 schema、默认值和相对目录结构，只由数据根目录区分。配置内容不保存环境字段，也不能引用另一环境的绝对路径。

路径与构建约束见 ADR-0008。
