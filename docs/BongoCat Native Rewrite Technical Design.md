# BongoCat Native Rewrite Technical Design

状态：架构决策稿，Phase 0 证据补齐与 Phase 1 渐进实现并行
最后更新：2026-09-23
首发平台：Windows 10 1903+、macOS 12+
后续平台：Linux（首发后评估）

> BongoCat Native Rewrite 采用单一 Rust 应用。GPUI 负责设置界面，Rust 平台模块直接负责输入、窗口、系统集成和 GPU 渲染。

## 1. 设计结论

```text
Rust 2024 edition application
├── GPUI                         设置、模型管理、快捷键和诊断 UI
├── Product Runtime              状态、输入语义、动画和配置协调
├── Live2D Runtime               模型、动作、表情、物理和音效
├── Windows Backend              windows-rs、Raw Input、Win32、D3D11
├── macOS Backend                objc2、CGEventTap、AppKit、Metal
└── Shared Assets / Fixtures     模型、schema、本地化和测试数据
```

主要决策：

- BongoCat 自有应用代码统一使用 Rust。
- Windows 只发布 `x86_64-pc-windows-msvc`，不构建或发布 x86 与原生 ARM64；Windows on ARM 通过系统 x64 仿真运行该构建。
- GPUI 只负责常规设置 UI，不承担模型渲染。
- 模型窗口由 Rust 平台模块直接创建和管理，与 GPUI 设置窗口共享同一应用生命周期。
- Windows 使用 D3D11，macOS 使用 Metal；首发不为了未来 Linux 强行统一 GPU backend。
- Windows 键鼠输入优先评估成熟输入库；若没有方案能同时满足事件边缘完整性、系统状态校正、
  Raw Input 设备语义和已确认的生命周期复位不变量，则直接使用 `windows-rs` 实现最小适配层，
  从架构上避免 issue #47 的永久卡键。
- macOS 输入优先评估成熟输入库；若没有方案能同时满足 listen-only CGEventTap、TCC 权限状态、
  tap 恢复和左右修饰键状态校正，则直接使用 `objc2` 封装最小适配层。
- 官方 Cubism Core 是预编译厂商二进制，是“应用代码纯 Rust”的唯一 FFI 例外；BongoCat 业务逻辑不得进入 SDK bridge。
- Linux 不进入首发范围，但共享业务 crate 不得依赖 Win32/AppKit 类型，不得故意封死后续 backend。

## 2. “纯 Rust”的定义

本项目中的“纯 Rust”定义为：

- 应用入口、UI、状态、动画、输入、配置、模型管理、窗口、渲染和系统服务的产品边界均由本
  Rust workspace 组装和拥有；实现可以使用经过评审的第三方 Rust crate。
- UI、运行时和平台服务均由同一个 Rust workspace 构建和管理，第三方依赖不得形成第二套业务
  runtime 或绕过既定 command/event/snapshot 边界。
- 允许通过窄 FFI 调用官方 Cubism Core 平台二进制，因为 `.moc3` 运行依赖厂商 SDK。
- 操作系统 API、GPU driver 和系统 framework 不属于应用语言范围。

因此对外准确表述应为“BongoCat 应用代码使用 Rust 重写”，而不是“最终二进制完全不含任何非 Rust 代码”。

## 3. 目标与非目标

### 3.1 目标

- 使用 Rust 实现完整的桌面应用、设置 UI 和实时运行链路。
- 使用一套 Rust 业务实现覆盖 Windows 和 macOS。
- 保持现有模型、动作、表情和用户资源格式尽可能兼容；配置使用全新的 Native Rewrite schema 和命名。
- 修复输入事件丢失导致的永久卡键，包括 issue #47 的截图快捷键场景。
- 设置界面具备一致、清晰、可主题化的桌面体验。
- 模型窗口具备低延迟、透明、置顶、穿透、多显示器和高 DPI/Retina 支持。
- 所有后台服务具有明确的 start、stop、restart 和 shutdown 生命周期。
- 使用 fixture 验证跨平台行为，而不是只依靠人工观察。

### 3.2 非目标

- 首发不支持 Linux，不承诺 Wayland 全局输入能力。
- 不提供进程内插件 ABI。
- 不要求 Windows/macOS 像素完全一致。
- 不把 GPUI fork、Zed 私有 UI crate 或第三方输入库变成业务 API。
- 历史版本只作为行为与模型资源参考；Native Rewrite 不读取或导入旧 Tauri/Pinia 配置。
- 不在技术 spike 通过前完整迁移所有产品功能。

## 4. 设计原则

| 原则           | 设计约束                                                   |
| -------------- | ---------------------------------------------------------- |
| 单一状态所有者 | runtime 独占输入、动画、配置和模型可变状态                 |
| 实时链路内聚   | 输入直接进入 runtime，模型求值后生成不可变渲染快照         |
| 输入最终一致   | 按键边沿、系统状态校正和生命周期复位共同维护 pressed state |
| UI 与渲染分离  | GPUI 负责设置，独立 overlay renderer 负责 Live2D           |
| 平台能力显式   | 系统 API 封装在平台模块，业务 crate 不接触平台 handle      |
| 先复用后自研   | 依次评估现有代码、标准库、平台能力、已安装及成熟第三方方案，再写最小自有实现 |
| 配置可恢复     | v1 schema、环境隔离、验证、备份和原子提交均可测试          |
| 行为可重复     | fixture、规范化状态快照和平台 smoke test 共同验收          |

## 5. GPUI 决策

### 5.1 为什么选择 GPUI

GPUI 用于设置窗口、模型管理、快捷键编辑、权限状态、更新和诊断：

- Rust 原生、GPU 加速，适合视觉统一的桌面工具界面。
- Windows 后端使用 Win32、DirectWrite、D3D11/DirectComposition，macOS 使用 AppKit/Metal。
- retained/immediate 混合模型适合设置表单、列表、实时预览和自定义控件。
- 已有目标桌面平台 backend，并为后续 Linux 留出路径。
- Zed 的实际产品规模证明其能够支撑复杂桌面 UI。

### 5.2 GPUI 边界

GPUI 仍是 pre-1.0，公共渲染 API 也没有稳定的 Windows/macOS 外部 Live2D 纹理合成路径。因此：

- 正式 Native workspace 精确固定 crates.io `gpui-kit = "=0.6.6"` 并提交 `Cargo.lock`；
  GPUI Kit 是唯一直接 GPUI 依赖，通过其 crates.io 同步包提供元数据对应 Zed `gpui 0.2.2`
  的整套 GPUI crate，禁止另行声明或替换 `gpui`、platform、component 和 assets。
- ADR-0056 的过渡期在根 `Cargo.toml` 使用临时 `[patch.crates-io]`，把 `gpui-kit` 固定到
  上游修复 rev `720aeef7b33f95fdefcf5e7dc6257308bc51aa22`；lockfile 中 GPUI Kit suite
  因此解析到同一 git commit。该 patch 只用于 `SettingGroup::variant()`，待 crates.io release
  包含等价能力后删除并恢复纯 registry 来源。
- 不自动跟随 Zed main，不直接依赖 Zed 应用内部 UI crate。
- 以 `gpui_kit::component` 提供的主题和基础组件为 design system 基础；窗口使用其官方
  `Root`，图标使用 `gpui_kit::assets`，项目只保留领域适配、产品 token 覆盖和组件库未覆盖
  的薄业务控件。应用统一调用 `gpui_kit::init`，平台入口只从 `gpui_kit::platform` 获取。
- GPUI `Entity` 只保存视图状态；真实输入、动画、配置和模型状态由 runtime 管理。
- 正式 `bongocat-ui-protocol` 定义设置 command/snapshot 协议，`bongocat-ui` 只负责 GPUI 视图与
  presentation policy；`bongocat-app` 的有界 service worker 独占配置写入和 runtime command，
  UI executor 不执行阻塞 I/O。
- `SettingsSnapshot.revision` 是 UI 快照排序用的单调版本，能够因 runtime、平台状态、诊断或
  catalog 变化推进；可编辑配置另携带可选 `config_revision`，仅由当前环境的持久化配置版本
  驱动。修改配置的 command 必须携带 `expected_config_revision`，在任何 config/runtime 写入
  前执行 compare-and-swap，避免输入诊断等无关变化制造假冲突。
- `appearance.theme` 通过独立 `SettingsTheme` 投影和 revision-checked typed command 修改，由
  Application owner 原子持久化，不进入 runtime。GPUI 收到新 snapshot 后调用组件库公开的
  `Theme::change` 即时更新内容；选择在快照往返前先乐观应用（`system` 在 macOS 上先清应用级
  覆盖再解析，见 ADR-0048 修正）；`light`/`dark` 同时请求匹配的原生窗口外观，`system` 清除
  覆盖并仅在该模式下响应系统外观通知。Entity 只缓存已经应用的显示模式以避免重复刷新，不成为
  配置事实来源。
- **原生表面（窗口框、系统弹框、右键菜单、托盘菜单、文件选择框）的主题由 `bongocat-platform`
  的 theme 模块独占（ADR-0048）**，UI 层不再直接调用平台外观 API。该模块只接受**已解析**的
  `AppTheme`（`System` 不往下传），并按平台能力矩阵落地：macOS 用进程级
  `NSApplication.appearance`，一处覆盖窗口框、弹框、菜单和面板；Windows 用
  `DWMWA_USE_IMMERSIVE_DARK_MODE` 覆盖窗口框——该属性一旦显式写入便不再自动跟随系统，因此
  偏好为 `system` 时（平台层收到的是 `None`，`System` 不往下传）以 gpui 同源的
  `UISettings` 查询实时重推导并写入该属性；其余表面接受**跟随系统主题**，由进程在创建任何
  窗口前调用一次未文档化的 `SetPreferredAppMode(AllowDark)` 使其生效。`system` 的解析口径统一
  在平台层（macOS 查 `NSApplication.effectiveAppearance`）且全 UI crate 只有一处解析入口，
  render 路径与 smoke 断言共用它——不读 gpui 的窗口外观缓存，因为该缓存只在延迟触发的
  `appearance_changed` 中更新；也不复用 gpui 的外观名映射，因为它不识别
  `AccessibilityHighContrastDarkAqua`，会把开启「提高对比度」的暗色系统判成浅色。主题失败一律
  降级为该表面保持系统外观，不报错、不改配置、不阻止启动。
- `appearance.language` 只接受 `system`、`zh-CN` 和 `en-US` 三个当前 v1 值，默认 `system`。
  平台 adapter 在启动时读取系统首选 locale；仅简体中文解析为 `zh-CN`，英语及其它 locale 都
  回退 `en-US`，不把解析结果写回配置。UI 通过独立 `SettingsLanguage`、revision-checked typed
  command 和 GPUI Kit `Select` 修改并立即刷新窗口标题、导航和当前可见文案。
  未知持久化值直接拒绝，不增加 alias、开发中间版本兼容或旧配置导入。
- 启动项等系统能力通过 UI 自有的 typed platform snapshot 显示，由 settings service worker
  读取和显式变更；外部状态变化递增 settings revision。读取失败只形成可重试状态，写入失败
  不改变 config/runtime，Development macOS 的 unsupported 状态不允许发出变更 command。
- 输入诊断通过 UI 自有的计数型 snapshot 投影 captured/reconciled/fallback/reset、sequence 和可靠队列
  transport 指标；不得包含具体按键、原始事件或时间戳。即使 queue full 等 transport-only 变化
  未推进 runtime revision，settings service 也必须观察投影变化并推进 settings revision。
- 设置 UI 采用 visual-first 契约（ADR-0054）：项目只定义并绘制用户可见、可操作的内容。
  BongoCat 不维护项目自有 AccessKit tree、native bridge、辅助技术 action、隐藏 label/action
  或只为辅助技术存在的文案；`gpui-kit` 传递提供的默认语义不作为项目 UI contract。
- 可见控件、键盘导航、focus、loading/error 和 value 共用同一份 UI snapshot；实现和 smoke
  只为可见行为建立断言，不复制辅助语义状态。
- GPUI 不加载 Cubism、不持有模型 GPU 资源、不驱动模型帧循环。
- 全局快捷键使用配置边界编译出的 `CompiledShortcuts`，由操作系统注册（Windows
  `RegisterHotKey`、macOS `RegisterEventHotKey`，见 ADR-0044），单一 owner 线程持有平台
  manager 并把共享 `ShortcutTable` 增量映射为真实注册；只有匹配当前 active model 的
  motion/expression target 才能转成 typed runtime command。只有物理按下边沿才分发：
  两个平台都会在组合键被按住期间按系统重复节奏继续投递 pressed 事件，重复事件必须被丢弃
  而不是再次触发目标，因此一次按住永远只产生一次动作。应用级 target 必须经 coordinator 的
  配置 revision-aware
  command 执行，平台层不得直接改 overlay 或设置状态。配置提交后通过共享 `ShortcutTable`
  原子替换 compiled bindings，owner 线程在下一轮轮询读取新表；快捷键注册与输入服务生命周期
  解耦，替换不涉及输入管线的 pressed set。增量映射以 chord 为身份：仍留在表内的 chord 复用既有
  注册（平台会拒绝重复注册，或产生重复的注册），只把其 target 更新为新表的目标；离开表的 chord
  必须注销并从 owner 的记录中移除，否则同一组合键再次进入表时不会被重新注册。跨模型复用同一
  组合键是合法配置，模型切换正是靠这一步把共享 chord 从旧模型改指新模型，否则它们会被下游的
  active-model 判定当作非活动模型丢弃。应用级 target 通过有界
  typed handoff 进入 settings service；显隐、镜像、穿透和置顶由唯一 Application owner
  按当前配置 revision 持久化，`open_settings` 交给 GPUI coordinator，避免平台线程直接触碰
  UI 生命周期。`open_settings` 通过线程安全的一次性请求位交给 GPUI frame source，后者在
  owner 线程切换设置窗口可见性：窗口已显示时隐藏，两个平台都复用同一个预渲染窗口，窗口不存在
  时才创建并显示。forwarder 必须支持有界停止与 join。
