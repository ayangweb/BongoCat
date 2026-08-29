# BongoCat Native Rewrite Implementation TODO

状态：Phase 0 执行中
最后更新：2026-08-29
当前分支：`next`
首发平台：Windows 10 1903+、macOS 12+
后续评估：Linux

> 执行基线：应用代码使用 Rust 2024 edition；GPUI 负责设置 UI；主猫窗口由 Rust 平台模块直接创建，不嵌入 GPUI renderer；Windows 使用 Raw Input + D3D11，macOS 使用 CGEventTap + Metal；官方 Cubism Core 是唯一厂商二进制/FFI 例外。生产产物不包含 Tauri、WebView、Vue、React 或 JavaScript runtime。

> 应用与存储基线：Bundle ID 固定为 `com.ayangweb.bongo-cat`；Development/Production 使用相同 schema 和不同数据根；新配置使用 `snake_case` 自有命名，不读取或导入旧 Tauri/Pinia 配置。

## 0. 执行规则

### 0.1 架构红线

- [ ] GPUI 只负责设置、模型管理、快捷键、权限、更新和诊断 UI。
- [ ] Rust runtime 是配置、输入、动画和当前模型状态的唯一事实来源。
- [ ] 主猫窗口必须是独立原生 overlay，不直接接入 GPUI renderer 私有接口。
- [ ] 输入 callback、runtime tick 和 renderer 不得经过 GPUI 响应式状态链。
- [ ] 不引入 Tauri、WebView、Node.js、JavaScript 或第二套 UI framework。
- [ ] 不使用 rdev 或 monio 事件流作为 pressed state 的唯一依据。
- [ ] 除官方 Cubism Core 外，不新增长期 C/C++/Swift 业务模块。
- [ ] 平台 unsafe/FFI 必须集中在小型 wrapper，业务 crate 默认禁止 unsafe。
- [ ] Linux 不阻塞首发，但共享 crate 不得暴露 Win32/AppKit 类型。
- [ ] Development 不得读取、写入、锁住或 fallback 到 Production 数据。
- [ ] 不实现旧配置字段 alias、自动导入或旧目录探测。

### 0.2 任务完成定义

任务只有同时满足以下条件才能勾选：

- [ ] 代码或文档已提交到当前工作分支，不只存在于临时 spike。
- [ ] 正常、错误、重启和 shutdown 路径均已处理。
- [ ] 新增行为有自动化测试或可重复的实机验收记录。
- [ ] 依赖版本、许可证和来源已锁定并可复现。
- [ ] 日志不记录实际按键序列、剪贴板内容或用户文件内容。
- [ ] 平台差异通过 adapter/`cfg` 收敛，没有扩散到业务层。
- [ ] 设计变化已有 ADR，技术设计、TODO 和实现一致。
- [ ] 验收证据包含构建 commit、系统版本、设备条件和结果。

### 0.3 阶段门禁

- [ ] Phase 0 未通过前，不实现完整设置 UI 或批量迁移旧代码。
- [ ] GPUI 与原生 overlay 共存 spike 未通过前，不铺开平台窗口实现。
- [ ] Cubism spike 未通过前，不删除 Pixi/easy-live2d 行为对照。
- [ ] 存储环境隔离测试未通过前，不允许开发构建使用生产数据根。
- [ ] 8 小时 soak、签名和更新回滚未通过前，不发布 stable。

### 0.4 状态、依赖与验收证据

- `[ ]` 表示未开始、进行中、被阻塞或尚缺任一验收条件；部分完成不得改成 `[x]`。
- `[x]` 只表示该行描述的完整工作已进入 `next`，并具有可重复验证证据；不代表所属 section 或 phase 自动完成。
- 被阻塞的任务在其下记录 `Blocked by`、阻塞日期、所需决策或外部条件，不创建假实现绕过。
- 有前置依赖的任务在开始前确认上游 contract 已冻结；若必须并行，先写清临时接口、owner 和回收日期。
- 每个 spike 必须包含：假设、范围、非目标、依赖版本/来源、运行命令、环境、成功条件、失败条件、原始结果位置和后续处置。
- 平台验收记录必须包含 commit、target triple、系统/SDK、CPU/GPU、显示器/DPI、权限、模型、运行时长和结果；“在本机打开过”不算可重复证据。
- 性能结果必须同时保存测试方法与原始数据；截图必须标注窗口尺寸、缩放、主题、模型和 build commit。
- spike 结束后必须明确 `promote`、`replace` 或 `delete`；不得让实验代码未经评审自然演变为生产模块。

### 0.5 阶段映射

Technical Design 使用 7 个产品阶段描述总体路线，本 TODO 为了设置更细的退出门槛拆成 10 个执行阶段：

| Technical Design                | Implementation TODO |
| ------------------------------- | ------------------- |
| Phase 0 风险验证和行为冻结      | Phase 0             |
| Phase 1 Rust 工程骨架           | Phase 1             |
| Phase 2 输入到 Live2D 最小闭环  | Phase 2-4           |
| Phase 3 产品 Runtime 与模型兼容 | Phase 2、Phase 4    |
| Phase 4 GPUI 设置和配置存储     | Phase 5-6           |
| Phase 5 系统集成                | Phase 7             |
| Phase 6 稳定性与发布            | Phase 8-9           |

阶段名称或边界变化时必须同时更新此映射和 Technical Design 的实施阶段摘要。

## 1. Phase 0：行为冻结与技术风险验证

目标：证明纯 Rust + GPUI 路线可行，并把旧版行为变成可测试输入。

### 1.1 文档与仓库基线

- [ ] 评审并确认 Technical Design 与本 TODO。
- [x] 新增 ADR-001：采用单一 Rust 应用。
- [x] 新增 ADR-002：GPUI 只用于设置 UI。
- [x] 新增 ADR-003：主猫使用独立 D3D11/Metal overlay。
- [x] 新增 ADR-004：输入采用事件 + 状态校正。
- [x] 新增 ADR-005：Cubism Core 是唯一厂商 FFI 边界。
- [x] 新增 ADR-006：首发不支持 Linux，但共享模块不封死后续 backend。
- [x] 新增 ADR-007：生产版本只有单一 Rust 运行环境，历史实现仅用于行为与资源对照。
- [x] 新增 ADR-008：固定 Bundle ID，并隔离 Development/Production 存储环境。
- [x] 新增 ADR-010：Windows 只保留 x64/ARM64，移除 i686，并把缺少 R5 desktop ARM64 Core 固定为发布阻塞。
- [x] 记录 `master`、`next`、旧版本 tag 和可回退 commit。
- [ ] 确认旧 Vue/Tauri 应用仍可构建和运行，保存命令与产物信息。
- [x] 建立 `docs/adr/`、`docs/benchmark/`、`docs/migration/` 目录。
- [x] 建立依赖许可证清单，确认当前 Native spike crate graph 与项目 MIT 发布兼容。
  - 状态（2026-08-29）：最新稳定版 `cargo-deny 0.20.2` 以四个 Windows/macOS target 扫描 13 个独立 workspace，license/source policy 通过并接入 CI；依赖升级后 package 节点数由 lockfile 动态决定，不再把旧的 535 节点快照当作当前事实。Cubism 厂商许可、未来产品依赖、SBOM 和 notice bundle 仍由各自后续门禁处理。
- [x] 审计 Native Rewrite 所有直接 Rust 依赖并升级到 crates.io 最新稳定版。
  - 验收证据：`docs/phase-0/rust-dependency-versions.md` 记录 2026-08-29 的 21 个直接依赖家族、升级范围和命令。原 18 个家族中 8 个已升级、10 个原本已是最新；后续新增的最新稳定版 `bindgen 0.72.1`、`sha2 0.11.0` 与 `libc 0.2.189` 也已精确锁定。完整 `cargo update` 后，最新 `gpui 0.2.2` 仍约束旧 generation 的 Metal/CoreGraphics 和 5 个有兼容更新的传递版本；均已记录 owner path，未静默覆盖或 fork。Dependabot 每周仅扫描 13 个 Native workspace 并向 `next` 提交分组更新。
- [ ] 冻结首发 target triple 和 CPU 架构矩阵，明确 Windows ARM64、macOS Intel 是否发布或仅测试。
  - 状态（2026-08-29）：ADR-0010 已固定 Windows 仅支持 x64/ARM64，i686 不再构建或发布。官方 Cubism Native R5 不提供 desktop Windows ARM64 Core，只有 experimental UWP ARM64 DLL，因此 ARM64 当前是发布阻塞；macOS Intel 和最终安装包形式仍待实机与发布链验证。
- [ ] 记录 Windows MSVC/SDK、macOS Xcode/SDK/Metal Toolchain 和 Rust toolchain 的最低可用组合。
- [ ] 保存旧版最后可用安装包、资源清单、签名状态和 SHA-256，不只记录源码 commit。

状态（2026-08-28）：`docs/phase-0/repository-baseline.md` 已冻结 Git 回滚引用、旧版发布矩阵、主要安装包 hash 和当前可验证的 macOS 签名状态；`docs/phase-0/toolchain-target-matrix.md` 已记录本机 macOS/Rust 环境及五个历史目标。Windows 实机工具链、Cubism 各架构二进制、长期产物归档和最终首发架构决策仍未完成，因此后三项保持未勾选。

### 1.2 旧版功能清单

- [x] 记录透明、无边框、缩放、透明度、拖动、置顶和穿透行为。
- [x] 记录 hover 隐藏、任务栏显示、显示/隐藏和窗口位置恢复。
- [x] 记录多显示器、负坐标、DPI/Retina 和显示器移除行为。
- [x] 记录 standard、keyboard、gamepad 三种模式的输入映射。
- [x] 记录左右手、鼠标按键、鼠标跟随、镜像和鼠标镜像语义。
- [x] 记录 motion、expression、physics、pose、音效和淡入淡出语义。
- [x] 记录全局快捷键和模型行为快捷键。
- [x] 记录模型导入、删除、切换、预置保护和自定义资源目录。
- [x] 记录托盘、设置、启动项、更新、日志、剪贴板和外部链接。
- [x] 记录 Windows 权限差异和 macOS Input Monitoring/Accessibility 流程。
- [x] 记录现有五种语言、主题、错误提示和首次启动流程。
- [x] 为功能标记 `P0 首发`、`P1 首发后` 或 `不迁移`。

