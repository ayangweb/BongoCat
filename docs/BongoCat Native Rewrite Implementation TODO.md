# BongoCat Native Rewrite Implementation TODO

状态：Phase 0 证据补齐与 Phase 1 渐进实现并行
最后更新：2026-08-30
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

- [ ] Phase 0 未通过前，不实现完整设置 UI、批量迁移旧代码或宣称相关平台能力完成；ADR-0011 允许已通过自动化 contract 的模块进入正式 workspace。
- [ ] GPUI 与原生 overlay 共存 spike 未通过前，不铺开平台窗口实现。
- [ ] Cubism spike 未通过前，不删除 Pixi/easy-live2d 行为对照。
- [ ] 存储环境隔离测试未通过前，不允许开发构建使用生产数据根。
- [ ] Cubism 书面授权、允许分发的 SDK/bindings/Core 清单、目标 ABI 和三个预置模型验证未通过前，不公开发布包含 Cubism artifact 的构建。
- [ ] 实机输入、辅助功能、GPU、8 小时 soak、签名和更新回滚未通过前，不发布 stable。

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
- [x] 固定上游 Bongo-Cat-Mver 行为参考仓库、commit、关键文件和使用边界。
  - 验收证据（2026-08-30）：`docs/migration/bongo-cat-mver-reference.md` 固定
    `MMmmmoko/Bongo-Cat-Mver` commit `4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c`，
    记录模型装配、更新顺序、纹理/alpha、输入模式和窗口行为的查阅入口；该仓库
    只作为行为证据，不进入 Native workspace 依赖图。
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

状态（2026-08-30）：已建立 v1 input/expected schema、9 组输入序列和规范化结果；输入、动作、表情、模型切换和音效 command 已覆盖。Rust 强类型 runner 现执行 51 个事件与 24 个 checkpoint，拒绝时间/设备生命周期/未连接 gamepad/非法 repeat 等错误并输出字段级差异；同时修复旧 Python runner 由 expected key 反向选择 parameter 的盲点，golden 现在包含序列完整参数域。跨文件检查、固定版本的 Draft 2020-12 标准 validator 与 Rust runner 已在本机及 commit `3c5f4e1` 的 push/PR CI 通过。物理键全集和旧版人工确认仍未完成。

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

状态（2026-08-30）：已在 `spikes/gpui-settings/` 建立隔离的 macOS 最小窗口，精确锁定 `gpui = 0.2.2` 并生成独立 lockfile；默认预编译 shader、release `.app`、原生 Application/Edit/Window 菜单、窗口关闭/重开和 shutdown smoke 通过。当前 spike 还验证了 System/Light/Dark 主题、焦点边框、Tab/Shift-Tab、Unicode/grapheme 文本编辑、选择、剪切、复制和粘贴，以及 GPUI executor 上的 bounded typed command/revision snapshot/shutdown acknowledgement，并保存浅色/深色截图证据。marked-text 纯状态 contract 已覆盖连续中文组合、已有多字节前缀、surrogate pair 和异常 range；本机 WeType 拼音 2.2.3 进一步通过真实系统组合更新、候选提交和已有中文前缀后的再次组合。项目自有 AccessKit tree 已由 macOS AppKit AX API 读取 9 个语义节点，Dark radio 的系统 press 经强类型 channel 回到 GPUI；Reset tooltip 已通过双平台原生合成 mouse-move -> GPUI 500ms delay -> build -> hover exit 链路，modal AlertDialog 的 Cancel 初始焦点、Tab/Shift-Tab 陷阱、Escape 关闭和背景语义隐藏也已验证。Windows UIA runner 已读取基础 role/name、selected/action、dialog，并通过 loading -> error -> retry/revision 2 恢复门禁；`busy=true` 因 runner 托管 UIA client 缺少属性标识而仍未验证。Apple 拼音、Windows IME、物理键盘/pointer、tooltip 朗读、目标 DPI 和真实辅助技术操作仍未验证，详见 `docs/phase-0/gpui-settings-spike.md`。

- [x] 建立最小 Rust workspace 和 GPUI hello/settings 窗口。
- [x] 固定 `gpui = "=0.2.2"` 并提交 Cargo.lock。
- [x] 禁止依赖 Zed 私有 UI crate；建立最小本地 design token。
- [ ] 验证 Windows/macOS 字体、中文输入法、复制粘贴和文本选择。
  - 状态（2026-08-30）：macOS release `.app` 已使用 WeType 拼音 2.2.3 逐键完成 `ni -> ni'hao -> 你好`，并在中文前缀后完成第二次 marked-text update/commit；未使用 paste 或直接 set-value。Apple 拼音、物理键盘和 Windows 字体/IME 仍待完成，因此保持未勾选。
- [ ] 验证键盘导航、焦点、tooltip、dialog 和菜单。
  - 状态（2026-08-30）：macOS `.app` 已验证 Reset command、modal dialog、Cancel 初始焦点、dialog 内 Tab/Shift-Tab 循环、Enter/Space button context、Escape 关闭和 GPUI 公共 tooltip help；AccessKit 隐藏 modal 背景节点。原生 Application/Edit/Window 菜单结构与 Select All/Cut/Paste 菜单动作已通过 AppKit run-loop smoke；双平台 probe 使用 `NSEvent mouseMoved:`/`WM_MOUSEMOVE` 进入 GPUI 平台回调，命中 Reset 后验证 500ms tooltip build 和 hover exit；Windows UIA 已加入 dialog open/focus/cancel 门禁。物理 pointer 与 VoiceOver/Narrator tooltip 朗读仍待完成，因此保持未勾选。
- [ ] 验证系统浅色/深色、缩放、Retina 和 Windows 高 DPI。
- [x] 验证窗口关闭、重开和退出生命周期；隐藏到托盘/菜单栏待系统集成阶段验证。
- [x] 验证 GPUI async executor 与 runtime channel 可安全通信；bounded command/reply、revision 过滤、receiver close 和 shutdown acknowledgement 已通过 contract test 与 macOS release `.app` smoke。
- [ ] 验证辅助功能树满足设置表单的基础要求。
  - 状态（2026-08-30）：macOS 本机已验证 role/title/value、selected/focus、busy/error 属性与 radio action；commit `21ee8aa` 的 push run `33291750411`、job `99204478369` 与 pull request run `33291751558`、job `99204481348` 已通过 Windows UIA role/name、radio selection action、selected state、loading、注入错误与 retry/revision 2 恢复。runner 托管 UIA client 缺少 `AriaPropertiesProperty` 标识，故 `busy=true` 投影仍未验证；真实 VoiceOver/Narrator 操作和宣读仍待完成，因此保持未勾选。
- [x] 记录首次打开、空闲 CPU、RSS 和二进制增量；`docs/benchmark/data/gpui-settings-macos-248a770-*.csv` 保存原始样本，方法、环境和限制见 `docs/phase-0/gpui-settings-spike.md`。
- [x] 安装并固定 macOS Metal Toolchain，验证 GPUI 默认预编译 shader 路径；`runtime_shaders` 不作为发布配置。
- [ ] 将 macOS spike 打包为最小 `.app`，验证 bundle id、菜单、激活、关闭和辅助功能树可被系统识别。
  - 状态：Bundle ID `com.ayangweb.bongo-cat`、菜单、激活、关闭/重开、退出、WeType 拼音组合提交与最小内容 AX tree/action 已通过；真实 VoiceOver、Apple 拼音和 error/loading 宣读仍待完成，因此保持未勾选。
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
  - 状态（2026-08-29）：Windows 已在一个 100-cycle driver-pool 预热批次后，对第二个等长 batch 使用 ToolHelp thread snapshot、`IDXGIAdapter3::QueryVideoMemoryInfo(LOCAL)` 和 process handle 执行零增长门禁；真实 hardware D3D11 已在 runner 通过。macOS release 100-cycle 在普通与 NSZombie 模式均通过，AppKit windows 与 Rust owner 都回到 0；1/10/100-cycle `leaks` 基线定位并消除了 AppKit transform animation retain cycle。macOS runner 在 commit `5baa6ba` 即使观测到两批 `currentAllocatedSize` 相等，随后 300-cycle 仍由 `5242880` 扩展到 `8388608`，证明无显示 compositor 的一次相等读数不是可靠收敛信号。当前 probe 对 window/owner/thread 保持零增长，并把 Metal 增长限制为按真实 drawable 尺寸和 `maximumDrawableCount` 计算的一个三缓冲 pool；超出仍失败，driver 零斜率留给 Instruments/Metal System Trace 长期采样。本机仍为 `393216 -> 393216`；新 runner 与 driver 专项证据待完成，因此保持未勾选。
  - 状态（2026-08-30）：push run `33270546247` 与本机均复现 macOS 瞬时进程线程数 `7 -> 8`，采样栈显示变化来自 AppKit/Metal/libdispatch/GPUI `async-io` worker，窗口数、Rust overlay owner 和 Metal allocation 均未增长。probe 现逐个预热 cycle 记录线程高水位，再拒绝测量 batch 超出该上界；这避免把系统 worker 池在两次瞬时采样间的收缩/恢复误报为 overlay 泄漏，同时仍会捕获随等长 batch 持续增长的线程。CI 复验和 driver 专项证据仍待完成。