- 快捷键页面由两个带标题的 group 组成，每个 group 的第一行是它自己的门禁开关：`启用窗口快捷键`
  （`shortcuts.commands_enabled`，默认 `true`）与 `启用模型快捷键`
  （`model.enable_behavior_shortcuts`，默认 `false`）。两个门禁彼此独立，各自只决定对应的一半是否
  进入活动的 `CompiledShortcuts`：都不清空、不改写配置中的绑定，因此重新打开时无需重录即可恢复
  全部已校验绑定。门禁的唯一实现点是 `ShortcutConfig::active_bindings`——"此刻生效的绑定"的唯一
  投影，模型侧的活动模型过滤也在同一处，应用层与平台层不得再判一次。门禁变更必须经
  revision-checked settings command 原子持久化并替换共享 shortcut table。
  两个开关都是正向字段的直出：可见 UI 和 settings command 读同一个布尔值，全链路不存在取反；
  开关只带标题、不带描述（标题已经表达了它的作用），因此两个分组第一行都是纯标题行。
  门禁关闭时该作用域的行在 UI 上置灰且不可交互（捕获与清除都不再可用，tab 跳过），
  mutator 用同一谓词守卫；这是「开关门禁」统一禁用绑定规则（ADR-0053，并经 ADR-0054
  收窄为“可见行与 mutator 同源”）的落地。
- `model.enable_behavior_shortcuts` 默认关闭是对旧版行为的刻意收窄：旧版在模型加载完成后无条件为
  每个 motion 和 expression 自动分配一整层 `primary + [Shift/Alt] + 数字/字母` 组合键，用户没有
  拒绝的机会。Native 按维护者决定移植这套自动分配，但把"是否让这些组合键真正生效"交给用户。
  该开关只作用于 motion/expression 绑定，不得清空或改写配置中的绑定，也不得连带禁用
  `open_settings`、overlay 显隐、镜像、穿透或置顶等应用级快捷键——应用级快捷键有它自己的开关。
- 模型行为快捷键的自动分配与旧版同构：模型激活时（`prepare_model` / `select_model`）按声明顺序
  遍历该模型的 motion 与 expression，依次填入 `[primary]`、`[primary, Shift]`、`[primary, Alt]`、
  `[primary, Shift, Alt]` 四层、每层先数字后字母的组合键，共 144 个名额；`primary` 在 macOS 是
  Command、其它平台是 Control。编号对每个模型独立从第一个名额开始——占用范围是"应用级 command 绑定
  + 该模型自身的绑定"，其它模型已占用的组合键不计入，因为同一时刻只有当前模型的绑定生效。已有绑定
  的行为永不重写，因此重复激活是幂等的，只有用户尚未录制的行为会被补上；同一作用域内已被占用的组合键
  跳过而非重用，否则 `shortcuts.conflict` 会让整份配置失效。分配结果随选中模型同一次 commit 落盘，
  且不依赖 `model.enable_behavior_shortcuts`——绑定在快捷键页面始终可见，门禁关闭时该分组行置灰
  不可改（ADR-0053），是否进入平台匹配表则由同一分组第一行的 `启用模型快捷键` 开关决定。
- 只有当前模型的绑定进入平台编译表。配置按模型保存绑定且跨模型允许复用同一组合键，所以
  `shortcuts.conflict` 是作用域内的判定（命令内唯一、同一模型内唯一、模型绑定不得与命令冲突），
  整份配置本身不是一张无歧义的表：`active_shortcuts` 先按当前模型投影再编译。`prepare_model` 与
  `select_model` 都在激活成功后重建该表，因此被离开的模型立即停止触发它独占的组合键，被切到的模型
  无需重启即可用自己的组合键，非活动模型的绑定也不再占用全局热键。
- `application.show_status_icon` 通过独立的 revision-checked settings command 修改。settings worker
  以有界 request/reply bridge 请求平台主线程隐藏或显示状态图标，平台成功后才由 Application owner
  原子提交配置；配置提交失败时必须把图标恢复为旧状态。macOS/Windows 共用 `tray-icon 0.25.0` 托盘
  owner 与直接依赖的 `muda 0.20.0` 菜单 owner：`TrayIcon`、菜单、菜单项 receiver 和强类型事件队列
  在整个运行期保持存活，`set_visible` 只改变平台表示（macOS 移除 `NSStatusItem`，Windows 保留注册并设置隐藏），
  重新显示不创建第二套业务状态或菜单 owner。overlay 右键由 `muda` 从 overlay 的真实 HWND/`NSView`
  弹出同一菜单，不借用托盘隐藏窗口。
  正式启动不创建或显示设置窗口，设置窗口、单实例唤醒和 application reopen 仍提供恢复入口；
  平台失败只返回稳定匿名 settings error。
- `application.show_taskbar_icon` 只控制 Windows GPUI 设置窗口的任务栏按钮，不改变窗口可见性，
  也不映射为 macOS Dock 图标。settings worker 通过独立的有界 request/reply bridge 请求 GPUI 主线程
  切换 HWND 的 `WS_EX_APPWINDOW`/`WS_EX_TOOLWINDOW` 并回读结果，平台成功后才由 Application
  owner 按 expected revision 原子提交；配置提交失败时恢复旧样式。启动和窗口创建必须先应用当前
  v1 值再显示窗口，平台失败只返回稳定匿名 settings error。
- 关闭设置窗口不影响 runtime、输入、音频、frame source 和 overlay，它们继续由 app
  coordinator 持有。两个平台共用一条生命周期：窗口在第一次打开时创建，此后由 coordinator 持有，
  close 一律被 platform adapter 拦截并只隐藏原生窗口（Windows `ShowWindow(SW_HIDE)`、macOS
  `NSWindow.orderOut:`），reopen 重显同一 `Entity` 并主动读取最新 revisioned snapshot；窗口从不
  销毁或重建，因此平台之间没有第二套路径。隐藏期间的周期性快照刷新必须停止：预渲染窗口在屏幕外
  渲染的内容无人可见，reopen 本身已经先读一次快照，因此只有可见窗口按 `1 s` 重读，隐藏窗口不再
  长期空转。系统菜单的 presentation 轮询同样不得每秒重建完整快照：它先读取只返回 revision 的命令
  （`ReadSnapshotRevision`，只比较 service 已持有的状态），仅在 revision 变化时才读取完整快照，
  完整快照中的模型目录扫描因此不再随 `20 Hz` 轮询反复执行；`CGPreflightListenEventAccess` 这类
  TCC 查询由 snapshot clock 按上限 `1 s` 缓存，只有显示用途的值不再要求每次快照都访问系统。Windows 侧拦截 close 同时避开 GPUI 0.2.2 的 Windows
  `WM_CLOSE` 销毁回调同步重入缺陷。Windows 显式退出先请求 frame source 停止并停止输入生产者，确认 frame
  source 已退出后再 shutdown/join
  runtime、配置和音频 owner，最后释放 renderer/GPU 与 overlay；由于 GPUI 0.2.2 及当前上游
  `main` 都会在最终 `WM_DESTROY` 同步重入 `AsyncApp` 并触发进程 fast-fail，平台 adapter 只在
  这些产品 owner 全部有序关闭后使用进程退出跳过有缺陷的 GPUI 窗口析构。该兼容措施不得提前
  终止业务 shutdown；升级到修复此回调的固定 GPUI 版本后必须移除，并恢复正常 GPUI 析构门禁。
- 正式 executable 无参数启动时持续运行到显式 Quit 或系统终止；正数 `--run-seconds` 只用于
  有界 smoke/诊断，不能成为安装包或 Finder/Explorer 启动的隐式退出条件。所有正常退出仍须
  进入同一 shutdown coordinator。
- Windows 的 GPUI 平台循环独占线程消息派发；宿主产品调用的 overlay `tick` 不得再次 pump
  Win32 队列，并且必须在没有持有 GPUI `App`/`Window` borrow 时执行。独立 preview 的
  `run_for` 循环才负责主动 pump。这一约束防止 D3D11 tick 同步派发设置窗口消息后重入
  `AsyncApp`。
- 平台服务不返回 GPUI 类型，避免框架扩散到业务模块。
- GPUI 升级必须单独提交，附变更说明、双平台构建和 UI smoke test。

所有新的 crates.io 直接依赖同样先选择引入时最新的非 yanked 稳定版，再精确 pin 并提交 lockfile。只有 Rust toolchain、目标平台、许可证或已验证的安全边界不兼容时才允许暂缓，且必须留下可复核的版本差异和解除条件；不能用旧版本回避正常的 API 迁移。传递依赖在上游约束允许的范围内保持最新，不 fork 上游只为修改版本号。

Phase 0 必须验证输入法、文本编辑、缩放、UI/主题、窗口重开、托盘应用生命周期，以及 GPUI 设置窗口与独立 overlay 共存。ADR-0011 允许已通过自动化契约的模块进入正式 workspace；未解决的问题继续阻塞对应完整功能或 stable 发布，并必须在进入完整 UI 实现前解决并记录。

## 6. 总体架构

```text
                       GPUI settings window
                              |
                       Commands / Snapshots
                              |
                     App coordinator/service
                              |
Platform input ---> Runtime thread ---> Model/Animation state
     |                    |                    |
     |                    +--> Config service |
     |                    +--> Audio service  |
     |                                         v
     +--> reconciliation                 Render snapshot
                                                   |
                                      Native overlay window
                                      D3D11 / Metal renderer
```

### 6.1 模块责任

- `app`：应用入口、服务装配、生命周期、单实例和 shutdown coordinator。
- `runtime`：唯一业务状态所有者，处理输入、快捷键、动画选择和模型命令。
- `ui`：显示 runtime snapshot，发送显式 command，不直接修改业务字段。
- `platform`：窗口、输入、托盘、权限、显示器、启动项、文件和更新。
- `model`：模型包解析、路径安全、资源索引和显式导入（目录与 BongoCatMver 源两种来源），并在导入时把包内的旧键位名归一化到产品词汇表。
- `live2d`：Cubism Core 生命周期、motion/expression/physics/pose 求值。
- `audio`：motion 音效的有序 command、FLAC 解码、唯一 voice、输出设备和 shutdown。
- `render`：不可变 render snapshot 和 renderer contract。
- `config`：环境隔离、当前 v1 schema、验证、备份和原子提交。

### 6.2 依赖方向

```text
ui protocol <------- app -------> runtime <------- platform adapters
                                  |
                                  v
                             model / live2d
                                  |
                                  v
                            render contract
                                  ^
                                  |
                     D3D11 renderer / Metal renderer
```

业务 crate 不得导入 Win32、Objective-C、GPUI 或 GPU handle。平台实现可以依赖业务定义的 command/event 类型。

## 7. 仓库布局

```text
BongoCat/
  Cargo.toml                  正式 Native Rewrite workspace 根
  Cargo.lock
  rust-toolchain.toml
  resources/                  随产品打包的模型与 Native 产品图标
  crates/
    bongocat-app/             入口、装配和 shutdown
    bongocat-input/           平台无关输入协议、producer 和 latest-value transport
    bongocat-ui-protocol/     设置/更新 DTO、typed command、reply 和 bounded client
    bongocat-runtime/         状态、输入 reducer、模型投影、动画和命令
    bongocat-config/          schema、环境隔离和原子存储
    bongocat-storage/         用户私有存储原语：权限、私有目录、原子替换
    bongocat-model/           模型包、目录导入和资源索引
    bongocat-live2d/          Cubism Core 边界与模型求值
    bongocat-audio/           motion 音效队列、解码与设备 owner
    bongocat-render/          render snapshot/contract
    bongocat-ui/              GPUI 设置界面和 design system
    bongocat-update/          发布清单读取、版本/target 判定、载荷验签与安装策略
    bongocat-platform/        Windows/macOS 平台服务
  shared/
    config/                   Native JSON schema、命名与存储契约
    behavior/                 输入、动画和快捷键规范
    fixtures/                 输入序列、预期状态和模型样本
    resources/                模型、图标和本地化
  tools/                      不随应用发布的验证与考古工具
  docs/
    adr/
    benchmark/
    migration/                历史参考，不进入生产配置路径
```

crate 是编译和责任边界，不是动态库。首期不为目录美观建立空 crate；只有依赖方向或测试隔离确实需要时才拆分。

`bongocat-ui-protocol` 只承载设置/更新服务的进程内强类型 contract：snapshot、command、reply、
operation control、state handle 和 bounded client/endpoint。它不依赖 GPUI、OS API、配置、平台、
本地化或 update 库；`bongocat-ui` 负责把这些 contract 映射为 GPUI view，app service 负责把
config/platform/update producer 映射为 protocol 类型。debounce、语言显示文案、更新窗口轮询节奏和
文件选择后的展示判断留在 UI/适配器，不进入跨层 contract。

正式 workspace 位于仓库根目录，是仓库中唯一的产品构建入口。历史 Vue/Tauri 实现仅在
远端 `master` 和 `pre-refactor-tauri` 分支中保留，当前工作树不包含其源码、资源或构建入口；
该路径安排不改变 crate 边界或产品架构。

## 8. Runtime 与并发

### 8.1 状态所有权

单一 runtime 线程拥有 `AppState`、`InputState`、`AnimationState` 和当前模型控制状态。其他线程不能通过共享可变引用修改它们。`bongocat-input` 只拥有平台无关的事件协议、可靠输入 hand-off、latest-value transport 和匿名诊断；`InputState`、输入到模型的投影以及 worker/shutdown 生命周期仍由 `bongocat-runtime` 独占。

```text
Input producers ---- reliable edge queue -----+
UI commands -------- reliable command queue --+--> runtime tick
Cursor motion ------- latest-value slot -------+        |
Gamepad axes -------- latest-value slot -------+        +--> UI snapshot
                                                        +--> Render snapshot
```

- Key/button down/up、设备连接和 command 必须可靠、有序；溢出是可观测错误，不能静默丢弃。
- 可靠队列溢出时必须清空无法证明顺序的缓存，并在队首注入 `Reset`；原始失败 item 返回 producer，溢出、恢复和被清理 item 数量进入诊断 snapshot。
- 鼠标移动和摇杆轴可以合并为最新值，不能阻塞边沿事件。
- runtime 在独立 latest-value 通道之后按可注入单调时钟平滑光标位置；以 60 FPS 为基准每帧
  保留 `0.75` 的剩余距离，并在逻辑坐标距离小于 `0.5` 时收敛到目标。首个样本和显示器
  viewport 变化直接对齐目标，避免跨显示器插值使用错误坐标系；renderer 只消费平滑后的参数。
- 手柄 axis latest-value 以 `{device_id, connection_generation, axis}` 为 key 并限制总 key 数；共享 runtime transport 为每个 device id 的每次连接分配单调 generation，平台将同一 connection 通过可靠 `GamepadConnected`/`GamepadDisconnected` 事件和 axis key 传递。断开只清理该 connection 的 pressed/axis，不重置其他输入；键鼠状态校正不扫描 gamepad pressed state。旧 generation 的迟到边沿和 axis 样本必须计数并拒绝。
- Runtime 在 latest-value 消费后统一应用 `GamepadAxisSettings` 的 stick/trigger dead-zone；平台
  adapter 只负责原始范围归一化和无效值诊断，不把设备默认 dead-zone 写入共享协议。轴值随后以
  `ModelInputSnapshot` 的不可变字段投影给 renderer。
