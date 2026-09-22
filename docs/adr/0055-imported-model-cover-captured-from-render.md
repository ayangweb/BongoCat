# ADR-0055: 导入后的模型封面由渲染一帧截取

状态：已接受（2026-09-22）
依赖：ADR-0002（GPUI for Settings UI）、ADR-0003（Native Overlay Renderers）、ADR-0037（在应用内导入 BongoCatMver 模型）、ADR-0047（模型管理页只做选择与元数据编辑）

## 背景

1. **转换出来的封面是源包里的同一张占位图。** BongoCatMver 转换按 ADR-0037 的输出布局，每个输入模式各取
   `img/<模式>/cat.png` 原字节装成 `resources/cover.png`。上游各模式的 `cat.png` 本身就是同一张手绘
   占位图，于是「标准 / 键盘 / 手柄」三个模型导入后封面逐字节相同，模型页看起来是三张重复卡片。
2. **封面本来就可以由模型自己画出来。** overlay 的渲染路径能把一个模型渲染成完整一帧：模型自己的背景图、
   Live2D drawable，以及键位层；预置模型的卡片图就是这个构图。缺的不是渲染能力，而是「谁来截、在哪截、
   什么时候截」。
3. **要被截的模型不是当前激活的模型。** 刚导入的模型此刻并没有被 overlay 渲染，因此必须新起一个私有
   runtime 和一个自己的原生窗口——形态上就是既有的 `run_model_preview`，区别只在于窗口永不显示、帧在
   present 之前就被读回。原生窗口只能在拥有它的线程上创建：macOS 是 AppKit 主线程，Windows 是创建 HWND
   的线程；本产品里两者都是 GPUI 线程（overlay 也由它 tick）。
4. **导入发生在 settings service worker 线程上。** 该 worker 执行 import command，不能建窗口，因此
   「把模型渲染成封面」必须跨线程交接。而 `bongocat-ui` 不依赖 runtime / render / Live2D（ADR-0002 与
   Technical Design 的 crate 边界），协议层不能携带模型类型，交接只能走 app 自己拥有、两个线程共享的队列。
5. **ADR-0047 的「明确不做」里写着「不让 renderer 或 runtime 参与封面」。** 本条明确推翻它：封面从此是
   渲染产物，而不是被原样搬运的源文件。ADR-0047 的其余决策（封面位置契约、PNG 契约、原子替换、
   预置模型只读、错误只用通用 Notification）保持不变。

## 决策

### 1. 导入成功后为每个安装的模型排队一次截取

`ApplicationMainThreadSignals`（由 `ApplicationShortcutSignals` 更名，因为它现在承载的不只是快捷键信号）
新增一条 cover capture 队列。settings worker 在 import command 成功后，把每个已安装模型连同它在
settings 协议里的身份（`SettingsModelKey`，origin 恒为 installed）入队。worker 只排队：它不渲染、不读
封面、不写任何字节，也不为截取失败而改变导入结果。

### 2. 由 GPUI 线程在 50 ms 轮询里完成渲染

新的 GPUI 任务每 `COVER_CAPTURE_POLL_INTERVAL_MS = 50` 毫秒取空队列，对每个请求：

1. 调用 `bongocat_overlay::capture_model_cover`（隐藏窗口 + GPU 读回，见决策 3）；
2. 用 `SettingsCommand::ReplaceModelCover`（bytes 载荷）把结果交回 settings worker 落盘；
3. 让设置窗口 `refresh_model_cover` 丢掉该模型的图像缓存。

第 3 步与 ADR-0047 残余风险 3 是同一条依赖：替换保持相同的包内路径，GPUI 的图像缓存按路径命中，
不显式失效就会继续画旧字节。

### 3. 两个平台后端共用一份「帧 → 封面」逻辑

`bongocat-overlay/src/cover.rs` 承担与 GPU 无关的一半，因此能在任何平台上被普通单元测试覆盖：把一帧
预乘 BGRA 读回转成直通 alpha RGBA、按可见像素裁掉空白边、把最长边降到 640、编码 PNG。
`macos.rs` 与 `windows.rs` 各自只负责产生那一帧：

- 私有 runtime 激活该模型（`ActivateModel`）并等待准备完成；
- 建立尺寸取自模型画布、**永不显示**的窗口（macOS 从不 `orderFrontRegardless`，Windows 不设
  `WS_VISIBLE` 也从不 `ShowWindow`）；
- 首帧按已提交模型的标准做 smoke 校验（渲染不出内容必须失败，不能把空白图当成封面），随后绘制
  `COVER_CAPTURE_FRAMES = 30` 帧让 idle 姿态与物理收敛，每帧只读回、不 present；
- 读回、裁切、编码后关掉 runtime 与窗口。整个过程的耗时上限是 `COVER_CAPTURE_TIMEOUT = 5s`。

捕获的是窗口显示的东西（背景图 + drawable + 键位层），也就是模型自己的样子。模式之间的差异来自模型的
背景图，因此不主动按下按键、不出现按键高亮。

### 4. 截取失败不改变任何东西