状态（2026-08-28）：`docs/phase-0/behavior-inventory.md` 已完成静态源码考古并冻结 47 项范围决策：34 项 `P0 首发`、4 项 `P1 首发后`、9 项 `不迁移`。完成表示功能入口、旧语义、已知风险和待确认项已有可追溯记录；Windows/macOS 实机行为、Cubism 兼容和 fixture 人工确认仍由后续 Phase 0 spike 验收，不因本节勾选而视为完成。

### 1.3 配置契约与资源考古

- [x] 明确 Native Rewrite 不读取、不探测、不导入旧 Tauri/Pinia 配置。
- [x] 固定 JSON `snake_case` 命名规则和首版领域字段命名基线。
- [x] 固定 Bundle ID `com.ayangweb.bongo-cat`。
- [x] 定义 Development/Production 双存储根；schema 和内部相对结构保持一致。
- [x] 保存匿名化旧配置样本和只读 inspector 作为历史参考，不接入生产配置路径。
- [x] 为 standard、keyboard、gamepad 预置模型生成文件清单和 hash。
- [x] 建立缺文件、损坏 JSON、非 ASCII 路径、超大纹理等模型 fixture。
- [x] 记录 model3、moc、texture、motion、expression、physics、pose、cdi 和音频用法。
- [x] 记录 background、cover、left-keys、right-keys 的实际语义。

状态（2026-08-28）：ADR-008 和 `shared/config/native-config-contract.md` 已冻结应用身份、环境隔离与新字段命名。旧配置兼容已移出产品范围；此前的合成 fixture 与 `tools/legacy-config-inspector/` 只保留为历史考古证据。六类合成模型包已覆盖缺失 moc、损坏 JSON、非 ASCII/空格路径、超大纹理、路径穿越和多 model3 入口，并由临时目录 validator 检查稳定诊断；Cubism 与 renderer 兼容仍待独立 spike。

### 1.4 行为 fixture

状态（2026-08-28）：已建立 v1 input/expected schema、9 组输入序列和规范化结果；输入、动作、表情、模型切换和音效 command 已覆盖，确定性 runner、跨文件检查和固定版本的 Draft 2020-12 标准 validator 均已在本机及 CI 通过。物理键全集和旧版人工确认仍未完成。

- [x] 定义稳定的 `PhysicalKey`、`MouseButton`、`GamepadButton` 和 axis 表示；canonical names、按钮阈值、axis 范围和未知码诊断已写入 `shared/behavior/input-semantics.md`。
- [x] 定义带单调相对时间的输入序列 JSON 格式。
- [x] 定义规范化 RuntimeSnapshot，排除平台坐标和浮点噪声。
- [x] 添加单键、重复键、长按和左右修饰键序列。
- [x] 添加组合键、鼠标移动/点击/拖动和多显示器坐标序列。
- [x] 添加手柄连接/断开、按钮、摇杆 dead-zone 和 trigger 序列。
- [x] 添加动作、表情、停止、模型切换和音效序列；优先级与切换清理规范见 `shared/behavior/animation-semantics.md`。
- [x] 添加丢失 KeyUp、设备断开、锁屏、睡眠和服务重启序列。
- [ ] 为 fixture 生成旧版观察结果并人工确认产品语义。
- [x] 将 Draft 2020-12 schema 校验与 `tools/validate-fixtures.py` 接入 CI，固定 `jsonschema==4.25.1`；验证脚本和工具依赖位于 `tools/validate-json-schema.py`、`tools/requirements-phase0.txt`。
- [x] 将跨文件 validator、固定版本 Draft 2020-12 validator 与确定性 input fixture runner 接入 `.github/workflows/native-rewrite-phase0.yml`。
- [x] fixture validator 拒绝逆序时间、重复 id、孤立 expected、未知事件和字段不匹配；`tools/run-input-fixtures.py` 执行确定性协议模型并比较 checkpoint。
- [x] expected snapshot 记录来源：旧版观察、产品决策或新行为修复，禁止无法追溯的 golden update。

### 1.5 GPUI spike

状态（2026-08-29）：已在 `spikes/gpui-settings/` 建立隔离的 macOS 最小窗口，精确锁定 `gpui = 0.2.2` 并生成独立 lockfile；默认预编译 shader、release `.app`、菜单、窗口关闭/重开和 shutdown smoke 通过。当前 spike 还验证了 System/Light/Dark 主题、焦点边框、Tab/Shift-Tab、Unicode/grapheme 文本编辑、选择、剪切、复制和粘贴，以及 GPUI executor 上的 bounded typed command/revision snapshot/shutdown acknowledgement，并保存浅色/深色截图证据。marked-text 纯状态 contract 已覆盖连续中文组合、已有多字节前缀、surrogate pair 和异常 range，修复了相对 UTF-16 selection 错按完整内容换算导致的越界风险。GPUI 绘制内容未出现在 macOS 辅助功能树中；真实系统 IME 组合态、完整 tooltip/dialog 和双平台辅助功能仍未验证，详见 `docs/phase-0/gpui-settings-spike.md`。

- [x] 建立最小 Rust workspace 和 GPUI hello/settings 窗口。
- [x] 固定 `gpui = "=0.2.2"` 并提交 Cargo.lock。
- [x] 禁止依赖 Zed 私有 UI crate；建立最小本地 design token。
- [ ] 验证 Windows/macOS 字体、中文输入法、复制粘贴和文本选择。
- [ ] 验证键盘导航、焦点、tooltip、dialog 和菜单。
- [ ] 验证系统浅色/深色、缩放、Retina 和 Windows 高 DPI。
- [x] 验证窗口关闭、重开和退出生命周期；隐藏到托盘/菜单栏待系统集成阶段验证。
- [x] 验证 GPUI async executor 与 runtime channel 可安全通信；bounded command/reply、revision 过滤、receiver close 和 shutdown acknowledgement 已通过 contract test 与 macOS release `.app` smoke。
- [ ] 验证辅助功能树满足设置表单的基础要求。
- [x] 记录首次打开、空闲 CPU、RSS 和二进制增量；`docs/benchmark/data/gpui-settings-macos-248a770-*.csv` 保存原始样本，方法、环境和限制见 `docs/phase-0/gpui-settings-spike.md`。
- [x] 安装并固定 macOS Metal Toolchain，验证 GPUI 默认预编译 shader 路径；`runtime_shaders` 不作为发布配置。
- [ ] 将 macOS spike 打包为最小 `.app`，验证 bundle id、菜单、激活、关闭和辅助功能树可被系统识别。
  - 状态：Bundle ID `com.ayangweb.bongo-cat`、菜单、激活、关闭/重开和退出已通过；GPUI 内容节点无法被辅助功能 API 识别，因此保持未完成。
- [ ] 生成 Windows spike 可执行文件，验证 MSVC、Windows SDK、D3D shader 工具和 manifest 前置条件。
- [x] 跟踪 `block 0.1.6`、`proc-macro-error2 2.0.1` future-incompatibility；`docs/phase-0/future-incompatibility.md` 明确当前图只接受用于 spike，macOS 输入产品边界迁移到 `objc2-core-graphics`，GPUI 图必须在进入产品 workspace 前通过升级或审计 patch 消除两条 warning。
- [x] 若存在发布阻塞，提交 GPUI go/no-go ADR；备选只评估 Iced。ADR-0009 记录 GPUI 0.2.2 的 AX gate、Iced 0.14.0 初步检查和解除条件；当前阻塞仍未解除。

### 1.6 原生 Overlay spike

- 状态（2026-08-28）：`spikes/overlay-lifecycle/` 已建立无平台依赖的生命周期 contract probe，显示/隐藏/重开、乱序 shutdown 拒绝、关闭后禁止重开和 100 次创建/销毁测试通过。它只固定平台 wrapper 必须遵守的状态迁移与 shutdown 顺序，不代表双平台窗口、透明合成或 GPU 已完成；详见 `docs/phase-0/overlay-lifecycle-spike.md`。
- 状态（2026-08-29）：macOS `spikes/gpui-overlay-macos/` 已在 Apple Silicon 实机验证 GPUI 设置窗口与独立 `NSPanel` + `CAMetalLayer` 共存、显示/隐藏/重显示、跨 Space 配置、鼠标穿透和正常退出；`.app` Bundle ID `com.ayangweb.bongo-cat` 与 ad-hoc strict codesign 通过。renderer 已从透明 clear 推进到 Rust 创建 Metal pipeline/vertex buffer、提交非空预乘 alpha draw，并在 release 100-cycle 的每轮等待 GPU 完成、回读非透明中心像素及验证 `rgb <= alpha`；本机结果为 `non_empty_frames=100`、AppKit windows `0 -> 0`、Rust owner `0 -> 0`、`clean_shutdown=true`。显式禁用无用途的 `NSPanel` 动画后，100-cycle `leaks --atExit` 不再出现 `_NSWindowTransformAnimation`、overlay 或 Metal retain stack，physical footprint 从 `38.4M` 降到 `16.3M`；剩余 18,816 bytes 均来自系统 XPC 常驻 stack。该合成几何尚不代表 Cubism texture/order/mask 完成；受控 drawable unavailable 也已验证设置窗口 degraded 与 quit 前 owner 释放。
- 状态（2026-08-29）：`spikes/overlay-windows/` 已实现线程限定的 Win32 popup 与独立 D3D11/DXGI/DirectComposition premultiplied-alpha renderer，并由同一 GPUI coexistence executable 驱动。renderer 已包含 Rust 顶点、运行时 HLSL 编译、shader/input layout/vertex buffer/blend/rasterizer state、非空 draw、staging readback 和 DPI-aware `ResizeBuffers`；`CULL_NONE` 修复后的 hardware D3D11 连续帧、readback、resize 与 100-cycle 已在 push/PR runner 通过。本批新增 DXGI device-lost/surface-unavailable 分类，以及运行中故障后的 owner 释放、有限退避和完整重建；真实驱动 device loss 仍待实机。合成几何尚不代表 Live2D texture/order/mask 或 GPU/线程专项泄漏完成。

- [x] 在 GPUI 应用生命周期内创建独立主猫原生窗口。
- [x] Windows 从 Rust 获得 HWND，完成透明 D3D11 clear/present。
  - 状态（2026-08-29）：两次 `windows-latest` 运行均使用 hardware D3D11 完成两次透明 composition swapchain clear/present，并验证正常退出。
