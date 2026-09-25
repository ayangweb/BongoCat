# ADR-0047: 模型管理页只做选择与元数据编辑

状态：已接受（2026-09-18）
依赖：ADR-0036（模型导入的压缩包来源与解压边界）、ADR-0037（在应用内导入 BongoCatMver 模型）、ADR-0020（GPUI Kit 统一依赖入口）、ADR-0065（模型模式元数据与卡片 Badge）

修订（2026-09-22）：决策 1 与「不做拖拽导入」中描述的导入入口已变：导入卡片不再走
「选择文件夹 / 选择 .zip」两个按钮加标题输入，而是按下即打开一个文件夹选择器并自动开始导入
（ADR-0036 文首修订）。**入口只接受文件夹**：压缩包上传本次不做，选择器与文案都不再提及压缩包。
当时“不做拖拽导入”的结论仅适用于本修订之前的实现；窗口级拖放入口由 2026-09-24 修订取代。
本 ADR 关于页面只做选择与元数据编辑的决策未变。

修订（2026-09-24，窗口级文件拖放）：设置窗口根视图现在接收 GPUI `ExternalPaths`，拖入任意页面都
显示覆盖整个窗口的导入蒙层。一次只接受一个文件夹；拖放路径先在 background executor 复验并
canonicalize，再进入既有的来源检查、转换选择、导入和封面截取流程。蒙层自身也注册 drag/drop handler，
避免渲染后遮住根视图导致真实松手失效；离开窗口则由 paint-phase 的 window listener 捕获
`FileDropEvent::Exited`，不依赖已不再 hovered 的 hitbox。正常、忙碌和多项目选择分别显示可操作、无等待和拒绝提示；
拒绝使用稳定 `SettingsErrorCode::ModelImportDropInvalid` 通知，不把路径带入 UI 文案或日志。

修订（2026-09-22，卡片编辑改为原地）：决策 1 里卡片的「编辑」形态已变。编辑器不再把封面按钮、
标题输入框和保存/取消三行插在封面下面，而是在**卡片已有的行里原地替换**：

- 标题行：卡片只建一个标题行（固定为控件高度，`Size::Medium`）；显示态装模型名，编辑态装同一尺寸
  的 GPUI Kit `Input`（保留组件自身的边框、背景与聚焦环），所以标题行的盒子不变，多出来的只是
  那个输入框，以及名字自身向右让出的 10px 内边距。
- 封面：更换封面的按钮改为叠加在封面右下角的浮层按钮，不再占一整行。
- 操作行：「使用 / 打开模型文件夹 / 编辑 / 删除」在编辑态换成「保存 / 取消」，行本身不变。

因此卡片在两个面几何完全一致（同宽同高、标题行与封面位置不变），打开编辑器不会改变卡片高度、
不会拖动整行、也不会移动下面的卡片。卡片里唯一随编辑态变化的行内容是状态行：它两面都保留，
因为它在编辑态消失同样会改变高度。文案随之把 `models.edit.cover.choose`/`.replace` 合并为
`models.edit.cover.label`（「选择封面」/「Choose cover」）——这个按钮永远作用在卡片的封面
区域上（有封面图是它，只有占位符也是它），区分「选」和「换」已无意义。本 ADR 关于页面只做选择与
元数据编辑的决策未变。

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
单一文件夹选择器加自动导入，压缩包入口暂缓）。设置窗口根视图同时接收 `ExternalPaths` 文件拖放，
因此拖入不必先切换到模型管理页，也不要求指针落在导入卡片上；拖放成功后仍由同一来源检查、导入、
取消和封面截取流程处理。其余每张卡片是
`cover.png` + 模式 Badge + 标题 + 可用性 + 操作行（选中 / 打开所在文件夹 / 编辑 / 删除，删除前弹出确认浮层）。

### 2. 预置模型可以改名和换封面，但与删除无关

预置模型随产品分发，它**没有**任何导入、删除或裁剪路径；改名与换封面则在两个 origin 上完全一致：
`ModelRowActions::can_edit` 不再区分 origin（`!commands_blocked`），`can_delete` 仍只对 installed
为真，`Application::set_model_title`/`set_model_cover` 也按 origin 路由而不是拒绝。