- 平台 input worker 通过独立 latest diagnostics producer 发布项目稳定计数；该通道不占用可靠
  command/input edge 队列，Windows service tick 与 macOS run-loop slice 都刷新 live snapshot。
  callback 原子计数、worker 恢复计数和 cursor latest 统计在平台 owner 内合并后进入
  `RuntimeSnapshot::platform_input`；停止输入时须先发布包含 `clean_shutdown` 的最终快照，再停止 runtime。
- 动画和延迟使用单调时钟 `Instant`，持久化时间才使用墙上时钟。
- Runtime 可接收 typed `Tick` command，在使用注入单调时钟的 contract/fixture 或受控
  coordinator 场景显式驱动一次评估；生产环境仍由 runtime worker 的定时等待负责周期 tick，
  UI 和平台 adapter 不直接调用 renderer。
- `maximum_fps` 是 `15..=240` 的 runtime-owned 强类型设置，通过 revision-checked settings
  command 持久化并进入 `RuntimeSnapshot`。它同时决定 runtime 周期评估、GPUI owner 调度的产品
  overlay frame source 和独立 overlay run loop 的下一帧间隔；变更无需重启。间隔是相对**帧截止
  时间**的等待，不是帧完成后的延时：一帧的开销由等待吸收，因此只要单帧工作在间隔内完成，实际
  节拍就等于设置值——按「完成后起算」会让实际节拍变成 `interval + work`，设置越高偏差越大。
  命令驱动的求值可以提前于截止时间出帧以压低输入延迟，这种帧不消耗周期槽位，因此周期节拍不变，
  被呈现的帧率仍由 frame source 自己的节拍决定。renderer 仍只消费
  不可变 `RenderSnapshot`，不读取 config 或 GPUI 状态。overlay 隐藏时，runtime 周期等待和产品
  frame source 统一降至 `100 ms`；可靠 command 仍会立即唤醒 runtime，产品重新显示的轮询延迟
  上限为 `100 ms`。
- `motion_start` 触发的动作只播放一个循环：到达 clip 声明时长后自然完成并清除 active motion，
  clip 的 `Meta.Loop` 不改变这一点（它描述资源如何制作，不描述产品如何触发），否则猫会永远
  停在动作里、回不到 idle 参数。同一动作在同一 priority 上仍在播放时，重复请求被忽略（R5
  motion queue 的 equal-priority 规则），因此按键重复和连按既不会重启 clip 也不会重放 motion
  音效；播放结束后的下一次触发重新播放。预览播放同样只播放一个循环，但每次请求都重新开始。
- `motion_stop` 只作用于匹配的当前动作。非零 `FadeOutTime` 在 runtime snapshot 中保留
  active identity 和首次 stop command sequence，renderer 以正弦权重淡出并在结束帧后
  清理；重复 stop 不重启计时，零时长立即清理，旧动作的 stop 不影响后启动动作。
- render snapshot 不含锁和平台对象，通过双缓冲或 latest-value channel 交给渲染线程。
- `ModelSettings` 是 runtime 的强类型模型交互设置：`mirror` 只影响不可变
  `RenderSnapshot::mirror_horizontal` 的水平变换，`mirror_pointer_tracking` 只反转
  指针的 X/Z 产品参数，`ignore_pointer` 跳过所有指针参数覆盖；三者通过
  `SetModelSettings` command 和 revisioned snapshot 传播，renderer 不读取配置。
- GPUI 通过 command/snapshot 边界交互，不直接持有 runtime mutex。
- GPUI 拥有平台主事件循环；应用 coordinator 在该主线程调度 overlay `tick`，GPUI
  `Entity` 不持有 renderer、render snapshot 或 frame-loop 状态。
- shutdown 顺序：阻止新的 frame tick -> 停止输入 -> 确认 frame source 已退出 -> runtime
  drain/停止 -> 保存配置 -> 停止音频并 join -> 释放 renderer/GPU -> 销毁 overlay -> 关闭 GPUI。

### 8.2 输入事件

```rust
enum InputEvent {
    KeyDown { key: PhysicalKey, at: Instant, repeat: bool },
    KeyUp { key: PhysicalKey, at: Instant },
    MouseDown { button: MouseButton, at: Instant },
    MouseUp { button: MouseButton, at: Instant },
    CursorMoved { position: PhysicalPoint, at: Instant },
    DeviceConnected { id: DeviceId, at: Instant },
    DeviceDisconnected { id: DeviceId, at: Instant },
    Reset { reason: InputResetReason, at: Instant },
}
```

应用优先保存物理键身份；字符、布局和显示名称属于映射层。左右 Ctrl/Alt/Shift/Meta 必须可区分。

## 9. 输入可靠性

### 9.1 issue #47

PixPin、Win+L 或其他系统级快捷键可能让应用收到按下边沿，却收不到对应的释放边沿。输入系统不能把事件流视为永远完整，必须通过系统状态查询和生命周期复位保证 pressed state 最终一致。系统查询集中在输入服务中，renderer 不直接读取键盘状态。

### 9.2 Windows

1. 使用 `windows-rs` 调用 `RegisterRawInputDevices`，后台窗口接收 `WM_INPUT`。
2. 从 scan code、extended flag 和 `RI_KEY_BREAK` 建立物理键边沿。
3. runtime 维护 pressed set，但事件流不是唯一事实来源。
4. 校正使用单调时钟周期调度；默认每 `250 ms` 查询一次，并要求同一个 key 连续 `2` 次快照缺失才确认释放，避免单次系统查询异常误清除。时钟回退不得推进调度游标。
5. 对 pressed set 中确认释放的键生成内部 `KeyUp`。
6. 会话锁定、桌面切换、睡眠、设备移除、服务重启和队列异常时发送 `Reset`；这些生命周期复位不等待确认阈值。
7. 必要时用 `WH_KEYBOARD_LL` 补充合成事件，但 hook 不得覆盖 Raw Input 物理状态。
8. `RegisterHotKey` 只处理应用快捷键；冲突必须反馈 UI 并保留旧绑定。
9. XInput 固定轮询 0–3 号 slot；连接/断开和按钮使用可靠序列，摇杆/trigger 使用带 connection generation 的 latest-values。平台 adapter 仅从 System32 动态解析系统 `xinput1_4.dll`，不要求构建或部署环境提供 SDK import library；backend/export 缺失必须进入诊断且不能影响键鼠服务。平台层只归一化原始范围，产品 dead-zone 由 runtime 配置统一决定。

Windows 的 `WM_QUERYENDSESSION` 与已确认的 `WM_ENDSESSION` 只记录系统终止请求并立即返回；
GPUI owner 在下一帧进入既有 shutdown coordinator，Win32 callback 不阻塞或析构 runtime/GPU 资源。

该方案不承诺安全桌面交付每个释放事件，而是保证丢事件不会产生永久卡键。
`model.release_fallback_timeout_ms` 只对 captured keyboard control 生效：runtime 以自身可注入的
单调时钟记录 down/repeat 的观察时刻，repeat 刷新期限，不比较平台 input service 的事件时间戳；
`0` 禁用，非零值到期时只释放键盘，不释放鼠标或手柄。该路径使用独立匿名计数，始终只是可靠
`KeyUp`、状态校正和生命周期 `Reset` 之后的最后保险，不是正常输入语义。

Windows 验收覆盖 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen、UAC、管理员/非管理员进程、睡眠唤醒、多键连按和队列压力。

### 9.3 macOS

- 使用 listen-only `CGEventTap`，不经过 GPUI 响应链。
- 区分 Input Monitoring 的 unknown、denied、granted、restart-required 状态。
- 全局输入监听只请求 Input Monitoring：`CGPreflightListenEventAccess` 只读查询，权限请求只由用户
  发起的明确设置操作触发。设置窗口不安装项目 AccessKit bridge，也不需要、不得请求 Accessibility
  trust；Input Monitoring 状态与输入服务运行状态分别投影，授权变化不等于 tap 已重启（ADR-0024）。
  可见 settings 窗口最多每秒一次读取只读 preflight 并通过 revisioned snapshot 刷新该状态；轮询不发起
  TCC request、不重试或重启 event tap，窗口释放后停止。
- `PlatformInputDiagnostics` 以稳定 `service_status` 和单调 `service_start_attempts` 公开输入服务
  的 not-started/running/permission-denied/backend-unavailable/failed/stopped 状态，不携带平台错误文本。
  平台 owner 每次产品启动只尝试一次；权限拒绝或 backend 启动失败不阻止 overlay/runtime，settings
  health 进入 degraded 并以匿名状态进入 revisioned snapshot，不在后台循环请求权限或重启服务。
- 设置窗口只展示用户可操作的设置，导航分两级。一级页面按领域划分，固定为 General、Models、
  Model window、Interaction、Input、Shortcuts、Application、About 八页，每页对应一个用户能直接说出的领域；一级页面内
  用带标题的 group 组织内容，同一页面下存在多于一个带标题的 group 时 sidebar 把它们渲染为二级菜单项，
  点击滚动到该分组。General 因此只保留外观（主题、语言），不再承载其它领域的分组。
  配置损坏在 settings window 出现前完成 fallback，不显示配置恢复横幅、按钮或重启提示。
  原 Diagnostics 页面已移除，输入可靠性计数、runtime/renderer 状态和 build 标识只留在
  app-owned 日志和匿名 diagnostics export 里：周期性刷新只服务于界面上仍在显示的数字。
  `OpenConfigBackupLocation` 与 `ExportDiagnostics` 仍是 settings service 的强类型排障
  command，不从常规 UI 触发。Development、Production 和 smoke 的设置窗口使用同一套可见
  页面、分组和按钮组成；smoke 只改变驱动方式，不额外显示“重置偏好设置”等产品窗口没有的控件。
- 产品启动时以只读 `CGPreflightListenEventAccess` 检查 Input Monitoring，缺失时用 `rfd` 的原生
  系统弹框引导用户前往「系统设置 → 隐私与安全性 → 输入监控」。检查非阻塞：主线程完成窗口、
  菜单栏等正常初始化后，由专用 worker 线程执行检查与提示，提示未应答或被关闭不影响任何产品
  窗口的显示与使用（ADR-0032「非阻塞执行修正」）。提示不写配置、不写 state、不缓存
  「稍后」，每次启动重新读取平台真实状态；提示本身不调用 TCC request，`CGRequestListenEventAccess`
  仍只在用户点击引导按钮后发生（ADR-0032）。实现必须使用 `rfd` 无父窗口的**异步**消息框：同步路径
  会在调用线程上构造 `PolicyManager`/`FocusManager` 并触碰共享 `NSApplication`，而 `gpui_macos` 的
  `MacPlatform::run` 需要该实例是自带 `platform` ivar 的 `GPUIApplication` 子类（ADR-0032「macOS 弹框实现修正」）。
- 监听 tap 被系统禁用、超时和 session 变化，并自动重建。
- listen-only tap 必须创建在 `kCGHIDEventTap` 的 `kCGHeadInsertEventTap` 位置（与 rdev 的 listen 一致），不得使用 session 层 tail append。实测 macOS 26.5.2：session tail 位置收不到右 Shift 的释放 `FlagsChanged`，且重复按下事件 flags 逐字节相同；同一台机器的 HID head 位置能收到全部修饰键的完整 press/release 对。tap 是 listen-only，只观察不修改、不吞事件；HID 层事件流跨用户会话可见，锁屏/快速用户切换仍依赖既有 session 生命周期 Reset 清空 pressed state。
- `FlagsChanged` 的 down/up 方向必须在 callback 中冻结，按优先级依次使用：事件 flags 相对上一个 `FlagsChanged` 事件的**设备位**跳变（flags 低 8 位中每个物理修饰键有独立 bit，左右天然区分，同侧兄弟键按住时家族 flag 不清零也不影响）、家族 flag 位跳变（rdev `LAST_FLAGS` 同思路）、按 callback 记录的前一边沿交替。HID head tap 下设备位跳变覆盖全部常规修饰键事件；CapsLock 的 `AlphaShift` 位反映锁存状态而非物理边沿，必须依赖交替回退；session tail 上观察到的右 Shift 事件缺失/不变序列也由交替回退兜底。decoder 状态不属于 runtime pressed state，并随任何 `Reset` 清空，周期校正强制释放候选时必须同步清除 decoder 记录。不得等到 consumer drain 时用较新的全局状态反推旧事件，无法识别的修饰键必须触发可观测 `Reset`。
- 对键盘和鼠标 pressed set 分别使用 `CGEventSourceKeyState`、`CGEventSourceButtonState` 校正；右侧修饰键键码（54/60/61/62）在按住时也返回 false，必须同时查询其家族主键码（55/56/58/59）作为状态来源；保留 0–31 号 mouse button 身份，按统一的 `250 ms`/连续 `2` 次缺失策略确认释放，睡眠、锁屏、权限变化和 tap 重启时直接复位。
- GameController owner 在服务期启用后台事件，连接分配新的 generation；按钮/连接边沿进入可靠队列，axis 进入固定容量 keyed latest-values，断开后旧 generation 的 callback 和待消费样本不得作用于重连设备。
- callback 只做映射和入队，不执行模型、文件或 UI 工作。

## 10. 平台实现

### 10.1 Windows

- 应用：GPUI/Win32 主事件循环，单实例使用 named mutex + 唤醒消息。
- Overlay：Win32 透明无边框 popup；无已保存 bounds 时以 `350px` 作为 `100%` 的默认逻辑
  宽度，高度按 Cubism Core 返回的当前模型 Canvas 宽高比自适应，两者再应用缩放
  设置；已保存 bounds 优先，设置/模型切换时重建窗口；非 click-through 模式的
  客户区支持拖动，click-through 仍返回 `HTTRANSPARENT`。右键拖动缩放窗口：位移越过
  `3px` 后按 `(dx + dy) * 0.5` 改 `overlay.scale_percent`（钳制在 `25–400`），窗口左上角
  不动，尺寸逐帧经 `IDXGISwapChain1::ResizeBuffers` 与就地重建的 render target、staging
  纹理、mask target 生效（不重建窗口、不重载模型纹理），松手后缩放写回配置（ADR-0057）。
  `keep_inside_screen` 开启时，窗口必须完整落在所有显示器矩形（`EnumDisplayMonitors` +
  `MONITORINFO.rcMonitor`）的并集内，因此允许覆盖任务栏，负坐标保持有效；跨显示器摆放只要不越过
  桌面边界就不纠正。创建、缩放/设置重建和模型重建立即收敛一个不可用的放置（窗口大于显示器时保留
  尺寸并把原点贴到显示器原点）；拖动后的收敛由 frame tick 驱动的延迟约束完成：窗口静止累计 1 秒后
  才移回显示器内，期间任何被观测到的位移都重新计时，因此跨显示器拖拽不会被打断。约束缓存最多
  `500ms` 复用一次放置检查，显示器变化（含拔掉外接屏）在静止窗口下也会被重新评估并纠正。关闭时
  不执行该收敛，但完全离开现存显示器的持久化 bounds 仍按 state 恢复规则回退。