- [ ] 验证退出顺序：frame source -> renderer -> GPU -> overlay -> GPUI。
  - 状态（2026-08-29）：GPUI executor 上的 60 Hz 定时 frame source 已连续驱动双平台 renderer，并在退出时通过停止确认后才释放 renderer/GPU/window；macOS 本机与 Windows hardware D3D11 runner 均已验证连续帧、resize、hide/show 和有序退出。生产 display-linked frame source 与 runtime 尚未接入，因此保持未完成。
- [x] 写明 GPUI/AppKit/Win32 主线程所有权、overlay 创建线程和跨线程 command 不变量。
- [ ] 注入 renderer 初始化失败、drawable/swapchain unavailable 和 device lost，设置窗口仍可打开并显示诊断。
  - 状态（2026-08-29）：Windows push/PR runner 已通过 renderer 初始化失败与 GPUI degraded 状态；macOS push/PR runner 已通过受控 drawable unavailable、GPUI degraded、正常 quit 与 owner 释放。运行中故障的双平台恢复状态机先释放旧 owner，有限退避后完整重建，GPUI 显示 recovering/recovered；device-lost 注入已通过 macOS 本机与 Windows runner。本批又为 Windows runner 增加独立 surface-unavailable 注入，要求 D3D11/DirectComposition owner 和 HWND 均早于重建释放，并验证 `failures=1 recoveries=1`。真实 swapchain unavailable 与双平台真实驱动 device loss 仍待完成。

### 1.7 输入可靠性 spike

- 状态（2026-08-28）：`spikes/input-state/` 已建立纯 Rust pressed-set contract，覆盖正常 down/up、重复 down、Reconcile、Reset、issue #47 的丢失 release 恢复、可靠事件的序列跳号/重复/乱序诊断，以及 `250 ms` 校正调度和连续 `2` 次缺失确认的误判保护；计数器不记录具体键值。平台采集、runtime 接入和管理员/权限场景仍待实机验证，详见 `docs/phase-0/input-state-spike.md`。
- 状态（2026-08-29）：`spikes/input-windows/` 已冻结 `RI_KEY_BREAK`、E0/E1、左右修饰键、PrintScreen、未知 scan code 保留和安全 `RAWINPUT` 字节解析 contract；隐藏顶层 HWND 的 callback 现在只生产带单调 sequence 的 keyboard/button/Reset/reconcile 事件，由容量 64 的可靠 FIFO 交给 message owner 消费。满载会丢弃不可信 backlog、插入 `QueueOverflow` Reset 并计数，shutdown 会先入队最终 Reset、关闭 producer 再 drain。RAWMOUSE 五个 canonical button、`GetAsyncKeyState` 校正及 lifecycle Reset 已接入；合成/物理 button release 和真实设备矩阵仍待验证，详见 `docs/phase-0/input-windows-spike.md`。
- 状态（2026-08-30）：`spikes/input-macos/` 已建立 macOS 权限/tap 生命周期 contract、listen-only `CGEventTap` 专用 run loop、panic-isolated callback、固定容量 callback queue 和候选 pressed-set 周期校正；callback edge/Reset 携带单调 sequence，严格 cycle validator 要求无 gap/duplicate 且 queued/consumed/discarded 完整守恒。`FlagsChanged` 方向现由 callback 的 event flags 与左右 modifier keycode 冻结，未知映射安全 Reset；keyboard、modifier 与 mouse 三条 private `CGEventSource` release-loss 均完成真实 callback 闭环，modifier/mouse 又分别通过 20/20 cycle，所有候选由校正清零且 gap/overflow/panic 为 0。物理输入/系统自然丢事件、系统自然 timeout、TCC 拒绝/撤销和真实锁屏/睡眠/快速用户切换恢复仍未完成，详见 `docs/phase-0/input-macos-spike.md`。

- [ ] Windows 实现 RegisterRawInputDevices 和 WM_INPUT 最小路径。
  - 状态（2026-08-29）：已实现注册、读取、注销和自动退出路径，并通过 Windows target 交叉 check/Clippy 以及 `windows-latest` 注册/退出 smoke。本批新增 `SendInput` scan-code down/up -> 系统 `WM_INPUT` callback -> raw decode 的闭环命令并接入 Windows runner；物理设备样本仍待实机，因此保持未勾选。
  - 状态（2026-08-30）：上述 contract 已提升到正式 `bongocat-platform`：专用隐藏 HWND
    注册 keyboard/mouse `RIDEV_INPUTSINK | RIDEV_DEVNOTIFY`，安全解析 x64 `RAWINPUT`
    bytes，将 key/button edge 送入正式 runtime，并把 pointer movement 分流到 cursor
    latest-value。产品 D3D11 session 负责在 runtime 前启动/停止/join 服务；物理设备样本
    和发布实机矩阵仍缺，因此总项保持未勾选。
- [x] 冻结 scan code、extended flag、左右修饰键和 RI_KEY_BREAK mapping contract；Win32 packet 接入仍待实机。
  - 状态（2026-08-29）：同一 safe raw-byte boundary 已覆盖 RAWMOUSE `usButtonFlags`，五个 canonical mouse button 的 down/up 与同包顺序由 contract test 固定；button pressed-state/校正已接入 Windows spike，commit `e776867` 的 push run `33256593886`、job `99111304790` 已通过 22 项 contract test 和五个 button VK 的真实查询。pointer movement 和 wheel 的 latest-value 分流仍属于平台 producer 实现。
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
  - 状态（2026-08-30）：正式产品 crate 新增同等强度的 ignored Windows smoke，故意吞掉
    已由 `WM_INPUT` 捕获的 A-up，要求两次 `250 ms` 系统快照最终清除正式 runtime 的
    `left_hand_down`，并校验 callback/捕获队列/runtime 队列无 overflow 或 panic。该 smoke
    已加入 Native workspace 的 Windows CI；PixPin 物理交互仍待用户实机验收。
- [ ] 实测 Win+L、PrintScreen、UAC 和管理员/非管理员场景。
- [ ] 进行 10 分钟高速鼠标 + 键盘压力测试，edge 丢失计数必须为 0。
  - 状态（2026-08-29）：3 秒有界 `SendInput` 压力 smoke 对 A、S、Space、左 Shift、左 Control 和 E0 右 Control 发送 128 轮、共 1536 个 down/up 边沿；commit `f68b46f` 的 push/PR Windows jobs 均已通过完整、有序、无 duplicate/unmatched/decode/panic/残留门禁。keyboard-under-pointer-flood 模式又在相同键盘边沿之间插入 3072 个不可合并的相对鼠标移动，commit `64dd9d3` 的 push/PR Windows jobs 均验证实际 mouse message 洪峰不阻塞可靠 release。两者都不能替代本项要求的 10 分钟物理键鼠与交互场景，因此保持未勾选。