- [x] macOS 从 Rust/objc2 创建 NSPanel + CAMetalLayer，完成透明 Metal clear/present。
- [x] 验证 overlay 不嵌入/替换 GPUI renderer 或依赖其私有对象。
- [x] 验证 GPUI 设置窗口与 overlay 同时存在，事件循环不冲突。
  - macOS 实机与 Windows push/PR runner 已分别通过。
- [ ] 验证 overlay 可置顶、穿透、显示/隐藏、拖动和缩放。
  - 状态（2026-08-29）：双平台置顶、穿透和显示/隐藏已通过；双平台 programmatic resize 和 backing-scale/swapchain 重建已实现。Windows drag 模式通过移除 `WS_EX_TRANSPARENT` 并让 `WM_NCHITTEST` 返回 `HTCAPTION` 进入系统拖动循环，macOS drag 模式通过 `movableByWindowBackground` 与 mouse-ignore 状态进入 AppKit 拖动循环；受控 smoke 验证两平台 click-through -> drag -> click-through、窗口位置 `24x18` 变化及 renderer 重建后重新应用。macOS 每帧还通过 `convertRectToBacking` 校正 drawable size，受控 stale-size smoke 已从 `1x1` 恢复到当前 Retina 尺寸并继续非空绘制；物理鼠标完整手势、外接显示器及 DPI/Retina 热切换仍待完成，因此保持未勾选。
- [ ] 连续创建/销毁 overlay 100 次，无窗口、swapchain、layer 或线程泄漏。
  - 状态（2026-08-29）：Windows warm-up 后 100 次完整 window/GPU owner 循环已通过，process handle 为 `172 -> 172`；macOS release 100-cycle 在普通与 NSZombie 模式均通过，AppKit windows 与 Rust owner 都为 `0 -> 0`。1/10/100-cycle `leaks` 基线定位并消除了 AppKit transform animation retain cycle；修复后只剩系统 XPC 常驻 stack。macOS 当前又以三个等长 100-cycle batch 区分 driver pool 初始化和持续增长，后两批连续两次复现 thread `9 -> 9`、Metal allocation `393216 -> 393216`。Windows GPU/线程与 macOS Instruments driver resource 仍待完成，因此保持未勾选。
- [ ] 验证退出顺序：frame source -> renderer -> GPU -> overlay -> GPUI。
  - 状态（2026-08-29）：GPUI executor 上的 60 Hz 定时 frame source 已连续驱动双平台 renderer，并在退出时通过停止确认后才释放 renderer/GPU/window；macOS 本机与 Windows hardware D3D11 runner 均已验证连续帧、resize、hide/show 和有序退出。生产 display-linked frame source 与 runtime 尚未接入，因此保持未完成。
- [x] 写明 GPUI/AppKit/Win32 主线程所有权、overlay 创建线程和跨线程 command 不变量。
- [ ] 注入 renderer 初始化失败、drawable/swapchain unavailable 和 device lost，设置窗口仍可打开并显示诊断。
  - 状态（2026-08-29）：Windows push/PR runner 已通过 renderer 初始化失败与 GPUI degraded 状态；macOS push/PR runner 已通过受控 drawable unavailable、GPUI degraded、正常 quit 与 owner 释放。本批又实现运行中故障的双平台恢复状态机：旧 owner 先释放，有限退避后完整重建，GPUI 显示 recovering/recovered，macOS 本机注入结果为 `frames=87 failures=1 recoveries=1`；Windows runner 已接入同等 smoke。Windows 真实 swapchain unavailable 与双平台真实驱动 device loss 仍待完成。

### 1.7 输入可靠性 spike

- 状态（2026-08-28）：`spikes/input-state/` 已建立纯 Rust pressed-set contract，覆盖正常 down/up、重复 down、Reconcile、Reset、issue #47 的丢失 release 恢复、可靠事件的序列跳号/重复/乱序诊断，以及 `250 ms` 校正调度和连续 `2` 次缺失确认的误判保护；计数器不记录具体键值。平台采集、runtime 接入和管理员/权限场景仍待实机验证，详见 `docs/phase-0/input-state-spike.md`。
- 状态（2026-08-29）：`spikes/input-windows/` 已冻结 `RI_KEY_BREAK`、E0/E1、左右修饰键、PrintScreen、未知 scan code 保留和安全 `RAWINPUT` 字节解析 contract；已实现隐藏顶层 HWND、Raw Input 注册、`WM_INPUT` 读取、周期校正、计数诊断和生命周期 Reset。commit `32bc9a37efd201a788511ee86e7350c6a5058ab3` 的 push Windows job 已通过 WTS 注册/注销以及 session/power 受控消息 smoke。真实设备样本、热插拔、锁屏/睡眠和丢失 release 校正仍待完成，详见 `docs/phase-0/input-windows-spike.md`。
- 状态（2026-08-29）：`spikes/input-macos/` 已建立 macOS 权限/tap 生命周期 contract、listen-only `CGEventTap` 专用 run loop、panic-isolated callback、固定容量 callback queue 和候选 pressed-set 周期校正；当前 macOS 会话累计完成 105 次创建运行停止 smoke，timeout/user-disable 各 20 次受控故障恢复，公开 NSWorkspace lifecycle Reset、成对注销和 callback close gate 也已验证。本批先以 private `CGEventSource` 投递 keyboard down/up，真实 callback 捕获后在 consumer 边界故意丢弃一次 KeyUp，再由 `CGEventSourceKeyState` 两次缺失确认释放候选，20/20 cycle 均无残留；随后为 mouse down/up 保留 0–31 号 button identity，并将 `CGEventSourceButtonState` 校正接入同一周期和 Reset 路径。物理输入/系统自然丢事件、真实 modifier/鼠标字段、系统自然 timeout、TCC 拒绝/撤销和真实锁屏/睡眠/快速用户切换恢复仍未完成，详见 `docs/phase-0/input-macos-spike.md`。

- [ ] Windows 实现 RegisterRawInputDevices 和 WM_INPUT 最小路径。
  - 状态（2026-08-29）：已实现注册、读取、注销和自动退出路径，并通过 Windows target 交叉 check/Clippy 以及 `windows-latest` 注册/退出 smoke。本批新增 `SendInput` scan-code down/up -> 系统 `WM_INPUT` callback -> raw decode 的闭环命令并接入 Windows runner；物理设备样本仍待实机，因此保持未勾选。
- [x] 冻结 scan code、extended flag、左右修饰键和 RI_KEY_BREAK mapping contract；Win32 packet 接入仍待实机。
- [x] 建立平台无关 pressed set contract；Windows `GetAsyncKeyState` 校正仍待实机接入。
  - 状态（2026-08-29）：Windows spike 已增加 physical-key 到 virtual-key 查询计划、input desktop guard、只查询本地 pressed candidates 的 `GetAsyncKeyState` adapter，以及未知键触发 Reset 的 contract；commit `09773f0066f526799eb702fb1759049d0de9732f` 的 push/PR Windows jobs 已通过 `250 ms` scheduler 和连续 `2` 次缺失确认 smoke，真实丢失 release 恢复仍待完成。
- [x] 定义校正频率、连续确认次数和误判保护。
  - 状态（2026-08-28）：`spikes/input-state/` 固定默认 `250 ms` 周期、连续 `2` 次缺失确认、单调时钟回退拒绝和 reset/up/down 清理待确认状态；平台 adapter 的周期调度和 runtime 消费仍待产品实现。
- [ ] 在锁屏、睡眠、设备移除和服务重启时发送 Reset。
  - 状态（2026-08-29）：Windows spike 已注册 `RIDEV_DEVNOTIFY` 和 WTS current-session notification，并在设备移除、服务停止、lock/unlock、connect/disconnect、suspend/resume 时 Reset。commit `32bc9a37efd201a788511ee86e7350c6a5058ab3` 的 push run `33234259414`、job `99052333561` 已通过 4 条受控 lifecycle 消息、4 个候选释放和 WTS 注销断言；真实设备拔插、Win+L 和睡眠/唤醒仍待完成。
  - 状态（2026-08-29）：macOS spike 已通过公开 NSWorkspace sleep/wake/session 通知的受控 callback smoke，四类通知合并形成 Reset 并释放缺失 KeyUp 候选；真实锁屏、睡眠/唤醒和快速用户切换仍待实机完成，因此本项保持未勾选。
- [ ] 实测 PixPin Ctrl+Alt+A，丢失 release 时不得永久高亮。
  - [x] contract probe 已覆盖丢失 A-up 后通过 Reconcile 清除残留；尚未在 Windows callback 上实测。
  - 状态（2026-08-29）：本批新增系统合成 A down/up，consumer 故意丢弃已捕获 release，再由两次 `GetAsyncKeyState` 快照清除 candidate 的 Windows runner smoke；它验证 callback 到 reconcile 的实现闭环，但不替代 PixPin/物理键实测。
- [ ] 实测 Win+L、PrintScreen、UAC 和管理员/非管理员场景。
- [ ] 进行 10 分钟高速鼠标 + 键盘压力测试，edge 丢失计数必须为 0。
- [ ] macOS 实现 CGEventTap、权限拒绝/授予和 tap 自动重启。
  - 状态（2026-08-29）：真实 listen-only tap 与受控 timeout/user-disable Reset + re-enable 已通过；TCC 拒绝、撤销、重新授予和系统自然 timeout 矩阵仍待实机完成。
- [ ] macOS 使用 CGEventSourceKeyState/CGEventSourceButtonState 校正 pressed state。
  - 状态（2026-08-29）：run-loop consumer 已从 KeyDown/Up、`FlagsChanged`、MouseDown/Up 和 Reset 分别维护 key/button 候选集合，每 `250 ms` 使用 `CGEventSourceKeyState`/`CGEventSourceButtonState` 校正，连续 `2` 次缺失才释放。keyboard callback release 受控丢弃的 20/20 cycle 均由校正清零；mouse button 0–31 身份、候选查询和两次缺失 contract 已通过，`--button-state 0` 实机系统查询也通过。物理输入/系统自然丢事件与产品 runtime pressed state 接入仍待完成，因此保持未勾选。