- Renderer：D3D11 + DXGI + DirectComposition/DWM，预乘 alpha。
- DPI：Per-Monitor-V2，处理 `WM_DPICHANGED`、显示器热插拔和负坐标。
- 输入：Raw Input、状态校正、可选低级 hook、XInput 手柄。
- 产品图标：`bongocat-app` 在构建期把 Native 自有 `.ico` 编译进 Windows executable，用于窗口、
  任务栏和文件身份；它不再是托盘图标来源，托盘也不会回退到系统通用应用图标。
- 托盘：`tray-icon 0.25.0` 拥有 `TrayIcon` 和固定 GUID，直接依赖的 `muda 0.20.0` 拥有菜单与
  右键弹出。Windows 状态图标从 Native 自有 `resources/icons/tray-windows.png` 解码，菜单、托盘
  隐藏点击恢复和 overlay 右键入口均由该唯一菜单 owner 管理；overlay 弹出使用自身 HWND，不借用
  托盘隐藏窗口。
- 启动项：统一由 `auto-launch 0.6.0` 提供双平台后端。Windows 写当前用户 HKCU Run 并同步
  `StartupApproved\Run` 启用标记；macOS 写 `~/Library/LaunchAgents/{app_name}.plist`
  （`RunAtLoad`）。Production 使用该后端；命令固定为当前 executable 加 `--run-seconds 0`，
  默认不要求管理员权限或 TCC 授权（ADR-0043）。该能力只属于已发布的产品：开发构建的可执行文件是
  构建产物而非已安装应用，所以 `bongocat-app` 按构建环境判定，开发构建直接汇报
  `Unsupported(BuildEnvironment)`、不触碰平台，设置里的启动项整行按统一禁用样式置灰，并保留可见的 `unsupported_build` 文案（ADR-0051）。
- 启动权限：产品启动时以自身进程令牌的 `TokenElevation` 判断是否已提权，未提权时用 `rfd` 的
  原生系统弹框说明「属性 → 兼容性 → 勾选以管理员身份运行此程序」路径，并提供定位当前
  executable 的操作。检查非阻塞：主线程完成窗口、菜单栏等正常初始化后，由专用 worker 线程
  执行检查与提示，提示未应答或被关闭不影响任何产品窗口（ADR-0032「非阻塞执行修正」）。
  产品不原地提权、不写 HKCU/HKLM、不注册 service，提示也不持久化任何状态；
  每次启动重新读取真实令牌状态，已提权则完全不提示（ADR-0032）。

### 10.2 macOS

- 应用：GPUI/AppKit 主事件循环，平台 UI 操作固定在 main thread。
- 产品图标：`.app` 的 `Info.plist` 以 `CFBundleIconFile` 指向随 bundle 签名封装的 Native 自有
  `.icns`；该键与 bundle 的其余生成键由打包工具写入，仓库只保留 `macos/Info.plist` overlay
  （`LSMultipleInstancesProhibited`、`NSPrincipalClass`），打包入口在签名前验证元数据与资源存在。
- Overlay：通过 `objc2` 创建透明 nonactivating `NSPanel`；无已保存 bounds 时以
  `350px` 作为 `100%` 的默认逻辑宽度，高度按 Cubism Core 返回的当前模型
  Canvas 宽高比自适应，两者再应用缩放设置；已保存 bounds 优先，允许通过窗口背景拖动。
  右键拖动缩放窗口：位移越过 `3px` 后按 `(dx + dy) * 0.5` 改 `overlay.scale_percent`
  （钳制在 `25–400`），窗口顶边不动（AppKit frame 原点在左下角，因此修正 origin.y），尺寸
  逐帧经 `setFrame:display:` 与重设的 drawable size 生效，mask 纹理随 drawable 尺寸重建，
  松手后缩放写回配置（ADR-0057）。
  `always_on_top` 开启时使用高于程序坞的 AppKit main-menu window level，关闭时恢复 normal
  window level。配置的 runtime snapshot 变化和任何 overlay 重建都必须立即重放当前层级，
  不能由其他路径覆盖。
  `keep_inside_screen` 开启时，窗口必须完整落在所有 `NSScreen` 的 `frame`（含菜单栏与程序坞
  占用条）并集内，因此允许覆盖系统区域，负坐标保持有效；跨显示器摆放只要不越过桌面边界就不纠正。
  需要纠正时选择与窗口交叠面积最大的 screen，完全无交叠时选择中心距离最近的 screen，再收敛原点。
  创建、缩放/设置重建和模型重建立即收敛一个不可用的放置（窗口大于显示器时保留尺寸并把原点贴到
  显示器原点）；拖动后的收敛由 frame tick 驱动的延迟约束完成：窗口静止累计 1 秒后才移回显示器内，
  期间任何被观测到的位移都重新计时，因此跨显示器拖拽不会被打断。约束缓存最多 `500ms` 复用一次
  放置检查，显示器变化（含拔掉外接屏）在静止窗口下也会被重新评估并纠正。关闭时不执行该收敛，但
  完全离开现存显示器的持久化 bounds 仍按 state 恢复规则回退。
- Renderer：Metal + `CAMetalLayer`，drawable size 跟随 backing scale。
- Spaces：按配置设置 collection behavior 和 full-screen auxiliary。
- 输入：CGEventTap、状态校正、GameController，必要时 IOHIDManager。
- 菜单栏：`tray-icon 0.25.0` 在 macOS 主线程拥有 `NSStatusItem`，同一份直接依赖的
  `muda 0.20.0` 菜单拥有命令项；状态图标使用 Native 自有 `resources/icons/tray-macos.png` 并作为
  template image。overlay 右键通过 content `NSView` 在同一主线程调用 `muda` 弹出。登录启动在
  macOS 13+ Production `.app` 使用 `SMAppService.mainAppService`。macOS 12 和 Development 构建
  明确报告 capability unsupported，不回退到废弃 API 或自行写 LaunchAgent。
- 发布：Hardened Runtime、签名、notarization 和 TCC 权限说明。

平台 `unsafe` 必须集中在小型 wrapper，写明安全不变量并有 smoke test。业务和 UI crate 默认禁止 `unsafe_code`。

## 11. Live2D 与渲染

模型资源中的 `resources/background.png` 作为独立背景资产随渲染资源提交，在 drawable
之前绘制；背景缺失时保持透明 overlay，背景文件损坏则拒绝该模型提交。按键图片从
`resources/left-keys` 和 `resources/right-keys` 按目录绑定，当前按下键优先使用精确文件名；
HID 功能键 F1-F24 缺少专属 `F1.png`…`F24.png` 时回退到该模型共享的 `Fn.png`，左右修饰键同理回退到
`Control`/`Shift`/`Alt`/`Meta`；没有匹配资源时不绘制按键层，也不产生任何按键动作（见下）。**`Fn` 是这张
共享功能键图的名字，不是 Fn 键**：旧版输入层把"模型没逐键画图"的 `F<数字>` 重写成 `Fn`，两个预置键盘
模型出厂的共享图就在这个主干下，所以它既不改名也不迁移。左右修饰键的精确名是两侧各自的
canonical 名（`AltLeft`/`AltRight`、`ControlLeft`/`ControlRight`、`ShiftLeft`/`ShiftRight`、
`MetaLeft`/`MetaRight`），`Alt`/`Control`/`Shift`/`Meta` 只作为两侧共用的家族图；`AltGr` 是只对右 Alt
生效的旧名（见 ADR-0038），`Return` 是主 Enter 的旧拼写（见 ADR-0039），`Function` 是地球键的旧名
（见 ADR-0049）——三者都排在最具体名之后，用于兼容没有经过导入归一化的包。小键盘（HID `0x53`…`0x63`）的
精确名是 `NumLock` 与 Mver 词汇表的 `Kp*`，其中在主键盘上重复的七个键回退到主键盘的键位图
（`Kp1`…`Kp9`、`Kp0` → `Num1`…`Num9`、`Num0`，`KpEnter` → `Enter`，`KpDivide` → `Slash`）；
`NumLock`、`KpMultiply`、`KpMinus`、`KpPlus`、`KpDecimal` 在主键盘上没有对应键，缺图时不绘制。
小键盘整块归左手，因为可复用的数字、Enter 和 Slash 图只存在于 `left-keys`（见 ADR-0040）。键位词表
覆盖标准 104/105 布局、小键盘与 Apple 地球键的全部按键，范围是
`0x04..=0x65` ∪ `{0x67}` ∪ `0x68..=0x73` ∪ `0xe0..=0xe7` ∪ `{0xff03}`（八个修饰键 usage；`0x66`
`Power` 两边都不产出）。这是**词表覆盖的上界，不等于"两个 adapter 实际能产出的集合"**：`IntlHash`
`0x32` 与 F21–F24 `0x70..=0x73` 目前两个平台都产不出，仍然命名。命名**不以预置模型当前是否有图、
也不以今天有没有硬件能按**为前提：它是与模型作者的契约，模型提供 `Dot.png`、`Minus.png`、
`Delete.png` 等任何键位图都必须在不改产品代码的前提下生效。反过来，"某个 adapter 能产出某个 usage"
必须有该 adapter 自己的断言支撑，不得从这张并集推断——`Apps` `0x65` 就曾在这个并集里躺了很久，而
两个平台都没有映射，于是 `Apps.png` 永远画不出来（见 ADR-0041 修订与 TODO 第 90 项）；`hand` 归属
覆盖同一集合（含修饰键块，见 ADR-0041 事实 4 修订），因为 `InputState::model_snapshot` 会丢弃没有
hand 归属的按键，且该集合由契约测试遍历而不是由实现恰好用到的区间决定。

地球键是这套词表里唯一不在 HID Keyboard/Keypad 页的键：它的 usage 是 Apple 厂商页 `0xFF` 的 `0x03`
（`KeyboardFn`）折叠成 `0xff00 | usage`，即 `bongocat-render::GLOBE_KEY_USAGE` = `0xff03`。任何
`0x04..=0xe7` 式区间都覆盖不到它，所以命名、绑定和两处契约并集都必须显式写出。它的精确名是 `Globe`，
旧名 `Function` 作为末位别名；它与功能键的候选列表完全不相交（见 ADR-0049）。平台可达性：macOS 经
CGEvent keycode `63`（`kVK_Function`）以 `FlagsChanged` + `MaskSecondaryFn` 到达，Windows 的 Fn 键由
键盘固件处理、Raw Input 从不报告，所以没有 Windows 映射。同理，F13–F24 在 Windows 侧没有可依据的扫描码
（不猜值）、在 macOS 侧 Carbon 没有 `kVK_F21`…`kVK_F24`，`0x70..=0x73` 属于"命名到位但两个平台都不可达"。

**只有模型确实提供对应键位图时，按键才产生动作**（见 ADR-0042）：`bongocat-app` 在激活模型时用
`bongocat-live2d::KeyImageInventory` 读取该模型的键位图清单（与渲染侧加载共用同一次目录扫描和同一套
候选回退），并只把清单里能画出来的键写进 `InputBindings`。写入 runtime 的每模型绑定因此是
"静态 hand 表 ∩ 该模型的键位图"：缺图的按键不进入 `InputState::model_snapshot`，既不驱动
`CatParamLeftHandDown`/`CatParamRightHandDown`，也不产生按键层，所以按键层与爪部反馈永远一致。
判断在 runtime 之前完成，renderer 仍只消费不可变 `RenderSnapshot`，不决定动作；该规则只覆盖键位图，
鼠标指针/按键与手柄按钮不属于键位图资源。功能键的范围、名字和左右手归属由
`bongocat-render` 的同一张 HID 表给出（`0x3a..=0x45` 与 `0x68..=0x73` 两段，中间是 PrintScreen
至方向键和数字键盘），避免按键图片解析和 runtime 绑定各自定义；没有 hand 归属的按键不产生按键层。

### 11.1 Cubism 边界

```text
Official Cubism Core binary
          |
   raw Rust sys bindings
          |
  Moc / Model safe wrappers
          |
model evaluation + render snapshot
```

- Core 二进制按平台分发，版本、hash、来源和许可证记录在构建清单中。
- 当前验证基线固定为 Cubism Native `5-r.5` / Core `06.00.0001`；升级必须重跑
  header/binding provenance、目标 ABI、三个预置 Moc、offscreen/enhanced rendering
  fixture 和双 renderer 门禁。
- raw binding 只由精确锁定的离线生成工具从 hash 固定的官方 header 生成；生成配置、target ABI、libclang 版本和输出 hash 必须进入 provenance，禁止手改生成代码。
- 维护者提供并批准用于开发的固定基线已存入 `vendor/cubism/5-r.5`，包括
  Core、官方 header 和由该 header 生成的 target binding。公开发布前另行核对
  attribution 与再分发清单；该发布工作不阻塞本地功能实现和 `next` 开发提交。
- 原始指针不离开 safe wrapper；Moc 必须比 Model 活得更久。
- Core 全局日志 callback 只在 callback 生命周期内有界复制至多 512 bytes，并以容量 128 的
  非阻塞队列交给专用 Rust worker；callback 不获取 writer mutex、不序列化、不执行文件 I/O。
  全局 callback-slot 竞争、队列满、停止后的迟到 callback 都只增加匿名 dropped 计数。关闭时先
  注销 Core callback，拒绝新记录，再排空已接收记录并 join worker；Core message 与日志路径不进入
  runtime、UI 或 diagnostics export。