- [ ] macOS 实现 CGEventTap、权限拒绝/授予和 tap 自动重启。
  - 状态（2026-08-30）：正式 `bongocat-platform` 已实现 listen-only tap、专用 run loop、
    callback panic boundary、固定容量边沿队列、overflow Reset 和受控 timeout/user-disable
    Reset + re-enable；TCC 拒绝、撤销、重新授予和系统自然 timeout 矩阵仍待实机完成。
- [ ] macOS 使用 CGEventSourceKeyState/CGEventSourceButtonState 校正 pressed state。
  - 状态（2026-08-30）：正式 run-loop consumer 已从 KeyDown/Up、带 callback-time pressed
    方向的 `FlagsChanged`、MouseDown/Up 和 Reset 维护 key/button 候选集合，每 `250 ms`
    使用 `CGEventSourceKeyState`/`CGEventSourceButtonState` 校正，连续 `2` 次缺失才释放。
    同进程 ignored 集成测试已证明合成 left Shift down/up 经正式 CGEventTap 进入 runtime
    `ModelInputSnapshot`，并以 `capture_queue_overflows=0`、`runtime_queue_overflows=0`、
    `callback_panics=0` 有序停止；runtime 提前停止路径亦已证明统一 disable/remove/join 后
    可立即重建第二个 tap。物理输入、系统自然丢事件与生命周期实测仍待完成，因此保持未勾选。
- [x] 连续 start/stop/restart 输入服务 100 次，无资源泄漏。
  - 验收证据（2026-08-29）：release probe 现在严格校验每个 cycle 的 enabled 恢复、callback panic、queue overflow/closed event 和 NSWorkspace observer 成对注销，任一失败均非零退出。`leaks --atExit` 的 100-cycle 报告 `0 leaks for 0 total leaked bytes`、physical footprint `5232K`，`NSZombieEnabled=YES` 另完成 100/100；两次均为 `queue_overflows=0 callback_panics=0 clean_shutdown=true`，且每个 tap worker 都已 join。timeout/user-disable 各 20 次恢复已另行通过；权限故障循环留在 TCC 矩阵，不阻塞本 restart owner 子项。
- [x] 记录 monio 对照结果，但不引入生产依赖；`docs/phase-0/monio-comparison.md` 基于 commit `d1766e0dcd20dea0435be16cd80adaa749b86e30` 记录 Raw Input、channel、reconciliation、Reset、callback 和许可证差异。
- [ ] 为 captured、reconciled、reset、duplicate、overflow 分别维护计数器，不记录具体键值。
  - 状态（2026-08-29）：macOS spike 已输出事件类型、reconciled release、Reset 次数/释放数、duplicate/unmatched 和 queue overflow/recovery 数量；Windows spike 已补齐 captured、reconciled、Reset、duplicate/unmatched、queue enqueued/consumed/overflow/recovery/discarded/closed push、sequence gap/duplicate、decode 和 callback panic 聚合计数。commit `98b27f2` 的 push run `33257310771`、Windows job `99113185410` 已通过受控 overflow Reset 及全部正常输入回归。两平台均不输出具体键值；产品 runtime 的统一诊断 snapshot 仍待完成。
- [x] 验证输入 callback panic 隔离、队列关闭和应用退出竞态，不允许 callback 访问已析构 runtime。
  - 验收证据：macOS event-tap 与 workspace callback 共用 autorelease/panic boundary，故意 panic 的测试确认 unwind 不越过 callback；固定队列 close 后拒绝新事件并可 drain；受控生命周期 smoke 在 callback gate 关闭后触发迟到通知，只增加 ignored 计数。observer token 成对注销，callback 只持有 queue/atomic，不捕获 runtime owner。产品 runtime 接入后仍须重跑对等 shutdown 测试。

### 1.8 Cubism/Renderer spike

- [ ] 确认 Cubism SDK/Core 版本、来源、再分发条款和 attribution 要求。
  - 状态（2026-08-30）：`docs/phase-0/cubism-sdk-source-and-license.md` 已固定 Native `5-r.5`/Core `06.00.0001`、archive/header/Core hashes、官方 tag/commit、下载入口和 RedistributableFiles 边界；macOS arm64 真实 Core/model probe 已通过。BongoCat 很可能属于需预先批准和单独协议的 Expandable Application；Framework 到 MIT Rust 实现的许可边界、最终 attribution、第二来源复核和 Live2D 书面授权仍未完成，因此保持未勾选并阻塞 stable 发布。
- [ ] 建立目标架构二进制清单、hash 和可重复获取流程。
  - 状态（2026-08-30）：r.5 Windows x64 与 macOS arm64/x64 artifact 路径和 SHA-256 已形成清单，Windows ARM64 已明确无 desktop artifact，i686 已排除；固定 archive hash 的离线 ZIP 检查和 macOS arm64 ABI 已通过。第二人/第二机器复核、Windows x64/macOS x64 原生 ABI 与授权后的可分发获取流程仍待完成。
- [x] 验证 Rust sys binding 加载 moc、创建 model 并读取 drawable 数据。
  - 验收证据（2026-08-30）：`tools/cubism-core-probe/` 首先使用隔离的真实 r.5 arm64 binding 与 Core `06.00.0001`，对三个预置 Moc 各完成 100 次 consistency/revive/initialize/update/array/drop。parameter/part/drawable/canvas 与 legacy baseline 一致，另读取 vertex/UV/index/mask、packed blend、render order、parent 与 r.5 offscreen 数组；`leaks --atExit` 为 0 bytes。commit `57118ff` 随后把审阅后的 binding 和 safe wrapper 提升到产品 workspace，并通过三个模型测试与 Windows x64 release 交叉 check。详见 `docs/phase-0/cubism-core-r5-probe.md`。
- [x] 包装 Moc/Model 生命周期，证明 Model 不会比 Moc 存活更久。
  - 验收证据（2026-08-30）：commit `57118ff` 的 `bongocat-live2d` 使用 Rust 对齐
    allocation 分别持有 revived Moc 与 Model，raw pointer 不离开 crate，显式按
    Model -> Moc 顺序析构；三个预置模型重复创建 snapshot 的测试均通过。
- [x] 用 Rust 解析三个预置 model3 和所有关联资源。
  - 验收证据：build `7ee8acd5f2a3d4dcb7a1dbc36623cbe497aeae49` 的 push run `33238204993` 与 PR run `33238206415` 各 16 jobs 全绿。`spikes/model-package/` 强类型解析 model3 v3，验证 moc、纹理、display info、expression、motion/audio、可选 physics/pose/user data 与 companion images，完整包索引冻结在 `shared/fixtures/model-fixtures/preset-model3-index.json`。2026-08-30 又将 3 个 cdi3、6 个 motion3 与 15 个 exp3 纳入强类型结构验证；cdi3 parameter/part 数量与 legacy Core baseline 一致，三个预置包、异常 fixture、跨根 symlink 和目录深度均有 Rust 测试，详见 `docs/phase-0/model-package-spike.md`。本项不包含 Core/model creation、动作求值或 renderer。
- [ ] Windows D3D11 绘制预置模型的 texture/order/alpha/mask。
  - 状态（2026-08-29）：Windows overlay 已实现合成几何的 D3D11 shader pipeline、预乘 alpha draw 和 staging texture 像素验证；预置模型 texture、drawable order 和 mask 尚未接入，因此保持未完成。
- [x] macOS Metal 绘制同一模型的 texture/order/alpha/mask。
  - 验收证据（2026-08-30）：commit `57118ff` 将三个预置模型的真实 Core drawable
    snapshot 接入独立 `NSPanel`/`CAMetalLayer`，按 render order 绘制 texture、normal/
    additive/multiplicative blend、预乘 alpha、multiply/screen color 和 inverted mask。
    本机 release preview 分别连续提交 standard 716 帧、keyboard 596 帧、gamepad
    597 帧，三者首帧 GPU readback 和截图检查均通过；每个模型含 5 个 masked drawable。
    后续产品 renderer 已按 Core `source_index` 每帧同步 vertex、index、render order、
    visibility、opacity、blend/color 和 mask 引用；类型化预览驱动下 standard 177/177、
    keyboard 175/175、gamepad 179/179 帧均产生变化 snapshot 并完成 Metal present。