- [x] 连续 start/stop/restart 输入服务 100 次，无资源泄漏。
  - 验收证据（2026-08-29）：release probe 现在严格校验每个 cycle 的 enabled 恢复、callback panic、queue overflow/closed event 和 NSWorkspace observer 成对注销，任一失败均非零退出。`leaks --atExit` 的 100-cycle 报告 `0 leaks for 0 total leaked bytes`、physical footprint `5232K`，`NSZombieEnabled=YES` 另完成 100/100；两次均为 `queue_overflows=0 callback_panics=0 clean_shutdown=true`，且每个 tap worker 都已 join。timeout/user-disable 各 20 次恢复已另行通过；权限故障循环留在 TCC 矩阵，不阻塞本 restart owner 子项。
- [x] 记录 monio 对照结果，但不引入生产依赖；`docs/phase-0/monio-comparison.md` 基于 commit `d1766e0dcd20dea0435be16cd80adaa749b86e30` 记录 Raw Input、channel、reconciliation、Reset、callback 和许可证差异。
- [ ] 为 captured、reconciled、reset、duplicate、overflow 分别维护计数器，不记录具体键值。
  - 状态（2026-08-29）：macOS spike 已输出事件类型、reconciled release、Reset 次数/释放数、duplicate/unmatched 和 queue overflow/recovery 数量，且不输出具体键值；Windows 与产品 runtime 的统一诊断 snapshot 仍待完成。
- [x] 验证输入 callback panic 隔离、队列关闭和应用退出竞态，不允许 callback 访问已析构 runtime。
  - 验收证据：macOS event-tap 与 workspace callback 共用 autorelease/panic boundary，故意 panic 的测试确认 unwind 不越过 callback；固定队列 close 后拒绝新事件并可 drain；受控生命周期 smoke 在 callback gate 关闭后触发迟到通知，只增加 ignored 计数。observer token 成对注销，callback 只持有 queue/atomic，不捕获 runtime owner。产品 runtime 接入后仍须重跑对等 shutdown 测试。

### 1.8 Cubism/Renderer spike

- [ ] 确认 Cubism SDK/Core 版本、来源、再分发条款和 attribution 要求。
  - 状态（2026-08-29）：`docs/phase-0/cubism-sdk-source-and-license.md` 已固定 Native R5/Core `06.00.0001`、官方 tag/commit、下载入口和 RedistributableFiles 边界。BongoCat 很可能属于需预先批准和单独协议的 Expandable Application；Framework 到 MIT Rust 实现的许可边界、最终 attribution 和 Live2D 书面授权仍未完成，因此保持未勾选。
- [ ] 建立目标架构二进制清单、hash 和可重复获取流程。
  - 状态（2026-08-29）：产品目标中的 R5 Windows x64 与 macOS arm64/x64 artifact 路径已形成清单，Windows ARM64 已明确无 desktop artifact，i686 已排除；离线 ZIP 检查流程已定义。合法下载 ZIP 的 archive/file hash、双人复核和真实 ABI 加载仍待完成。
- [ ] 验证 Rust sys binding 加载 moc、创建 model 并读取 drawable 数据。
  - 状态（2026-08-29）：已用 hash 固定的 legacy Web Core `5.1.0` 为三个预置 moc 建立可重复 baseline，记录 MOC enum、consistency、parameter/part/drawable count 和 canvas，并接入 CI 漂移检查；这只是未来 R5 wrapper 的对照，尚未完成 Native R5 sys binding 或 drawable 数据读取。
- [ ] 包装 Moc/Model 生命周期，证明 Model 不会比 Moc 存活更久。
- [x] 用 Rust 解析三个预置 model3 和所有关联资源。
  - 验收证据：build `7ee8acd5f2a3d4dcb7a1dbc36623cbe497aeae49` 的 push run `33238204993` 与 PR run `33238206415` 各 16 jobs 全绿。`spikes/model-package/` 强类型解析 model3 v3，验证 moc、纹理、display info、expression、motion/audio、可选 physics/pose/user data 与 companion images，完整包索引冻结在 `shared/fixtures/model-fixtures/preset-model3-index.json`。三个预置包与六类异常 fixture、跨根 symlink、目录深度均有 Rust 测试；详见 `docs/phase-0/model-package-spike.md`。本项不包含 Core/model creation、动作求值或 renderer。
- [ ] Windows D3D11 绘制预置模型的 texture/order/alpha/mask。
  - 状态（2026-08-29）：Windows overlay 已实现合成几何的 D3D11 shader pipeline、预乘 alpha draw 和 staging texture 像素验证；预置模型 texture、drawable order 和 mask 尚未接入，因此保持未完成。
- [ ] macOS Metal 绘制同一模型的 texture/order/alpha/mask。
  - 状态（2026-08-29）：macOS overlay 已完成合成几何的真实 Metal pipeline、预乘 alpha draw 和 100 帧 GPU readback，不再只是透明 clear/present；预置模型 texture、drawable order 和 mask 尚未接入，因此保持未完成。
- [ ] 验证 motion、expression、physics、pose 至少各一个真实样本。
- [ ] 验证模型切换/销毁 100 次，无 CPU/GPU 资源增长。
- [ ] 记录与 easy-live2d 的差异和必须兼容项。
- [ ] 若纯 Rust Framework 逻辑不可行，提交 go/no-go ADR；不得静默加入 C++ 业务桥。
- [x] 建立 Cubism Framework 行为来源清单，逐项说明 motion、expression、physics、pose 的 Rust 实现依据和许可边界。
  - 验收证据：`docs/phase-0/cubism-framework-behavior-sources.md` 固定 R5 tree、16 个关键 Framework blob、双平台 sample owner、行为 oracle 与禁止直接翻译的许可边界；离线 SDK inspector 会验证这些 blob。Live2D 对独立 Rust 实现和生成 binding 发布的书面许可仍是 P0 阻塞，不因本项勾选而视为解决。
- [ ] 对 raw binding 生成流程固定 header、生成器版本和输出审阅方式，禁止手改生成代码后失去可重复性。
  - 状态（2026-08-29）：`tools/cubism-bindgen/` 已精确锁定最新稳定版 `bindgen 0.72.1`，固定当前 R5 可用且属于产品矩阵的 Windows x64 与 macOS arm64/x64、`csm*` 白名单、Rust 1.85/edition 2024、配置/output hash 和仓库外生成门禁；自有合成 header 的三 target golden、7 项安全测试与 CI 漂移检查已完成。工具明确拒绝 i686 与当前无 Core 的 Windows ARM64。合法 R5 header 的真实 SHA、授权后的生成/双人审阅以及 Core compile/link/ABI smoke 仍待完成，因此保持未勾选。

### 1.9 Phase 0 退出门槛

- [ ] GPUI 设置窗口与原生 overlay 可同时运行、关闭和重开。
- [ ] Windows/macOS 至少一个预置模型完成输入到原生绘制闭环。
- [ ] Windows issue #47 复现用例不产生残留键。
- [ ] macOS 权限拒绝、授予、重启和恢复路径可解释。
- [ ] 三个预置模型的兼容差异已知且没有未决 P0 阻塞。
- [ ] Cubism 发布授权、二进制来源和打包方式有书面结论。
- [ ] 形成 Phase 0 报告，明确 GO、GO WITH CONDITIONS 或 NO-GO。
- [ ] GO WITH CONDITIONS 必须为每个条件指定 owner、截止阶段和失败时的回退决策。

## 2. Phase 1：Rust 工程骨架

目标：建立可持续开发、测试和发布的全 Rust 工程。

### 2.1 目标目录

- [ ] 根 Cargo workspace 仅包含新 Rust 应用和 crate。
- [ ] 创建 bongocat-app：入口、服务装配和 shutdown。
- [ ] 创建 bongocat-runtime：状态、输入语义、动画和 command。
- [ ] 创建 bongocat-config：环境隔离、schema、验证和原子存储。
- [ ] 创建 bongocat-model：模型包、导入和资源索引。
- [ ] 创建 bongocat-live2d：Cubism safe wrapper 和模型求值。
- [ ] 创建 bongocat-render：render snapshot 和 renderer contract。
- [ ] 创建 bongocat-ui：GPUI 页面和 design system。
- [ ] 创建 bongocat-platform：Windows/macOS 系统服务。
- [ ] 创建 shared/config、behavior、fixtures、resources。
- [ ] 避免空 crate；没有独立依赖/测试价值时先作为模块。

### 2.2 工程质量

- [ ] 固定 stable Rust toolchain、target 和必要 components。
- [ ] 在 workspace manifest 声明 `rust-version`，CI 验证最低版本和当前 stable，不依赖开发机偶然安装的 nightly。
- [ ] 禁止应用依赖未固定 git branch，提交 Cargo.lock。
- [ ] 平台依赖使用 target-specific dependency，Windows feature 不进入 macOS，macOS framework 不进入 Windows。
- [ ] 审查 Cargo feature union，禁止测试/诊断/运行时 shader feature 意外进入 release 产物。
- [ ] 业务、配置、模型和 UI crate 使用 forbid unsafe_code。
- [ ] 平台 unsafe wrapper 写明线程、指针、所有权和析构不变量。
- [ ] 配置 rustfmt、Clippy -D warnings、cargo test 和许可证检查。
- [ ] 配置 `cargo deny`/等价检查：license、advisory、banned source、重复高风险依赖和 unknown registry。
- [ ] 配置 panic hook 和 release 可诊断退出。
- [ ] 定义线程、任务、channel、窗口和 GPU object owner。
- [ ] 建立结构化日志字段和用户路径脱敏规则。
- [ ] 提供开发/测试所需 Cubism 二进制的可验证安装说明。
- [ ] 构建脚本默认不联网；外部 SDK、shader compiler 和生成器必须先由显式 bootstrap 步骤准备。
- [ ] 定义 debug、release、profiling 三种 profile，profiling 产物不得误发布。

### 2.3 CI

- [x] Windows：format、Clippy、unit test、release check；GPUI settings/overlay spike 已由 GitHub `windows-latest` 执行。
  - 验收证据：commit `221f5483976b64b7cbf6c5818ee5714ad47de479`，push run `33182146480` 与 pull request run `33182148815` 均成功；不代表 Windows 字体、IME、DPI、辅助功能或图形实机验收完成。