定制内容必须落在用户侧，因为预置包位于 app 包内（macOS `Contents/Resources/models`、Windows
安装目录 `resources/models`），签名包与 Program Files 都不可写：

- **标题**写进 `config.model.preset_models`，该列表只保存 `id` 与 `title`，但列表各自独立
  判重、各自独立生命周期：导入创建 installed 记录、删除移除它，预置记录只由改名创建、谁都不删。
  列表为空表示所有预置都还用构建给的名字。预置模式由稳定 id 派生，不写入用户配置。
- **模式**由 `SettingsModelEntry.input_mode` 投影到封面上方的 GPUI Kit `Badge` + `Tag` 组合；
  页面不扫描资源、不解析标题。installed 的模式在导入 metadata 中持久化，普通包若无法通过
  `left-keys`/`right-keys` 资源判定则不会进入 store；详见 ADR-0065。
- **封面**写进 `StorageLayout::model_overrides`（`<data>/model-overrides/<id>/resources/cover.png`），
  由 `bongocat-model-store::PresetCoverStore` 提供唯一写入口，布局与包内封面共用
  `package_cover_path`。快照投影优先取这份替换封面，包内封面只作为回退。
- 预置**包本身永远不被写入**：`crates/bongocat-app/src/settings.rs` 的
  `service_renames_and_covers_a_model_of_either_origin` 断言替换后包内 `cover.png` 逐字节不变。

### 3. 封面位置成为跨 crate 契约

`bongocat-model` 公开布局事实，`bongocat-model-store` 提供两个用户侧写入口：

- `PACKAGE_RESOURCES_DIRECTORY`、`PACKAGE_COVER_FILE`、`package_cover_path(root)` 是唯一来源，
  BongoCatMver 转换改用同一组常量（原先的私有 `OUTPUT_*` 常量删除）。
- `ModelStore::replace_cover(id, bytes)` 负责「同目录临时文件 + rename」的原子替换，并按既有
  Unix 私有模式落盘；失败时不留半写的封面。`PresetCoverStore::replace_cover(id, bytes)` 是同一
  形状的写入口，只是根目录在用户侧（决策 2），因此两边的封面路径都由同一个
  `package_cover_path` 推出。
- 设置页**不推导路径**：它只消费 `SettingsModelEntry.directory`/`cover`，两者都由 app 层读取磁盘
  后填充（`cover` 为 `None` 表示这个模型没有任何封面，页面画占位符而不是猜一条不存在的路径）。

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

### 8. 窗口级拖放使用单文件夹契约

设置窗口的根视图和临时蒙层都注册 GPUI `ExternalPaths` 的 drag/drop handler；另有一个 paint-phase
的 window listener 接收 `FileDropEvent::Exited`，不依赖离开窗口后已经失效的 hitbox。蒙层只在文件
拖入期间存在，覆盖设置窗口的完整 viewport，因此当前页面、滚动位置和指针所在的子控件不会改变导入
语义；蒙层出现后仍需保留自己的 drop handler，不能只依赖根视图的命中测试。

- `Ready`：恰好一个路径且当前没有其它模型来源操作；松手后先在 background executor 调用
  `bongocat_platform::validate_model_folder`，只把 canonical directory 交给既有 `InspectModelSource`
  流程。`Ready`、`Busy` 和 `InvalidSelection` 都只显示各自标题，不显示额外说明。
- `Busy`：已有 picker、drop 校验、来源检查、转换对话框、导入或封面截取；松手不启动第二条流程。
- `InvalidSelection`：零个或多个路径；蒙层立即说明“一次一个模型文件夹”，松手后通过稳定错误通知
  结束，不把用户路径写入通知或日志。单个路径若是常规文件、已不存在或无法 canonicalize，则在
  background 复验失败后走同一稳定通知。

拖放只改变来源的进入方式，不复制模型解析/复制逻辑，也不绕过 `ModelStore` 的安全校验、取消和
原子提交。拖放路径的目录/类型/canonicalize 检查属于平台 adapter；UI executor 不遍历、复制或解析
模型文件。

## 明确不做

- **不为预置模型提供删除**：`delete_model` 的预设拒绝（`PresetModelDeletion`）保持不变。改名与
  换封面自 2026-09-22 修订起对预置开放（决策 2），删除仍然只属于用户导入的模型。