- [ ] 验证 motion、expression、physics、pose 至少各一个真实样本。
  - 状态（2026-08-30）：6 个预置 motion3 与 15 个 exp3 已通过强类型结构解析和计数门禁；本机 13 个历史 physics3 也以匿名只读方式通过 v3 静态 parser，共覆盖 86 setting/139 input/206 output/267 vertex。合成 pose3 已固定 Type/fade/group/part/link 拒绝边界，但不是真实样本。motion/expression 的实际时间求值、参数混合与优先级，physics/pose 的实际求值，以及授权 physics/pose fixture 仍未完成，因此本项保持未勾选。
- [x] 验证模型切换/销毁 100 次，无 CPU/GPU 资源增长。
  - 验收证据（2026-08-30）：macOS release switch probe 先以不存在的 PNG 验证失败 GPU
    prepare 不改变 active generation，随后执行 100 个 standard -> keyboard -> gamepad ->
    standard 完整循环，共提交 300 个连续 Cubism/GPU generation 和 300 次非透明 readback；
    359 帧中 351 帧为动态 snapshot，Metal current allocated size 在 warmup/settle 后均为
    `54,427,648` bytes。`leaks --atExit` 的相同 300 次切换退出 footprint `35.5M`、peak
    `102.8M`，仅保留已知系统 AppIntents/NSXPC 18,816 bytes，无 BongoCat/Cubism/Metal/
    overlay owner 栈。该项不包含后续 motion audio owner。
- [x] 记录与 easy-live2d 的差异和必须兼容项；`docs/phase-0/easy-live2d-compatibility.md` 基于 lockfile 固定的 `easy-live2d 0.4.4` 及安装产物 hash，冻结 BongoCat 实际 API 面、跨帧参数 override、update order、motion sound、ready/销毁与 renderer 语义，并明确多 model3、JSON5、破坏性切换、全局 ticker、WebGL/Pixi 和错误吞噬不进入兼容范围。该项只完成旧库边界，不代表 R5 Core/Framework/renderer 已通过。
- [ ] 若纯 Rust Framework 逻辑不可行，提交 go/no-go ADR；不得静默加入 C++ 业务桥。
- [x] 建立 Cubism Framework 行为来源清单，逐项说明 motion、expression、physics、pose 的 Rust 实现依据和许可边界。
  - 验收证据：`docs/phase-0/cubism-framework-behavior-sources.md` 固定 R5 tree、16 个关键 Framework blob、双平台 sample owner、行为 oracle 与禁止直接翻译的许可边界；离线 SDK inspector 会验证这些 blob。最终发布方式与 attribution 留在发布清单，不再阻塞 Rust 功能实现。
- [ ] 对 raw binding 生成流程固定 header、生成器版本和输出审阅方式，禁止手改生成代码后失去可重复性。
  - 状态（2026-08-30）：`tools/cubism-bindgen/` 已精确锁定最新稳定版 `bindgen 0.72.1`，固定当前 r.5 可用且属于产品矩阵的 Windows x64 与 macOS arm64/x64、`csm*` 白名单、Rust 1.85/edition 2024、配置/output hash 和隔离 staging；自有合成 header 的三 target golden 已覆盖 r.5 render order/blend/offscreen API。commit `57118ff` 已固定真实 target bindings；工具继续拒绝 i686 与当前无 Core 的 Windows ARM64。第二人重生成审阅、Windows x64/macOS x64 原生 ABI 仍待完成，因此保持未勾选。

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

状态（2026-08-30）：维护者通过 ADR-0011 授权 Phase 0 外部证据补齐与正式实现并行。
先提升 runtime/config contract，Cubism、完整 UI 和平台能力仍服从各自门禁。

### 2.1 目标目录

- [x] `native/` 正式 Cargo workspace 仅包含新 Rust 应用和 crate；发布切换时再提升为根构建入口，迁移期不破坏历史 Tauri workspace。
- [x] 创建 bongocat-app：入口、服务装配和 shutdown。
  - 验收证据（2026-08-30）：正式双平台 `bongocat-app` 入口现装配配置、单一 runtime
    owner、预置模型与输入映射、平台输入、cursor latest-value transport 和 Metal/D3D11
    overlay；应用拥有唯一 render consumer，并以输入 -> runtime -> renderer/window 顺序
    停止。输入权限/启动失败降级而不阻止模型窗口显示；`--run-seconds 0` 可持续运行，默认
    30 秒用于有界开发 smoke。GPUI 共存、installed-model 选择和跨平台多模型切换确认仍属
    后续任务。
- [x] 创建 bongocat-runtime：状态、输入语义、动画和 command。
- [x] 创建 bongocat-config：环境隔离、schema、验证和原子存储。
- [x] 创建 bongocat-model：模型包、导入和资源索引。
  - 验收证据（2026-08-30）：正式 `bongocat-model` 已实现可移植 `ModelId`、model3
    索引、引用规范化、文件/包/纹理上限、跨根 symlink 防护和
    `PreparedModel`；三个预置包与缺失 moc、损坏 JSON、非 ASCII、超大纹理、
    路径穿越、多入口及跨根 symlink 均由产品 workspace 测试。`ModelStore` 又完成
    环境模型根、受限 staging copy、flush、复验、同根 rename commit 和无覆盖语义；
    用户模型 catalog、加载、删除、writer lock 和崩溃 staging 回收已进入产品入口；
    `PresetModelCatalog` 以真实只读目录签发预置 `CommittedModel`，拒绝 symlink root/entry
    和 catalog root 逃逸。完整 sidecar 强类型校验与预置/用户合并视图继续由 Phase 4 跟踪。
- [ ] 创建 bongocat-live2d：Cubism safe wrapper 和模型求值。
  - 状态（2026-08-30）：commit `57118ff` 已建立正式 crate，完成 Core 版本门禁、
    Moc/Model safe owner、drawable snapshot 和三个预置模型测试；随后又在加载时解析并
    验证 Core parameter id/range/default 表，冻结 19 个类型化产品参数和三预置支持矩阵，
    提供 finite/clamp/normalized update，不向上暴露 raw pointer。motion、expression、
    physics、pose 求值尚未实现，因此总项保持未完成。
- [x] 创建 bongocat-render：render snapshot 和 renderer contract。
  - 验收证据（2026-08-30）：正式 crate 已从 Cubism 边界接管 `RenderSnapshot`、
    `RenderResources` 和强类型 `DrawableId`/`TextureId`，并提供带 model generation/
    frame number 的 latest-frame transport。10,000 帧测试验证 coalescing/accounting，
    close 后可 drain pending 且拒绝迟到帧，倒退 frame/generation 会显式失败；正式
    Live2D 与 macOS Metal overlay 已改用该 contract。
- [ ] 创建 bongocat-ui：GPUI 页面和 design system。
- [ ] 创建 bongocat-platform：Windows/macOS 系统服务。
  - 状态（2026-08-30）：正式 crate 已接入 macOS listen-only CGEventTap 与 Windows Raw
    Input，两平台都提供可靠键鼠边沿、周期状态校正和独立 cursor latest-value producer；
    输入启动时主动发布当前光标，随后按光标所在显示器查询 viewport 并进入正式 runtime。
    Windows 还处理设备移除、WTS session、电源、队列溢出和 shutdown Reset。平台类型没有
    泄漏到 runtime；macOS 生命周期通知、双平台 GameController/XInput 与其余系统服务尚未
    迁入，因此总项保持未完成。
- [ ] 创建 shared/config、behavior、fixtures、resources。
- [x] 避免空 crate；首批只建立 app/runtime/config 三个有独立依赖和测试价值的 crate。

### 2.2 工程质量