失败时保留源封面、不重试、不阻塞导入、不经 UI 报错。封面是显示用美术而非模型数据（ADR-0047 决策 4），
错误的图不是坏掉的模型。可观测性为零这件事是本节的一部分，见残余风险 2。

### 5. 封面写入仍走同一条 store 通道

`Application::set_model_cover_bytes` 与 `set_model_cover` 共享同一个 `install_model_cover` 契约：PNG 签名
+ 不超过包的每文件上限，然后 `ModelStore::replace_cover` 原字节落盘。`SettingsCommand::ReplaceModelCover`
与 `SetModelCover` 的差别只是载荷（bytes 还是 path），因为捕获出来的封面在被安装之前根本不是一个文件。

### 6. `capture-cover` 子命令作为可复现证据

`bongocat-overlay capture-cover <standard|keyboard|gamepad> <out.png>` 让整条链路（隐藏窗口、GPU 读回、
裁切、编码）脱离导入流程单独执行，因此平台 backend 可以在没有 UI 的情况下被检查与回归。

## 明确不做

- **不加偏好开关**：用户要的是「导入后就是模型自己的样子」，而不是多一个开关。此前考虑过的「偏好设置
  增加一个封面字段」被否决。
- **不放松 ADR-0047 决策 4 的 PNG 契约**：仍然只校验签名与大小，不重编码，不为 JPEG/WebP 打开 `image`
  feature。
- **不为已存在的模型回填封面**：本次只覆盖导入路径（含 MVer 三模式各自的导入），不扫描历史模型，
  也不新增「重新生成封面」入口。
- **不让 `bongocat-ui` 依赖 runtime / render / Live2D**：协议只携带 `Vec<u8>`，交接走 app 侧共享队列。
- **不改预置模型**：预置封面仍是 product files，`ReplaceModelCover` 对它同样返回
  `PresetModelMetadataImmutable`。

## 残余风险与待验证项（不得当作已确认）

1. **截取在 GPUI 线程上同步进行**：约 30 帧，实测量级为数百毫秒，期间 overlay 帧循环与设置窗口都会
   停顿。它发生在用户刚点完导入之后，但确实是可见的卡顿；要消除它需要把截取改造成跨 tick 的可让步
   会话（每 tick 画一帧），本次没有做。
2. **失败是静默的**：报出「封面截取失败」需要新增 `ApplicationLogCode`、双语 i18n 文案（`tools/validate-locales.py`
   会校验键集合）或复用 `models.cover` 的失败通知。本次没有引入任何一条，用户只会看到封面没变。
3. **Windows 后端未编译、未实机验证**：本机 macOS 无法交叉编译 `windows-msvc`（`libdeflate-sys` 的 C 构建
   需要 Windows SDK），仓库的 Windows 验证在 `windows-latest` runner 上执行。Windows 侧的风险集中在
   `Renderer::draw_inner` 新增的 `capture` 分支与 `read_staging_frame` 的行拷贝。
4. **截的是 idle 一帧**：若模型第一秒的物理尚未收敛，截出来的姿态可能与用户随后看到的略有差异；
   30 帧与 5s 上限是为此设的收敛窗口，不是精度保证。
5. **隐藏窗口仍需合成器环境**：D3D11 + DComp 或一个可用的 Metal drawable。没有 GPU、没有窗口服务器的
   环境会失败并保留源封面。
6. **图像缓存失效依赖捕获方显式调用**：设置窗口未打开（句柄为 `None`）时跳过，依赖窗口之后重新加载
   同一路径时读到新字节。
7. **每次导入多一个隐藏窗口 + 一个独立 runtime**：MVer 三模式会串行处理三次，模型越大耗时越长。

## 验证

已完成（2026-09-22，本机 macOS arm64）：

- 新增单元测试：`cover.rs` 覆盖预乘 BGRA 转换、行 pitch 校验、内容裁切、缩放与 PNG 编码（`bongocat-overlay`
  52 测试，此前 43）；`bongocat-app` 新增
  `service_queues_a_cover_capture_per_imported_model_and_installs_the_result`，走完整服务链路断言导入后队列里
  恰好一个请求、其 id 与快照里 installed 条目的 id 一致、`ReplaceModelCover` 的字节落到
  `<包根>/resources/cover.png` 并出现在快照 `cover` 字段、非 PNG 被拒绝且原封面不变（app lib 139 测试）。
- 端到端证据：`cargo run --locked -p bongocat-overlay -- capture-cover keyboard /tmp/cover-check.png`
  → 186,091 bytes / 640×352 PNG，与常量搬迁前的同一命令输出逐字节相同；standard / keyboard / gamepad
  三个预置模型各自产出不同构图的封面（此前三个模式共用一张占位图）。
- `just check` 的六道门全过：`cargo fmt --all -- --check`；三组严格 Clippy（workspace 排除 app、app
  `storage-test-injection`、app `production`）；`cargo test --locked --workspace` 全绿（816 项）；
  `cargo check --locked --workspace --release`。

**未运行**：双平台实机导入（导入后观察卡片封面变化）、`--settings-window-smoke`、模型页 opt-in smoke、
Windows 编译与实机截取、真实社区 MVer 模型的批量回归。