- 不把未经验证的新纯 Rust Cubism 兼容 crate 作为生产基础。
- `.model3.json`、motion、expression、physics 和 pose 兼容性由 fixture 验证。
- 每帧从 Core 默认 parameter 开始，依次应用 motion、expression、自动 EyeBlink/Breath、
  physics/pose（实现后）、类型化产品输入，最后调用 Core update。motion 的自然结束与显式停止均使用 model3/
  curve fade，显式停止的外层正弦权重与 curve 权重相乘。`PartOpacity` motion curve
  遵循 R5 Framework 语义，按 curve ID 写入 Core part-opacity sink，和普通 parameter
  curve 使用独立的目标表与权重路径。model3 `Groups` 由模型索引保留并校验；motion `Model` target 中
  `EyeBlink` 对匹配的 Parameter curve 做乘法、`LipSync` 做加法，对未被 Parameter curve
  覆盖的首个同名 Parameter group（最多 64 个 ID）使用 motion fade 插值。`Opacity` 作为
  独立 model opacity 进入 `RenderSnapshot`，只在 renderer 最终颜色 pass 与 drawable/
  窗口透明度相乘，不参与 mask 生成，并保持到后续 motion opacity curve 更新或模型切换。
  expression 使用 `.exp3.json` 的
  Add/Multiply/Overwrite 和正弦淡入淡出；替换期间最多保留上一层与当前层，稳定后只保留
  最新 expression，使内存和每帧成本保持有界。真实输入最后覆盖对应产品 parameter，
  避免表情让按下状态失真。
- 模型加载采用 prepare/commit/rollback，失败时保留当前可用模型。
- 模型切换是 CPU/GPU 两阶段提交：runtime 先保留旧 active model/bindings，准备新的
  Cubism generation，并随候选 `RenderSnapshot` 发布一次性强类型 commit token；平台
  renderer 只有在纹理、mesh、mask 和 pipeline 资源全部准备成功后才能确认。runtime
  收到匹配 token 的确认后才替换产品事实状态；renderer 拒绝时双方继续使用旧模型。
- 普通 render frame 使用独立单调 transport sequence，可以按 latest-value 合并；模型
  commit feedback 使用不可覆盖的可靠单槽并且同时只允许一个候选 generation，不能被
  普通帧、cursor 或输入事件合并。等待 GPU 确认期间可靠输入仍由旧 bindings 消费。

官方 Cubism Framework 的动作、物理等逻辑必须在 Phase 0 验证 Rust 实现的兼容性。未达到退出门槛时必须形成 go/no-go ADR，不能绕过该门槛扩大实现范围。

### 11.2 Renderer

GPUI renderer 与 Live2D renderer 完全分离：

```text
RenderSnapshot
├── model opacity / horizontal mirror / drawables / offscreens / order / opacity / masks
├── color + alpha blend / multiply + screen color
├── vertex / uv / index buffers
├── texture ids
└── transform / viewport
        ├── Windows D3D11 renderer
        └── macOS Metal renderer
```

renderer 负责遮罩、混合、裁剪、纹理上传、dirty flag、present 和 GPU 生命周期；不读取配置、不决定动作、不访问 GPUI entity。

Cubism Core 在每次 `UpdateModel` 后的 drawable dynamic flags 必须随 `RenderSnapshot` 一起复制，
并在 `ResetDrawableDynamicFlags` 前完成读取。v1 中 drawable index topology 在同一 model generation
内不可变，backend 检测到变化即拒绝该 snapshot；Windows 与 macOS 只在
`vertex_positions_changed` 时重写对应 vertex buffer，render order、visibility、opacity 与颜色仍以
同一帧 snapshot 更新 CPU-side draw state。

所有 v1 PNG RGBA 贴图（Cubism texture、背景与按键 overlay）按 sRGB 编码解释；Windows 使用
`R8G8B8A8_UNORM_SRGB`、macOS 使用 `RGBA8Unorm_sRGB`，使 shader sampling 和颜色计算在
linear 空间进行。最终预乘 alpha 颜色在两端都必须经过 linear -> sRGB 编码：macOS 将它交给
`BGRA8Unorm_sRGB` drawable，Windows 的 flip presentation model 只能使用非 sRGB 的
`B8G8R8A8_UNORM` back buffer，因此编码由该 buffer 的 render target view 以
`B8G8R8A8_UNORM_SRGB` 承担；normal、additive 与 multiplicative blend 使用相同 linear
premultiplied 输入。clipping mask 只携带 alpha，保持 linear UNORM，避免对 coverage 作
gamma 转换。缺少这一编码时 linear 值会按已编码值被直接扫描输出，中间调整体偏暗（黑与白不受
影响），并让两平台观感不一致。v1 不解释或转换嵌入 ICC/wide-gamut profile；模型导入将此类颜色
管理作为明确的后续能力，而不是让平台默认行为决定结果。系统级显示色彩管理仍存在平台差异：
macOS 由 Core Animation 把 sRGB 内容转换到显示器色彩空间，Windows DirectComposition 不做
这一步，因此同一组 sRGB 值在广色域显示器上的绝对观感仍可能不同；这属于合成器行为，不由
renderer 消除。

窗口圆角是 overlay 窗口自身的形状属性，与模型、动作和输入无关。`overlay.corner_radius_percent`
按窗口宽高的百分比给出四个角的椭圆半径：`N%` 表示水平半轴为窗口宽度的 `N%`、垂直半轴为窗口
高度的 `N%`，与旧版 CSS `border-radius: N%` 相同，因此同一个数值在非正方形窗口上产生的水平
圆角比垂直圆角更大；`0` 保持直角，`50` 时四条弧线相接、内容被裁剪为窗口的内切椭圆。renderer
在片元着色器中按 drawable 像素位置求该椭圆的 coverage，并把它乘进每次绘制的 alpha，因此圆角
只改变窗口边缘的合成结果，不改变 `RenderSnapshot`、模型资源、绘制顺序或 blend 模式。该值只
作用于 overlay 窗口；GPUI 设置窗口和其他产品窗口保持各自的平台边框。改变圆角与改变缩放、
不透明度、屏幕范围约束一样需要重建原生窗口资源。

指针悬停隐藏是 overlay 窗口的临时呈现状态，不是窗口可见性。`overlay.hide_on_pointer_hover`
开启时，指针进入 overlay 窗口矩形并停留 `overlay.hide_on_pointer_hover_delay_seconds` 之后，owner 把
窗口的呈现 alpha 淡到 `0` 并强制指针穿透；指针离开窗口矩形后按同样的时长延迟淡回
`opacity_percent`，并把穿透恢复为 `overlay.click_through`。窗口本身既不隐藏也不销毁，
`overlay.visible` 不受影响，frame source 继续按 `maximum_fps` 出帧，shutdown 顺序不变。隐藏期间
穿透强制为开，因此不可见的 overlay 不会吞掉本该落到下层窗口的点击。

淡入淡出复刻旧版 CSS `transition-opacity-300` 的 `300 ms`，但按「从过渡起点起算的绝对经过时间」
求值，而不是按帧累加：同样的时间点在不同帧率下得到同一个 alpha，60 FPS 与 240 FPS 的采样结果
一致。首版按线性插值，不复刻旧版 CSS 的 `transition-timing-function`：该曲线来自样式框架默认值、
未在旧版代码中显式声明，因此不视为已冻结的行为。命中测试用指针采样与窗口矩形的半开区间比较
（`x >= left && x < left + width`，纵向同理），因此相邻显示器共享的边界像素只会落在其中一个窗口内。
平台采样与窗口矩形的坐标空间不同时必须在 owner 内换算，不允许把平台坐标泄漏给 runtime 或 renderer。
圆角窗口的透明角落仍算在窗口矩形内，与旧版一致。

指针采样缺失、或平台输入服务不在 `Running` 状态时一律视为「不在窗口内」。这条降级规则保证指针
链路失效时 overlay 最坏情况是保持可见，而不会永久停在全透明且穿透的状态。悬停隐藏与
`opacity_percent` 共同决定最终 alpha，因此改变不透明度仍会重建窗口，而开关和延迟本身在 frame
tick 内原地生效。

原生 overlay 窗口创建后默认保持隐藏。平台 owner 只有在对应 renderer 已成功完成至少一次
非空帧 draw/present 后才允许首次显示；启动、隐藏后重显、设置导致的窗口重建和模型切换重建
都遵守同一顺序。首帧提交或验证失败时窗口保持隐藏，模型准备失败仍保留当前可用窗口与模型，
不得暴露未初始化 swapchain/drawable 造成黑框或不透明闪烁。

overlay 隐藏时不连续消费或 present 普通动画帧，latest data frame 继续在 transport 中合并；
可靠 model commit 被消费时淘汰早于候选 generation 的旧 data frame，避免重显回退 GPU owner。
带 model commit token 的可靠控制帧仍由 `100 ms` 上限的隐藏 tick 单独消费：候选 GPU owner
完成一次隐藏 draw/present 验证后回报 prepared 并替换旧 owner，失败则回报 rejected 并保留旧
owner。后续重显必须先同步当前 latest frame、再次成功 present，再显示已提交的候选窗口。

renderer 的模型资源准备结果通过稳定项目 error code 回到 runtime，不返回 GPU handle、
平台对象或任意字符串协议。候选准备失败只结束对应模型 command，不得终止 frame loop、
清空当前 GPU model 或让 runtime 提前宣布候选模型 active。

临时 presentation unavailable（例如 macOS `CAMetalLayer` 未交付 drawable）不是模型或设备
准备失败：frame source 必须以单调、有限的退避延迟重试，成功 present 后重置；不得把重复临时
不可用写成 failure 日志或停止 frame source。候选模型的可靠 commit token 必须保留至实际首帧
成功，不能因临时 drawable 缺失被拒绝。未知 D3D11 present/device-loss 仍按明确 HRESULT 恢复策略
处理，不能笼统归类为临时成功。

Linux 阶段再决定增加 Vulkan/OpenGL backend，或基于数据迁移到 wgpu。首发优先保证 Windows/macOS 透明窗口的确定性。

### 11.3 Motion 音效

- `bongocat-audio` 使用精确锁定的 `rodio 0.22.2`，只启用 output playback 与 FLAC；
  rodio/CPAL 类型不进入 runtime、model、UI 或 renderer 公共接口。
- runtime 只在 motion priority 与资源解析均成功后，通过固定容量有序队列非阻塞发布
  强类型 `Prepare`/`ActivatePrepared`/`Play`/`Stop`。解码、文件 I/O 和设备创建全部由独立 worker 执行。
- renderer 与 audio 对候选模型并行 prepare；audio worker 将去重 sound 解码为 immutable PCM，二者完成
  或 audio 已稳定降级后才 commit 模型。`ActivatePrepared` 只保留活动模型 cache；output stream 的惰性打开由
  audio owner 执行且不能阻塞 model 或 motion。已活动模型的 motion 在同一 runtime command 中发布缓存 `Play`
  并启动 motion；不得等待 mixer position、设备回调或轮询音频诊断。audio owner 优先使用 512-frame buffer，设备拒绝时回退默认 sink。
- 同时最多一个 motion voice。新动作（包括没有 sound 的动作）、显式停止、禁用音效、
  成功模型 commit 和 shutdown 都停止旧 voice；被 priority 拒绝或加载失败的动作不改变
  当前 voice。
- model3 sound 相对路径必须先通过模型包规范化与包根约束，再由 runtime 组合为本地路径；
  audio backend 不解析 model3，也不接受 URL 或未验证的模型引用。
- 文件、解码、输出设备和队列故障只更新匿名诊断，不改变 motion、input 或 render 状态。
  满载恢复丢弃不可信 backlog 并停止 voice，禁止阻塞输入边沿。
- motion3 UserData 由 Live2D evaluator 保留并按单调 elapsed 的 `(previous, current]` 区间
  产生强类型 occurrence；循环跨越不重复，时间回退不重放，单 tick 最多发布 256 项并
  对跳过数量计数，避免睡眠恢复后的无界分配。

## 12. 配置、模型与安全

应用身份：

```text
com.ayangweb.bongo-cat
```

构建产物携带不可变的 `Development` 或 `Production` 环境。环境由构建入口显式选择，运行时参数、环境变量和设置项均不能切换。两个环境使用相同 schema 和相对目录结构，只改变数据根目录：

正式 app 使用 Cargo feature 选择环境：默认不启用 `production`，即 Development；Production
build/package 必须显式启用 `bongocat-app/production`。packaging 继续校验 `--environment` 只接受
`development`/`production`，并只在 Production 时给子 Cargo 命令添加该 feature。运行时不读取
`BONGOCAT_BUILD_ENV` 或其他环境变量。正式 `Application::start` 只以编译期环境调用当前平台 path
resolver，不接受外部 `StorageLayout`、根目录或生产路径覆盖；隔离临时根注入只存在于显式
`storage-test-injection` Development 测试产物，Production 与该 feature 的组合在编译期失败，
默认产品 CLI 和 API 均不包含该入口。