- [ ] 固定 stable Rust toolchain、target 和必要 components。
- [ ] 在 workspace manifest 声明 `rust-version`，CI 验证最低版本和当前 stable，不依赖开发机偶然安装的 nightly。
- [x] 禁止应用依赖未固定 git branch，提交 Cargo.lock。
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
- [x] CI 不下载 Cubism 二进制；正式 workspace 的三平台 job 不需要 SDK 即可验证非 Cubism 模块。
- [ ] GPU、权限、签名测试分离为实机/nightly job。
- [x] 正式 runtime/config/app 在 Ubuntu job 执行 check/test，不生成 Linux 安装包。
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
  - 状态（2026-08-30）：正式 runtime 已有 typed `RuntimeCommand`、带 revision/
    command sequence 的 `RuntimeSnapshot`、模型摘要及项目自有 `InputEvent`/
    `InputSnapshot`；`wait_for_command` 可区分并发 command 的完成。正式
    `bongocat-render` 已定义不可变 `RenderSnapshot`/资源 contract 和 latest transport；
    producer 现由 runtime worker 持有并随 shutdown 关闭，完整 product command 集仍待实现。
- [ ] 单一 runtime owner 管理可变业务状态。
  - 状态（2026-08-30）：正式 runtime worker 已独占 overlay、pressed input、输入诊断、
    已提交模型和 mutable Cubism model evaluation，并只发布不可变 runtime/render snapshot；
    Cubism 对象在线程内创建，未使用 `unsafe impl Send/Sync`。应用与 UI client 只通过有界
    typed command 和 snapshot 访问；motion/expression/physics/pose 动画状态仍待接入。
- [ ] key/button edge 和 command 使用可靠有序队列。
  - 状态（2026-08-30）：正式 `ApplyInput` 与其他 command 共用有界 FIFO，input event
    另带独立单调 sequence；`InputProducer` 以非阻塞 publish 返回原始拒绝事件，并向
    app/platform 暴露 recovery API。macOS/Windows 正式 producer 均已接入；command 与
    input 共用容量 64 的产品 FIFO，正式 gamepad producer 仍待建立。
- [ ] 为每个可靠队列定义容量、生产者、消费者、满载策略和关闭语义，不使用无界队列逃避背压设计。
  - 状态（2026-08-28）：`spikes/input-queue/` 已验证固定容量 FIFO、满载返回原事件、关闭 drain 和 latest-value 槽位；`spikes/runtime-contract/` 进一步验证固定容量 command queue、Condvar 唤醒、溢出 Reset、worker drain 和 join 报告；runtime 的实际容量与产品 channel 选型仍待产品 crate。
  - 状态（2026-08-30）：正式 app 当前使用容量 64 的共享 command/input FIFO，唯一
    runtime worker 消费，owner shutdown 使用可靠控制消息并 join；cursor 已使用独立单槽
    latest-value transport，停止后拒绝新 sample 并在 shutdown 消费 pending sample。
    Windows 正式 input owner 也已具备 start/stop/join 和最终 Reset；gamepad axis 通道及
    producer 生命周期尚未建立，因此总项保持未勾选。
- [ ] edge/command 携带单调 sequence id，诊断可发现乱序、重复和丢失但不记录具体键值。
  - 状态（2026-08-29）：Windows callback queue 的 edge、Reset 和 reconcile tick 已携带单调 `u64` sequence，正常压力路径要求 gap/duplicate 均为 0，受控 overflow 以 discarded backlog 数量产生等量 gap 并由 Reset 恢复。command queue 与产品 runtime 的统一 sequence contract 仍待实现，因此保持未勾选。
  - 状态（2026-08-29）：macOS callback queue 也已为 edge/Reset 分配单调 `u64` sequence，overflow Reset 继承被拒事件序号，consumer 统计 gap 与 duplicate/out-of-order；普通 tap、timeout/user disable 和 lifecycle 本机回归均为 0。commit `d7501dc` 的 push run `33257871184` 已通过 contract job `99114627795` 及原生 macOS job `99114627654` 的 input check/Clippy/test/release 门禁。command queue 与产品 runtime 的统一 contract 仍待实现，因此保持未勾选。
  - 状态（2026-08-28）：`spikes/input-state/` 已验证可靠输入事件的重复/乱序忽略与跳号安全 reset；`spikes/runtime-contract/` 已验证 typed command sequence、跳号前 `WorkerRecovery` reset、重复/过期 sequence 丢弃和诊断计数；平台 producer、输入事件 sequence 与产品 runtime 接入仍待产品 crate。
  - 状态（2026-08-30）：产品 runtime 现分别维护 command sequence 与 input sequence；
    input 重复/乱序计数后忽略，跳号计数缺失数量、先以 `SequenceGap` Reset 再应用当前
    边沿，snapshot 只暴露聚合诊断。macOS/Windows 正式 producer 均经 `InputProducer`
    分配 sequence；正式 gamepad producer 与完整统一诊断仍待建立。
- [ ] cursor/gamepad axis 使用 latest-value 合并通道。
  - 状态（2026-08-29）：Windows RAWMOUSE movement 已从可靠 edge FIFO 分流到独立 latest-value 槽位；safe decoder 保留 relative/absolute/virtual-desktop 语义，16ms owner tick 在 callback 外查询当前 cursor，pointer flood 要求 captured sample 全部由 coalesced 或 consumed 解释且不影响 keyboard release。commit `098d532` 的 push run `33258305541`、Windows job `99115756881` 已通过强化后的 3072 movement/1536 keyboard edge 回归。Gamepad axis、macOS cursor 和产品 runtime 通道仍待实现，因此保持未勾选。
  - 状态（2026-08-29）：macOS `MouseMoved` 与 left/right/other drag 已分流到独立 latest-value slot，run-loop owner 约每 16ms 消费一次并在 shutdown flush；10,000-sample contract 证明 cursor flood 不占用可靠 button edge 队列，严格报告要求 `captured = coalesced + consumed` 且 close 后无迟到发布。commit `500a956` 的 PR run `33258718745` 中，原生 macOS job `99116842307` 与 contract job `99116842405` 均通过。Gamepad axis、产品 runtime 通道和物理 cursor callback 实测仍待完成，因此保持未勾选。
  - 状态（2026-08-29）：平台无关 keyed latest-values contract 已为 gamepad axis 固定容量、按 key 合并、完整 accounting、关闭语义和连接 generation；10,000 次同轴更新只消费最终值，新 key 超容量明确失败，断开后的旧 generation 不会污染复用 device id 的重连。commit `16a51bb` 的 push run `33259120950`、job `99117907732` 已通过 11 项测试；该提交只完成容器契约，不包含平台 producer 或产品 runtime。
  - 状态（2026-08-29）：macOS Phase 0 producer 已使用最新稳定版 `objc2-game-controller 0.3.2` 枚举 `GCExtendedGamepad`，把连接/断开/按钮放入可靠 FIFO，把六轴放入 `{device_id, generation, axis}` latest-values，并处理后台投递策略、slot 复用、迟到 callback、断开丢弃和 shutdown。30 项 library test 中的 10,000-axis flood 不阻塞按钮 release；本机 1 秒 framework smoke 完成 37 次枚举和干净恢复全局策略，但 `observed_controllers=0`，物理手柄和产品 runtime 仍待完成，因此总项保持未勾选。
  - 状态（2026-08-29）：Windows Phase 0 producer 已把 XInput 0–3 slot 的连接/断开/标准按钮映射到可靠 FIFO，把六轴映射到 generation-keyed latest-values；33 项 library test 覆盖全范围归一化、10,000-axis flood、overflow Reset、断开丢弃、slot 重连和 shutdown。x64/ARM64 MSVC check 已通过；commit `b6bbd73` 的 push run `33260707799`、job `99122041439` 与 PR run `33260709475`、job `99122046077` 均通过真实 XInput API smoke，push job 完成 124 次无错误 slot 查询并干净关闭。runner `peak_connected=0`，物理手柄和产品 runtime 仍待完成，因此总项保持未勾选。
  - 状态（2026-08-30）：正式 runtime 已增加独立 cursor latest-value 单槽，每 `16 ms` 或
    可靠 command 到达时消费；10,000 sample flood 满足
    `published = coalesced + consumed + pending`，且不会延迟可靠 KeyUp。正式 macOS producer
    的 callback 只覆盖原始坐标槽，run-loop worker 在 callback 外查询 active display viewport；
    启动位置与后续移动均进入 runtime，并驱动 Live2D pointer/head/eye 参数。Windows 正式
    producer 同样只在 Raw Input callback 标记 movement，worker 在 callback 外查询 cursor
    和 monitor viewport 后进入该单槽。gamepad axis 的正式 runtime/product producer 仍未
    接入，因此总项保持未勾选。