- **不把封面写进配置**：封面是图片字节，不是配置字段；用户侧替换封面是布局相同的文件，配置里
  只有标题这一行文本。installed 的模式元数据是例外，它是强类型小字段而不是图片资源。
- **不在 UI 重新判断普通包模式**：ModelStore 在 staging 提交前按 `right-keys`、`East`、`DPad*`
  等资源规则完成一次判定并把结果写入 metadata；页面不重复扫描资源，判定失败直接拒绝导入。
- **不重编码封面、不新增图片依赖**：只校验 PNG 签名字节。
- **不在模型页恢复任何行为/表情入口**：表情列表已在快捷键页。
- **拖放不支持多项目或常规文件**：拖放契约一次只接受一个目录；多选在蒙层阶段拒绝，单路径的
  文件、缺失或无法 canonicalize 的目录在后台复验后拒绝，二者都以稳定通知说明边界。压缩包仍不属于
  当前来源。
- **不让 renderer 或 runtime 参与封面**：封面只被设置页读取。

## 残余风险与待验证项（不得当作已确认）

1. **没有「恢复内置」入口**：改名后只能自己再把名字打回去；换过的封面覆盖了内置封面在页面上的
   位置，包内那张原图仍在，但没有任何入口指回它。这与已安装模型是对称的（它的原封面被就地覆盖，
   同样拿不回来），但预置的原始封面其实还躺在 app 包里，所以这是**有意为之的不对称**而不是丢失。
2. **配置备份不含用户侧封面**：`model-overrides/` 是新的用户数据根，配置备份只覆盖 `config.json`；
   一份恢复回来的配置会重新指向用户侧的覆盖文件。文件被删掉时页面回落到包内封面，不算错误。
3. **预置元数据记录不做裁剪**：某个预置从这个构建里消失时，它的记录仍然留着（不可见、无副作用）；
   若将来某个构建又重新带上同一个 id，那时的名字会被这条旧记录接管。反向的取舍是：不裁剪就不会
   误删用户改过的名字。
4. **封面必须是 PNG**：选到 JPEG/WebP 会被拒绝（`model_cover_invalid`）。放宽需要为 `image`
   打开对应 feature 并决定是否重编码。
5. **封面替换后必须显式失效图像缓存**：替换保持同一路径（用户侧那份也是），GPUI 的资源缓存按路径
   命中，所以保存成功后由页面调用 `ImageSource::remove_asset`。这条依赖是隐式的，将来若把封面改成
   版本化文件名即可去掉。
6. **封面按 `object_fit(Cover)` 裁切**：非卡片比例的封面会被裁边而不是拉伸，极端比例会损失内容。
7. **导入前的标题输入仍在**：本项保留了在导入卡片里命名，因此「卡片上的改名」与「导入时的命名」
   都是标题来源；两者都写同一条元数据，不存在冲突，但不是一个入口。
8. **实机 UI 未验证**：未运行 `--settings-window-smoke` / 模型页 opt-in smoke，未在双平台实机
   点击卡片、选择封面或打开文件夹。
9. **Windows 的 `open_directory` 未实机验证**：本机 macOS 只能证明调用形态与 macOS 行为。
10. **编辑当前使用的模型时标题不再是强调色**：输入框内的文字颜色由输入控件自身的 editor style
   （`theme.foreground`）决定，行的文字颜色管不到它，所以显示态的强调色在编辑态失效。
11. **正文卡片高度 232 → 238**：标题行从「一行文本」（约 26px）变成「一个控件高度」（32px）后，
   卡片自然高度增加 6px，`MODEL_CARD_MIN_HEIGHT` 同步改为 238 并由测试钉住。封面、操作行和
   间距都没变。
12. **编辑态的标题停在原地，但文字会在框内让出内边距**：输入框是 GPUI Kit 的 `Input` 原样
    （边框 + 背景 + 聚焦环），组件自带 `px 10 / py 8` 的留白，所以打开编辑器时名字会向右移动
    10px（垂直方向因为两侧对称且盒子同高，位置不变）。显示态的标题没有这段缩进——它和封面左边缘
    对齐——这是刻意保留的：给显示态也加内边距会让卡片标题永远比封面缩进 10px。
