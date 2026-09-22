# ADR-0054: 配置损坏恢复收口为 back-to-back fallback，设置 UI 采用视觉优先契约

状态：已接受（2026-09-22）
依赖：ADR-0005（配置原子提交与备份）、ADR-0053（开关门禁规则，本 ADR 取代其中项目 AccessKit tree 部分）

## 背景

维护者要求（2026-09-22）把当前应用收敛到“能看见的功能就是功能”，不再为配置损坏维护恢复
窗口、恢复提示、恢复按钮或重启门禁，也不再把屏幕阅读器和项目自有的辅助技术树当成产品功能：

1. 当前 `config.json` 可用时直接使用。
2. 当前配置损坏时，从最新可用备份恢复。
3. 当前配置损坏且没有可用备份时，写入并使用默认配置。
4. Development、Production 和 smoke 的设置窗口必须使用同一套可见组成；不能再出现 smoke 有
   “重置偏好设置”、正常运行却没有的窗口。
5. 只给用户看的文字和控件负责界面；删除只为辅助技术存在的隐藏文案、AccessKit 节点、action、
   native bridge 和直接依赖。

这里的“视觉优先”只描述当前设置 UI 的实现范围。项目只定义和绘制用户可见、可操作的
内容；以后不新增项目自有的无障碍树、原生桥接、隐藏文案或 action。主题、本地化、键盘
操作和 GPUI Kit 自身提供的默认语义仍可保留。

## 决策

### 1. 配置 fallback

`ConfigStore::load_or_default` 是配置加载的唯一入口，行为固定为：

- 当前 v1 配置通过完整 typed validation 时，原样加载。
- 当前文件 parse/validate 失败时，按新到旧读取本环境自有 `config-*.json`，使用第一份完整
  有效的当前 v1 备份；恢复后的当前文件必须重新读取并验证一致。
- 当前配置损坏且没有有效备份时，直接原子写入当前 v1 默认配置，并返回该默认配置。
- 当前配置缺失时直接写入并返回默认配置。
- 每个阶段复用既有 writer lock、原子替换和损坏文件 quarantine；不再有“等待用户点恢复默认”的
  中间状态。

非 v1 当前文件仍按当前 v1 的严格版本入口处理，不属于自动 downgrade/migration。`next` 不为旧
Tauri/Pinia、开发中间版本或未知未来版本增加转换器、alias、fallback UI 或兼容分支。

### 2. 设置窗口没有配置恢复 UI

- 不创建 recovery-only 设置窗口，不创建受限 runtime，不做 application operational gate。
- 不显示窗口级、页面级或分组级的配置恢复提示。
- 不提供 `RestoreDefaultConfiguration` 按钮、command、snapshot 字段或“恢复后必须重启”流程。
- `ExportDiagnostics` 和备份目录打开能力可以保留为排障入口，但不得重新长出配置恢复页面。
- 成功加载、备份恢复和默认 fallback 都回到普通启动路径；正常关闭与重启语义不变。

### 3. 设置窗口的可见组成必须一致

- Development、Production 和 smoke 使用同一个设置窗口视图及同一页面/分组/按钮组成。
- smoke 通过测试 seam 触发导航、主题和窗口检查，不得通过隐藏或额外渲染用户控件来制造差异。
- 更新入口若由 application 注入，smoke 也必须注入等价 callback，使其可见组成与产品一致；
  smoke 是否真正发起网络更新不属于可见组成条件。
- 以后新增可见按钮或分组时，产品窗口、development smoke 和 release smoke 必须同时获得它；
  以后删除可见内容时也必须三处一致。

### 4. 设置 UI 的辅助技术范围

- 设置 UI 是 visual-first：项目只定义并绘制用户可见、可操作的内容。
- `bongocat-ui` 和 `bongocat-platform` 不维护项目自有的 AccessKit tree、Accessibility adapter、
  native view bridge、辅助技术 action channel、隐藏标签或 only-for-assistive-technologies 文案。
- 除 `gpui-kit` 的传递实现外，workspace 不直接依赖 `accesskit`、`accesskit_macos` 或
  `accesskit_windows`。
- 可保留的键盘行为包括 tab 顺序、可见 focus、Enter/Space 激活、Escape 取消；这些由可见控件
  和 `gpui-kit` 默认能力承担，不额外复制一份辅助语义状态。
- Tooltip 服务鼠标/触摸等可见交互；若文案只为了让屏幕阅读器读出而存在，则不写。
- ADR-0053 的“可见行、AccessKit tree、mutator 三层同源”收窄为“可见行与 mutator 同源
  `SettingGate`”；GPUI 若自带语义由其组件决定，不建立项目 contract。

### 5. 文档契约

以后实现或评审设置 UI 时：

- 先问“用户是否看得见、是否可以操作”；看不见的实现默认删除。
- 不以 screen reader、VoiceOver、Narrator、UIA、AX tree 或 Accessibility permission 作为
  功能完成条件。
- 不在项目代码新增 `*_accessibility`/`*_a11y` 模块、隐藏 `Status` 节点、`supports_click`
  contract 或屏幕阅读器 smoke。
- 不把 `gpui-kit` 内部可能携带的语义当作 BongoCat 的业务 contract。
- 新增可见功能只保留实现功能所需的最小状态和测试，不预先建立恢复 UI、辅助桥或跨层协议。

## 后果

- 配置损坏的普通路径收敛为“备份优先，否则默认值”，没有额外窗口和用户操作。
- 产品/开发/smoke 的可见窗口一致，按钮差异不会再来自运行模式。
- 项目自有辅助功能代码、隐藏文案和直接 AccessKit 依赖被删除；`gpui-kit` 仍可透明使用其
  内部默认语义。
- 自动化测试继续覆盖可见功能、输入、配置和 smoke；不再以辅助树断言证明 UI 完成。

## 明确不做

- 不删除操作系统或 GPUI 的辅助能力；只是不把它列为 BongoCat 自有的 UI contract。
- 不删除主题、本地化、键盘导航、可见 focus 和 tooltip。
- 不因“以后可能需要”而提前保留恢复窗口、恢复状态、辅助桥、feature flag 或空模块。
- 以后不在设置 UI 的项目代码里新增无障碍树、原生桥接、隐藏文案或 action。

## 验证

- `bongocat-config`：有效当前配置、有效备份优先、无有效备份回落默认值。
- `bongocat-app` / `bongocat-ui`：无 recovery-only window、无配置恢复 command/snapshot 字段。
- Development、Production、smoke 的 Settings window 查看同一页面与更新入口组成。
- workspace 搜索不再出现项目自有 `accessibility`、`accesskit` 模块、节点或直接依赖。