- [ ] 队列溢出必须计数、记录并触发安全恢复。
  - 状态（2026-08-28）：`spikes/input-queue/` 的 `push_with_overflow_reset` 已固定溢出返回原事件、清空不可信缓存、注入 `Reset` 并记录恢复/丢弃计数；`spikes/runtime-contract/` 已将同一策略应用到 typed command queue 并通过 worker snapshot 暴露诊断；runtime producer、实际容量和输入/command sequence 仍待产品实现。
  - 状态（2026-08-30）：产品 `InputProducer` 已聚合 enqueued、queue full、overflow 后
    recovery 和 stopped 数量，所有 clone 共用 sequence；被拒事件消耗 sequence，使下一次
    成功 publish 在 runtime 触发 gap Reset，显式 recovery Reset 保留 `QueueOverflow`
    原因且只计一次。macOS 正式 callback 已改用该 producer；Windows 正式 callback 尚未
    接入，故保持未勾选。
- [ ] 动画、长按和延迟统一使用 Instant。
- [ ] 实现可注入 clock 和确定性 tick。
- [ ] 实现 starting、ready、degraded、stopping、stopped 状态。
- [ ] 实现 shutdown drain、超时和错误聚合。
- [ ] command 定义幂等性和重复提交语义；有副作用的长操作使用 operation id 去重。
- [ ] runtime tick 设置工作预算，模型解析、磁盘、音频初始化和 GPU 上传不得阻塞实时队列。
  - 状态（2026-08-28）：`spikes/runtime-contract/` 已通过 14 项测试，覆盖状态机、单调 tick、operation 去重、typed bounded worker、递增 snapshot revision、sequence gap/duplicate、overflow Reset、shutdown drain/timeout、command error 和 panic/join 诊断；产品 runtime 的输入、模型、配置服务、工作预算和真实线程 owner 仍待 Phase 1/2。

### 3.2 输入语义

- [ ] 分离 PhysicalKey、布局字符和显示名称。
  - 状态（2026-08-30）：正式 runtime 的 `PhysicalKey` 使用平台无关 USB HID usage，
    已与字符输入分离；布局字符和本地化显示名称类型尚未进入设置 UI。
- [ ] 定义左右手、组合键、repeat、单键模式和自动释放语义。
- [ ] 定义鼠标按钮、滚轮、移动和拖动语义。
- [ ] 定义手柄按钮、axis、trigger、dead-zone 和断开复位。
- [x] 每个 pressed key 记录来源、按下时间和最后校正时间。
  - 验收证据（2026-08-30）：runtime owner 的私有 `PressedRecord` 保存 `InputSource`、
    `MonotonicMillis pressed_at` 与最近一次仍按下校正时间；单元测试固定三字段，并确保
    具体键值不进入公开诊断 snapshot。
- [ ] 每个 pressed key 最终经 KeyUp、reconcile 或 Reset 释放。
  - 状态（2026-08-30）：产品状态机已覆盖 captured/reconciled Up、连续两次缺失确认、
    lifecycle Reset、sequence gap 和非单调时间恢复；issue #47 合成回归不会残留按键。
    正式平台 producer 与周期 scheduler 接线及 Windows 实机场景仍待完成。
- [ ] 实现 fixture runner 和规范化 snapshot 比较。
  - 状态（2026-08-29）：`spikes/fixture-runner/` 已用 Rust 强类型解析并执行全部 9 组共享 fixture，在 24 个 checkpoint 比较完整规范化 snapshot，且已接入 Phase 0 Linux contract matrix；产品 runtime 的 `InputEvent`/`RuntimeSnapshot` 尚未建立，因此本项保持未勾选。

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

- [x] 创建 listen-only CGEventTap 和专用 run loop/source。
  - 验收证据（2026-08-30）：正式 `MacInputService` 在独立 worker 上创建 session-level
    listen-only tap 和 CFRunLoop source；同进程合成 Shift down/up 集成测试通过后，stop
    禁用 tap、移除 source、发布最终 Reset 并 join worker。
- [x] 映射 keycode、flags changed 和左右修饰键。
  - 验收证据（2026-08-30）：macOS virtual keycode 映射为稳定 USB HID usage；
    `FlagsChanged` 结合事件 flags 和 callback-time modifier set 区分左右修饰键方向，
    unit test 与 left Shift callback→runtime 集成测试均通过。
- [ ] 处理 tap timeout、user disable、权限变化和自动重建。
- [x] 通过 CGEventSourceKeyState 校正 pressed set。
  - 验收证据（2026-08-30）：正式服务按 `250 ms` 周期查询候选 key/button 的系统状态，
    连续 `2` 次缺失才由 runtime reconcile 释放；按键、修饰键和 button 31 的受控丢失
    release 测试在 Phase 0 spike 通过，正式服务的 down/up runtime 闭环亦已通过。
- [ ] 权限拒绝时进入 degraded，不产生重试风暴。
- [ ] 锁屏、睡眠、快速用户切换和 tap 重启发送 Reset。
- [ ] GameController 设备和 profile 映射进入统一事件。
  - 状态（2026-08-29）：extended profile 已映射 south/east/west/north、shoulder、trigger、menu/options、stick button、D-pad 与六个标准 axis 到项目类型；按钮阈值、axis/trigger 范围、generation、可靠 overflow Reset 和 latest-value accounting 均有 contract test。真实 controller 连接/热插拔/profile callback 尚未取得设备证据，统一产品 `InputEvent` 也尚未建立，因此保持未勾选。
- [ ] event tap callback 使用 autorelease pool/panic boundary，run loop 停止后不再触达已释放 producer。
- [ ] 明确辅助功能与 Input Monitoring 各自真正需要的能力，避免请求不必要的 TCC 权限。

### 3.5 配置 v1

- 状态（2026-08-29）：`spikes/config-store/` 已建立 typed NativeConfig、Bundle ID、Development/Production 隔离目录、snake_case 序列化、schema 校验、原子 commit probe、expected revision、OS writer lock contract、中断提交恢复 contract 和双平台真实 path resolver。Windows jobs 先后暴露只读 handle flush、强杀后锁释放延迟，以及首次启动 recovery 后立即重锁提交默认值的竞态；启动恢复以 10 ms 间隔有界重试最多 1 秒，`load_or_default` 又把 recover/read/create-default 合并到单个 guard，普通 commit 仍立即报告竞争。备份策略和 GPUI command 边界仍待产品 crate 阶段完成，详见 `docs/phase-0/config-store-spike.md`。

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

- [x] snapshot 只含不可变绘制数据和稳定资源 id。
  - 验收证据（2026-08-30）：`bongocat-render` 的 frame 只携带 `Arc<RenderResources>`/
    `Arc<RenderSnapshot>`、独立单调 transport sequence、model generation、frame number
    和可选模型提交 token；drawable、mask 与 texture
    通过不可混用的强类型 ID 关联，不含锁、平台对象、GPU handle 或 GPUI 状态。
- [x] 定义 CPU model evaluation 与 GPU upload 所有权边界。
  - 验收证据（2026-08-30）：`bongocat-live2d` 独占 mutable Cubism Model 并生成不可变
    snapshot/资源包；Metal overlay 只按 contract 创建/更新 GPU resource，不读取 model、
    runtime 配置或输入状态。标准模型 release 预览通过 178 帧 contract 传递和 177 次 present。
