# ADR-0047: 模型管理页只做选择与元数据编辑

状态：已接受（2026-09-18）
依赖：ADR-0036（模型导入的压缩包来源与解压边界）、ADR-0037（在应用内导入 BongoCatMver 模型）、ADR-0020（GPUI Kit 统一依赖入口）

修订（2026-09-22）：决策 1 与「不做拖拽导入」中描述的导入入口已变：导入卡片不再走
「选择文件夹 / 选择 .zip」两个按钮加标题输入，而是按下即打开一个文件夹选择器并自动开始导入
（ADR-0036 文首修订）。**入口只接受文件夹**：压缩包上传本次不做，选择器与文案都不再提及压缩包。
“不做拖拽导入”的结论不变，理由里的按钮形态换成现在的单一入口。本 ADR 关于页面只做选择与元数据
编辑的决策未变。

## 背景

1. **旧版模型页的交互是封面卡片网格**：每张卡片显示模型的 `cover.png` 与标题，卡片底部只有
   「选中」「打开所在文件夹」这类入口。Native Rewrite 此前实现成「导入面板 + 行列表（选中/删除）+
   活动模型的行为预览列表」，与旧版页面的信息层级完全不同，用户要求在保持旧版交互风格的前提下
   重做，并明确移除表情入口。
2. **页面当时拿不到封面与目录。** `SettingsModelEntry` 只有 `id`/`title`/`origin`/`availability`，
   既没有包目录（无法提供「打开所在文件夹」），也没有封面路径（无法画封面），于是页面既不能显示
   模型自己的图，也不能把用户送到模型文件夹。
3. **`resources/cover.png` 的布局事实当时只存在于转换代码里。** BongoCatMver 转换用私有的
   `OUTPUT_RESOURCES`/`OUTPUT_COVER` 常量把旧版 `cat.png` 装成 `resources/cover.png`；如果设置页
   自行拼这条路径，两处一旦漂移就会出现「转换装好了但页面找不到」。
4. **标题是可编辑元数据，且只有导入模型有记录。** 显示名来自
   `config.model.installed_models[].title`；预设模型没有记录，显示名回落到稳定 id，而预置模型本身
   位于随产品分发的 `resources/models/`。`Application::delete_model` 已经拒绝删除预设。
5. **行为预览是当时模型页唯一的行为 UI，而快捷键页已经列出同一批行为。** 快捷键页的模型页签从
   当前激活且 Ready 的模型目录生成 motion/expression 行用于绑定；模型页的预览列表是同一批数据的
   第二个入口，用户认为重复。
6. **模型页的错误提示混用过四种呈现方式**：导入失败是危险色 `Tag`、目录失败是危险色文本、
   行为预览失败与其余命令失败走通用 `Notification`、封面选择失败一度内联在卡片里。同一页面上
   同级别的失败有不同的样式与生命周期。

## 决策

### 1. 页面 = 网格首位导入卡片 + 封面卡片

模型管理页只承担两件事：**选择模型** 与 **编辑该模型的标题/封面**。导入卡片占据网格第一格，点击
后仍走既有的来源选择流程（撰写时为「选择文件夹 / 选择 .zip」按钮加标题输入，2026-09-22 起为
单一文件夹选择器加自动导入，压缩包入口暂缓）、标题在选中文件夹时按文件夹名预填；其余每张卡片是
`cover.png` + 标题 + 可用性 + 操作行（选中 / 打开所在文件夹 / 编辑 / 删除，删除前弹出确认浮层）。

### 2. 预置模型只读

预置模型随产品分发，且没有配置元数据记录，因此页面**不提供**改名、换封面、删除：
`SettingsModelEntry` 的这两个能力由 `ModelRowActions::can_edit`/`can_delete` 表达，预设恒为
`false`，`Application::set_model_title`/`set_model_cover` 也在服务层拒绝（`PresetModelMetadata`）。
想自定义预置模型的用户通过导入得到一份可编辑的副本。

### 3. 封面位置成为跨 crate 契约