| 平台    | Development                                                         | Production                                                         |
| ------- | ------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Windows | `%APPDATA%\com.ayangweb.bongo-cat\development\`                     | `%APPDATA%\com.ayangweb.bongo-cat\production\`                     |
| macOS   | `~/Library/Application Support/com.ayangweb.bongo-cat/development/` | `~/Library/Application Support/com.ayangweb.bongo-cat/production/` |

每个根目录包含 `config.json`、`state.json`、`models/`、`model-overrides/`、`backups/`、`logs/`、`updates/` 和 `locks/`。`model-overrides/` 是预置模型用户侧内容的命名空间（每张替换封面一个 `<id>/resources/cover.png`），因为预置包位于 app 包内、不可写；它与 `models/` 一样只被设置页与 app 层写入。锁、单实例命名、更新 channel 和诊断同样按环境隔离；任何环境不得读取、写入或 fallback 到另一个环境。`updates/` 是保留给更新的环境私有命名空间：当前 `cargo-packager-updater` 使用进程临时文件/目录下载和暂存载荷，不落在该目录下，因此 `updates/` 当前无写入方，仅作为环境形状契约的一部分保留。`StorageLayout` 只描述这些用户数据路径；安装器使用平台 `InstallationLayout` 描述 product files root，不能从用户数据根推导或操作安装目录。

预置模型属于 product files：macOS 从 `BongoCat.app/Contents/Resources/models` 解析，Windows 从
`bongocat-app.exe` 同级 `resources/models` 解析。仅未打包的开发二进制可在该相对路径缺失时回退到
仓库 `resources/models`；安装产物不得依赖源码树、当前工作目录或用户数据目录。

要求：

- Native Rewrite 配置从全新 schema 开始，不读取、不探测、不导入旧 Tauri/Pinia store。
- JSON key 使用 `snake_case`，字段按当前领域语义命名，不提供旧字段 alias。
- `next` 是全新的初始版本，当前完整配置统一使用 `schema_version: 1`。v1 直接包含成对的
  `selected_model_origin`/`selected_model_id`、用户导入模型的元数据列表
  `model.installed_models` 与内置模型的同名列表 `model.preset_models`（每条含稳定唯一 `id`
  与可编辑 `title`），以及
  `input.gamepad_stick_dead_zone` 和 `input.gamepad_trigger_dead_zone`；两个 dead-zone 都
  必须是 `[0, 1)` 的有限数。两个元数据列表各自判重：列表内 `id` 不得重复，`title` 去除首尾
  空白后不得为空。`preset_models` 为空表示所有内置模型都还用构建给的名字，它没有任何导入、
  删除或裁剪路径。
- `next` 开发期间不读取或转换任何早期中间结构，不实现 schema migration、字段 alias 或版本兼容
  分支。新增字段直接更新当前 v1 的 Rust 类型、JSON Schema、默认值和 fixture。解析入口保留显式
  版本检查并拒绝非 v1 数据；首次正式发布后的后续版本再以该发布版为基线单独设计迁移链。
- 写入使用同目录临时文件、flush、原子替换和提交后验证；替换前把当前配置封装为带格式版本、
  墙上时间、源 schema 和 revision 的环境内备份。每个环境只管理 `config-*.json` 自有命名空间，
  按持久排序键保留最新 8 份且总计不超过 8 MiB；系统时钟回退不得让新备份被误删，未知文件
  不参与清理。备份写入或收敛失败时保留当前配置；原子替换后的重读、typed validation、revision
  或值比较失败时逐字节恢复替换前配置，固定 temp 不得残留，后续启动必须可以重新尝试提交。
- 正式配置提交先以只创建方式写入并 flush 固定的同目录 `config.json.tmp`，再使用平台原子替换
  提交 `config.json`。启动在同一 writer lock 内先处理残留 temp：有效 current 优先并把有效 temp
  归档为 stale；current 缺失或损坏时才提升有效 temp；无效 temp 单独归档且不覆盖 current；非 v1
  schema temp 原样保留并明确报错。stale/invalid 归档共用每环境最多 4 份、总计 8 MiB 的自有
  命名空间，未知文件不参与清理。仅启动恢复以 10 ms 间隔重试 writer lock 最多 1 秒，普通提交
  仍立即报告竞争；app 只保留不含路径、原始字节或 I/O 文本的匿名恢复动作。
- 当前配置属于 v1 但 parse/validate 失败时，只在同一 writer lock 内从新到旧检查自有备份，
  同时验证 envelope 格式、源 schema、源 revision 和完整 typed config。找到第一份有效备份后，
  将损坏原文写入 `config-corrupt-*.bin` quarantine，原子写回该配置并重新读取验证。
  没有有效备份时，直接 quarantine 损坏原文、原子写入并验证当前 v1 默认配置。两条 fallback
  都返回普通可用配置，没有 `RecoveryRequired`、recovery-only settings window、snapshot 恢复字段、
  `RestoreDefaultConfiguration` command 或“恢复后必须重启”流程。非 v1 schema 仍按当前 v1
  的严格版本入口报告 unsupported，禁止自动转换、downgrade 或覆盖未知格式。
- 配置写入将权限/只读文件系统、存储空间/配额不足和 temp 目标占用分类为稳定的匿名失败原因；
  settings 只显示可操作的项目文案，不泄漏路径或操作系统原始错误。写入只清理由当前调用成功创建的
  temp；若固定 temp 已被文件、目录、符号链接或并发创建占用，则保留该条目和 current 并明确失败。
  权限与磁盘满在 temp 创建前后都必须可受控注入，验证失败不改变当前 snapshot/revision。
- `OpenConfigBackupLocation` 是强类型 command，打开当前构建环境的 `backups/`；自 Diagnostics 页面
  移除后它没有 UI 入口，只由 settings service、隔离 smoke 与排障路径使用；
  路径只由 Application 从正式 `StorageLayout` 派生并传给 platform adapter，不进入 command、
  snapshot、错误或 GPUI Entity。platform adapter 先验证绝对目录并 canonicalize，再通过 `opener`
  crate 交给系统默认程序；成功只返回当前 snapshot 且不推进 revision，失败只返回
  `BackupLocationOpenFailed`。
- `config.json` 只包含用户设置；窗口布局写入 `state.json`，pressed state、权限结果和模型解析缓存不持久化。
- `state.json` 使用独立 v1 schema，保存设置窗口的逻辑坐标、尺寸与 maximized 状态，以及 overlay
  的坐标与尺寸；不读取或转换 `next` 开发期间出现过的其他结构。坐标支持多显示器负值并设有有限范围，
  设置窗口尺寸限制为 `640x480..16384x16384`，overlay 尺寸限制为
  `64x64..16384x16384`。缺失、损坏、越界、未知字段或读取失败只回退到鼠标当前所在显示器
  居中的默认尺寸，不得阻塞 config 或 runtime 启动；非 v1 state 回退显示且不被覆盖。
- state 通过环境内独立的 `state.writer.lock` 和原子替换提交，提交后重读 typed state，失败恢复
  替换前 bytes；它不进入 config revision、backup 或 quarantine。GPUI bounds observer 对连续变化
  合并 150 ms 后通知 settings worker，overlay frame source 仅在几何真正变化时通知同一 worker；
  正常 shutdown 前仍强制 flush。配置提交、模型切换和窗口重建必须保留另一窗口已保存的几何，
  窗口完全离开当前显示器时回退到鼠标当前所在显示器居中，fullscreen 不持久化。
- 模型导入防止路径穿越、符号链接逃逸、压缩炸弹和覆盖现有用户数据。
- 模型来源只有一种：用户选中的文件夹，就地读取后复制进 store 自己的 staging。压缩包来源与
  它的解压边界已随实现一并移除（ADR-0036 已撤回，见该 ADR 的撤回说明）；要恢复时先与维护者确认。
- 导入在提交前把包内 `resources/left-keys` 与 `resources/right-keys` 下的旧键位名归一化到产品
  词汇表：`Alt.png` → `AltLeft.png`、`AltGr.png` → `AltRight.png`（ADR-0038）。这一步只作用在
  `ModelStore` 自己的 staging 上，因此用户选中的源目录始终是只读输入；改名不改变文件数与字节数，
  `ModelImportProgress` 的计数仍然真实。canonical 文件已存在时保留它、旧名文件原样留下，不猜
  作者意图。Mver 转换路径不做这一步——它的输出已经按产品词汇表命名。
- 第二种来源是 BongoCatMver 模型（ADR-0037），同样按内容识别，且需要两条独立证据：根
  `config.json` 能解析成 legacy 的 section 形状，且它命名的模式里至少有一个在
  `<资源根>/<模式>/cat_model/` 下恰好带一个 `.model3.json`。缺任一条就回退到普通包导入并
  由那条路径报错，检测本身不报错。
- 一个 Mver 源产出**多个** BongoCat 模型：每种输入模式各自转换成一个包，各自分配随机 UUID
  v4 存储键与元数据记录，因此三种模式是模型列表里三个可独立启用/改名/删除的条目。每个模式
  独立提交——某个模式的键位图损坏不影响已转换成功的模式，与"这几种模式彼此独立"的事实一致。
- 转换只把合成后的包写进 `ModelStore` 自己的 staging，并与目录复制共用同一个 `PreparedModel`
  校验与单次 `rename` 提交尾部，因此不可能绕过包校验，也不存在第二个临时位置。
- 键位图合成是"最小画布上的 Porter-Duff over"：画布取两层较小的宽高、两层锚在原点，因此过大
  的一层被裁切而非缩放；模式没有 `keyboard/` 图集时按字节安装 paw 图而不合成；某个绑定缺 paw
  或配套键帽时只跳过该绑定。输出文件名沿用产品自己的词汇表——键盘键是 `bongocat-live2d` 从
  HID usage 解析出的名字，手柄键是随包预置 gamepad 模型已经装载的名字（`gamepad` 的键位表用
  XInput 序号，不是虚拟键码）；无法命名的控制码不产出图片也不报错。
- 合成图按 lossless 方式重编码（`oxipng`，只开库入口）：位深/颜色类型/调色板/灰度缩减保持解码
  后像素不变，`optimize_alpha` 只改写全透明像素的颜色通道。有损量化库因许可证（GPL）被排除；
  Zopfli 后端实测多 5% 体积换 15 倍时间，不采用。重编码失败写回普通编码结果，不让转换失败。
- Mver 源的检测与转换只在 settings service worker 执行，UI executor 不做阻塞文件或模型解析；
  跨模型进度在应用层折叠（累计已完成模型的文件数与字节数、stage 取最大值），因此用户看到的是
  不倒退且终值等于各模型总和的一条序列，而不是卡在第一个模型的终值。标题为
  「用户标题 · 本地化模式名」，模式名在截断之后拼接，长来源名不能挤掉三个模型之间唯一的区别。
  诊断上只新增 store 码 `SourceConversionFailed`（映射到既有
  `ModelImportSourceUnsupported`），没有新增用户可见错误码。
- 模型包在反序列化或图片解码前执行 JSON 字节/深度、单文件/整包字节、文件数、目录深度
  和纹理尺寸上限。model ID、资源路径、model3 数组索引、任意 JSON bytes 和 PNG header/
  dimensions 具有可收缩 property contract；接受的资源路径必须规范化为幂等、相对且只含
  normal component 的包内引用，任何随机输入不得 panic、越界索引或绕过分配上限。
- model ID 是跨 Windows/macOS 可移植的 ASCII store key：禁止前导/尾随点、路径分隔符及
  Windows 保留设备 stem（含带扩展名的 `CON`、`PRN`、`AUX`、`NUL`、`COM1..9`、
  `LPT1..9`），不得让同一 installed identity 在目标平台解析为设备或隐藏路径。
- 任意外部目录只能产生待导入的 `PreparedModel`；只有当前环境 `ModelStore` 完成复制、复验和
  原子提交后签发的 `InstalledModel` 才能进入 runtime 激活 command。
- 应用装配时打开并持有只读 `PresetModelCatalog`；设置与模型管理只消费预置目录和当前
  环境 `ModelStore` 的合并目录，不在 UI executor 临时扫描或解析模型文件。
- installed 目录扫描逐条目降级：`ModelStore::list` 只在 store 根目录本身不可读或 writer
  lock 竞争时失败，单个条目绝不使整表不可用。文件管理器与操作系统元数据（`.DS_Store`、
  `.localized`、`Thumbs.db`、`desktop.ini`、AppleDouble `._*`）按文件名忽略，既不计入
  目录也不计入诊断；其余不是「自有模型目录」的条目（非常规目录、符号链接、非 UTF-8 名称、
  非可移植 `model_id` 的目录名）静默跳过并计入 `InstalledModelCatalog::skipped_entries`。
  带合法 `model_id` 但包校验失败的目录仍签发 `Invalid` 条目，保持可见。过滤完全在 store
  内部完成：`skipped_entries` 不进入 settings snapshot，也不产生任何用户可见文案或无障
  碍输出，用户只看到可用的模型集合；符号链接始终不被跟随。
- 标准预置模型 `standard` 是始终可用的默认回退：目录身份 `(origin, model_id)` 在数据结构
  上区分预置模型与用户导入的自定义模型，所有自定义模型不可用时仍能回退到 `standard`。
- 应用启动时执行模型恢复：优先激活配置中成对记录的 `selected_model_origin`/
  `selected_model_id`；该模型缺失或不可用时记录匿名回退事件、把 `standard` 预置持久化为
  修正后的选择并激活它，恢复失败不阻塞启动。未配置选择时同样默认激活 `standard`。启动
  还会在 operational 状态下清理指向已不存在模型目录的 `installed_models` 元数据记录；
  目录存在但内容无效的记录保留，由合并目录的稳定诊断码呈现。启动期清理失败的记录留待
  下次启动重试，不影响其他模型或应用整体。
- 模型导入 command 携带标题与文件选择来源（用户选中的文件夹）；标题只是显示
  名称，不参与身份——settings service worker 在导入前用随机 UUID v4（`uuid 1.26.1`，精确 pin）
  生成当前 store 内唯一的可移植存储 ID，因此重复导入同一目录不会覆盖已有模型，
  显示名称可以随时编辑，用户也无需发明任何 ID。导入成功后把标题写入 `installed_models`
  元数据：标题取导入 command 携带的值，页面在选中来源时按文件夹自身的名字预填，
  导入后可在模型卡片里改名；空白值
  依次降级为该名字和模型 ID，超长标题截断到元数据上限；元数据提交失败按导入失败报告且已安装
  目录保留。删除模型在 store 删除成功后同步移除对应元数据记录。
- 模型目录身份是 `(origin, model_id)`。同一 `model_id` 的 preset 与 installed 条目都保留，
  后续选择 command 必须携带 origin，不得以静默覆盖解决冲突。Models 页面的顺序由
  `Application::model_catalog` 一处决定，分两半：preset 在前，按 `MverInputMode::ALL` 的
  模式顺序（标准 → 键盘 → 手柄，不是 id 字母序——三个 id 的字母序恰好是它的倒序）；
  installed 在后，按 `config.model.installed_models` 的记录位置，也就是导入顺序，因此新导入的
  模型落在页面末尾且此后不再移动。没有元数据记录、只存在于 store 根目录的包排在该半区末尾并
  按 id 排序；`model_id` 在两半区都出现时 preset 一定在前，因为整个 preset 半区都排在前面。
- 模型改名与换封面对两个 origin 完全一致（见 ADR-0047）。`installed_models[].title` 与
  `preset_models[].title` 是可编辑显示名，也是**唯一一条存放在模型目录之外**的模型事实；封面在
  installed origin 上是包内文件 `resources/cover.png`，在 preset origin 上是用户侧的
  `<data>/model-overrides/<id>/resources/cover.png`（app 包不可写，见 ADR-0047 决策 2），两边都由
  `bongocat-model` 的 `package_cover_path` 推出布局，分别由 `ModelStore::replace_cover` 与
  `PresetCoverStore::replace_cover` 做同目录原子替换。导入成功后，每个新安装模型的封面会被换成
  **该模型自己渲染的一帧**（见 ADR-0055）：settings worker 只把模型与它在协议里的身份排队，
  GPUI 线程在永不显示的原生窗口里渲染、读回、裁切并编码，再经 `ReplaceModelCover` 写回同一位置，
  因此转换输出的占位封面只在捕获失败时保留。预置模型只多一件事是禁止的：删除。它的包永远是
  product files，改名与封面只是用户侧的记录，包本身从不被写入。
- settings 快照把页面需要、但不属于配置的模型事实一并投影：每个模型条目携带自己的包目录与包内
  封面路径（包内没有封面时为 `None`），页面因此只显示模型自己的图并可直接打开模型文件夹，
  而不自行推导任何路径。
- 模型选择 command 携带完整复合身份。应用先校验并加载候选，再以 expected revision
  持久化选择并启动 runtime/renderer 两阶段提交；候选被拒绝时 runtime 保留旧模型，应用
  使用刚取得的 config revision 原子恢复旧选择。配置写入失败时不得发送激活 command。
- 合并目录通过强类型 settings snapshot 投影来源、可用状态、资源计数和稳定诊断码；无效
  模型继续可见，但用户路径、底层 I/O 文本和模型内容不得进入 UI snapshot 或日志。
- 模型导入 command 只由 settings service worker 执行复制和复验；成功后刷新合并目录，但
  不隐式激活模型或修改选择配置。失败只返回稳定、
  可操作且不含用户路径的导入错误码。settings snapshot revision 同时观察 runtime 变化并为
  catalog-only 变化递增，禁止返回内容已变但 revision 未变的快照。settings client 为每次导入
  分配跨 clone 单调递增的强类型 operation ID；operation 只公开 prepare/copy/validate/commit
  stage、已复制文件数和字节数，不携带用户路径，并通过共享原子取消令牌在 service worker
  阻塞于分块复制时仍可取消。cancel 在原子 rename 提交前生效并清理 staging，
  final result 携带同一 operation ID；后续 Models 页面只消费该契约，不自行执行文件 I/O。
- 模型来源选择由 `bongocat-platform` 的私有 adapter 调用 `rfd 0.17.2`，当前只有一个入口
  `pick_model_folder`：两平台都用 `pick_folder`（macOS `AsyncFileDialog`，Windows `FileDialog`）。
  macOS 只在 AppKit 主线程且已有窗口可作为 sheet parent 时创建 `AsyncFileDialog`，缺少 sheet
  parent 时返回 `BackendUnavailable`，不回退同步 `runModal`；Windows 在专用 worker 的 STA 中调用
  `FileDialog`。两平台都只向上返回 `Selected(PathBuf)`/`Cancelled` 和稳定无路径错误码，Rust 侧
  重新检查绝对、存在与类型（必须是目录）并 canonicalize；选择层不判定所选目录里是不是可用的
  模型包（那由 store 按内容判定并给出稳定码），真正的包解析/复制仍只由 settings worker 执行。
  `rfd` 将取消与后端失败都表示为 `None`，当前 adapter 按取消处理；对话框取消不是错误，错误
  不得携带系统文本或用户路径。
- 剪贴板 adapter 通过私有 `arboard 3.6.1` 边界只接受最多 1 MiB、无内嵌 NUL 的纯文本；无文本
  返回空选项，过大、无效、拒绝或系统故障只返回稳定匿名错误，绝不记录文本。macOS 调用必须
  在 AppKit 主线程和 autorelease pool 内，Windows 的打开、写入和关闭由 `arboard` 的 RAII
  guard 在同一线程完成。依赖类型、系统错误和剪贴板内容都不离开 adapter；底层读取在项目
  大小校验前可能已物化系统提供的完整文本，这是当前第三方文本 API 的已知边界。
- Models 页面在 GPUI Entity 中只保存视图状态——导入草稿（来源目录、由来源名派生的标题、导入状态
  与本次导入的 baseline，运行中的 operation monitor 就在该状态里）、删除确认、编辑草稿和每行的
  焦点句柄——不持有任何路径文案：导入卡片只显示匿名的 choosing/importing/capturing 步骤，不进入
  snapshot、状态文案或日志。导入进行中以 100 ms UI timer 重新渲染卡片，读的只有 operation ID 与
  取消状态（不读文件数/字节数），cancel 直接设置共享原子令牌；失败与取消都回到上传提示并只经
  `Notification` 报告，不提供页面内 retry。GPUI executor 不遍历、解析或复制模型文件，成功只
  刷新 catalog，不隐式切换 active model。
- 模型删除 command 同样携带 `(origin, model_id)`；preset 永不可删。installed 模型始终可删，
  包括当前 runtime active 或配置所选的那个：删除前先按同一 typed selection 路径切回标准预置，
  切换失败即中止删除，绝不在 runtime 仍持有该包时移除文件。删除本身仍以 rename 后删除事务退休；
  同 ID preset 不得阻止删除 installed 副本。成功刷新 catalog，并把回退后的选择一并反映到快照。
- 更新的 manifest 获取、版本比较、下载、验签、安装与重启由 `cargo-packager-updater 0.2.3` 承担；
  `bongocat-update` 只保留 BongoCat 侧策略与诊断契约，不自行实现传输或替换层。信任模型为
  **detached minisign 签名**：发行载荷必须由发布私钥签名，客户端以编译进构建的公钥校验，
  私钥不得进入源码、产物或配置。签名端与打包端是同一个 crate（`cargo_packager::sign`），
  因此签名器与验签器不会各自演进。ADR-0021、ADR-0022、ADR-0025 与 ADR-0026 定义的 detached
  清单签名、manifest v1 schema、单调 `release_sequence` 防降级与 24 小时调度已由 ADR-0029 取代；
  ADR-0029 的库选择与 zipsign 归档内嵌签名模型已由 ADR-0034 取代，均不再有效。
- 更新源由不可变 `ReleaseConfiguration` 描述：发行仓库、构建期 channel、target triple、二进制名与
  macOS bundle 名全部是编译期常量，任何用户配置、CLI 或运行时输入都不能改变它们。Development
  构建的 channel 不允许联网或安装，`check` 与 `install` 在发出任何请求前即失败关闭；Production
  构建从公开发行渠道更新。更新 channel 按环境隔离是 `AGENTS.md` §10 的约束，独立于各 ADR。
- 签名公钥缺失时失败关闭：`RELEASE_SIGNING_KEY` 已内嵌发布公钥（key ID
  `DF5E2C9D255DD85E`）；缺失、空串或纯空白时 runtime 在发出任何请求前即返回
  `update_signature_key_missing`，绝不静默接受未签名载荷。`UpdateRuntime::is_available()` 仅在
  production channel 与有效公钥同时具备时为真，系统菜单据此决定是否显示「检查更新」入口。该
  门禁产出的是稳定、无路径的错误码，而不是库错误。
- 发行清单是**一份共享 manifest**（`latest.json`），形状为库的 *static* 形状：顶层 `version`、
  可选的 `notes` 加一个 `platforms` 映射，键为 `<os>-<arch>`，每项含 `url`、`signature`、`format`；
  runtime 从 `releases/latest/download/latest.json` 读取。平台键拼写必须与库一致（`macos` 而非
  `darwin`，`aarch64` 而非 `arm64`）。`crates/bongocat-packaging` 每个 target 写一份 fragment
  （`<os>-<arch>.json`，文件名即平台键），发布前由同一个工具的 `--merge-manifests` 合并成共享
  manifest——manifest 的形状与资产名由一处拥有，工作流只调用工具。发布漏掉某个平台键时，
  runtime 命中 `update_no_matching_asset`。`notes` 是发布说明，由 `--release-notes <file>` 写入，
  上限 32 KiB，超长在字符边界截断并追加可见标记；它随同一次请求到达客户端，不需要第二次网络调用。
  说明的内容不是两次 tag 之间的提交摘要，而是该版本在双语 changelog 里的条目：发布工作流先用
  `--extract-release-notes <file>` 从 `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 各取出产品版本对应的
  同一条目，按「英文正文 → `---` → 中文正文」合成一份文件，再把这一份同时喂给 manifest 与
  `gh release create --notes-file`，所以发布页和更新窗口渲染的是同一段文本。条目按 Markdown 二级
  标题匹配而不是按版本号搜索文本，因此正文里提到的版本、`###` 子标题和围栏代码块里的示例都不会
  被误当条目；版本在 changelog 里没有条目时提取直接失败，而不是发出一份描述别的版本的说明。