- [x] 双缓冲/latest snapshot，renderer 不阻塞 runtime。
  - 验收证据（2026-08-30）：单槽 latest-frame transport 实现非阻塞 publish、coalescing、
    单调 transport sequence、关闭后 drain 和 10,000 帧 accounting；producer 由正式
    runtime worker 持有，Metal overlay 只消费 immutable frame。三个预置 release 预览分别
    present `174/175/178` 帧，shutdown 后均满足 published = coalesced + consumed 且 pending=0。
  - 状态（2026-08-30）：模型 commit feedback 与 latest frame 分离为不可覆盖的可靠单槽；
    occupied/closed/stale 均有计数，普通 frame coalescing 不会丢模型提交结果。runtime 等待
    GPU token 时仍消费可靠 input edge，renderer 不持有 runtime 锁。
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
  - 状态（2026-08-30）：该能力已进入正式 `bongocat-model` 并覆盖三个预置包及
    非 ASCII/反斜杠/遍历拒绝；待完整 sidecar contract 和导入链完成后统一勾选。
- [ ] 验证 moc、texture、motion、expression、physics、pose、cdi 和音频。
  - 状态（2026-08-30）：正式 crate 已验证所有引用存在于 canonical package root，
    moc/普通文件大小、PNG header/尺寸及关联 JSON object 可读性；motion、expression、
    physics、pose、cdi 的完整结构语义仍由 spike 覆盖，尚未全部提升到产品 crate。
- [x] 拒绝路径穿越、符号链接逃逸、绝对路径和覆盖安装资源。
  - 验收证据（2026-08-30）：prepare 拒绝遍历、绝对/平台前缀和跨根 symlink；import
    进一步拒绝所有 symlink 与特殊文件，使用 `create_new` staging 文件和非覆盖目录
    rename。重复 ID 测试在第二次导入失败后验证既有用户 marker 未改变。
- [x] 限制模型总大小、单文件大小、纹理尺寸和 JSON 深度。
  - 验收证据（2026-08-30）：正式 crate 在任何 Cubism/GPU 工作前限制包总 byte、
    文件数、单文件/JSON byte、目录/JSON 结构深度和 PNG IHDR 声明尺寸；超大纹理与
    65 层嵌套 JSON 均有产品测试，所有上限集中在 `ModelPackageLimits`。
- [ ] 资源缺失/损坏返回具体错误，不使应用整体退出。
  - 状态（2026-08-30）：稳定 `ModelDiagnostic`/`ModelError` 已接入 app；集成测试证明
    缺失 moc 的新模型准备失败后，当前模型及 runtime revision 均不变。完整 sidecar
    诊断映射与 GPUI error/retry 状态仍待完成。
- [ ] 建立预置只读索引和用户模型可写索引。
  - 状态（2026-08-30）：用户侧已完成。`ModelStore` 以当前环境 `models/` 为持久事实
    来源，确定性列举 ready/invalid 条目，并提供已安装模型加载、活动模型删除保护、
    rename 后删除、环境 writer lock 及严格命名的崩溃 staging 回收。预置只读根与
    预置/用户合并视图仍待实现，因此总项保持未勾选。

### 5.2 Cubism safe layer

- [ ] 封装 Core version、logging、Moc consistency 和 Model creation。
  - 状态（2026-08-30）：Core version、Moc consistency 和 Model creation 已进入正式
    safe wrapper；Core logging callback 与稳定日志边界尚未实现。
- [ ] 用 Rust owner 保证 Moc、Model 和 buffer 析构顺序。
- [ ] 校验 parameter/part/drawable id、index 和范围。
  - 状态（2026-08-30）：正式 wrapper 已在 Model 创建时一次性验证 product parameter
    ID/range/default，按模型解析 stable index，并验证 drawable array、index、texture、
    mask、vertex、opacity/color；part 表和完整 custom parameter 诊断尚未完成。
- [ ] 模型切换使用 prepare/commit/rollback。
  - 状态（2026-08-30）：正式 runtime/Metal 产品链已实现 CPU/GPU 两阶段提交。runtime
    在候选 generation 的 texture/mesh/mask 全部由 Metal prepare 并回报匹配 token 前保留
    旧 `active_model`、Cubism owner 和 input bindings；GPU 拒绝映射为稳定
    `GpuPreparationFailed`，旧 generation 以更高 transport sequence 继续动态出帧。
    单元回归覆盖 CPU load 失败、GPU 拒绝、迟到状态不提交、等待期间 KeyUp/KeyDown 不受
    阻塞、输入越过已排队普通命令及后续有效 generation；本机真实预览完成 100 轮/300 次 standard -> keyboard ->
    gamepad 切换，343 个动态 snapshot，Metal allocation `54,427,648 -> 54,427,648` bytes。
    Windows D3D11 产品 renderer 尚未接入相同 token，因此总项保持未勾选。
- [ ] 加载失败保留当前可用模型。
  - 状态（2026-08-30）：文件解析在 runtime 外完成，只有由环境 `ModelStore` 或预置
    `PresetModelCatalog` 签发、调用方无法自行构造的 `CommittedModel` 能进入
    `ActivateModel`。runtime worker 在替换 active model 前完成 Cubism load、首轮参数求值
    和首帧 publish；损坏 Moc 切换会返回稳定 command failure，旧 model generation 继续
    出帧，随后有效切换才递增 generation。Metal renderer 又将新 generation 的 texture、
    mesh、mask target 和 canvas 组装为临时 `GpuModel`，完整验证后一次 commit；失败 prepare
    保留当前 GPU generation，300 次真实切换无 allocation 增长。正式产品链随后增加
    runtime/GPU commit token 和稳定拒绝反馈，GPU 失败时旧 Cubism/model/bindings/GPU
    generation 均保持 active 并恢复出帧；Windows 产品 renderer 与实际损坏资源注入仍待
    完成，因此两项保持未勾选。
- [ ] FFI 错误映射为稳定 Rust error code。

### 5.3 动作与状态

- [ ] 实现 parameter 默认值、保存/恢复和 clamp。
  - 状态（2026-08-30）：Core range/default 已进入类型化查询，绝对值和 normalized 写入
    拒绝非 finite、自动 clamp 并明确返回 unsupported；motion/expression 前后的参数
    save/restore 和 runtime owner 接入尚未完成。
- [ ] 实现 motion curve、fade、priority 和 completion。
- [ ] 实现 expression 混合和互斥/叠加语义。
- [ ] 实现 physics、pose、eye blink、breath 等实际需求。
- [ ] 实现键盘、鼠标、手柄到参数/动作/表情映射。
- [ ] 实现镜像、鼠标镜像和坐标归一化。
- [ ] 随机行为支持测试 seed。
- [ ] 逐项记录与旧版的可接受差异。

### 5.4 GPU 绘制

- [ ] 实现 drawable order、visibility、opacity 和 dynamic flags。
  - 状态（2026-08-30）：macOS Metal renderer 已消费每帧 Core snapshot，并按 stable
    `source_index` 更新固定 GPU buffer、重新应用 render order/visibility/opacity/color/
    mask 状态；基于 Core dynamic flags 的 dirty-only upload 与 Windows 对等实现尚未完成。
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
- [x] 用户模型只通过显式、受验证的导入进入当前环境，不扫描旧应用目录。
  - 验收证据（2026-08-30）：`bongocat-app` 不再提供任意外部目录激活入口；模型必须
    先经 `ModelStore::import` 复制、复验和 commit，随后只能按已安装 `ModelId` 加载；
    runtime 激活 command 只接受 store 签发的 `InstalledModel`。Development/Production
    两个 app 同时存活并以相同 ID 导入的测试验证目录与 lock 均互不影响。

### 7.3 跨环境隔离