`bongocat-model` 公开布局事实与唯一的写入口：

- `PACKAGE_RESOURCES_DIRECTORY`、`PACKAGE_COVER_FILE`、`package_cover_path(root)` 是唯一来源，
  BongoCatMver 转换改用同一组常量（原先的私有 `OUTPUT_*` 常量删除）。
- `ModelStore::replace_cover(id, bytes)` 负责「同目录临时文件 + rename」的原子替换，并按既有
  Unix 私有模式落盘；失败时不留半写的封面。
- 设置页**不推导路径**：它只消费 `SettingsModelEntry.directory`/`cover`，两者都由 app 层读取磁盘
  后填充（`cover` 为 `None` 表示包内没有封面，页面画占位符而不是猜一条不存在的路径）。

### 4. 封面只接受 PNG，字节原样落盘

封面是显示用美术，不是模型数据，因此校验只到「PNG 签名 + 不超过包的每文件上限
（`ModelPackageLimits::maximum_file_bytes`）」；通过后按原字节写入，与转换安装旧版 `cat.png`
的做法一致，不做重新编码。这样也避免为 JPEG 等格式引入解码 feature（workspace 的 `image` 只开
`png`）。

### 5. 「打开所在文件夹」沿用备份目录的能力注入

`bongocat-platform::open_directory` 已有实现（`opener` + 绝对路径/存在性校验），复用即可；
settings 服务通过 `ModelLocationCapability` 注入，与配置备份目录同一 seam。原因是单元测试必须能
断言「打开所在文件夹」的结果，而真实实现会启动系统文件管理器。

### 6. 模型页的错误只用通用 Notification 组件

页面级错误（源选择器失败、导入失败、封面选择器失败、目录读不出、改名/换封面/打开位置失败）一律
经 `Window::push_notification` + `NotificationType::Error` 呈现；内联 `Tag`/文本只表达**进度与选择
状态**（未选择/已选择/正在选择/导入进度/完成/取消），失败状态的标签留空。目录读取失败只在
「从可读转为不可读」时推送一次，页面用 `model_catalog_error_reported` 记住已经说过，避免每次
快照重复同一条消息。

### 7. 行为预览退出模型页

模型页不再列出/预览行为，`SettingsCommand::PreviewModelBehavior`、
`SettingsErrorCode::ModelBehaviorPreviewUnavailable/Failed` 及其文案随之删除。行为目录仍由快捷键
页消费（未激活或未声明的行为不生成编辑入口）。runtime 侧的预览能力
（`Application::preview_motion`/`set_expression`）保留不动，恢复设置页预览是新增一个命令的事。

## 明确不做

- **不为预置模型提供删除或编辑**：`delete_model` 的预设拒绝保持，改名/换封面同样拒绝。
- **不把封面写进配置**：封面是包内文件，不是配置字段；`schema_version: 1` 与
  `installed_models[].title` 结构不变，本项不产生配置迁移。
- **不重编码封面、不新增图片依赖**：只校验 PNG 签名字节。
- **不在模型页恢复任何行为/表情入口**：表情列表已在快捷键页。
- **不做拖拽导入**：旧版导入区可以拖入文件，本次仍是按钮（2026-09-22 起是单一文件夹选择器），
  因为 GPUI 侧没有可用且已注册的文件拖放通道，本次不新增该通道。
- **不让 renderer 或 runtime 参与封面**：封面只被设置页读取。

## 残余风险与待验证项（不得当作已确认）

1. **预置模型不能改名或换封面**是本决策的直接意图，但确实改变了「预置模型也能自定义外壳」的预期
   可能性；用户若需要，只能导入一份副本。
2. **封面必须是 PNG**：选到 JPEG/WebP 会被拒绝（`model_cover_invalid`）。放宽需要为 `image`
   打开对应 feature 并决定是否重编码。
3. **封面替换后必须显式失效图像缓存**：替换保持同一路径，GPUI 的资源缓存按路径命中，所以
   保存成功后由页面调用 `ImageSource::remove_asset`。这条依赖是隐式的，将来若把封面改成
   版本化文件名即可去掉。