13. **编辑态输入框的文字比组件默认大一档**：为了让两个面的字一样大，输入框的文本用 `text_base()`
    （16px）而不是组件为该尺寸准备的 14px；在 32px 的框里垂直留白因此比组件的设计值紧一些。
    想回到组件原样，删掉 `.text_base()` 即可（代价是编辑时字号从 16px 变 14px）。
14. **预置卡片的「打开所在文件夹」仍指向 app 包内的模型目录（只读）**：决策 5 把这个入口称作
    「手工替换美术」的退路，这对预置不再成立——在那里放一张 `cover.png` 不会生效，封面要走编辑器
    里的「选择封面」。按钮本身没有改（它打开的确实是这个模型所在的目录），因此这是一条**已知的
    不对称**，而不是被忽略的缺陷；若将来要收口，方向是让预置的这个入口指向
    `model-overrides/<id>/`，代价是它与「模型在哪」不再一致。
15. **拖放尚未完成双平台实机验收**：当前已覆盖 headless GPUI 的完整窗口蒙层、离开清理、单目录
    进入既有检查命令和多项目拒绝；尚未在 Windows Explorer 与 macOS Finder 中完成真实文件拖放，
    也未做 125/150/200% DPI、Retina 和真实主题下的目视检查。实现和自动化 contract 不等于这两个
    平台门禁已经通过。

## 验证

已完成（2026-09-18，本机 macOS arm64，固定工具链 1.97.1。**历史记录**：「预置模型只读」相关的
几条已被 2026-09-22 的修订取代，见下文）：

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

### 修订（2026-09-22，卡片编辑改为原地）已完成

本机 macOS arm64，固定工具链 1.97.1：

- `bongocat-ui` 新增 `opening_a_models_editor_does_not_change_the_card`：用真实卡片渲染模型页，
  记录卡片与标题行的 `Bounds`，经 `ModelRowAction::Edit` 打开编辑器后重新渲染，断言卡片与标题行
  的 bounds 完全相等、输入框自身的盒子（含边框）等于标题行的盒子；另外断言卡片高度等于
  `MODEL_CARD_MIN_HEIGHT`（否则两面可能只是在被网格拉伸过的单元格里相等），以及封面按钮落在
  标题行上方（即画在封面上，而不是下方新增一行）。
- 文案：`models.edit.cover.choose`/`.replace` 合并为 `models.edit.cover.label`，两个 locale 同步
  （239 键 × 2），`tools/validate-locales.py` 通过；`bongocat-i18n` 的两条目录扫描测试通过
  （新字面量可解析、无孤立键）。
- `just check` 六道门全过：`cargo fmt --all -- --check`、三组严格 Clippy、`cargo test --locked
  --workspace`（`bongocat-ui` 141，全绿）、`cargo check --locked --workspace --release`。
- Python：`tools/validate-fixtures.py`、`tools/validate-json-schema.py` 与 `tools/tests`（63 项）通过。

**未运行**：双平台实机点击（打开编辑器、点「更改封面图」、保存/取消）、深色主题下的实机外观检查、
`--settings-window-smoke`。

### 修订（2026-09-22，预置模型同样可编辑）已完成

本机 macOS arm64，固定工具链 1.97.1：

- `bongocat-model` 新增 4 个 `preset_covers` 测试：替换落在
  `<override 根>/<id>/resources/cover.png`（与 `package_cover_path` 同形）、不留 `.new` 暂存文件、
  二次替换覆盖前一次、未替换时读路径不创建任何东西、且根目录/`<id>`/`resources` 为 `0o700`、
  文件为 `0o600`。
- `bongocat-app` 改写 `service_renames_and_covers_a_model_of_either_origin`（原
  `service_renames_and_recovers_an_installed_models_title_and_cover`）：走完整服务链路断言
  预置改名后快照标题变化、`config.model.preset_models` 恰好一条记录（installed 记录不受影响）、
  换封面后快照指向用户侧覆盖文件且字节一致，并断言**包内 `cover.png` 逐字节不变**。