- 更新载荷：macOS 为已完成的 `.app` 打包成的 `BongoCat-<version>-<triple>.app.tar.gz`
  （归档根必须是 `BongoCat.app/`，库会丢弃根条目再装到 bundle 路径）；Windows 复用已发布的
  NSIS 安装器 `BongoCat_<version>_x64.exe`。`.dmg` 不是更新载荷——它是人工安装路径。
  签名是打包流程的最后一步：minisign 签名覆盖发布时的确切字节，签名后不得改名、重压缩或 strip。
- 传输与安装由库承担，启用 Rustls 与 HTTPS；库的 `reqwest` 后端不启用。安装位置：macOS 由库从
  运行中的可执行文件推导其所属 `.app` 并整包替换；Windows 由库运行下载到的安装器
  （`install_mode = Quiet` → NSIS `/S` 静默、`/R` 重启）后自行退出进程，因此
  `UpdateOutcome::Installed` 只在 macOS 可观测。安装根为 `$LOCALAPPDATA\Programs\BongoCat`，
  用户可写，不需要提权。
- 库的错误、配置与平台类型不得扩散为项目公共 API。`cargo_packager_updater::Error` 在
  `bongocat-update` 边界内映射为 14 个稳定错误码，未识别的变体降级为 `update_internal_failed`；
  诊断导出只消费这些码与匿名聚合计数，不含任何库类型或动态平台文本。
- 打包入口 `crates/bongocat-packaging` 产出并签名更新资产：每个 target 的载荷、其 `.sig` 与
  manifest fragment；发布前再用同一个工具的 `--merge-manifests` 把 fragment 合并成共享
  `latest.json`。签名密钥通过 `SIGNING_PRIVATE_KEY` 注入，未设置即跳过签名（本地构建）；
  release workflow 反过来断言发布构建一定签过名，未配置密钥时直接失败，而不是发出一批无人能
  更新的产物。
- 更新管线由 `bongocat-app::ApplicationUpdateService` 独占：一条独立线程持有 `UpdateRuntime`，
  是唯一触碰网络、下载与安装的组件。它**不复用设置服务循环**——设置循环是串行阻塞的，一次百 MB
  级传输会让所有设置读写排队。命令通道有界且非阻塞；状态经 `UpdateStateHandle` 覆盖发布，窗口
  按 250 ms 轮询，因此窗口关闭、隐藏或渲染慢都不会反压 worker。**操作属于 worker，不属于窗口**，
  所以关闭窗口不取消任何操作。
- 更新窗口（`bongocat-ui::update_window`）是单例，入口是系统菜单「检查更新」与设置页 About。
  它渲染 `Unavailable` / `Idle` / `Checking` / `UpToDate` / `Available` / `Downloading` /
  `Verifying` / `Installing` / `Installed` / `Failed` 十种状态。**`Downloading` / `Verifying` /
  `Installing` 三段可观测靠拆分库调用实现**：runtime 用 `download_extended` + `install` 两次调用，
  因为库的 `download_and_install` 把验签与安装合成一次调用。失败带明确的 stage，下载阶段的错误
  再按 code 区分为传输失败与验签失败。
- 更新内容（`notes`）来自 manifest，是不可信输入，由 `bongocat-ui::update_markdown` 用
  `pulldown-cmark`（CommonMark + 删除线/任务列表，`default-features = false`）解析为不含 GPUI
  类型的中间表示后渲染，不存在可注入的 markup 层；原始 HTML 按字面文本显示，图片只渲染 alt 文本、
  不发起请求，链接仅 HTTPS 且无空白/控制字符才可点击。输入截断至 32 KiB，块嵌套超过 8 层压平但
  不丢内容；GFM 表格不渲染（按 CommonMark 退化为段落）。
- 更新协议由 `bongocat-ui-protocol` 拥有，`bongocat-ui` 只负责窗口与展示策略；两者不依赖
  `bongocat-update`。`bongocat-app` 做穷尽映射（stage 与 14 个错误码），新增一项会让映射编译失败，
  直到它被赋予用户可见含义。
- 安装后是否需要重启进程是**平台事实**：macOS 由库整包替换 `.app`，运行中的进程此后执行已删除的
  文件（预设模型目录是惰性读盘的），因此安装成功后自动重启——先按 §5.3 的顺序完成产品 shutdown，
  再 `exec` 新构建；Windows 由安装器 `/R` 重启，`Installed` 在该平台不可观测。
- 自动检查由 GPUI 侧调度（开关值只有设置服务读得到）：启动后等 10 秒首次检查，之后每 24 小时一次，
  发现可用更新且窗口未打开时打开更新窗口。间隔未持久化。
- 传输有 30 分钟上限：transport 自身无超时，不设界会让 worker 永久阻塞；取值覆盖整条载荷传输。
  下载**不支持取消**——库没有 abort 钩子，真取消只能自研下载与验签，而自研验证层已被 ADR-0029/0034
  删除，因此不提供取消按钮而不是提供一个假的。
- 真实签名公钥注入、真实发布链路验证、操作系统包签名验证与失败启动恢复仍未完成；在这些证据齐备前
  不得声称更新功能或 stable 发布完成。ADR-0034 记录了换实现新引入的能力损失（归档完整性校验、
  per-platform 资产匹配、公钥轮换窗）与待验证项；ADR-0035 记录了 worker、窗口、发布说明与
  "安装后协调 shutdown 而非安装前"这一处需要维护者复核的取舍。
- 日志不记录真实按键序列、剪贴板内容或用户文件内容。
- 匿名 diagnostics export 由 settings service 的强类型 command 触发（自 Diagnostics 页面移除后无 UI
  入口，只由隔离 smoke 与排障路径使用），在当前环境 logs 目录以同目录
  原子替换写出固定格式的 JSON。导出只包含稳定错误码、匿名聚合计数、模型来源计数和 revision；
  不包含模型 ID、用户路径、按键值、原始配置/事件内容、时间戳或动态平台错误文本。
- 应用日志由 app-owned writer 以固定组件、级别和 code 写入 `application-<utc_day>.jsonl`，
  不接受自由文本。单文件上限为 1 MiB，应用日志总量上限为 8 MiB、最多保留 8 个文件，且只
  保留最近 7 个 UTC 日；达到文件上限时执行有界轮转，启动和日期切换时执行过期/总量清理。
  清理或写入失败只增加匿名 dropped/pruned 计数，不删除当前配置或模型数据。