4. **封面按 `object_fit(Cover)` 裁切**：非卡片比例的封面会被裁边而不是拉伸，极端比例会损失内容。
5. **导入前的标题输入仍在**：本项保留了在导入卡片里命名，因此「卡片上的改名」与「导入时的命名」
   都是标题来源；两者都写同一条元数据，不存在冲突，但不是一个入口。
6. **实机 UI 未验证**：未运行 `--settings-window-smoke` / 模型页 opt-in smoke，未在双平台实机
   点击卡片、选择封面或打开文件夹。
7. **Windows 的 `open_directory` 未实机验证**：本机 macOS 只能证明调用形态与 macOS 行为。

## 验证

已完成（2026-09-18，本机 macOS arm64，固定工具链 1.97.1）：

- `bongocat-model` 95 测试（新增 2）：`a_cover_replacement_lands_on_the_package_cover_and_leaves_no_staging_file`
  断言替换落在 `<包根>/resources/cover.png`、内容逐字节相等且不留 `.cover.png.new`；
  `a_cover_cannot_be_replaced_on_a_model_that_is_not_installed` 断言缺失模型返回 `NotFound`。
- `bongocat-platform` 58 测试（新增 1，改写 1）：`a_cover_selection_is_a_canonical_regular_file`
  断言封面选择器只要求「真实常规文件」并把选择 canonical 化，目录与缺失文件仍报
  `SelectionInvalid`；后台线程用例扩展到 `pick_model_cover`。
- `bongocat-app` 130 测试（新增 2，改写 2）：`service_renames_and_recovers_an_installed_models_title_and_cover`
  走完整服务链路断言改名持久化到配置、空标题报 `ModelTitleInvalid`、封面字节落到包内并出现在快照、
  非 PNG 报 `ModelCoverInvalid` 且原封面不变、预设改名/换封面报 `PresetModelMetadataImmutable`；
  `service_opens_a_models_own_folder_without_advancing_revision` 用注入的文件管理器断言打开的正是
  模型目录、配置 revision 不变、后端拒绝与模型目录消失都报 `ModelLocationOpenFailed`。
- `bongocat-ui` 115 测试（改写 6）：`model_row_actions` 断言预置恒不可编辑/删除、目录缺失不呈现
  打开入口；tab 顺序断言覆盖五个控件与确认删除态；导入状态断言改为「失败态内联文本为空、
  失败经通知呈现」；行为标识的作用域改由快捷键行断言（`shortcut_behavior_rows` 区分同一模型的
  motion/expression，也区分两个模型的同一行为）。
- 文案：`tools/validate-locales.py` 通过（386 键 × 2，删除 12 键、新增 15 键）；另以脚本核对
  Rust 中引用的 203 个字面量键全部存在于目录，且 `models.`/`errors.settings.` 命名空间无孤立键
  （该脚本发现并修掉了一处 `models.edit.cover.selected` → `models.edit.cover.replace` 的静默失配，
  这正是 `validate-locales.py` 不覆盖的语义漂移）。
- `just check` 的六道门全过：`cargo fmt --all -- --check`；三组严格 Clippy（workspace 排除 app、
  app `storage-test-injection`、app `production`）；`cargo test --locked --workspace` 全绿
  （app 130 + bin 21、ui 115、model 95、platform 58、runtime 73、config 51、live2d 53、overlay 43、
  update 36、packaging 23、render 15、i18n 4 等）；`cargo check --locked --workspace --release`；
  `tools/validate-fixtures.py`（9 输入 + 8 模型用例）、`tools/validate-json-schema.py`
  （14 config + 6 state + 9/9）与 `tools/tests`（63 项）。

**未运行**：双平台实机设置页点击（选中/编辑/换封面/打开文件夹）、`--settings-window-smoke`、
模型页 opt-in smoke、Windows 实机 `open_directory`、真实社区模型回归。