- [ ] Development 与 Production 的相对目录树和 JSON schema 完全一致。
- [ ] 配置、state、模型、备份、日志、锁和单实例 namespace 均包含环境边界。
- [ ] 两个环境可同时运行，不争用 writer lock、模型目录或日志文件。
  - 状态（2026-08-30）：config store 已通过双环境进程测试；正式 app 又以相同模型 ID
    同时写入两套环境，验证 `models/` 和 `locks/models.writer.lock` 分离。日志 writer
    尚未实现，因此保持未勾选。
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
  - 状态（2026-08-29）：四 slot owner、connection generation、可靠按钮边沿、六轴合并、查询错误和有序 shutdown 已进入 Windows spike；无人值守 runner 的真实 `XInputGetState` smoke 与物理单/多手柄热插拔矩阵仍待完成。

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
   - 状态（2026-08-30）：默认 shader、bundle、Application/Edit/Window 原生菜单与编辑动作、窗口生命周期、主题、基础文本编辑/剪贴板、runtime bridge、性能基线与 AppKit AX tree/action 通过；WeType 拼音 2.2.3 已在 release `.app` 完成真实 marked-text update/commit 和已有中文前缀后的再次组合。Reset tooltip 已通过原生合成 mouse-move、500ms build 和 hover exit，modal dialog、焦点陷阱、Escape 恢复和背景语义隐藏已完成可见/AX smoke；AX value/invalid 可观察延迟 runtime 的 loading -> error -> retry/revision 恢复。ADR-0009 仍等待 Apple 拼音、物理键盘、真实 VoiceOver、物理 pointer 与 tooltip 朗读等证据。
9. [ ] `P0-GPUI-WINDOWS`：在 Windows 构建同一 spike，验证字体、IME、DPI、辅助功能和正常退出。
   - 状态（2026-08-30）：push run `33255204781`、job `99107586036` 已通过窗口、首帧、runtime、有序 shutdown 和进程外 UI Automation role/name/selection action；commit `45b8dba` 的 push run `33273470907`、job `99156013603` 又通过 modal dialog、Cancel 初始焦点、dismiss 与语义子树恢复；commit `21ee8aa` 的 push run `33291750411`、job `99204478369` 与 pull request run `33291751558`、job `99204481348` 已通过 loading、注入错误、retry 和 revision 2 恢复。runner 托管 UIA client 不提供 `AriaPropertiesProperty` 标识，不能用它验证 AccessKit `busy=true`。字体、真实 IME、DPI 切换和 Narrator 仍待 Windows 实机，因此保持未勾选。
10. [ ] `P0-OVERLAY`：GPUI 生命周期内完成 Windows D3D11/macOS Metal 透明 clear/present、错误注入和 100 次重建。

- [x] 先完成无平台依赖的 overlay lifecycle contract probe；平台窗口和 GPU 验证仍未完成。
- [x] Windows Win32/D3D11/DirectComposition owner、故障降级、析构顺序与 100-cycle 已通过既有 push/PR `windows-latest`；macOS 本机与 push/PR runner 的透明 clear/present、drawable unavailable、显式 shutdown 与 100-cycle 也已通过，并通过 `leaks` 基线消除窗口动画 retain cycle。GPUI 定时 frame source、双平台 resize、有序停止、原生 drag 状态切换及受控运行中故障恢复已实现；双平台具有 process thread 与 API 可见 GPU allocation 门禁，macOS 又以逐帧 backing-size 校正修复跨显示器后 drawable 尺寸漂移。commit `5baa6ba` 证明单次 `currentAllocatedSize` 相等不能代表无显示 compositor pool 收敛；当前按实测物理尺寸和三缓冲上限计算一个 drawable pool，commit `fd9ad85` 的 push run `33255204781`、job `99107586014` 已通过新门禁。完整 `P0-OVERLAY` 还等待 Windows 真实 swapchain unavailable、双平台真实 device-lost、driver 专项采样、物理拖动及显示器/DPI 切换。

11. [ ] `P0-INPUT-WINDOWS`：完成 Raw Input + pressed set + `GetAsyncKeyState` 校正并实测 issue #47 场景。

- [x] 完成平台无关 pressed-set contract 和 issue #47 恢复测试；Windows 采集与校正仍未完成。
- [x] Windows 系统合成 input -> `WM_INPUT` -> 故意丢 release -> `GetAsyncKeyState` reconcile 闭环已通过 push run `33249296927`、job `99092066404`；PixPin、Win+L、UAC 和物理设备矩阵仍待完成。

12. [ ] `P0-INPUT-MAC`：完成 CGEventTap 权限拒绝/授予/恢复、状态校正、GameController 和 100 次 restart。

- [x] 完成权限/tap 生命周期 contract、只读 preflight、真实 callback 和受控 disable 恢复；TCC 权限矩阵与系统自然 timeout 仍未完成。
- [x] 完成候选 pressed set 到 `CGEventSourceKeyState` 校正快照的边界和周期调度；真实 callback release 受控丢弃后的 20-cycle 闭环已通过，正式 `MacInputService` 又完成 left Shift down/up 到 runtime `ModelInputSnapshot` 的同进程集成测试。物理输入、系统自然丢事件和生命周期实测仍未完成。
- [x] 完成 GameController extended-profile producer、可靠按钮边沿、keyed axis、连接 generation、background delivery 和 handler shutdown contract；framework 无设备 smoke 已通过，物理 controller/profile/热插拔矩阵仍待完成。

13. [ ] `P0-CUBISM`：确认 SDK/许可证/binding 生成，三个预置模型完成 Core、资源和 renderer spike。

- [x] 完成平台无关 Rust model3/package parser、所有结构化 sidecar 静态 preflight、三个预置规范化索引与异常资源安全 contract；Native Core、binding、Framework 求值和 D3D11/Metal 绘制仍未完成。
- [x] 完成 6 个预置 motion3 与 15 个 exp3 的强类型结构、segment/Meta 计数、fade/parameter/blend 校验；这不代表 motion/expression 行为求值完成。
- [x] 完成 3 个预置 cdi3 的强类型 parameter/group/part 与 group 拓扑校验，并将规范化索引升级到 schema v2；跨资源 ID 以未来 Core 表为准。
- [x] 完成 physics3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；13 个历史文件只作为本地结构覆盖，不作为可分发 fixture 或行为求值证据。
- [x] 完成 pose3 静态 preflight、匿名摘要 CLI 和合成错误 contract；没有授权真实样本或 fade/link 求值证据。
- [x] 完成 userdata3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；三个预置模型没有真实 userdata3。
- [x] 完成 macOS arm64 真实 r.5 sys binding/Core probe；三个预置 Moc 各 100 次生命周期、drawable 与 r.5 offscreen 数组边界、legacy count 对照和 `leaks` 0-byte 门禁通过。Windows x64/macOS x64 ABI、非零 offscreen fixture、产品 safe wrapper、Framework 求值与 renderer 仍未完成。
- [ ] 取得可分发授权的 physics3/pose3 fixture 后完成强类型结构和 Framework 求值；三个预置模型不含这两类资源，不得以合成样本冒充兼容证据。

14. [ ] `P0-GO-NO-GO`：汇总证据、阻塞和条件，形成完整功能与 stable 发布决议。
    - 状态（2026-08-30）：ADR-0011 已形成 `IMPLEMENTATION GO WITH RELEASE CONDITIONS`，允许建立正式 workspace；这不勾选完整 Phase 0 决议。标准 Native `5-r.5` ZIP/hash、产品 safe wrapper、macOS Metal 三预置模型绘制和 Windows x64 交叉 check 已验证；Framework 求值、D3D11 模型绘制、其他原生 ABI、GPUI 辅助功能/IME 与双平台物理输入/GPU 矩阵继续阻塞对应功能声明，最终合规清单只阻塞 stable 发布。

15. [x] `P1-RUNTIME-CONFIG`：建立正式 workspace，提升 runtime 生命周期、强类型 command/snapshot 与 Development/Production 配置隔离闭环。
    - 依赖：ADR-0011、`spikes/runtime-contract/`、`spikes/config-store/`。
    - 退出条件：workspace 默认命令通过；环境由构建产物固定；两个数据根无读取、写入或锁 fallback；runtime 正常启动、更新 snapshot、拒绝队列溢出并有序 shutdown。
    - 验收证据（2026-08-30）：`native/` 仅包含 app/runtime/config；11 项单元测试覆盖严格 schema、共享默认 fixture、原子写入、revision 冲突、双环境根、typed snapshot、队列满返回原 command 和 shutdown。Development 默认构建与 `BONGOCAT_BUILD_ENV=production` 构建使用同一代码、不同编译期常量；format、Clippy、test 和 release check 本机通过，三平台 CI 已配置。

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