- [x] macOS：format、Clippy、unit test、release check；GPUI settings/overlay spike 均纳入 `macos-spikes` job。
- [x] 缓存 key 包含所有 `Cargo.lock`/`Cargo.toml` 和 Rust toolchain hash；Linux contract 与 macOS GPUI jobs 均使用该 key。
- [ ] CI 不下载未经版本/hash 固定的 Cubism 二进制。
- [ ] GPU、权限、签名测试分离为实机/nightly job。
- [ ] 共享 crate 增加 Linux cargo check，但不生成首发安装包。
- [ ] CI 校验 fixture JSON Schema、跨文件一致性、本地化 key 和生成文件是否漂移。
  - [x] 已接入 Draft 2020-12 schema、fixture 跨文件一致性和五种历史 locale 的 key/类型/占位符校验。
  - [x] Cubism raw binding 工具已用自有合成 header 对三个当前可绑定 target 执行 deterministic golden 漂移检查；真实 R5 bindings 因许可门禁不进入 CI。
  - [ ] 生成文件漂移校验仍待 Native 资源生成链建立后补齐。
- [ ] 保存失败测试日志、截图和 renderer validation 输出，同时执行路径/按键隐私清理。
- [ ] 构建产物记录 source commit、Cargo.lock hash、toolchain、target 和 feature set。

### 2.4 Phase 1 退出门槛

- [ ] Windows/macOS debug/release 骨架均可构建。
- [ ] GPUI 空设置窗口可打开，overlay 可显示测试帧。
- [ ] CI 在干净环境复现构建。
- [ ] 应用可正常退出，所有 worker 有明确 join 结果。
- [ ] Windows/macOS release dependency tree 与批准清单一致，无意外 Tauri/WebView/JavaScript runtime。

## 3. Phase 2：Runtime、输入和配置

### 3.1 Runtime

- [ ] 定义 AppCommand、InputEvent、RuntimeSnapshot、RenderSnapshot。
- [ ] 单一 runtime owner 管理可变业务状态。
- [ ] key/button edge 和 command 使用可靠有序队列。
- [ ] 为每个可靠队列定义容量、生产者、消费者、满载策略和关闭语义，不使用无界队列逃避背压设计。
  - 状态（2026-08-28）：`spikes/input-queue/` 已验证固定容量 FIFO、满载返回原事件、关闭 drain 和 latest-value 槽位；`spikes/runtime-contract/` 进一步验证固定容量 command queue、Condvar 唤醒、溢出 Reset、worker drain 和 join 报告；runtime 的实际容量与产品 channel 选型仍待产品 crate。
- [ ] edge/command 携带单调 sequence id，诊断可发现乱序、重复和丢失但不记录具体键值。
  - 状态（2026-08-28）：`spikes/input-state/` 已验证可靠输入事件的重复/乱序忽略与跳号安全 reset；`spikes/runtime-contract/` 已验证 typed command sequence、跳号前 `WorkerRecovery` reset、重复/过期 sequence 丢弃和诊断计数；平台 producer、输入事件 sequence 与产品 runtime 接入仍待产品 crate。
- [ ] cursor/gamepad axis 使用 latest-value 合并通道。
- [ ] 队列溢出必须计数、记录并触发安全恢复。
  - 状态（2026-08-28）：`spikes/input-queue/` 的 `push_with_overflow_reset` 已固定溢出返回原事件、清空不可信缓存、注入 `Reset` 并记录恢复/丢弃计数；`spikes/runtime-contract/` 已将同一策略应用到 typed command queue 并通过 worker snapshot 暴露诊断；runtime producer、实际容量和输入/command sequence 仍待产品实现。
- [ ] 动画、长按和延迟统一使用 Instant。
- [ ] 实现可注入 clock 和确定性 tick。
- [ ] 实现 starting、ready、degraded、stopping、stopped 状态。
- [ ] 实现 shutdown drain、超时和错误聚合。
- [ ] command 定义幂等性和重复提交语义；有副作用的长操作使用 operation id 去重。
- [ ] runtime tick 设置工作预算，模型解析、磁盘、音频初始化和 GPU 上传不得阻塞实时队列。
  - 状态（2026-08-28）：`spikes/runtime-contract/` 已通过 14 项测试，覆盖状态机、单调 tick、operation 去重、typed bounded worker、递增 snapshot revision、sequence gap/duplicate、overflow Reset、shutdown drain/timeout、command error 和 panic/join 诊断；产品 runtime 的输入、模型、配置服务、工作预算和真实线程 owner 仍待 Phase 1/2。

### 3.2 输入语义

- [ ] 分离 PhysicalKey、布局字符和显示名称。
- [ ] 定义左右手、组合键、repeat、单键模式和自动释放语义。
- [ ] 定义鼠标按钮、滚轮、移动和拖动语义。
- [ ] 定义手柄按钮、axis、trigger、dead-zone 和断开复位。
- [ ] 每个 pressed key 记录来源、按下时间和最后校正时间。
- [ ] 每个 pressed key 最终经 KeyUp、reconcile 或 Reset 释放。
- [ ] 实现 fixture runner 和规范化 snapshot 比较。

### 3.3 Windows 输入

- [ ] 独立消息窗口接收 Raw Input，不占用 renderer 热路径。
- [ ] 注册 keyboard/mouse 并处理设备热插拔。
- [ ] 完整处理 scan code、E0/E1、左右修饰和特殊键。
- [ ] 去重 Raw Input、可选 hook 和合成事件。
- [ ] 对 pressed set 执行 GetAsyncKeyState 校正。
- [ ] 处理 power、session lock/unlock 和 input desktop 变化。
- [ ] 管理员权限差异产生诊断，但默认不要求提权。
- [ ] RegisterHotKey 冲突返回错误并保持旧绑定。
- [ ] issue #47 固定为发布回归项。
- [ ] 明确 Raw Input scan code 到可查询 virtual-key 的映射，无法可靠校正的键必须有 Reset/保险策略和诊断。
- [ ] 处理输入设备提供伪造、重复或异常长度 Raw Input 数据的边界，不信任设备名称和 handle 生命周期。

### 3.4 macOS 输入

- [ ] 创建 listen-only CGEventTap 和专用 run loop/source。
- [ ] 映射 keycode、flags changed 和左右修饰键。
- [ ] 处理 tap timeout、user disable、权限变化和自动重建。
- [ ] 通过 CGEventSourceKeyState 校正 pressed set。
- [ ] 权限拒绝时进入 degraded，不产生重试风暴。
- [ ] 锁屏、睡眠、快速用户切换和 tap 重启发送 Reset。
- [ ] GameController 设备和 profile 映射进入统一事件。
- [ ] event tap callback 使用 autorelease pool/panic boundary，run loop 停止后不再触达已释放 producer。
- [ ] 明确辅助功能与 Input Monitoring 各自真正需要的能力，避免请求不必要的 TCC 权限。

### 3.5 配置 v1

- 状态（2026-08-29）：`spikes/config-store/` 已建立 typed NativeConfig、Bundle ID、Development/Production 隔离目录、snake_case 序列化、schema 校验、原子 commit probe、expected revision、OS writer lock contract、中断提交恢复 contract 和双平台真实 path resolver。Windows 首次 run 暴露只读 handle flush 的 `AccessDenied` 后已修复；push run `33251278193`、job `99097261951` 通过 17 项 unit test 和强制终止持锁子进程的 integration test，验证 `%APPDATA%` 路径、kernel lock 释放、临时配置归档和当前配置保留。备份策略和 GPUI command 边界仍待产品 crate 阶段完成，详见 `docs/phase-0/config-store-spike.md`。