- writer 初始化后同步创建环境隔离的运行标记；标记只含 v1 schema 与固定 phase：`running`、
  `shutting_down` 或 `panicked`。下一次启动将旧标记匿名分类为 forced/unknown、shutdown interrupted
  或 panic 后立刻覆盖为新的 `running`，避免重复恢复循环；只有 runtime、音频与配置相关 owner
  完成正常 shutdown 后才删除。process panic hook 使用非阻塞写入追加固定 `application/error/panicked`
  code，不读取 payload、源码位置、backtrace 或用户路径；Development-only 隔离 smoke 必须以同一
  release 可执行文件产生真实 `panic=abort`，验证日志脱敏、配置字节不变、重启分类和标记清理；
  默认产品 CLI/API 不暴露该测试入口。
- 匿名 diagnostics export 的摘要只读取 app-owned writer 和 Cubism Core 的匿名
  written/dropped/rotated/pruned/active-bytes/retained-files/retained-bytes 统计，并另输出二者
  retained-bytes/files 的饱和聚合，不读取或复制 Core message。可预览的
  application lifecycle 历史仅能按 ADR-0027 严格重新解析为固定 code record，再与该摘要组成当前
  环境的私有 preview bundle；Core 历史内容不属于该 bundle。

初始字段命名和数据分类见 `shared/config/native-config-contract.md`，环境和 Bundle ID 决策见 ADR-0008。

继续兼容：

```text
.model3.json  .moc3  .motion3.json  .exp3.json
.physics3.json  .pose3.json  .cdi3.json
texture_*.png  audio files
resources/left-keys  resources/right-keys
resources/background.png  resources/cover.png
```

新增 manifest 只能是可选元数据，不能破坏现有模型。

## 13. 测试、指标与可观测性

### 13.1 测试层级

- Rust 单元测试：reducer、输入映射、动画、配置、路径验证和模型语义。
- fixture：相同输入序列产生规范化状态快照。
- Cubism fixture：三个预置模型和异常/自定义样本的加载、动作、表情、物理和销毁。
- GPUI 测试：设置表单、命令、错误状态、键盘导航和窗口重建。
- 平台集成：窗口、输入、权限、显示器、托盘、启动项和单实例。
- renderer smoke/golden：非空帧、alpha、遮罩、blend 和资源语义。
- 性能/soak：启动、首帧、frame time、输入延迟、CPU/RSS/GPU 和 8 小时运行。
  macOS 定时 diagnostic preview 对每次 `NativeOverlay::draw` 调用记录有界的微秒样本，输出
  nearest-rank p50/p95/p99 与主线程完整循环错过下一 60 FPS deadline 的次数；采样不包括输入、
  runtime handoff 或 sleep，最多保留 4,096 个样本并显式报告溢出数。它是可复现的 renderer
  baseline 输入，不替代 Instruments、Metal System Trace 或跨设备发布验收。

### 13.2 输入不变量

- 每个 pressed key 最终由 KeyUp、状态校正或 Reset 释放。
- 队列压力不能静默丢 key/button edge。
- 重复 KeyDown 不破坏按压状态或边沿动画。
- 设备断开、锁屏和睡眠后 pressed set 为空。
- 鼠标移动合并不能阻塞键盘释放。

### 13.3 验收指标

- 60 FPS 时 p95 frame time <= 16.7 ms。
- input callback 到 runtime 接收 p95 目标 <= 2 ms。
- 正常压力测试 key/button edge 丢失计数为 0。
- 8 小时固定模型无持续内存增长或 GPU 资源泄漏。
- 所有线程在退出超时内完成 join，不依赖进程强杀。

Windows 使用 ETW/WPA、PresentMon、GPUView；macOS 使用 Instruments、Metal System Trace 和 os_signpost。基准记录硬件、系统、模型、DPI、FPS、样本和构建 commit。

## 14. Linux 策略

Linux 是后续能力，不是隐藏的首发任务：

- runtime、配置、模型和 UI 不使用 Windows/macOS 类型。
- GPUI 保持 X11/Wayland 构建路径，但首期 CI 不发布 Linux 安装包。
- X11 评估 XInput2；Wayland 全局输入受 compositor/portal 限制，不能承诺功能等价。
- Linux renderer、托盘、启动项和打包单独立项并建立能力矩阵。
- 不用轮询兼容层掩盖 Wayland 权限或协议缺失。

## 15. 风险与 Phase 0 退出条件

| 风险                         | 控制措施                         | 退出条件                          |
| ---------------------------- | -------------------------------- | --------------------------------- |
| GPUI pre-1.0                 | 精确 pin、UI 封装、升级隔离      | 双平台 UI/IME/主题 smoke 通过    |
| GPUI 与 overlay 生命周期冲突 | 最小平台原型                     | 两窗口反复开关并正常退出          |
| Rust Live2D 工作量过大       | Core/动作/物理/renderer spike    | 三个预置模型完成输入到绘制闭环    |
| 透明合成不稳定               | D3D11/Metal 截图和压力测试       | alpha、置顶、穿透双平台通过       |
| 输入仍卡键                   | Raw Input/CGEventTap + reconcile | #47 和生命周期矩阵无残留键        |
| Cubism 授权不明确            | 二进制/许可证清单                | 发布方式有书面结论                |
| 后续 Linux 不等价            | 单独能力矩阵                     | 不影响 Windows/macOS 首发         |

任一核心退出条件失败，先记录 ADR 并调整受影响的实现或发布目标，不能用产品代码、合成测试或编译结果掩盖 spike 失败。按 ADR-0011，未完成的外部证据不阻止无关模块的渐进实现，但持续阻塞对应功能声明和 stable 发布。

## 16. ADR 摘要

### ADR-001：单一 Rust 应用

Rust 同时承担 UI、业务、平台调用和渲染实现，所有模块在同一 workspace 内以强类型接口协作。

### ADR-002：GPUI 只用于设置 UI

GPUI 提供设置体验，但不成为 Live2D renderer 或实时状态所有者，以隔离 pre-1.0 风险。

### ADR-003：平台原生 Overlay Renderer

Windows 使用 D3D11，macOS 使用 Metal。模型窗口不嵌入 GPUI renderer。

### ADR-004：输入状态可校正

输入事件提供低延迟边沿，系统状态查询与生命周期 Reset 保证最终一致。任何 hook 库都不是唯一事实来源。

### ADR-005：Cubism 是唯一厂商 FFI 边界

允许调用官方 Cubism Core；BongoCat 业务不得进入 SDK bridge。

### ADR-006：首发不支持 Linux，但不封死 Linux

共享模块保持平台无关；Linux 的输入和窗口限制在后续能力矩阵中诚实处理。

### ADR-007：单一 Rust 运行环境

生产版本的 UI、运行时、平台服务和渲染器均属于同一 Rust 应用。历史版本只用于行为与资源对照。

### ADR-008：应用身份与存储环境隔离

Bundle ID 固定为 `com.ayangweb.bongo-cat`。Development 与 Production 使用相同数据结构和不同存储根，Native Rewrite 不读取旧配置。

### ADR-011：渐进实现与发布门禁分离

允许正式 Rust workspace 在外部证据补齐期间持续开发；Cubism 授权与分发、实机矩阵、签名和稳定性证据保持 stable 发布门禁。

### ADR-012：有序 Motion 音频服务

runtime 非阻塞发布强类型音效命令，独立 Rust worker 使用最小 rodio/FLAC feature 管理唯一 voice、错误恢复和 shutdown；音频失败不影响动作或渲染。

### ADR-013：启动项能力与环境隔离（已被 ADR-0043 取代）

Windows 当前用户 Run value 按 Development/Production 分名；macOS 13+ 只允许 Production
`.app` 使用 `SMAppService.mainAppService`，macOS 12 与 Development 明确报告不支持且不回退。

### ADR-043：启动项后端统一 auto-launch

双平台启动项后端统一为 `auto-launch 0.6.0`：macOS 12+ 全版本 LaunchAgent plist、Windows
HKCU Run（CurrentUser），环境隔离通过不同 `app_name` 保持；后端只产生 Enabled/Disabled，
`Stale`/`RequiresApproval`/`NotFound` 保留为契约变体。Windows stale 检测退役。

### ADR-051：开发构建不提供登录时启动

登录时启动只属于已发布的产品：开发构建的可执行文件是 Cargo 构建产物，注册项会在 `cargo build`
或清理 `target/` 后指向已过期或不存在的二进制。`bongocat-app` 因此按构建环境判定，开发构建直接汇报
`Unsupported(BuildEnvironment)` 且不触碰平台（读取与写入两个方向都答同一状态，写入是 no-op 而非
错误），设置里的启动项整行只在该构建不提供该能力时按统一样式禁用，并保留可见的 `unsupported_build` 文案说明原因。
取代 ADR-0043 中"Development 构建从此支持启动项"这一条决策，平台后端与环境隔离本身不变。

### ADR-021：签名更新 Manifest 信任边界（已被 ADR-0029 取代）

自研 manifest 验签、构建环境绑定与单调 sequence 防降级已退役，更新信任模型改为归档签名。
正文保留在 ADR-0021 中作为历史记录。

### ADR-022：更新 Manifest 传输 Envelope（已被 ADR-0029 取代）

携带固定 key ID 与 signature header 的 manifest envelope 随 ADR-0021 一并退役，传输改由更新库
承担。正文保留在 ADR-0022 中作为历史记录。

### ADR-029：第三方更新库边界（已被 ADR-0034 取代）

`bongocat-update` 只保留构建期 `ReleaseConfiguration`、环境 channel 门禁、签名公钥失败关闭与
匿名诊断契约；下载、校验、安装与重启全部交给第三方更新库。ADR-0021、ADR-0022、ADR-0025 与
ADR-0026 由本 ADR 取代。本 ADR 的库选择（`self_update 1.3.0`）与签名方案（zipsign 归档内嵌签名）
已由 ADR-0034 取代。正文保留在 ADR-0029 中作为历史记录。

### ADR-0036：模型导入的压缩包来源与解压边界（**已撤回**）

**该 ADR 已撤回（2026-09-22）**：压缩包来源、解压边界与 `SourceArchiveUnsupported` 稳定码连同
实现一并移除，`bongocat-model` 不再依赖 `zip`/`flate2`，`ModelPackageLimits` 不再有
`maximum_archive_bytes`。模型来源只有一种：用户选中的文件夹。ADR 正文保留在
`docs/adr/0036-model-archive-import-boundary.md` 作为恢复该功能时的设计输入——**恢复前先与维护者确认**
（压缩包上传需要一组本次不做的新功能）。压缩包与第三方模型内容不进入仓库。

### ADR-0035：更新 worker、更新窗口与发布说明

更新管线由独立 worker 线程独占，窗口只消费它发布的状态；UI 拥有协议，app 做穷尽映射。三段可观测
（下载/校验/安装）靠把库的 `download_and_install` 拆成 `download_extended` + `install` 实现。发布说明
随共享 manifest 的 `notes` 一起发布（有界、无第二次请求）。安装后重启是平台事实：macOS 自动重启，
Windows 由安装器 `/R` 负责。明确不做下载取消。**记录一处与 §8.4 措辞不同的取舍**：shutdown 协调放在
安装完成之后、替换进程之前，理由见该 ADR。

### ADR-0034：Detached Minisign 更新信任模型

更新库改为 `cargo-packager-updater 0.2.3`，信任模型改为 detached minisign 签名。签名端与打包端
是同一个 crate（`cargo_packager::sign`），签名器与验签器不会各自演进。发行清单是一份共享
`latest.json`（库的 *static* 形状，含 `<os>-<arch>` 平台映射），由每个 target 的 fragment 合并
而成；macOS 载荷是根为 `BongoCat.app/` 的 `.app.tar.gz`，Windows 载荷复用已发布的 NSIS 安装器；
两者各附一个 `.sig`，签名是打包流程的最后一步。本 ADR 取代 ADR-0029 的更新库与签名部分，并记录
换实现新引入的能力损失（归档完整性校验、per-platform 资产匹配、公钥轮换窗）与待验证项。

### ADR-0030：先复用现有方案

新功能按现有代码、标准库、平台能力、已安装依赖、成熟第三方方案、最小自有实现的顺序选择。
只有现成方案无法满足已确认的边界，或引入成本明显高于自研时才自行实现；第三方类型和生命周期
仍受现有架构、安全与依赖规则约束。

### ADR-0031：托盘第三方库边界

macOS/Windows 托盘使用 `tray-icon 0.25.0`，菜单与右键弹出使用直接依赖的 `muda 0.20.0`；平台
adapter 负责加载 PNG、映射强类型 action、调用 hide/show，并从 overlay session 的真实
HWND/`NSView` 弹出菜单。第三方类型、句柄和错误不进入 runtime/UI 公共 API，Windows 固定 GUID
与双平台唯一 owner 由 ADR-0031 约束。

### ADR-023：Windows Per-User Installer

Windows 首发采用固定、可审计 NSIS per-user installer，不请求管理员权限或触及环境数据。原设计中的
「独立 Rust update helper 只接收已验证 artifact」已随 ADR-0029 作废：不再有独立 helper，替换由
更新库在进程内完成（Windows 上由库运行下载到的安装器），installer 权限、原子替换与 rollback 仍是
独立发布门禁。

## 17. 实施阶段

### Phase 0：风险验证和行为冻结

冻结参考行为和模型样本，定义全新配置命名与环境隔离契约；完成 GPUI + 独立 overlay、Cubism、输入可靠性、透明合成和许可证 spike。

### Phase 1：Rust 工程骨架

在 Phase 0 外部证据补齐期间渐进建立 Cargo workspace、CI、日志、配置最小实现、runtime 生命周期、GPUI 空设置窗口和双平台空 overlay；只提升已通过对应 contract 的模块。

### Phase 2：输入到 Live2D 最小闭环

加载标准模型，键鼠输入驱动参数/动作并绘制；Windows 完成 #47 校正，macOS 完成权限和 tap 恢复。

### Phase 3：产品 Runtime 与模型兼容

实现状态、动画、手柄、动作、表情、物理、音效和模型管理，并由 fixture 验证。

### Phase 4：GPUI 设置和配置存储

完成 design system、设置页、模型管理、快捷键、权限、诊断、环境隔离与当前 v1 schema。

### Phase 5：系统集成

完成托盘/菜单栏、启动项、单实例、更新、日志导出、签名和权限流程。

### Phase 6：稳定性与发布

完成性能基线、异常恢复、8 小时 soak、安装/升级/回滚和发布清单。达到门槛后完成发布切换。

详细任务和完成定义见 `BongoCat Native Rewrite Implementation TODO.md`。