- `bongocat-ui`：`model_row_actions_preserve_origin_availability_and_active_identity` 断言预置
  `can_edit == true` 且 `can_delete == false`；新增
  `a_preset_models_card_opens_the_same_in_place_editor` 打开预置卡片的编辑器，断言草稿的
  origin/id/初始标题正确、并在重新投影后存活，且卡片同时画出标题输入框与封面按钮。
  这条用例覆盖了**两处编译器抓不到的 origin 守卫**（`begin_model_edit` 直接拒绝预置、
  `sync_model_row_focus` 在下次投影时丢弃非 installed 的草稿）；两处分别用变异验证：
  改回任一个即变红。smoke 的模型页检查由「预置不得暴露编辑」改为「必须至少有一个可编辑的预置」。
- 配置契约：`shared/config/config.schema.json` 保留 `preset_models` 的 `id/title` 形状，
  `installed_models` 使用含必填 `input_mode` 的专用定义；14 个既有 fixture 补 `"preset_models": []`，
  另有 installed mode accept/reject fixtures，`tools/validate-json-schema.py` 的语义检查按列表
  各自判重并拒绝缺失/未知模式。
- 错误口径：删除 `SettingsErrorCode::PresetModelMetadataImmutable`（`ALL` 36 → 35）与两个 locale
  键；`ModelNotInstalled` 因同时服务两个 origin 而改名 `ModelNotFound`（文案改为「找不到该模型」/
  "The model was not found"），`ApplicationError` 同名改动。
- 顺带修掉一处**既有假断言**：`selecting_a_model_keeps_the_behavior_bindings_the_user_recorded`
  用 `config_revision() > revision_before` 判断「配置被重写」。配置 revision 是持久化文档的
  内容哈希（`revision_for_bytes`），大小关系没有语义；新增字段改变了哈希，这条断言随即失败。
  已改为 `!=` 并注明原因。
- `just check` 六道门全过（`cargo test --locked --workspace`：ui 142、app 138 + bin 22、model 88、
  config 57、runtime 75、platform 64、live2d 55、overlay 52、i18n 11 等，34 个测试二进制无失败）；
  Python 三个校验器与 `tools/tests`（63 项）通过。
- 实机：`cargo run --locked -p bongocat-app --release --features storage-test-injection
  --target-dir target/storage-test-injection -- --models-page-smoke --run-seconds 4` 退出 0 且
  stderr 为空。它用真实窗口、真实设置服务与仓库内三个预置跑完整条模型页检查，覆盖了本决策新增的
  「必须至少有一个可编辑的预置」smoke 断言。

**未运行**：双平台实机点击（改名/换封面预置卡片、确认包内文件未被写入）、深色主题实机外观、
`--settings-window-smoke` 与 `--models-page-smoke` 之外的 opt-in smoke。

### 修订（2026-09-24，窗口级文件拖放）已完成自动化部分

- `bongocat-platform` 公开 `validate_model_folder`，复用原生文件夹选择器的绝对路径、目录类型和
  canonicalize 复验；调用点明确放在 `background_executor`，UI 不执行文件 I/O。
- `bongocat-ui` 新增 `ModelImportState::ValidatingDrop`、窗口级 `ModelDragOverlayState` 和主题化蒙层。
  选择器取得路径后的 `Inspecting` 状态与拖放复验共用
  `models.import.step.validating_source` 的文件夹读取文案。
  根视图与蒙层都注册 `ExternalPaths` drag/drop handler，paint-phase window listener 接收离开事件；
  `Ready`、`Busy`、`InvalidSelection` 三种状态各有独立标题，多项目 drop 不启动导入并通过
  `SettingsErrorCode::ModelImportDropInvalid` 通知说明一次一个文件夹的边界。提示内容区去掉边框和
  背景色，只保留主题化图标与文字层次。
- 自动化覆盖：完整 viewport 蒙层与离开清理（含指针移出 viewport）、忙碌状态不启动第二条来源流程、
  单目录进入既有 `InspectModelSource` command、多项目拒绝，以及选择器取得路径后与拖放共用文件夹读取
  文案和状态转换；`bongocat-platform`、`bongocat-ui-protocol`、`bongocat-ui` 定向测试和 locale validator
  已通过。
- **未运行**：Windows Explorer 与 macOS Finder 的真实文件拖放、拖动时不同页面的目视命中、双平台
  DPI/Retina 主题布局和真实键盘/指针交互。因此本 ADR 的实现决策已进入代码，但双平台实机验收仍未完成，
  不应把自动化测试结果写成跨平台行为已确认。