- [ ] 定义带 `schema_version` 的 Rust 配置结构和 JSON schema，JSON key 使用 `snake_case`。
- [ ] 区分用户配置、运行时状态和诊断数据。
- [ ] 为字段定义范围、默认值和跨字段约束。
- [x] 在 spike 中实现不可变 `BuildEnvironment::{Development, Production}`；未知或缺失环境的打包构建失败仍待产品构建链验证。
- [x] Windows 使用 `%APPDATA%\BongoCat\<environment>\` 数据根。
- [x] macOS 使用 `Application Support/com.ayangweb.bongo-cat/<environment>/` 数据根。
  - 双平台 target-specific resolver test 已通过。
- [x] 两个环境的 `config.json`、`state.json`、`models/`、`backups/`、`logs/` 和 `locks/` 相对结构一致；spike 测试逐项比较相对路径。
- [ ] 环境不能由 CLI、进程环境变量或设置项在运行时切换，也不能 fallback 到另一环境。
- [x] 在 spike 中实现同目录临时文件、flush、原子替换、提交后验证和上一份有效配置备份；双平台 OS file lock 与强制进程终止恢复已通过。
- [x] 在 spike 中拒绝损坏配置并保留原始文件；中断提交恢复会保守提升有效临时文件并归档无效/陈旧副本，隔离备份保留策略、默认恢复和 GPUI 用户诊断仍未完成。
- [ ] 配置写入去抖，退出前强制 flush。
- [ ] GPUI 只通过 typed command 获取 snapshot 和提交 patch。
- [x] 在 spike 中以包含环境目录的持久 `locks/config.writer.lock` 拒绝并发 writer，并通过 OS advisory lock 在 guard drop 后允许重试。
- [x] 强制终止持锁进程后由内核释放 writer lock，下一进程可恢复已 flush 的临时配置且不覆盖当前配置。
  - 验收证据（2026-08-29）：macOS 本机与 Windows push run `33251278193`、job `99097261951` 均通过；平台文件权限仍待产品 crate。
- [ ] 新配置文件和备份使用最小用户权限，不继承过宽 ACL/文件 mode。
- [x] 在 spike 中以稳定 NativeConfig revision 拒绝过期 writer，避免静默覆盖较新的用户修改；GPUI snapshot/command 携带 revision 仍待产品 crate。

### 3.6 Phase 2 退出门槛

- [ ] 输入 fixture 在双平台产生相同规范化状态。
- [ ] 10 分钟压力测试无 edge 丢失和永久残留。
- [ ] 100 次输入服务 restart 无资源泄漏。
- [ ] 配置并发更新、崩溃中断和损坏恢复测试通过。
- [ ] queue overflow、runtime panic、writer lock 冲突和 shutdown timeout 均有确定的 degraded/recovery 结果。

## 4. Phase 3：原生 Overlay 与 Renderer

### 4.1 窗口契约

- [ ] 定义 create/show/hide/move/resize/scale/opacity/pass-through/topmost。
- [ ] contract 使用逻辑坐标，平台 adapter 负责物理像素。
- [ ] 保存显示器稳定标识、归一化位置和 fallback。
- [ ] 显示器移除后将 overlay 移回可见工作区。
- [ ] renderer 不直接读取配置，窗口命令由 runtime 协调。
- [ ] GPUI 设置窗口与 overlay 的关闭语义分离。

### 4.2 Windows D3D11

- [ ] 创建透明、无边框、跳过任务栏的 Win32 popup。
- [ ] 使用 Per-Monitor-V2，处理 WM_DPICHANGED/WM_DISPLAYCHANGE。
- [ ] 实现 D3D11 + DXGI + DirectComposition/DWM 预乘 alpha。
- [ ] 配置变化时切换 HWND_TOPMOST/HWND_NOTOPMOST，禁止帧轮询。
- [ ] 切换 click-through 并验证拖动模式。
- [ ] 处理 device lost、resize、休眠和 GPU 切换。
- [ ] D3D11 debug layer 无未处理 warning/error。

### 4.3 macOS Metal

- [ ] 在 GPUI/AppKit 主线程创建 nonactivating NSPanel。
- [ ] 配置透明、无标题、阴影、鼠标穿透和层级。
- [ ] 配置 Spaces 和 full-screen auxiliary 行为。
- [ ] 使用 CAMetalLayer，按 backingScaleFactor 更新 drawable size。
  - 状态（2026-08-29）：Phase 0 wrapper 已在每帧从 content view backing 坐标同步 `drawableSize`，并通过受控陈旧尺寸恢复与 programmatic resize；产品 platform/renderer 尚未建立，因此保持未勾选。
- [ ] 处理 display change、Retina 切换、睡眠和 drawable unavailable。
  - 状态（2026-08-29）：受控 drawable unavailable 与逐帧 backing-size 恢复已通过；真实 display/Retina 热切换、display removal 和睡眠仍待实机。
- [ ] 设置窗口激活不破坏 overlay 层级和鼠标行为。
- [ ] Metal validation 无资源/生命周期错误。

### 4.4 RenderSnapshot

- [ ] snapshot 只含不可变绘制数据和稳定资源 id。
- [ ] 定义 CPU model evaluation 与 GPU upload 所有权边界。
- [ ] 双缓冲/latest snapshot，renderer 不阻塞 runtime。
- [ ] 支持目标 FPS、不可见暂停/降频和刷新率变化。
- [ ] 首帧前不出现黑框或不透明闪烁。
- [ ] shutdown 先停 frame source，再释放 GPU/window。
- [ ] 明确 sRGB/linear、预乘 alpha 和 texture color space，避免两平台颜色或边缘混合语义漂移。
- [ ] present 失败、窗口隐藏和 drawable unavailable 时限流，不产生 busy loop 或日志风暴。

### 4.5 Phase 3 退出门槛

- [ ] 双平台透明、置顶、穿透、缩放和多显示器通过。
- [ ] resize/scale/显示器切换 30 分钟无 device loss 死循环。
- [ ] 窗口创建/销毁 100 次无资源增长。
- [ ] 空场景 frame time 和空闲 CPU 基线已记录。

## 5. Phase 4：Live2D、动画和音效

### 5.1 模型包

- [ ] 解析 model3 并规范化相对路径。
- [ ] 验证 moc、texture、motion、expression、physics、pose、cdi 和音频。
- [ ] 拒绝路径穿越、符号链接逃逸、绝对路径和覆盖安装资源。
- [ ] 限制模型总大小、单文件大小、纹理尺寸和 JSON 深度。
- [ ] 资源缺失/损坏返回具体错误，不使应用整体退出。
- [ ] 建立预置只读索引和用户模型可写索引。

### 5.2 Cubism safe layer

- [ ] 封装 Core version、logging、Moc consistency 和 Model creation。
- [ ] 用 Rust owner 保证 Moc、Model 和 buffer 析构顺序。
- [ ] 校验 parameter/part/drawable id、index 和范围。
- [ ] 模型切换使用 prepare/commit/rollback。
- [ ] 加载失败保留当前可用模型。
- [ ] FFI 错误映射为稳定 Rust error code。

### 5.3 动作与状态

- [ ] 实现 parameter 默认值、保存/恢复和 clamp。
- [ ] 实现 motion curve、fade、priority 和 completion。
- [ ] 实现 expression 混合和互斥/叠加语义。
- [ ] 实现 physics、pose、eye blink、breath 等实际需求。
- [ ] 实现键盘、鼠标、手柄到参数/动作/表情映射。
- [ ] 实现镜像、鼠标镜像和坐标归一化。
- [ ] 随机行为支持测试 seed。
- [ ] 逐项记录与旧版的可接受差异。

### 5.4 GPU 绘制

- [ ] 实现 drawable order、visibility、opacity 和 dynamic flags。
- [ ] 实现 normal/additive/multiplicative blend。
- [ ] 实现 clipping mask、inverted mask 和 mask texture 生命周期。
- [ ] 实现 texture upload、sampler、过滤和颜色空间策略。
- [ ] 只在 dirty 时更新必要 GPU 资源。
- [ ] D3D11/Metal 对相同 snapshot 行为一致。
- [ ] 建立非空帧、alpha、mask 和 blend 截图 smoke test。

### 5.5 音效

- [ ] 选择跨平台 Rust 音频后端并审查许可证/维护性。
- [ ] 支持现有 motion 音频格式和相对路径。
- [ ] 定义并发、打断、音量和模型切换停止语义。
- [ ] 音频失败不阻塞动画或渲染。
- [ ] shutdown 停止 stream 并释放设备。

### 5.6 Phase 4 退出门槛

- [ ] 三个预置模型通过兼容矩阵。
- [ ] 自定义模型 fixture 成功/失败行为符合规范。
- [ ] 模型切换 100 次无 CPU/GPU/音频持续增长。
- [ ] 输入、动作、表情、物理和音效闭环不依赖 GPUI。
- [ ] 模型 parser 完成 fuzz/property test，畸形 JSON、索引和尺寸不能触发 panic、越界分配或路径逃逸。

## 6. Phase 5：GPUI 设置应用

### 6.1 Command/Snapshot 边界

- [ ] 按 app、window、input、model、shortcut、update、diagnostics 定义 command。
- [ ] command 使用强类型 request/result 和稳定 error code。
- [ ] 长操作提供 operation id、progress、cancel 和 final result。
- [ ] snapshot 包含 revision，UI 处理过期结果和并发编辑。
- [ ] 禁止通用 set_value(path, any) API。
- [ ] 不向 UI 发送逐帧数据、原始按键流或 GPU/model pointer。
- [ ] command/snapshot 有纯 Rust contract test。

### 6.2 GPUI 状态规则

- [ ] Entity 只保存表单草稿、选择、展开、导航和临时 UI 状态。
- [ ] runtime snapshot 是显示配置/状态的唯一来源。
- [ ] command 成功后使用新 revision/snapshot 更新 UI。
- [ ] command 失败恢复草稿并显示可操作错误。
- [ ] 设置窗口重建时从 runtime 恢复，不依赖旧 Entity。
- [ ] UI executor 不持有 runtime 写锁或执行阻塞文件操作。

### 6.3 Design System

- [ ] 定义颜色、排版、间距、圆角、边框、阴影和焦点 token。
- [ ] 实现 Button、IconButton、TextInput、NumberInput、Slider、Switch。
- [ ] 实现 Select、Menu、Tabs、Tooltip、Dialog、Toast。
- [ ] 实现 List、EmptyState、ErrorState、Progress 和 Skeleton。
- [ ] 控件具有 hover、active、focus、disabled、loading 和 error 状态。
- [ ] 支持浅色、深色和系统主题。
- [ ] 图标统一使用 Lucide 资源并提供 tooltip/accessibility label。
- [ ] 不直接复制 Zed 产品内部组件源码，除非许可证和维护边界明确。

### 6.4 页面

- [ ] 应用框架：导航、标题、主题、语言、更新状态和错误边界。
- [ ] 通用：启动项、任务栏/菜单栏、语言、主题和日志。
- [ ] 窗口：显示器、位置、缩放、透明度、置顶、穿透和显隐。
- [ ] 模型：预置/用户模型、导入、删除、切换和兼容诊断。
- [ ] 输入：键鼠、手柄、忽略鼠标、单键模式和校正状态。
- [ ] 快捷键：捕获、冲突、清除和恢复默认。
- [ ] 动作/表情：绑定、预览 command 和错误状态。
- [ ] 权限：macOS 状态/跳转和 Windows 权限差异。
- [ ] 更新：检查、下载、验证、安装和回滚提示。
- [ ] 诊断：版本、renderer、GPU、输入、权限、模型错误和日志导出。
- [ ] About：许可证、Cubism attribution、第三方依赖和隐私说明。

### 6.5 UI 质量

- [ ] 迁移五种本地化并建立缺失 key 检查。
- [ ] 表单全键盘可操作，焦点可见且顺序正确。
- [ ] tooltip/dialog/menu 不被窗口边界错误裁剪。
- [ ] 800x600 和常见缩放无文本重叠或溢出。
- [ ] Windows 125/150/200% 和 macOS Retina 截图检查。
- [ ] 模型扫描/导入具有 loading、empty、error、cancel 状态。
- [ ] 复杂列表和动态文本不会导致布局跳动。
- [ ] UI 中不出现开发说明、架构术语或操作教学段落。
- [ ] screen reader 可识别 label、value、role、错误和进度；颜色不是状态的唯一表达方式。
- [ ] 中文、英文、德文等长文本和系统字体 fallback 下仍满足布局约束。
- [ ] 降低动态效果/高对比度等系统辅助设置有明确支持或书面限制。

### 6.6 Phase 5 退出门槛

- [ ] 所有 P0 设置可通过 GPUI 修改并由 Rust 原子持久化。
- [ ] 设置窗口销毁/重建后状态一致。
- [ ] 设置窗口关闭时 overlay CPU、帧率和输入不受明显影响。
- [ ] GPUI test、contract test 和双平台截图检查通过。

## 7. Phase 6：配置存储与环境隔离

### 7.1 Schema 与命名

- [x] 发布 `shared/config/config.schema.json`，并用有效/拒绝样本验证 schema 边界。
- [ ] 实现 `shared/config/native-config-contract.md` 中的首版字段，调整时同步 contract 和测试。
- [x] 每个字段记录默认值、范围、单位和跨字段约束；schema、typed validation 与边界 fixture 已对齐，后续新增字段必须同步三者。
- [ ] 未知字段采用明确的拒绝、忽略或诊断策略。
- [ ] 只为 Native Rewrite schema 建立 sequential `schema_version` 演进。
- [ ] 不包含旧 Pinia store key、旧字段 alias 或自动导入逻辑。

### 7.2 环境与持久化事务

- [ ] 构建系统显式产生 Development/Production 元数据，发布构建拒绝默认值。
- [ ] path resolver 返回当前平台与环境的数据根，不能接受任意外部生产路径。
- [ ] 实现 load -> parse -> validate -> upgrade native schema -> atomic commit -> verify。
- [ ] backup 包含 Native schema 版本和时间，并限制数量与总大小。
- [x] spike 中途提交中断后可安全恢复或重试；失败不覆盖当前可用配置。
  - 状态（2026-08-29）：`ConfigStore::recover_interrupted_commit` 覆盖主配置有效/缺失/损坏与临时文件有效/无效组合，恢复在 OS writer lock 内执行并保留诊断副本；父进程强制终止已写入并 flush 临时配置的持锁子进程后，macOS 本机与 Windows runner 均验证 lock 自动释放、当前配置保留和 interrupted archive。产品备份上限仍待完成。
- [ ] GPUI 显示错误摘要、备份位置和恢复默认 command。
- [ ] 用户模型只通过显式、受验证的导入进入当前环境，不扫描旧应用目录。

### 7.3 跨环境隔离

- [ ] Development 与 Production 的相对目录树和 JSON schema 完全一致。
- [ ] 配置、state、模型、备份、日志、锁和单实例 namespace 均包含环境边界。
- [ ] 两个环境可同时运行，不争用 writer lock、模型目录或日志文件。
  - 状态（2026-08-29）：config-store process test 已在 macOS 和 Windows 同时启动 Development/Production writer，分别提交 sentinel 并在重启后读取各自值，两个 lock root 互斥；模型目录和日志 writer 仍待完成，因此保持未勾选。
- [ ] 开发构建即使收到指向 Production 的 CLI 参数或进程环境变量也拒绝越界。
- [ ] Production 不自动复制 Development 数据；需要测试数据时使用显式导入。
- [ ] 更新 channel 与环境绑定，Development 不能安装 Production 更新或反向覆盖。

### 7.4 测试与门槛

- [x] 在平台无关 spike 中验证 Development/Production 根目录不同且相对结构一致，并在 Windows/macOS target-specific test 验证真实 resolver。
- [x] 两个环境写入不同 sentinel，重启和并发运行后仍只读取各自数据。
  - 验收证据（2026-08-29）：macOS 本机与 commit `cf16291e8cee027b6983abcf919a32fb5a0278a5` 的 Windows push run `33251410654`、job `99097619545` 均通过 `development_and_production_processes_commit_and_restart_independently`；产品 state/model/log 服务仍由各自阶段验证。
- [ ] 覆盖损坏、截断、错误类型、越界值和未知字段。
- [ ] 覆盖无权限、磁盘满、目标占用和中途退出。
- [ ] 覆盖非 ASCII/超长路径、缺失和重复模型。
- [ ] Native schema upgrade 重复执行 10 次结果一致。
- [ ] 失败注入不丢当前环境的配置或用户模型。
- [ ] 发布依赖和运行日志中没有旧 Tauri/Pinia 配置探测。
- [ ] Bundle ID 精确验证为 `com.ayangweb.bongo-cat`。

## 8. Phase 7：原生系统集成

### 8.1 应用生命周期

- [ ] 单实例唤醒已有进程并打开设置或显示 overlay。
- [ ] GPUI 设置窗口按需创建，关闭不退出后台应用。
- [ ] 托盘/菜单栏 command 统一进入 runtime。
- [ ] 系统关机、注销和普通退出进入 shutdown coordinator。
- [ ] panic/crash 生成本地诊断并避免配置半写入。
- [ ] 定义正常退出、强制退出、崩溃和系统终止的恢复标记；下次启动可区分并避免无限恢复循环。

### 8.2 Windows

- [ ] Shell_NotifyIcon + HMENU 托盘。
- [ ] named mutex + registered message/IPC 唤醒单实例。
- [ ] 当前用户启动项启用、禁用和状态检测。
- [ ] 文件选择、外部 URL 和剪贴板使用最小权限 wrapper。
- [ ] 选择并记录 MSIX、WiX 或 NSIS 打包 ADR。
- [ ] 对安装目录、用户数据目录和更新临时目录分别建模。

### 8.3 macOS

- [ ] NSStatusItem + NSMenu 菜单栏。
- [ ] NSApplication activation/reopen/single-instance 行为。
- [ ] SMAppService 启动项启用、禁用和状态检测。
- [ ] NSOpenPanel、NSWorkspace 和 pasteboard 最小权限 wrapper。
- [ ] .app bundle、entitlements、Hardened Runtime 和 notarization 流程。
- [ ] TCC 权限状态变化可在 UI 实时刷新。

### 8.4 更新与诊断

- [ ] 设计纯 Rust 更新 client、manifest 和签名验证。
- [ ] 只允许 HTTPS，固定公钥来源和轮换流程。
- [ ] 校验版本、target、arch、hash 和签名。
- [ ] 下载支持取消、断点/重试策略和失败清理。
- [ ] 安装前协调 runtime/renderer shutdown，失败可回滚。
- [ ] 测试断网、代理、中断、签名错误和降级攻击。
- [ ] 日志 rotation、总大小和保留天数有上限。
- [ ] 记录 renderer/input/model/config/update 的稳定 error code。
- [ ] 日志导出生成可预览的脱敏包。
- [ ] 更新 manifest 定义 schemaVersion、channel、最低可升级版本、发布时间和防回滚字段。
- [ ] 更新 helper/installer 的权限边界、替换原子性和失败恢复经过单独威胁建模。

### 8.5 Phase 7 退出门槛

- [ ] 托盘/菜单栏、单实例、启动项、更新、日志和退出双平台通过。
- [ ] 断网和系统服务失败不影响本地 overlay 运行。
- [ ] 安装包、权限和更新机制通过安全审查。

## 9. Phase 8：测试、性能与稳定性

### 9.1 自动化测试

- [ ] Runtime reducer、输入语义和动画单元测试。
- [ ] motion/expression priority 和可注入 clock 测试。
- [ ] 配置 schema、Native 版本演进、环境隔离和原子写入测试。
- [ ] 模型路径安全和损坏资源测试。
- [ ] Cubism safe wrapper 生命周期测试。
- [ ] 输入 fixture 和丢 release 恢复测试。
- [ ] GPUI component、command 和窗口重建测试。
- [ ] Windows/macOS 安装、首次启动、升级和卸载 smoke test。
- [ ] 公共 contract/schema 兼容性测试；支持窗口内的 Native config、UI snapshot 和更新 manifest 可读取。
- [ ] release 构建启用 panic/allocator/overflow 策略的真实测试，不只测试 debug 行为。

### 9.2 Windows 实机矩阵

- [ ] Windows 10 1903+ 和最新 Windows 11。
- [ ] 管理员/非管理员与不同完整性级别前台应用。
- [ ] PixPin Ctrl+Alt+A、Win+L、PrintScreen 和 UAC。
- [ ] 单屏、多屏、负坐标、热插拔和 100/125/150/200% DPI。
- [ ] 集显/独显、device loss、远程桌面和睡眠唤醒。
- [ ] XInput 连接、断开和多个手柄。

### 9.3 macOS 实机矩阵

- [ ] macOS 12 和最新稳定版本。
- [ ] Intel（若发布支持）和 Apple Silicon。
- [ ] Input Monitoring/Accessibility 未授权、拒绝、授权和撤销。
- [ ] Retina/非 Retina、外接显示器、Spaces 和全屏辅助。
- [ ] 锁屏、睡眠、快速用户切换和权限变化。
- [ ] GameController 连接、断开和不同 profile。
- [ ] 签名、notarization 和 Gatekeeper 首次启动。

### 9.4 性能基线

- [ ] 固定模型、窗口、DPI、FPS 和输入脚本。
- [ ] 测量冷/热启动、设置首次打开和首个 Live2D 帧。
- [ ] 测量空闲/活跃 CPU、RSS、GPU、显存和功耗。
- [ ] 测量 frame time p50/p95/p99 和 missed frame。
- [ ] 测量 input capture-to-runtime p50/p95/p99。
- [ ] 测量 runtime-to-present 和模型切换耗时。
- [ ] Windows 保存 ETW/WPA、PresentMon/GPUView 证据。
- [ ] macOS 保存 Instruments、Metal System Trace/os_signpost 证据。

### 9.5 稳定性

- [ ] 30 分钟高频键鼠 + 手柄 + 设置修改压力测试。
- [ ] 1000 次显示/隐藏、穿透和置顶切换。
- [ ] 100 次模型切换和损坏模型恢复。
- [ ] 100 次 GPUI 设置窗口创建/销毁。
- [ ] 100 次输入服务 restart。
- [ ] 8 小时固定模型 soak。
- [ ] 8 小时活跃输入/模型轮换 soak。
- [ ] 检查线程、handle、memory、GPU、audio 和日志增长。

### 9.6 退出指标

- [ ] 60 FPS 时 p95 frame time <= 16.7 ms。
- [ ] input callback 到 runtime p95 <= 2 ms（超出需书面分析）。
- [ ] 正常压力测试 key/button edge 丢失计数为 0。
- [ ] pressed state 在 release/reconcile/reset 后全部清零。
- [ ] 8 小时无持续内存/GPU 资源增长。
- [ ] 所有 worker 在退出超时内 join。
- [ ] stable 无未说明的 P0/P1 crash 或数据丢失问题。

## 10. Phase 9：发布切换

### 10.1 发布准备

- [ ] 定义 alpha、beta、stable 渠道和版本规则。
- [ ] 生成 SBOM、第三方许可证、Cubism attribution 和构建 provenance。
- [ ] Windows 产物签名并验证安装/卸载和 SmartScreen。
- [ ] macOS app 签名、notarize、staple 并验证 Gatekeeper。
- [ ] 产物不包含 WebView bundle、Node、旧前端或开发资源。
- [ ] 从干净 checkout 按书面步骤生成相同内容清单；不可避免的签名/时间戳差异单独记录。
- [ ] 对安装包、应用二进制、模型资源和更新 manifest 生成 SHA-256 并写入 release provenance。
- [ ] 更新 manifest 只引用 HTTPS 和签名产物。
- [ ] 准备已知差异、全新配置、环境隔离、备份恢复和问题反馈说明。

### 10.2 分阶段发布

- [ ] 内部 dogfood 覆盖至少一台 Windows 和一台 macOS 主力设备。
- [ ] alpha 收集 input reset、renderer reset、model load 和 config recovery 指标。
- [ ] beta 扩大模型/显示器/权限组合并冻结 schema/command contract。
- [ ] stable 前验证替换已安装旧版后二进制可正常启动，并明确提示新配置不会导入旧设置。
- [ ] Development/Production 的配置、更新 channel 和数据目录不会互相污染，Native schema 降级行为有明确限制。
- [ ] 验证失败更新可回滚，当前 Native 配置备份仍可用。
- [ ] 发布依赖和产物始终不包含 legacy config inspector 或旧 store 读取器。

### 10.3 旧代码退役

- [ ] 删除 Tauri、Vue、Pinia、Pixi.js 和 easy-live2d 依赖。
- [ ] 删除 src/ Web 前端、src-tauri runtime 和旧 plugin。
- [ ] 删除 rdev 和旧 device emit/listen 路径。
- [ ] 删除 gilrs 高频 IPC 路径；保留与新手柄方案无关的有效 fork 修复需单独评估。
- [ ] 删除旧不安全 updater 配置和宽泛 asset scope。
- [ ] 删除旧模型复制代码前确认 Native 显式导入覆盖支持的模型格式。
- [ ] 更新 README、开发环境、贡献指南和架构图。
- [ ] 保留旧版本 tag/分支作为行为与模型资源参考，不重写历史。

### 10.4 最终完成定义

- [ ] Windows/macOS stable 安装、升级、运行、更新和卸载通过。
- [ ] 生产产物不依赖 Tauri、WebView、JavaScript 或 Node.js。
- [ ] 关闭 GPUI 设置窗口不影响输入、动画、音效和 overlay。
- [ ] issue #47 和输入生命周期回归矩阵通过。
- [ ] 三个预置模型和支持范围内自定义模型通过兼容矩阵。
- [ ] Native 配置写入/schema 演进可恢复，模型显式导入无已知数据丢失路径。
- [ ] 性能、稳定性、安全和许可证门槛有可追溯证据。

## 11. Linux 后续 Backlog（不阻塞首发）

- [ ] 建立 X11/Wayland 功能能力矩阵，不假设全局输入等价。
- [ ] 评估 XInput2、evdev 权限和 Wayland portal/compositor 限制。
- [ ] 评估 Vulkan/OpenGL 或 wgpu overlay renderer。
- [ ] 验证 GPUI X11/Wayland 设置 UI、输入法和辅助功能。
- [ ] 评估托盘、启动项、窗口层级、透明、穿透和多桌面差异。
- [ ] 明确 AppImage/Flatpak/deb/rpm 的权限和资源分发策略。
- [ ] 只有输入、透明窗口和渲染达到门槛后才加入支持列表。

## 12. 当前执行队列

按顺序执行。`P0-GPUI-PACKAGE` 通过前不开始完整 UI；`P0-OVERLAY` 通过前不创建产品 platform workspace。

1. [x] `P0-BASELINE`：提交 Technical Design、Implementation TODO、AGENTS 和 ADR-001 至 005。
2. [x] `P0-FIXTURE-V1`：提交 input/expected schema、9 组核心输入 fixture 和跨文件 validator。
3. [x] `P0-GPUI-LIFECYCLE-MAC`：macOS 隔离 spike 精确锁定 GPUI 0.2.2，窗口打开并通过 GPUI `quit()` 正常退出。
4. [ ] `P0-DOC-CONSISTENCY`：维护 ADR-006/007/008，记录旧版 tag/安装包 hash、target triple 和工具链矩阵。
   - 状态（2026-08-28）：ADR 与仓库/发布基线已完成；target/toolchain 文档为 provisional，仍待 Windows 实机、GPUI 发布构建和 Cubism 架构证据后冻结。
5. [x] `P0-ARCHAEOLOGY`：补齐完整功能优先级和模型异常 fixture。
   - 状态（2026-08-28）：旧配置兼容已从产品范围移除；47 项功能优先级、预置模型资源清单、自定义模型匿名统计和六类模型异常目录 fixture 已完成。实机行为确认继续由输入、overlay 与 Cubism spike 承担。
6. [x] `P0-CONFIG-CONTRACT`：固定 Bundle ID、自有字段命名和 Development/Production 隔离存储契约。
   - 状态（2026-08-29）：ADR-008 与 naming contract 已完成；双平台 path resolver、环境隔离、OS writer lock、原子提交和强制进程终止恢复已在 config-store spike 验证。构建产物固定环境和完整产品配置服务仍属于 Phase 1/6。
7. [x] `P0-RUNTIME-CONTRACT`：冻结生命周期、单调 tick、operation 去重、shutdown drain 与超时结果。
   - 状态（2026-08-28）：`spikes/runtime-contract/` 已通过 14 项 contract test 并接入 CI，补齐 typed bounded worker、snapshot revision、command sequence gap/duplicate、overflow Reset、shutdown drain/timeout 和 panic/join 诊断；实际输入、模型、配置服务和平台 runtime 仍待 Phase 1/2。
8. [ ] `P0-GPUI-PACKAGE-MAC`：使用默认预编译 shader 构建 `.app`，验证 IME、剪贴板、焦点、辅助功能、主题和窗口重开。
   - 状态（2026-08-29）：默认 shader、bundle、菜单、窗口生命周期、主题、基础文本编辑/剪贴板、runtime bridge 和 macOS 性能基线通过；marked-text 纯状态 contract 已覆盖连续中文组合和 UTF-16 多字节边界，但不替代真实系统 IME。ADR-0009 已记录辅助功能 P0 gate，内容节点缺失，真实 IME、完整 tooltip/dialog/focus chain 仍待验证。
9. [ ] `P0-GPUI-WINDOWS`：在 Windows 构建同一 spike，验证字体、IME、DPI、辅助功能和正常退出。
   - 状态（2026-08-29）：push run `33250457705`、job `99095132076` 已在 `windows-latest` 启动同一 GPUI settings executable，并通过窗口创建、首帧 `scale_factor`、runtime revision 和有序 shutdown 检查；窗口创建失败现在非零退出。字体、IME、DPI 切换和 UI Automation 仍待 Windows 实机，因此保持未勾选。
10. [ ] `P0-OVERLAY`：GPUI 生命周期内完成 Windows D3D11/macOS Metal 透明 clear/present、错误注入和 100 次重建。

- [x] 先完成无平台依赖的 overlay lifecycle contract probe；平台窗口和 GPU 验证仍未完成。
- [x] Windows Win32/D3D11/DirectComposition owner、故障降级、析构顺序与 100-cycle 已通过既有 push/PR `windows-latest`；macOS 本机与 push/PR runner 的透明 clear/present、drawable unavailable、显式 shutdown 与 100-cycle 也已通过，并通过 `leaks` 基线消除窗口动画 retain cycle。GPUI 定时 frame source、双平台 resize、有序停止、原生 drag 状态切换及受控运行中故障恢复已实现；macOS 又以逐帧 backing-size 校正修复跨显示器后 drawable 尺寸漂移，并为 100-cycle 等长批次加入 thread/RSS/Metal allocation 门禁。完整 `P0-OVERLAY` 仍等待 Windows 真实 swapchain unavailable、双平台真实 device-lost、Windows GPU/线程与 macOS driver 专项采样、物理拖动及显示器/DPI 切换。

11. [ ] `P0-INPUT-WINDOWS`：完成 Raw Input + pressed set + `GetAsyncKeyState` 校正并实测 issue #47 场景。

- [x] 完成平台无关 pressed-set contract 和 issue #47 恢复测试；Windows 采集与校正仍未完成。
- [x] Windows 系统合成 input -> `WM_INPUT` -> 故意丢 release -> `GetAsyncKeyState` reconcile 闭环已通过 push run `33249296927`、job `99092066404`；PixPin、Win+L、UAC 和物理设备矩阵仍待完成。

12. [ ] `P0-INPUT-MAC`：完成 CGEventTap 权限拒绝/授予/恢复、状态校正和 100 次 restart。

- [x] 完成权限/tap 生命周期 contract、只读 preflight、真实 callback 和受控 disable 恢复；TCC 权限矩阵与系统自然 timeout 仍未完成。
- [x] 完成候选 pressed set 到 `CGEventSourceKeyState` 校正快照的边界和周期调度；真实 callback release 受控丢弃后的 20-cycle 闭环已通过，物理输入/系统自然丢事件、runtime 接入和生命周期实测仍未完成。

13. [ ] `P0-CUBISM`：确认 SDK/许可证/binding 生成，三个预置模型完成 Core、资源和 renderer spike。

- [x] 完成平台无关 Rust model3/package parser、三个预置规范化索引与异常资源安全 contract；Native Core、binding、Framework 求值和 D3D11/Metal 绘制仍未完成。

14. [ ] `P0-GO-NO-GO`：汇总证据、阻塞和条件，确认后再建立完整产品 workspace。

## 13. 待决策清单

| 决策                                                          | 最迟完成              | 阻塞内容                           |
| ------------------------------------------------------------- | --------------------- | ---------------------------------- |
| Windows/macOS 首发 CPU 架构和 target triple                   | `P0-DOC-CONSISTENCY`  | CI、SDK 二进制、签名和安装包矩阵   |
| GPUI 默认 shader 构建工具链及上游 future-incompatibility 处置 | `P0-GPUI-PACKAGE-MAC` | 产品 workspace 和发布构建          |
| Cubism Core/Framework 版本、获取方式和再分发条款              | `P0-CUBISM`           | Live2D safe layer、CI 和公开安装包 |
| Rust 音频后端及现有 FLAC 支持                                 | Phase 4 开始前        | motion sound、资源和许可证         |
| Windows 安装格式与更新 helper 权限模型                        | Phase 7 开始前        | 签名、升级、回滚和卸载             |
| macOS 最低系统、Intel 支持和 universal binary 策略            | Phase 1 开始前        | target、依赖、CI 和 notarization   |

每项决策必须落入 ADR 或对应设计文档，并从本表移除；不得只在聊天记录中形成结论。
