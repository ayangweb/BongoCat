# BongoCat Native Rewrite Implementation TODO

状态：Phase 0 证据补齐与 Phase 1 渐进实现并行
最后更新：2026-09-05
当前分支：`next`
首发平台：Windows 10 1903+、macOS 12+
后续评估：Linux

> 执行基线：应用代码使用 Rust 2024 edition；GPUI 负责设置 UI；主猫窗口由 Rust 平台模块直接创建，不嵌入 GPUI renderer；Windows 使用 Raw Input + D3D11，macOS 使用 CGEventTap + Metal；官方 Cubism Core 是唯一厂商二进制/FFI 例外。生产产物不包含 Tauri、WebView、Vue、React 或 JavaScript runtime。

> 应用与存储基线：Bundle ID 固定为 `com.ayangweb.bongo-cat`；Development/Production 使用相同 schema 和不同数据根；新配置使用 `snake_case` 自有命名，不读取或导入旧 Tauri/Pinia 配置。

> 初始版本基线：`next` 只开发全新的首版，当前完整配置、state 和内部持久格式统一从 v1 开始。
> 首次正式发布前不实现版本迁移、schema 兼容、旧数据转换或历史版本判断；新增字段直接修改当前
> v1。保留版本字段和严格的当前版本解析入口，首次发布后的后续版本再以实际发布基线设计迁移。

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
- [ ] `next` 的配置、state 和内部持久格式保持 v1，不包含开发中间版本的迁移或兼容分支。

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
  - 状态（2026-09-01）：在 macOS 26.5.2 使用仓库锁定的 pnpm 依赖运行 `pnpm build` 成功；
    Vite 完成 4,406 个模块的 production bundle，`dist/` 生成 18 个资源文件，随后图标脚本
    完成 macOS/Windows/移动端图标生成。该命令只验证旧前端静态构建，Tauri native 编译、安装
    和运行仍待隔离环境复核，因此本项保持未勾选。
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
  - 状态（2026-08-31）：run `33407515845` 的 Windows Native 单测已通过 XInput
    trigger/shoulder 回归，但产品 smoke 在 settings snapshot 替换 AccessKit 节点期间对旧 UIA
    element 调用 `Toggle()` 得到瞬时 `Unrecognized error`。runner 现为两次 action 和状态轮询
    重新按 name 解析当前节点，并分别使用 2 秒 action/5 秒投影上限；action 未执行、状态未变化
    或未恢复仍失败。commit `119ea66` 的 run `33408664176`、Windows Native job
    `99542490478` 已通过 role/value、两次 action、状态恢复和 focus；真实 Narrator 证据仍待
    完成，因此保持未勾选。
- [x] 记录首次打开、空闲 CPU、RSS 和二进制增量；`docs/benchmark/data/gpui-settings-macos-248a770-*.csv` 保存原始样本，方法、环境和限制见 `docs/phase-0/gpui-settings-spike.md`。
- [x] 安装并固定 macOS Metal Toolchain，验证 GPUI 默认预编译 shader 路径；`runtime_shaders` 不作为发布配置。
- [ ] 将 macOS spike 打包为最小 `.app`，验证 bundle id、菜单、激活、关闭和辅助功能树可被系统识别。
  - 状态：Bundle ID `com.ayangweb.bongo-cat`、菜单、激活、关闭/重开、退出、WeType 拼音组合提交与最小内容 AX tree/action 已通过；真实 VoiceOver、Apple 拼音和 error/loading 宣读仍待完成，因此保持未勾选。
- [ ] 生成 Windows spike 可执行文件，验证 MSVC、Windows SDK、D3D shader 工具和 manifest 前置条件。
- [x] 跟踪 `block 0.1.6`、`proc-macro-error2 2.0.1` future-incompatibility；`docs/phase-0/future-incompatibility.md` 记录 macOS 输入产品边界已迁移到 `objc2-core-graphics`，ADR-0011 允许 GPUI 精确锁定图进入最小产品窗口，但两条 warning 继续阻塞受影响的未来 Rust 工具链与 stable 发布，解除需上游升级或审计 patch。
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
  - 状态（2026-08-31）：run `33329882403` 与 `33330226417` 又稳定复现三批预热均为 7、
    首个等长测量批次才变为 8，证明固定预热高水位仍会误报延迟系统 worker。线程门禁现将
    首个测量批次纳入 baseline，后续两个等长批次不得继续突破；单元回归固定 `7 -> 8,8`
    通过、`7 -> 8,9` 失败。本机 release 100-cycle 结果为三批测量均 8、window/owner
    `0 -> 0`、Metal `393216 -> 393216`；新 CI 与 driver 专项证据仍待完成。
  - 状态（2026-08-31）：Windows push run `33344285573`、job `99345359251` 在完整
    100-cycle 预热后仍于首个等长测量批次观察到线程 `8 -> 9`，而同提交 PR job
    `99345364672` 通过，确认一次瞬时 before/after 也会误报延迟 D3D/DirectComposition
    worker。Windows probe 现让首个测量批次建立最终线程高水位，后续两个等长批次不得
    再突破；handle 与 DXGI local memory 仍从预热后跨全部 300 个测量 cycle 执行增长门禁。
    新 CI 与 driver 专项证据仍待完成。
  - 状态（2026-08-31）：run `33365732170`、job `99405876909` 又在第三测量批次才观察到
    系统线程池 `8 -> 9`，而窗口/GPU/handle、正常绘制和两类 renderer recovery 均已通过，证明
    固定“首批建立最终基线”仍依赖 worker 创建时机。Windows probe 现执行至少 3、最多 6 个
    等长 batch，每次 high-water 增长后要求连续 2 批稳定；持续逐批增长无法在上限内收敛并失败。
    handle 与 DXGI local memory 继续从预热后跨全部测量区间执行增长门禁。commit `b947816`
    的 pull request run `33366352371`、job `99407700219` 在 4 个测量 batch 后通过：线程
    `8 -> 9` 后稳定、handle `194 -> 194`、DXGI local memory `0 -> 0`、400 个非空帧且
    clean shutdown；三平台 workspace 与其余 jobs 同时全绿。driver 专项长期采样仍待完成。
  - 状态（2026-08-31）：run `33406956326` 的 macOS job 也在第 3 个测量批次才从线程 7
    增至 8；normal/recovery smoke 已分别以 67/81 帧通过，失败仅来自旧的固定三批线程门禁。
    macOS probe 现与 Windows 一致，最多执行 6 个等长 batch，每次 high-water 增长后要求连续
    2 批稳定，逐批增长仍失败；输出和 CI 按实际 `measurement_batches` 校验非空帧守恒。本机
    release 100-cycle 以 3 批、300 帧、window/owner `0 -> 0`、thread `7 -> 7`、Metal
    `393216 -> 393216` 通过。commit `119ea66` 的 run `33408664176`、macOS spike job
    `99542490704` 也以 3 批、300 帧、window/owner `0 -> 0`、thread `8 -> 8` 通过；Metal
    `3145728 -> 5242880` 未超过一个三缓冲 pool 预算。driver 专项长期采样仍待完成。
- [ ] 验证退出顺序：frame source -> renderer -> GPU -> overlay -> GPUI。
  - 状态（2026-08-29）：GPUI executor 上的 60 Hz 定时 frame source 已连续驱动双平台 renderer，并在退出时通过停止确认后才释放 renderer/GPU/window；macOS 本机与 Windows hardware D3D11 runner 均已验证连续帧、resize、hide/show 和有序退出。生产 display-linked frame source 与 runtime 尚未接入，因此保持未完成。
  - 状态（2026-08-31）：修复 headless runner 将多个 GPUI timer 同批唤醒时 auto-quit
    抢先停止 frame source 的竞态；有界退出现先等待 resize，故障注入时还等待 renderer
    recovery，超时仍由原有 teardown 断言失败。本机 normal/recovery smoke 分别提交 65/80
    帧，均 `resize_completed=true`，recovery 路径为 `failures=1 recoveries=1`。commit
    `119ea66` 的 run `33408664176` 已由 Windows spike job `99542490539` 验证 normal 68 帧、
    device/surface recovery 70/63 帧，并由 macOS spike job `99542490704` 验证 normal 56 帧、
    recovery 73 帧；均在 teardown 前完成 resize/recovery，因此不改变总项状态。
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
- [x] Windows D3D11 绘制预置模型的 texture/order/alpha/mask。
  - 验收证据（2026-08-31）：commit `4b35d3a` 将三个预置模型的真实 Core drawable 与纹理
    接入正式 Win32/D3D11/DirectComposition overlay；renderer 按 `(render_order, id)` 排序，
    实现 normal/additive/multiplicative blend、预乘 alpha、multiply/screen color、逐 drawable
    mask render target 与 inverted mask，并在首帧和每次切模后执行 staging texture 非空像素
    readback。commit `a778c5d` 及后续资源稳定性修复加入三预置事务切换、失败 GPU prepare
    保留与 thread/handle/GPU 门禁；push run `33338724170`、Windows job `99330269568` 完成
    311 帧、309 个动态 snapshot 和 9 次正式三模型切换，最终 standard 为 21 drawables、
    5 masked drawables、3 textures，且 `failed_gpu_prepare_preserved=true`。D3D11 debug layer、
    真实 device-lost 和 driver 专项矩阵仍由 overlay/发布门禁继续跟踪，不反向取消本子项。
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
  - 状态（2026-08-30）：6 个预置 motion3 已完成实际时间求值，三个 model3 声明的
    9 个 exp3 已进入正式 runtime 并以真实 Core/drawable 验证 Add 淡入与替换；合成
    Core 测试另覆盖 Multiply/Overwrite。15 个预置 exp3 均已有结构门禁。本机 13 个历史
    physics3 仍只以匿名只读方式通过静态 parser，合成 pose3 也仅固定结构拒绝边界；
    physics/pose 的实际求值和可分发真实 fixture 尚未完成，因此总项保持未勾选。
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

- [x] GPUI 设置窗口与原生 overlay 可同时运行、关闭和重开。
  - 验收证据（2026-08-31）：commit `9365eda` 的 push run `33333789799` 中，Windows job
    `99316966532` 通过真实 `WM_CLOSE`、保留 Entity 隐藏、后台 frame tick、同一 Entity 重显、
    revisioned snapshot 恢复和全部产品 owner 有序 shutdown；macOS job `99316966517` 通过
    Entity 销毁、单一新 Entity 重建和同等 lifecycle，Ubuntu job `99316966591` 通过共享门禁。
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
    30 秒用于有界开发 smoke。正式 GPUI 最小设置窗口现与产品 overlay/runtime 共存，
    有界 service worker 提供 revisioned snapshot、显隐/音效持久化和显式 shutdown；
    installed-model 选择、窗口重开和跨平台多模型切换确认仍属后续任务。
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
  - 状态（2026-08-31）：正式 crate 已完成 Core 版本门禁、Moc/Model safe owner、
    drawable snapshot、parameter id/range/default、motion3 curve/fade 和 exp3
    Add/Multiply/Overwrite/transition 求值；三个预置模型均在加载阶段缓存所有声明的
    motion/expression，不向上暴露 raw pointer。motion 主动 stop fade、PartOpacity 以及
    EyeBlink/LipSync/Opacity Model target 已进入正式 runtime/render contract；UserData 也已
    进入跨帧、循环去重和有界诊断 contract。physics 与 pose 求值尚未实现，因此总项保持未完成。
- [x] 创建 bongocat-audio：motion 音效 command、FLAC、设备 owner 和 shutdown。
  - 验收证据（2026-08-31）：ADR-0012 精确锁定最新稳定版 `rodio 0.22.2`，只启用
    `playback + flac`；独立 worker 以容量 16 的强类型队列管理唯一 voice，runtime 只做
    非阻塞 publish。真实预置 FLAC decoder、缺失/损坏文件、设备 backend 错误映射、
    motion 抢占、无 sound、显式停止、配置禁用、成功切模、overflow backlog 恢复、迟到
    command 和 shutdown/join 均有自动化 contract；失败只进入匿名 diagnostics。
- [x] 创建 bongocat-render：render snapshot 和 renderer contract。
  - 验收证据（2026-08-30）：正式 crate 已从 Cubism 边界接管 `RenderSnapshot`、
    `RenderResources` 和强类型 `DrawableId`/`TextureId`，并提供带 model generation/
    frame number 的 latest-frame transport。10,000 帧测试验证 coalescing/accounting，
    close 后可 drain pending 且拒绝迟到帧，倒退 frame/generation 会显式失败；正式
    Live2D 与 macOS Metal overlay 已改用该 contract。
- [ ] 创建 bongocat-ui：GPUI 页面和 design system。
  - 状态（2026-08-31）：正式 crate 已建立平台无关的有界 typed command/reply、稳定错误码、
    revisioned snapshot 与 closed-service contract；Windows/macOS GPUI 最小窗口提供真实
    loading/error/disabled 状态、可见焦点、系统明暗配色、overlay 显隐和 motion audio
    switch。完整基础控件、页面、AccessKit adapter、IME/本地化和窗口重建尚未完成，
    因此保持未勾选。
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
  - 状态（2026-09-01）：`native/rust-toolchain.toml` 已固定 Rust `1.97.1`、`clippy` 和 `rustfmt`，`native-toolchain` job 也验证当前 stable 与该版本；Windows ARM64 desktop Core、macOS Intel 发布形式和完整 target 发布矩阵仍待外部证据，因此保持未勾选。
- [x] 在 workspace manifest 声明 `rust-version`，CI 验证最低版本和当前 stable，不依赖开发机偶然安装的 nightly。
  - 验收证据（2026-09-01）：`native/Cargo.toml` 的 workspace package 声明 `rust-version = "1.97"`，全部 Native crate 继承该字段；`native-toolchain` job 对当前 stable 和 `1.97.1` 均执行 `cargo check --locked --workspace`。
- [x] 禁止应用依赖未固定 git branch，提交 Cargo.lock。
- [x] 平台依赖使用 target-specific dependency，Windows feature 不进入 macOS，macOS framework 不进入 Windows。
  - 验收证据（2026-09-01）：`bongocat-platform`、`bongocat-overlay`、`bongocat-audio`、`bongocat-runtime`、
    `bongocat-ui` 和 `bongocat-app` 的平台依赖均位于 target-specific manifest；三平台 Native workspace
    CI 的 locked check/Clippy/test/release 组合验证了目标条件解析。
- [x] 审查 Cargo feature union，禁止测试/诊断/运行时 shader feature 意外进入 release 产物。
  - 验收证据（2026-09-01）：正式 workspace 只有 Development-only `storage-test-injection`
    feature；`bongocat-app` 在 Production cfg 下以编译期错误拒绝该 feature，默认 release/
    Production 构建不携带测试存储根覆盖；workspace 没有 runtime shader 或诊断 release feature。
    CI 的 `--all-features` Clippy 与默认 release/Production check 分离执行，并通过组合拒绝回归。
- [x] 业务、配置、模型和 UI crate 使用 forbid unsafe_code。
  - 验收证据（2026-09-01）：runtime/config/model/UI/app/audio/render/live2d 源入口均声明
    `#![forbid(unsafe_code)]`；overlay 在非 Windows/macOS 共享编译路径增加同一门禁，平台 FFI 仅保留
    target-specific wrapper，Native workspace Clippy/test/release check 通过。
- [x] 平台 unsafe wrapper 写明线程、指针、所有权和析构不变量。
  - 验收证据（2026-09-01）：`bongocat-platform`、`bongocat-overlay` 和 `bongocat-live2d`
    的每个非平凡 `unsafe` block 都有紧邻的 `SAFETY:` 不变量说明，覆盖主线程/owner thread
    限定、裸指针与 slice 的有效范围、COM/AppKit/Metal/Cubism handle 所有权及析构顺序；
    共享业务 crate 继续使用 `#![forbid(unsafe_code)]`。静态扫描未发现缺少说明的 block，
    Native 三平台 Clippy `-D warnings`、workspace tests 和 release check 通过。
- [x] 配置 rustfmt、Clippy -D warnings、cargo test 和许可证检查。
  - 验收证据（2026-09-01）：Native 三平台 workflow 执行 locked format、workspace Clippy `-D warnings`、
    workspace tests、release check 和 pinned dependency policy；本机同命令通过。
- [x] 配置 `cargo deny`/等价检查：license、advisory、banned source、重复高风险依赖和 unknown registry。
  - 验收证据（2026-09-01）：`tools/check-native-dependencies.sh` 固定 `cargo-deny 0.20.2`，对 Native
    workspace 和独立工具执行 locked license/source policy，workflow `33480729115` 及后续 run 通过。
- [x] 配置 panic hook 和 release 可诊断退出。
  - 状态（2026-09-01）：正式 `Application` 入口在完成日志 writer 初始化后安装可恢复的
    process panic hook；hook 只写固定 `application/error/panicked` JSONL 事件，不读取 panic
    payload、源码位置或 backtrace，并使用非阻塞锁避免二次 panic/死锁。`ApplicationPanicHook`
    在 owner drop 时恢复之前的 hook；单元测试覆盖含用户路径 payload 的脱敏、日志锁占用时
    直接丢弃和 hook 恢复。新增环境隔离的持久运行标记：只有完整 runtime/audio shutdown 才清理，
    下次启动会记录匿名 `previous_run_unclean` 事件；panic、shutdown 错误和强制终止会保留标记。
    Diagnostics 导出格式版本已提升到 2，并增加固定事件 code 的聚合计数；不导出原始日志、panic payload 或路径；
    release 实机崩溃收集仍待完成，因此本项保持未勾选。`bongocat-app` 63 项 app/lib 测试和
    app Clippy 已在本机通过；marker 逻辑随 commit `19afddf`（远端合并提交 `b6244cc`）进入 `next`。
  - 状态（2026-09-04）：新增 Development-only 隔离父/子进程 smoke，以同一 executable 在正式
    Application owner 存活时触发 panic；父进程验证固定 panic code、payload/路径脱敏、配置字节
    不变、unclean 重启分类及正常 shutdown 清除 marker。本机 debug 行为闭环通过，双平台
    `panic=abort` release runner 已由 `P3-PANIC-DIAGNOSTICS-RELEASE` 完成。
- [x] 定义线程、任务、channel、窗口和 GPU object owner。
  - 验收证据（2026-09-01）：Technical Design 第 8 节冻结 runtime、输入 producer、GPUI
    主线程、frame source、renderer/GPU 和 settings service 的 owner 边界及 shutdown 顺序；
    正式 app coordinator、runtime、overlay/platform adapter 与 settings worker 分别持有这些
    owner，跨线程只使用有界 typed channel/latest-value transport。Windows/macOS release
    lifecycle、overlay recovery、model switch、recovery window 和显式 Quit smoke 均验证
    stop -> runtime/config -> audio/renderer/GPU/overlay 的 join 与析构顺序；Native 三平台
    workspace format、Clippy、test 和 release check 通过。平台真实驱动、权限和长时 soak
    仍由对应 Phase 0/8 门禁跟踪，不扩大本项完成范围。
- [x] 建立结构化日志字段和用户路径脱敏规则。
  - 验收证据（2026-09-01）：Application sink 只接受固定 component/level/code 字段，Cubism Core
    callback 使用独立结构化 sink 并将路径、换行和超长消息脱敏/截断；panic hook 不读取 payload，
    Diagnostics 导出只包含匿名统计与固定事件计数。app/Core 单元测试覆盖路径脱敏、长度上限、
    callback panic boundary、日志轮转和导出无路径，三平台 Native CI 通过。
- [x] 提供开发/测试所需 Cubism 二进制的可验证安装说明。
  - 验收证据（2026-09-01）：`docs/phase-0/cubism-sdk-source-and-license.md` 第 4 节提供
    维护者人工接受 Live2D 协议后下载固定 `5-r.5` ZIP、校验 archive/header/Core SHA-256、
    运行离线 inspector、生成并审阅 target bindings、执行 Core/模型 ABI smoke 的逐步流程；
    完整 SDK 保存在仓库外，普通构建、CI 和打包不联网或下载 artifact。`native/README.md`
    同步说明固定 vendor 基线和离线构建边界。第二来源复核、Windows/macOS 全 ABI 以及最终
    再分发授权仍由 P0-CUBISM/stable 发布门禁跟踪，不扩大本项完成范围。
- [x] 构建脚本默认不联网；外部 SDK、shader compiler 和生成器必须先由显式 bootstrap 步骤准备。
  - 验收证据（2026-09-01）：正式 `bongocat-app`/`bongocat-live2d` build script 只读取显式
    build environment、本地 vendor header 和已提交资源，不执行下载或网络命令；macOS packaging
    只调用本地 Cargo、provenance 和 bundle 工具。Cubism inspector、bindgen 与 Core probe
    均是离线 CLI，要求维护者先准备并校验 SDK，CI 不下载 Cubism、shader compiler 或生成器。
    Native 三平台 locked format、Clippy、test、release/Production check 及依赖策略通过。
- [x] 定义 debug、release、profiling 三种 profile，profiling 产物不得误发布。
  - 验收证据（2026-09-01）：`native/Cargo.toml` 显式定义 dev（debug、incremental、unwind）、
    release（symbols stripped、LTO、abort）和 profiling（继承 release、保留完整 debug、关闭
    LTO）profile；CI 与打包入口只使用 release，provenance 记录 profile，profiling 不进入发布
    workflow。三平台 Native workspace release check 通过。

### 2.3 CI

- [x] Windows：format、Clippy、unit test、release check；GPUI settings/overlay spike 已由 GitHub `windows-latest` 执行。
  - 验收证据：commit `221f5483976b64b7cbf6c5818ee5714ad47de479`，push run `33182146480` 与 pull request run `33182148815` 均成功；不代表 Windows 字体、IME、DPI、辅助功能或图形实机验收完成。
- [x] macOS：format、Clippy、unit test、release check；GPUI settings/overlay spike 均纳入 `macos-spikes` job。
- 状态（2026-09-05）：run `33940224182` 的 Windows Native job `101236050267` 在隔离
  storage smoke 产物冷编译阶段耗尽原步骤 10 分钟时限，尚未启动恢复窗口。CI 现将两平台
  Development-only release 测试产物的构建拆为独立 30 分钟步骤，恢复、state 与 panic smoke
  直接执行该产物并各限制为 2 分钟；Windows 内部 10 秒窗口发现、15 秒退出和 20 秒 state
  恢复期限不变。YAML 语法与 whitespace 检查本机通过；Windows 冷构建和图标资源退出条件
  仍等待更新后的原生 CI，不以扩大编译预算代替产品运行验收。
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
  - [x] `tools/record-native-provenance.py` 生成无绝对路径的 JSON；Native 三平台 CI 上传 runner
        provenance，macOS `.app` 将其放入 `Contents/Resources/build-provenance.json`。工具测试验证
        commit、锁文件 hash、toolchain、target、profile、feature set 和 environment 字段；签名安装包
        与 Windows 最终发布 artifact 仍待发布 workflow 迁移后接入。commit `6c6120b` 的 Native
        workflow run `33479155904`（后续 `d6d27b3`/`33479624906`）三平台 provenance artifact 已成功上传。

### 2.4 Phase 1 退出门槛

- [ ] Windows/macOS debug/release 骨架均可构建。
- [ ] GPUI 空设置窗口可打开，overlay 可显示测试帧。
  - 状态（2026-08-31）：macOS 本机已提升为正式设置窗口 + 真实 Cubism/Metal 模型绘制并
    通过 release 有界 smoke；Windows x64 hardware CI 与正式窗口截图仍待当前提交验证。
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
    producer 现由 runtime worker 持有并随 shutdown 关闭；`StartMotion`/`StopMotion` 使用
    强类型 motion identity 和 priority，完整 product command 集仍待实现。
- [ ] 单一 runtime owner 管理可变业务状态。
  - 状态（2026-08-30）：正式 runtime worker 已独占 overlay、pressed input、输入诊断、
    已提交模型和 mutable Cubism model evaluation，并只发布不可变 runtime/render snapshot；
    Cubism 对象在线程内创建，未使用 `unsafe impl Send/Sync`。应用与 UI client 只通过有界
    typed command 和 snapshot 访问；motion playback 也由该 worker 独占并发布
    `ActiveMotionSnapshot`，expression/physics/pose 动画状态仍待接入。
- [ ] key/button edge 和 command 使用可靠有序队列。
  - 状态（2026-08-30）：正式 `ApplyInput` 与其他 command 共用有界 FIFO，input event
    另带独立单调 sequence；`InputProducer` 以非阻塞 publish 返回原始拒绝事件，并向
    app/platform 暴露 recovery API。macOS/Windows 正式 producer 均已接入；command 与
    input 共用容量 64 的产品 FIFO，gamepad producer 已在双平台平台层接入，产品实机闭环仍待完成。
- [ ] 为每个可靠队列定义容量、生产者、消费者、满载策略和关闭语义，不使用无界队列逃避背压设计。
  - 状态（2026-08-28）：`spikes/input-queue/` 已验证固定容量 FIFO、满载返回原事件、关闭 drain 和 latest-value 槽位；`spikes/runtime-contract/` 进一步验证固定容量 command queue、Condvar 唤醒、溢出 Reset、worker drain 和 join 报告；runtime 的实际容量与产品 channel 选型仍待产品 crate。
  - 状态（2026-08-30）：正式 app 当前使用容量 64 的共享 command/input FIFO，唯一
    runtime worker 消费，owner shutdown 使用可靠控制消息并 join；cursor 已使用独立单槽
    latest-value transport，停止后拒绝新 sample 并在 shutdown 消费 pending sample。
    Windows 正式 input owner 也已具备 start/stop/join 和最终 Reset；gamepad axis 通道及
    producer 生命周期已建立，实机手柄证据仍待完成，因此总项保持未勾选。
  - 状态（2026-09-01）：runtime command producer 现为 bounded FIFO 的正式匿名诊断来源，
    snapshot 记录 `enqueued`、`queue_full` 和 `runtime_stopped`，并在队列满载及 shutdown
    后发送回归中验证计数不泄露 command payload。该计数与既有 input/cursor/gamepad
    transport 诊断保持独立；双平台真实压力与手柄证据仍待完成，因此总项保持未勾选。
- [ ] edge/command 携带单调 sequence id，诊断可发现乱序、重复和丢失但不记录具体键值。
  - 状态（2026-08-29）：Windows callback queue 的 edge、Reset 和 reconcile tick 已携带单调 `u64` sequence，正常压力路径要求 gap/duplicate 均为 0，受控 overflow 以 discarded backlog 数量产生等量 gap 并由 Reset 恢复。command queue 与产品 runtime 的统一 sequence contract 仍待实现，因此保持未勾选。
  - 状态（2026-08-29）：macOS callback queue 也已为 edge/Reset 分配单调 `u64` sequence，overflow Reset 继承被拒事件序号，consumer 统计 gap 与 duplicate/out-of-order；普通 tap、timeout/user disable 和 lifecycle 本机回归均为 0。commit `d7501dc` 的 push run `33257871184` 已通过 contract job `99114627795` 及原生 macOS job `99114627654` 的 input check/Clippy/test/release 门禁。command queue 与产品 runtime 的统一 contract 仍待实现，因此保持未勾选。
  - 状态（2026-08-28）：`spikes/input-state/` 已验证可靠输入事件的重复/乱序忽略与跳号安全 reset；`spikes/runtime-contract/` 已验证 typed command sequence、跳号前 `WorkerRecovery` reset、重复/过期 sequence 丢弃和诊断计数；平台 producer、输入事件 sequence 与产品 runtime 接入仍待产品 crate。
  - 状态（2026-08-30）：产品 runtime 现分别维护 command sequence 与 input sequence；
    input 重复/乱序计数后忽略，跳号计数缺失数量、先以 `SequenceGap` Reset 再应用当前
    边沿，snapshot 只暴露聚合诊断。macOS/Windows 正式 producer 均经 `InputProducer`
    分配 sequence；正式 gamepad producer 与完整统一诊断仍待建立。
  - 状态（2026-09-01）：command queue 现由 runtime worker 使用单调 sequence tracker，
    跳号累计缺失数量后继续处理当前 command，重复或乱序 envelope 被安全丢弃；四类计数
    进入匿名 `RuntimeSnapshot.command_transport`，并覆盖 `u64` wraparound 的纯 Rust 回归。
    平台 producer 的真实压力与跨进程故障注入仍待完成，因此总项保持未勾选。
  - 状态（2026-09-01）：正式 `InputState` 也改用 wrapping-forward distance 判断 sequence，
    正确接受 `u64::MAX -> 0` 的连续边沿，并在回绕后的 gap、重复和反向 envelope 上保持确定的
    Reset/忽略语义；新增边界回归通过。双平台真实压力与跨进程故障注入仍待完成，因此总项保持未勾选。
  - 状态（2026-09-01）：tracker 改用 wrapping distance 判断序列方向，跨 `u64::MAX -> 0`
    的丢失、重复和反向 envelope 均有确定分类；模型准备期间的 deferred command 会在
    实际消费时才记账，避免被输入边沿绕行误报为乱序。runtime 46 项定向测试通过。
  - 状态（2026-09-01）：`wait_for_command`、模型准备等待和输入序列等待统一使用同一
    wrapping-forward 判定，避免序列回绕后因普通 `>=` 比较提前返回或永久等待；新增纯
    Rust 回归覆盖边界。
- [ ] cursor/gamepad axis 使用 latest-value 合并通道。
  - 状态（2026-08-29）：Windows RAWMOUSE movement 已从可靠 edge FIFO 分流到独立 latest-value 槽位；safe decoder 保留 relative/absolute/virtual-desktop 语义，16ms owner tick 在 callback 外查询当前 cursor，pointer flood 要求 captured sample 全部由 coalesced 或 consumed 解释且不影响 keyboard release。commit `098d532` 的 push run `33258305541`、Windows job `99115756881` 已通过强化后的 3072 movement/1536 keyboard edge 回归。Gamepad axis、macOS cursor 和产品 runtime 通道仍待实现，因此保持未勾选。
  - 状态（2026-08-29）：macOS `MouseMoved` 与 left/right/other drag 已分流到独立 latest-value slot，run-loop owner 约每 16ms 消费一次并在 shutdown flush；10,000-sample contract 证明 cursor flood 不占用可靠 button edge 队列，严格报告要求 `captured = coalesced + consumed` 且 close 后无迟到发布。commit `500a956` 的 PR run `33258718745` 中，原生 macOS job `99116842307` 与 contract job `99116842405` 均通过。Gamepad axis、产品 runtime 通道和物理 cursor callback 实测仍待完成，因此保持未勾选。
  - 状态（2026-08-29）：平台无关 keyed latest-values contract 已为 gamepad axis 固定容量、按 key 合并、完整 accounting、关闭语义和连接 generation；10,000 次同轴更新只消费最终值，新 key 超容量明确失败，断开后的旧 generation 不会污染复用 device id 的重连。commit `16a51bb` 的 push run `33259120950`、job `99117907732` 已通过 11 项测试；该提交只完成容器契约，不包含平台 producer 或产品 runtime。
  - 状态（2026-08-29）：macOS Phase 0 producer 已使用最新稳定版 `objc2-game-controller 0.3.2` 枚举 `GCExtendedGamepad`，把连接/断开/按钮放入可靠 FIFO，把六轴放入 `{device_id, generation, axis}` latest-values，并处理后台投递策略、slot 复用、迟到 callback、断开丢弃和 shutdown。30 项 library test 中的 10,000-axis flood 不阻塞按钮 release；本机 1 秒 framework smoke 完成 37 次枚举和干净恢复全局策略，但 `observed_controllers=0`，物理手柄和产品 runtime 仍待完成，因此总项保持未勾选。
  - 状态（2026-08-29）：Windows Phase 0 producer 已把 XInput 0–3 slot 的连接/断开/标准按钮映射到可靠 FIFO，把六轴映射到 generation-keyed latest-values；33 项 library test 覆盖全范围归一化、10,000-axis flood、overflow Reset、断开丢弃、slot 重连和 shutdown。x64/ARM64 MSVC check 已通过；commit `b6bbd73` 的 push run `33260707799`、job `99122041439` 与 PR run `33260709475`、job `99122046077` 均通过真实 XInput API smoke，push job 完成 124 次无错误 slot 查询并干净关闭。runner `peak_connected=0`，物理手柄和产品 runtime 仍待完成，因此总项保持未勾选。
  - 状态（2026-09-01）：正式 runtime 已增加独立 cursor latest-value 单槽，每 `16 ms` 或
    可靠 command 到达时消费；10,000 sample flood 满足
    `published = coalesced + consumed + pending`，且不会延迟可靠 KeyUp。正式 macOS producer
    的 callback 只覆盖原始坐标槽，run-loop worker 在 callback 外查询 active display viewport；
    启动位置与后续移动均进入 runtime，并驱动 Live2D pointer/head/eye 参数。Windows 正式
    producer 同样只在 Raw Input callback 标记 movement，worker 在 callback 外查询 cursor
    和 monitor viewport 后进入该单槽。gamepad axis 已接入双平台服务，实机手柄证据仍待完成，
    因此总项保持未勾选。
- [ ] 队列溢出必须计数、记录并触发安全恢复。
  - 状态（2026-08-28）：`spikes/input-queue/` 的 `push_with_overflow_reset` 已固定溢出返回原事件、清空不可信缓存、注入 `Reset` 并记录恢复/丢弃计数；`spikes/runtime-contract/` 已将同一策略应用到 typed command queue 并通过 worker snapshot 暴露诊断；runtime producer、实际容量和输入/command sequence 仍待产品实现。
  - 状态（2026-08-30）：产品 `InputProducer` 已聚合 enqueued、queue full、overflow 后
    recovery 和 stopped 数量，所有 clone 共用 sequence；被拒事件消耗 sequence，使下一次
    成功 publish 在 runtime 触发 gap Reset，显式 recovery Reset 保留 `QueueOverflow`
    原因且只计一次。macOS 正式 callback 已改用该 producer；Windows 正式 callback 尚未
    接入，故保持未勾选。
- [ ] 动画、长按和延迟统一使用 Instant。
- [ ] 实现可注入 clock 和确定性 tick。
  - 状态（2026-09-01）：正式 runtime 已使用 `MonotonicClock` 驱动动作、表情和自动效果，
    并新增 typed `RuntimeCommand::Tick` 允许 coordinator/fixture 在注入时钟下显式驱动
    单次评估；定时 loop 仍保留用于生产运行，完整动画/长按迁移和 fixture 对接仍待完成。
- [ ] 实现 starting、ready、degraded、stopping、stopped 状态。
- [ ] 实现 shutdown drain、超时和错误聚合。
  - 状态（2026-09-01）：runtime shutdown 现在先关闭 command producer gate，避免关闭开始后新
    command 进入队列；worker 以非阻塞方式排空已接收 command 后处理 shutdown，即使命令队列已满
    也不会在 shutdown timeout 内卡在发送端。新增满队列拒绝/排空 contract 已通过；超时错误
    聚合和真实阻塞工作预算仍待完成。
  - 状态（2026-09-01）：显式 timeout 现在在 deadline 到达时立即返回 `TimedOut` 并放弃 join
    handle；worker 继续异步完成已接收队列的 drain/shutdown，避免 `RuntimeOwner::Drop` 在错误
    返回后再次无界等待。新增零时限回归确认调用方有界返回且 worker 最终进入 `Stopped`；
    超时错误聚合和真实阻塞工作预算仍待完成。
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
  - 状态（2026-08-31）：正式 runtime 已接入带 device generation 的 16 个标准手柄按钮、可靠
    pressed edge、匿名计数和 Reset；六轴/trigger 的 generation-keyed latest-value、dead-zone
    与 Stick 参数投影已完成；Settings service/client 与 General 页面现可 revision-checked
    持久化 stick/trigger dead-zone。平台采集、连接/断开生命周期和实机验证仍待完成。
- [x] 每个 pressed key 记录来源、按下时间和最后校正时间。
  - 验收证据（2026-09-05）：runtime owner 的私有 `PressedRecord` 保存 `InputSource`、
    `MonotonicMillis pressed_at`、最近一次仍按下校正时间与 runtime 单调时钟观察时间；单元测试固定
    四字段，并确保
    具体键值不进入公开诊断 snapshot。
- [ ] 每个 pressed key 最终经 KeyUp、reconcile、Reset 或最终 fallback 释放。
  - 状态（2026-08-30）：产品状态机已覆盖 captured/reconciled Up、连续两次缺失确认、
    lifecycle Reset、sequence gap 和非单调时间恢复；issue #47 合成回归不会残留按键。
    正式平台 producer 与周期 scheduler 接线及 Windows 实机场景仍待完成。
- [ ] 实现 fixture runner 和规范化 snapshot 比较。
  - 状态（2026-08-29）：`spikes/fixture-runner/` 已用 Rust 强类型解析并执行全部 9 组共享 fixture，在 24 个 checkpoint 比较完整规范化 snapshot，且已接入 Phase 0 Linux contract matrix。
  - 状态（2026-09-01）：正式 `bongocat-runtime` 新增 `shared_input_fixtures` 集成测试，真实驱动
    typed `InputEvent`、cursor/axis latest producer 和 `RuntimeCommand::Tick`，对 8 组纯输入 fixture
    的 17 个 checkpoint 比较匿名计数、左右手/鼠标投影、Reset 原因和 cursor 样本；Native workspace
    CI 会随 `cargo test --workspace` 执行，且 `Gamepad*Down` 参数会映射到正式手柄/左右手投影并断言。
    状态（2026-09-01）：在 macOS/Windows 正式 runtime 集成测试中，使用三个预置模型中的
    `standard`/`keyboard` 包、真实 model commit feedback 和可注入单调时钟执行第 9 组
    `model-motion-expression-audio` fixture；5 个 checkpoint 已验证 model switch 清理、motion
    priority/stop、expression selection 以及音频触发不进入 render snapshot 的契约。音频不可用时
    motion side effect 仍被 runtime 诊断为 rejected，未阻塞动作或渲染。fixture 的物理模型轨迹、
    可用音频设备和 GPU/实机证据仍待完成，因此总项保持未勾选。

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
  - 状态（2026-09-01）：正式服务识别 timeout/user-disable 后先停止 callback 接收、丢弃未消费
    capture、向 runtime 发送 `ServiceRestart` Reset，再从同一稳定 callback context 创建并启用新的
    listen-only tap/source；旧 source 在替换前从专用 run loop 移除，`tap_restarts` 进入实时诊断。
    permission 已撤销时只结束 worker 并报告 `PermissionDenied`，不会形成重试风暴。系统自然 timeout、
    TCC grant/revoke 和 session 变化的实机矩阵尚未完成，因此总项保持未勾选。
- [x] 通过 CGEventSourceKeyState 校正 pressed set。
  - 验收证据（2026-08-30）：正式服务按 `250 ms` 周期查询候选 key/button 的系统状态，
    连续 `2` 次缺失才由 runtime reconcile 释放；按键、修饰键和 button 31 的受控丢失
    release 测试在 Phase 0 spike 通过，正式服务的 down/up runtime 闭环亦已通过。
- [x] 权限拒绝时进入 degraded，不产生重试风暴。
  - 验收证据（2026-09-01）：共享 `PlatformInputDiagnostics` 新增匿名强类型 service status 与
    `service_start_attempts`；双平台 worker 发布 Running/Stopped，overlay owner 的 `FnOnce` contract
    将 PermissionDenied/backend/其他启动失败映射为 degraded snapshot 且只调用 backend 一次。
    settings revision/health 与 Diagnostics 投影该状态。重新 ad-hoc 签名的 Development `.app` 在未获
    Input Monitoring 时真实显示 `Runtime Degraded`、`Permission required`、`Start attempts: 1`，
    overlay/settings 保持可用；800px 宽可视检查无重叠或裁剪，未触发权限请求或重试。
- [ ] 锁屏、睡眠、快速用户切换和 tap 重启发送 Reset。
  - 状态（2026-09-01）：正式 macOS worker 注册 NSWorkspace will-sleep/did-wake 与 session
    resign/active 四类公开通知；通知在 autorelease/panic boundary 内合并为原子 lifecycle signal，
    每个 run-loop slice 最多触发一次 `ServiceRestart` Reset，并复用 tap/source 重建与候选清理。
    observer token 在 worker shutdown 时先关闭 callback gate 后逐一注销。受控 signal contract 已通过，
    NSWorkspace notification-center 注册/投递/关闭 contract 也覆盖四类公开通知。加入 AppKit cold
    initialization 后 startup 独立使用 5 秒上限，shutdown/join 仍保持 2 秒；三项真实 callback/
    cursor/restart smoke 连续 3 轮通过。新增端到端 smoke 在绑定键保持 pressed 时投递公开 session
    resign 通知，验证可靠 Reset 清除状态、`recovery_resets`/`tap_restarts` 增长，且替换 tap 继续接收
    新 down/up；四项 opt-in smoke 同轮通过。真实锁屏、睡眠、快速用户切换和系统通知时序仍待
    macOS 实机矩阵，因此总项保持未勾选。
- [ ] GameController 设备和 profile 映射进入统一事件。
  - 状态（2026-08-29）：extended profile 已映射 south/east/west/north、shoulder、trigger、menu/options、stick button、D-pad 与六个标准 axis 到项目类型；按钮阈值、axis/trigger 范围、generation、可靠 overflow Reset 和 latest-value accounting 均有 contract test。真实 controller 连接/热插拔/profile callback 尚未取得设备证据，统一产品 `InputEvent` 也尚未建立，因此保持未勾选。
- [x] event tap callback 使用 autorelease pool/panic boundary，run loop 停止后不再触达已释放 producer。
  - 验收证据（2026-09-01）：event tap 与 GameController block 共用 autorelease/panic boundary；受控
    panic 被匿名计数、关闭 capture 并请求可靠恢复，不会穿越 FFI。callback context 使用稳定 Box，
    shutdown 先关闭 accepting gate、禁用 tap 并移除 source，再释放 context；unit contract 和既有
    runtime stop -> tap cleanup -> second service start smoke 覆盖恢复与析构顺序。
- [ ] 明确辅助功能与 Input Monitoring 各自真正需要的能力，避免请求不必要的 TCC 权限。

### 3.5 配置 v1

- 状态（2026-08-29）：`spikes/config-store/` 已建立 typed NativeConfig、Bundle ID、Development/Production 隔离目录、snake_case 序列化、schema 校验、原子 commit probe、expected revision、OS writer lock contract、中断提交恢复 contract 和双平台真实 path resolver。Windows jobs 先后暴露只读 handle flush、强杀后锁释放延迟，以及首次启动 recovery 后立即重锁提交默认值的竞态；启动恢复以 10 ms 间隔有界重试最多 1 秒，`load_or_default` 又把 recover/read/create-default 合并到单个 guard，普通 commit 仍立即报告竞争。备份策略和 GPUI command 边界仍待产品 crate 阶段完成，详见 `docs/phase-0/config-store-spike.md`。

- [x] 定义带 `schema_version` 的 Rust 配置结构和 JSON schema，JSON key 使用 `snake_case`。
  - 验收证据（2026-09-01）：`bongocat-config` 的 `NativeConfig`/`ApplicationState` 与
    `shared/config/config.schema.json`、`state.schema.json` 同步；serde 输出使用 `snake_case`，
    Draft 2020-12 validator 和 Native config/state fixtures 已在 workspace tests 与 CI 校验。
- [x] 区分用户配置、运行时状态和诊断数据。
  - 验收证据（2026-09-01）：用户配置写入 `config.json`，窗口状态写入独立 `state.json`，运行时
    snapshot/输入诊断只经 typed API 暴露，日志和匿名 diagnostics export 不复用用户配置结构。
- [x] 为字段定义范围、默认值和跨字段约束。
  - 验收证据（2026-09-01）：Rust `NativeConfig::validate` 与 JSON Schema 固定 FPS、缩放、透明度、
    dead-zone、超时和语言约束，并拒绝未配对的 model id/origin、未知字段和非法快捷键；对应
    valid/invalid fixtures 与 config crate tests 通过。
- [x] 在 spike 中实现不可变 `BuildEnvironment::{Development, Production}`；未知或缺失环境的打包构建失败仍待产品构建链验证。
- [x] Windows 使用 `%APPDATA%\BongoCat\<environment>\` 数据根。
- [x] macOS 使用 `Application Support/com.ayangweb.bongo-cat/<environment>/` 数据根。
  - 双平台 target-specific resolver test 已通过。
- [x] 两个环境的 `config.json`、`state.json`、`models/`、`backups/`、`logs/`、`updates/` 和 `locks/` 相对结构一致；spike 测试逐项比较相对路径。
- [x] 环境不能由 CLI、进程环境变量或设置项在运行时切换，也不能 fallback 到另一环境。
  - 验收证据（2026-09-01）：`bongocat-app/build.rs` 只在编译期读取并严格校验
    `BONGOCAT_BUILD_ENV`，将不可变 cfg 注入应用；运行时 API 只使用该 cfg 对应的
    `BuildEnvironment`，无 CLI/设置切换或另一环境 fallback。build-environment contract、
    Development/Production root 隔离和跨环境应用测试通过。
- [x] 在 spike 中实现同目录临时文件、flush、原子替换、提交后验证和上一份有效配置备份；双平台 OS file lock 与强制进程终止恢复已通过。
- [x] 在 spike 中拒绝损坏配置并保留原始文件；中断提交恢复会保守提升有效临时文件并归档无效/陈旧副本，隔离备份保留策略、默认恢复和 GPUI 用户诊断仍未完成。
- [ ] 配置写入去抖，退出前强制 flush。
- [ ] GPUI 只通过 typed command 获取 snapshot 和提交 patch。
  - 状态（2026-09-01）：正式 UI 对显隐、motion audio、模型交互和 gamepad dead-zone 使用有界
    typed command/reply，成功结果携带新 runtime revision，文件写入在 app service worker 完成；
    其余配置域尚未接入，故总项保持未勾选。
- [x] 在 spike 中以包含环境目录的持久 `locks/config.writer.lock` 拒绝并发 writer，并通过 OS advisory lock 在 guard drop 后允许重试。
- [x] 强制终止持锁进程后由内核释放 writer lock，下一进程可恢复已 flush 的临时配置且不覆盖当前配置。
  - 验收证据（2026-08-29）：macOS 本机与 Windows push run `33251278193`、job `99097261951` 均通过；平台文件权限仍待产品 crate。
- [x] 新配置文件和备份使用最小用户权限，不继承过宽 ACL/文件 mode。
  - 验收证据（2026-09-01）：`bongocat-config` 的 `StorageLayout` 创建 root、models、backups、logs、updates 和
    locks 目录时在 Unix 强制 `0700`；config/state、备份、锁和原子替换结果统一为 `0600`，覆盖
    首次创建、恢复和 verification rollback。Windows 依赖 `%APPDATA%` 用户目录 ACL，不修改系统
    ACL；Unix 权限回归测试验证目录/文件 mode，config crate 46 项测试和 Native workspace tests 通过。
    `bongocat-app` 的 application logs、轮转日志和运行标记，以及 `bongocat-model` 的 installed
    model、导入 staging 文件和 model writer lock 也在创建/重开时强制相同的 `0700`/`0600` 边界；
    app/model 权限回归测试覆盖首次创建、轮转和导入提交。Cubism Core 日志与 diagnostics
    导出同样使用 `0700` 父目录和 `0600` 原子替换文件，live2d core-log 权限回归测试通过。
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
  - 状态（2026-08-30）：正式 Windows overlay 已使用 D3D11/DXGI/DirectComposition
    绘制三个预置模型，并消费与 Metal 相同的 immutable frame、model generation 和
    commit token。完整 resize、device-loss、D3D debug layer 与实机 GPU 矩阵仍待完成，
    因此保持未勾选。
- [ ] 配置变化时切换 HWND_TOPMOST/HWND_NOTOPMOST，禁止帧轮询。
- [ ] 切换 click-through 并验证拖动模式。
- [ ] 处理 device lost、resize、休眠和 GPU 切换。
- [ ] D3D11 debug layer 无未处理 warning/error。

### 4.3 macOS Metal

- [ ] 在 GPUI/AppKit 主线程创建 nonactivating NSPanel。
- [ ] 配置透明、无标题、阴影、鼠标穿透和层级。
  - 状态（2026-09-04）：正式 macOS overlay 将 `always_on_top` 映射为高于 Dock 的
    `NSMainMenuWindowLevel`，关闭时恢复 `NSNormalWindowLevel`；设置与快捷键提交后由 runtime
    snapshot 在下一次主线程 frame tick 重建并重放当前层级，模型切换重建复用同一映射。单元回归
    覆盖 true/false，真实 Spaces、全屏辅助与设置窗口激活矩阵仍待实机完成，因此保持未勾选。
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
    snapshot/资源包；Metal/D3D11 overlay 只按 contract 创建/更新 GPU resource，不读取
    model、runtime 配置或输入状态。标准模型 release 预览通过 178 帧 contract 传递和
    177 次 Metal present，Windows 产品 smoke 也完成非空 D3D11 staging readback。
- [x] 双缓冲/latest snapshot，renderer 不阻塞 runtime。
  - 验收证据（2026-08-30）：单槽 latest-frame transport 实现非阻塞 publish、coalescing、
    单调 transport sequence、关闭后 drain 和 10,000 帧 accounting；producer 由正式
    runtime worker 持有，Metal overlay 只消费 immutable frame。三个预置 release 预览分别
    present `174/175/178` 帧，shutdown 后均满足 published = coalesced + consumed 且 pending=0。
  - 状态（2026-08-30）：模型 commit feedback 与 latest frame 分离为不可覆盖的可靠单槽；
    occupied/closed/stale 均有计数，普通 frame coalescing 不会丢模型提交结果。runtime 等待
    GPU token 时仍消费可靠 input edge，renderer 不持有 runtime 锁。
  - 状态（2026-09-01）：`bongocat-render` 将带 `model_commit` 的控制帧放入独立可靠槽，
    普通 latest 数据帧即使连续 coalesce 也不会覆盖待确认的模型提交；7 项 render contract
    与 Windows 失败回归均通过，双槽 pending accounting 保持守恒。
- [ ] 支持目标 FPS、不可见暂停/降频和刷新率变化。
  - 状态（2026-09-04）：目标 FPS 与不可见降频子能力已闭环。`model.maximum_fps` 通过 settings typed command
    和 expected config revision 在 `15..=240` 内校验、持久化并进入 runtime snapshot；runtime
    周期评估、GPUI 产品 frame source 及双平台独立 overlay run loop 都按最新值计算下一帧间隔，
    修改无需重启。overlay 隐藏时 runtime 与产品 frame source 统一降至 `100 ms`，可靠 command
    仍可立即唤醒 runtime，重新显示的轮询延迟不超过 `100 ms`。越界值和 stale revision 保留旧
    runtime/config。刷新率变化仍未实现，因此总项保持未勾选。
- [x] 首帧前不出现黑框或不透明闪烁。
  - 验收证据（2026-09-04）：双平台正式 `NativeOverlay` 以共享 presentation state 强制
    “成功 draw/present 后才可见”；产品启动、隐藏后重显、overlay 设置重建、模型重建和独立
    renderer preview 均改为先提交并验证非空帧，再调用 `orderFrontRegardless`/`ShowWindow`。
    未提交帧的显示请求由 contract 拒绝，首帧失败保持窗口隐藏并拒绝候选 model commit。
    本机 macOS release 产品 lifecycle smoke 已完成隐藏 `NSPanel` 的首帧 Metal present 后显示；
    commits `0ea8997`、`5f5c9aa` 和 `2f8999e` 的 GitHub Actions run `33860274701` 已在 macOS job
    `100982884758` 与 Windows job `100982884958` 通过完整 workspace 和产品 lifecycle smoke，
    Windows smoke 包含真实 D3D11 present 后 `ShowWindow` 时序。
- [x] shutdown 先停 frame source，再释放 GPU/window。
  - 验收证据（2026-09-04）：产品 coordinator 使用共享 stop request 与 run-guard acknowledgement；
    shutdown 在停止 input producer 后有界等待 frame source guard 退出，未确认会记录稳定匿名错误，
    runtime/config/audio shutdown 与 renderer/GPU/window 释放只在该等待之后执行。commit `99f0977`
    的本机单元测试、完整 workspace 门禁、macOS release settings/Models lifecycle 与隐藏切模 smoke
    通过；包含后续 Windows overlay 修复的 run `33865854261` 又在 macOS job `101000445151` 与
    Windows job `101000445117` 通过完整 release lifecycle 和有序 shutdown。
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
    safe wrapper；2026-09-01 已在 `bongocat-live2d::CoreLogHandle` 接入 Core logging
    callback。callback 以 panic-safe、最大 512 bytes 消息、路径脱敏和 1 MiB 单文件预算
    写入当前 Development/Production 环境的 `logs/cubism-core.jsonl`，超出预算计数丢弃，
    handle drop 先卸载 callback 再释放 sink；纯 Rust 测试覆盖过滤、容量、写入和卸载。
    Core logging 已完成，但该行仍等待 FFI 错误映射、完整 Moc/Model 资源矩阵和双平台
    实机证据后再勾选。
- [ ] 用 Rust owner 保证 Moc、Model 和 buffer 析构顺序。
- [ ] 校验 parameter/part/drawable id、index 和范围。
  - 状态（2026-08-30）：正式 wrapper 已在 Model 创建时一次性验证 product parameter
    ID/range/default，按模型解析 stable index，并验证 drawable array、index、texture、
    mask、vertex、opacity/color；part 表和完整 custom parameter 诊断尚未完成。
- [ ] 模型切换使用 prepare/commit/rollback。
  - 状态（2026-08-30）：正式 runtime/Metal/D3D11 产品链已实现 CPU/GPU 两阶段提交。runtime
    在候选 generation 的 texture/mesh/mask 全部由 renderer prepare 并回报匹配 token 前保留
    旧 `active_model`、Cubism owner 和 input bindings；GPU 拒绝映射为稳定
    `GpuPreparationFailed`，旧 generation 以更高 transport sequence 继续动态出帧。
    单元回归覆盖 CPU load 失败、GPU 拒绝、迟到状态不提交、等待期间 KeyUp/KeyDown 不受
    阻塞、输入越过已排队普通命令及后续有效 generation；本机真实预览完成 100 轮/300 次 standard -> keyboard ->
    gamepad 切换，343 个动态 snapshot，Metal allocation `54,427,648 -> 54,427,648` bytes。
    commits `a778c5d` 至 `69877dd` 又接通 Windows token：无效纹理候选被拒绝后 CPU model、
    D3D11 generation、input bindings 与旧帧均保持可用；被拒 generation 允许形成单调 gap，
    每个有效 generation commit 前均通过非空 staging readback。push run `33315085958`/
    job `99266903250` 与 PR run `33315088327`/job `99266908843` 在 100 轮完整 warmup 后正式
    提交 9 次切换，输出 309 个动态 snapshot、`failed_gpu_prepare_preserved=true`、DXGI
    `0 -> 0` bytes，并通过稳定 thread/handle 门禁。100 次正式计量、真实 device-loss 和
    物理 GPU 矩阵仍待完成，因此总项保持未勾选。
- [ ] 加载失败保留当前可用模型。
  - 状态（2026-08-30）：文件解析在 runtime 外完成，只有由环境 `ModelStore` 或预置
    `PresetModelCatalog` 签发、调用方无法自行构造的 `CommittedModel` 能进入
    `ActivateModel`。runtime worker 在替换 active model 前完成 Cubism load、首轮参数求值
    和首帧 publish；损坏 Moc 切换会返回稳定 command failure，旧 model generation 继续
    出帧，随后有效切换才递增 generation。Metal renderer 又将新 generation 的 texture、
    mesh、mask target 和 canvas 组装为临时 `GpuModel`，完整验证后一次 commit；失败 prepare
    保留当前 GPU generation，300 次真实切换无 allocation 增长。正式产品链随后增加
    runtime/GPU commit token 和稳定拒绝反馈，GPU 失败时旧 Cubism/model/bindings/GPU
    generation 均保持 active 并恢复出帧；Windows 产品 renderer 现也以实际缺失纹理注入
    验证同一回滚语义。完整损坏资源矩阵、device-loss 和用户模型失败路径仍待完成，因此
    两项保持未勾选。
- [ ] FFI 错误映射为稳定 Rust error code。
  - 状态（2026-09-01）：`Live2dErrorCode` 已提供固定 snake_case `as_str`/`Display` 标识，所有
    Core、模型、motion 和 expression 错误共用 17 个唯一 code；`Live2dError` 的 detail 仍可包含
    诊断信息，但 code 本身不含路径或其他动态内容。纯 Rust 唯一性和格式测试已通过；跨 crate
    UI/诊断投影及完整错误矩阵仍待完成。

### 5.3 动作与状态

- [ ] 实现 parameter 默认值、保存/恢复和 clamp。
  - 状态（2026-08-30）：Core range/default 已进入类型化查询，绝对值和 normalized 写入
    拒绝非 finite、自动 clamp 并明确返回 unsupported；正式 frame pipeline 现于 motion
    前恢复全部 Core parameter default，再按 motion -> expression -> typed product input
    顺序覆盖，停止 motion 或替换 expression 后不残留旧值。physics 所需的分层状态仍未完成。
- [ ] 实现 motion curve、fade、priority 和 completion。
  - 状态（2026-08-31）：正式 `bongocat-live2d` 已严格解析 motion3 v3 Meta、user data 和
    linear/Bezier/stepped/inverse-stepped segment，验证 finite/time/count 边界并以二分反解
    非受限 Bezier 时间。三个预置模型的全部 motion 引用均通过真实解析、循环时间求值和
    Core/drawable 变化测试；非循环自然 completion、model3/curve fade、`idle/normal/force`
    抢占、同级最新请求、旧 stop 隔离、错误资源保留当前动作及模型 commit 后清理均进入
    typed runtime。主动 stop 现在按 FadeOutTime 正弦衰减，snapshot 保留首次 stop sequence，
    重复 stop 不重启计时；真实 Core 测试覆盖停止瞬间、半程和结束帧。PartOpacity target
    已按官方 Framework 的 parameter sink 语义进入真实 Core 求值且不错误套用 parameter
    fade。model3 Groups 已进入 v1 产品索引并校验非空 target/name/parameter ID；Model target
    按 R5 顺序实现 EyeBlink 参数乘法、LipSync 参数加法、未覆盖 group 参数的 motion fade
    和独立 Opacity render contract。真实 Core 测试覆盖左右眼、嘴部参数和 opacity snapshot，
    D3D11/Metal 均只在最终颜色 pass 应用 model opacity。UserData 现按单调 elapsed 的
    `(previous,current]` 产生 occurrence，循环边界不重复、回退不重放、单 tick 上限 256
    并计数跳过；accepted motion 的相对 FLAC 音效也已进入独立 owner。UI 选择入口仍未完成，
    因此保持未勾选。
- [x] 实现 expression 混合和互斥/叠加语义。
  - 验收证据（2026-08-30）：正式 `bongocat-live2d` 严格解析 Type、fade、parameter、
    duplicate ID 与 Add/Multiply/Overwrite；三个 model3 声明的 9 个 exp3 全部在模型 prepare
    阶段缓存。`SetExpression` 使用强类型 name/command/snapshot 和可注入单调时钟，上一层
    按 FadeOutTime、当前层按 FadeInTime 正弦过渡，最多同时保留两层。真实 Core
    测试覆盖三种 blend，runtime 测试覆盖 drawable 变化、快速替换、无效请求保留、GPU
    rejection 保留和成功模型 commit 清理；产品输入最后应用。快捷键/GPUI 入口由后续项跟踪。
- [ ] 实现 physics、pose、eye blink、breath 等实际需求。
  - 状态（2026-09-01）：正式 runtime 已在 motion/expression 之后、产品输入之前加入可注入单调时钟驱动的
    `ParamBreath` 四秒正弦周期和 `EyeBlink` 五秒周期（每周期 180ms 闭眼）；缺失参数安全跳过，纯函数
    边界测试固定周期与范围。新增三预置模型 contract 验证，确认 `EyeBlink` group 的双眼参数与
    `ParamBreath` 均可通过同一 safe parameter API 驱动；新增 runtime precedence 回归锁定
    `motion -> expression -> automatic effects -> product input -> Core update`，眨眼/呼吸不会被旧层残留值
    覆盖。physics/pose 仍等待可授权真实 fixture、R5 黑盒轨迹和求值实现，不得以合成数据宣称完成。
- [ ] 实现键盘、鼠标、手柄到参数/动作/表情映射。
  - 状态（2026-09-01）：正式 `InputBindings` 现支持按 `GamepadButton` 的强类型左右手映射，
    `gamepad` 预置将 South/East 分别投影到左右手；Windows/macOS 预览路径与 runtime 共用该
    contract，release 后仍需动作/表情快捷键映射、用户可编辑绑定和物理设备回归，因此保持未勾选。
- [ ] 实现镜像、鼠标镜像和坐标归一化。
  - 状态（2026-09-01）：正式 runtime 已加入 typed `ModelSettings` command/snapshot；`mirror`
    进入不可变 `RenderSnapshot::mirror_horizontal`，Windows D3D11 与 macOS Metal 共享同一
    中心变换规则；`mirror_pointer_tracking` 按旧行为反转 X/Z 指针参数，`ignore_pointer`
    跳过指针覆盖。启动配置投影、runtime/renderer 回归和 macOS 参数/变换测试已通过；Settings
    service/client 已支持 revision-checked 原子持久化，General 页面已提供三个可键盘/无障碍操作的
    toggle。平滑坐标策略、多显示器实机和完整 mirror fixture 仍未完成，checkbox 保持未勾选。
  - 状态（2026-09-04）：光标 latest-value 进入 runtime 后按可注入单调时钟执行帧率无关的
    指数平滑，保持旧版 60 FPS 下每帧 `0.75` 衰减并在逻辑距离 `< 0.5` 时收敛；首个样本和
    viewport 变化直接对齐，周期 tick 在没有新 sample 时继续推进。纯 Rust 与 runtime 集成回归
    覆盖单帧/半帧等价、跨显示器保护和连续模型参数投影；多显示器物理光标与完整 mirror
    实机证据仍待完成，因此总项保持未勾选。
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

- [x] 选择跨平台 Rust 音频后端并审查许可证/维护性。
- [x] 支持现有 motion 音频格式和相对路径。
- [x] 定义并发、打断、音量和模型切换停止语义。
- [x] 音频失败不阻塞动画或渲染。
- [x] shutdown 停止 stream 并释放设备。

验收证据（2026-08-31）：ADR-0012 记录 `rodio 0.22.2` 的 MIT/Apache-2.0、活跃维护、
Windows/macOS output、最小 feature 和可替换边界，并说明上游仍约束 `cpal 0.17.3`。
模型 parser 先规范化并限制 sound 相对路径，runtime 仅在 priority/resource 接受后发布；
唯一 voice 使用 full volume，新 motion/无 sound/stop/disable/成功切模/shutdown 均停止旧
voice。7 项 audio test 覆盖真实 48 kHz stereo FLAC、资源/解码失败、恢复、抢占、overflow
和有序释放；runtime/app tests 证明 unavailable backend 不使 motion command 失败，设置
持久化与 worker join 完成。默认设备热切换、100 次模型/音频资源测量和 8 小时 soak 仍由
Phase 6/8 门禁跟踪，不反向取消本节的功能 contract 完成。

### 5.6 Phase 4 退出门槛

- [ ] 三个预置模型通过兼容矩阵。
- [x] 自定义模型 fixture 成功/失败行为符合规范。
  - 验收证据（2026-08-31）：正式 `bongocat-model` 严格读取共享 `cases.json`，隔离物化 1 个
    accept 与 5 个 reject package，并以产品 `PreparedModel` 和 transactional `ModelStore`
    同时执行。测试逐 case 比对 stable diagnostic 与声明 preflight stage，强制所有 fixture
    目录注册；成功仅提交目标模型，失败不留下 staging/destination，且两条路径都不修改源包。
- [ ] 模型切换 100 次无 CPU/GPU/音频持续增长。
- [ ] 输入、动作、表情、物理和音效闭环不依赖 GPUI。
- [x] 模型 parser 完成 fuzz/property test，畸形 JSON、索引和尺寸不能触发 panic、越界分配或路径逃逸。
  - 验收证据（2026-08-31）：`bongocat-model` 每次测试执行 6 组、每组 512 case 的可收缩
    property contract，覆盖任意 model3 bytes、随机 texture/group 数组位置、任意 model ID、
    UTF-8/平台路径、截断或随机 PNG header 及全范围 `u32` dimensions/limit；portable model
    ID 固定拒绝尾点及大小写不敏感的 Windows 设备 stem（含扩展名）；纯 byte-slice parser
    与产品文件入口共用实现。固定回归另证明 JSON bytes、package bytes 和 file count
    在解析/清单增长前拒绝，既有深度、symlink 和 oversized texture fixture 继续通过。
    最新稳定 `proptest 1.11.0` 仅作 dev dependency，关闭 fork/timeout 等默认 feature，只启用
    `std`；其 Rust 1.85 下限兼容 workspace 1.97，MIT/Apache-2.0 且仍活跃维护，替换边界仅为
    本 crate 测试生成器。完整 format、Clippy、workspace test、release check 与 license/source
    policy 本机通过。

## 6. Phase 5：GPUI 设置应用

### 6.1 Command/Snapshot 边界

状态（2026-09-01）：overlay 的 click-through、always-on-top、scale 和 opacity 已定义为
`bongocat-runtime::OverlaySettings`，通过 revisioned runtime snapshot 与 typed settings
command 在配置事务后更新；非法范围由 runtime 拒绝并保留上一组值。`Application` 和
`SettingsClient` 已接线并有 persistence/rejection contract。General 页面现已通过
typed command/snapshot 暴露 click-through 与 always-on-top，并将设置变更应用到双平台
overlay owner；设置更新失败时保留旧 snapshot。scale/opacity 的可见控件、双平台实机
动态重建与 device/display 专项证据仍待完成。General 页面现已增加 25–400% 的 25% 步进缩放
和 1–100% 的 10% 步进透明度控件，按钮和 AccessKit 语义复用同一 snapshot，并在边界禁用。
双平台实机动态重建与 device/display 专项证据仍待完成，故不将 Phase 5 或 P0 overlay 门禁标记完成。

- [ ] 按 app、window、input、model、shortcut、update、diagnostics 定义 command。
- [ ] command 使用强类型 request/result 和稳定 error code。
  - 状态（2026-09-01）：`SettingsErrorCode` 已提供固定 snake_case 标识和 29 项唯一性 contract，
    与既有用户可读文案分离；Service、model import/delete、config、startup、window 和 shutdown
    错误均沿用该枚举。`RuntimeRenderErrorCode` 已通过 UI 自有的
    `SettingsRuntimeErrorCode` 投影到 `SettingsSnapshot.runtime_diagnostics`，Diagnostics 页面显示
    匿名 renderer 错误和最近失败 command 序号；统一 Diagnostics 导出、input/model/config/update
    跨域聚合仍待完成。
- [ ] 长操作提供 operation id、progress、cancel 和 final result。
  - 状态（2026-08-31）：模型导入已完成首个正式长操作契约：所有 `SettingsClient` clone
    共用单调 typed ID，progress 仅含 stage/file count/byte count，共享原子 token 可在 settings
    worker 复制期间取消，final result 回传同一 ID。系统文件选择、Models 页面消费以及后续
    update/download 等长操作仍待接入，因此总项保持未勾选。
- [x] snapshot 包含 revision，UI 处理过期结果和并发编辑。
  - 验收证据（2026-09-01）：正式 `SettingsSnapshot` 始终携带单调 revision；UI 对异步结果
    只接受不早于当前快照的 revision。已接入的显隐、overlay 设置和 motion audio command
    均携带提交时的 `expected_config_revision`，settings worker 在配置/runtime/模型切换前拒绝过期编辑并返回匿名
    `SnapshotOutdated`；冲突后 UI 自动读取最新 snapshot 并保留可操作错误。正式 app 回归
    验证显隐、overlay、motion audio、模型交互和 gamepad dead-zone 的过期提交不改变 runtime 状态、配置字节或 revision，成功提交、错误文案和 shutdown
    路径均通过 `bongocat-app`/`bongocat-ui` 定向测试。
- [ ] 禁止通用 set_value(path, any) API。
- [ ] 不向 UI 发送逐帧数据、原始按键流或 GPU/model pointer。
- [ ] command/snapshot 有纯 Rust contract test。
  - 状态（2026-09-01）：正式 contract 已覆盖 FIFO command、typed reply、receiver close、
    revision 单调更新、配置原子持久化和 shutdown acknowledgement；shortcut settings command
    现使用 typed request/result，支持 revision 检查、校验错误映射、snapshot projection 和重启
    恢复回归。完整 app、model、update、diagnostics command 集及平台捕获仍待定义，因此保持未勾选。

### 6.2 GPUI 状态规则

- [ ] Entity 只保存表单草稿、选择、展开、导航和临时 UI 状态。
- [ ] runtime snapshot 是显示配置/状态的唯一来源。
- [ ] command 成功后使用新 revision/snapshot 更新 UI。
- [ ] command 失败恢复草稿并显示可操作错误。
- [ ] 设置窗口重建时从 runtime 恢复，不依赖旧 Entity。
- [ ] UI executor 不持有 runtime 写锁或执行阻塞文件操作。
  - 状态（2026-08-31）：当前最小窗口仅 await `SettingsClient`，独立有界 worker 独占
    `Application`、配置 I/O 和 runtime 等待；后续页面仍须持续遵守该边界。

### 6.3 Design System

状态（2026-09-04）：按 GPUI Kit 官方仓库、docs.rs 和 crates.io metadata 确认 `gpui-kit 0.6.0`
是当前最新稳定版，使用 Apache-2.0 许可证并默认提供 component/assets。Native workspace
现以精确固定的 crates.io `gpui-kit = "=0.6.0"` 作为唯一直接 GPUI 依赖，已删除 Zed 与旧组件
git source 及 `gpui`、platform、component、assets 的直接 manifest 依赖。完整 `cargo update`
解析到 `gpui-pre 0.3.3`；该同步包元数据对应 Zed `gpui 0.2.2` revision
`5b055fa789a8b8d38ac951a6e0cde272f66b4495`。设置窗口调用 `gpui_kit::init`，使用
`gpui_kit::component::Root` 并随系统外观同步 `Theme`；状态标签、开关、
按钮、模型 ID、overlay scale/opacity 与 gamepad dead-zone 已迁移到 `Tag`、
`Switch`、`Button`、`Input` 和 `NumberInput`。输入实体通过 `InputEvent` 与
`NumberInputEvent` 接入现有 typed command/draft，并从 snapshot 同步。`0.6.0` 没有普通 Card
primitive，设置内容容器使用官方 `GroupBox::outline()`，导航继续保留无状态薄封装；快捷键捕获、确认删除和平台辅助功能焦点
继续保留领域适配层。语言设置使用官方 `Select`；当前没有标签页或浮层需求，后续出现对应交互时
直接使用 `TabBar`、`Dialog`/`Menu`，不预建无业务用途的组件。双平台辅助功能与缩放实机证据
仍待补齐，详见 ADR-0020。偏好设置整体使用 `gpui_kit::component::setting`
官方 `Settings`、`SettingPage`、`SettingGroup`、`SettingItem` 和 `SettingField` 结构；General
按 Overlay、Model interaction、Input、Startup 分组，Models 与 Diagnostics 使用独立页面和
纵向设置项。官方 Settings sidebar 提供页面切换与搜索过滤，搜索覆盖设置标题、描述和显式关键词；
所有变更仍经原有 revisioned snapshot 与 typed command 回调。图标由 `gpui_kit::assets` 提供，并在所有 GPUI
应用入口通过 `Application::with_assets` 注册，NumberInput 的 `Minus`/`Plus` SVG 可正常加载。
当前 GPUI 同步包内置 element-level AccessKit adapter；现有项目语义桥接仍负责已验证的
双平台 AX/UIA contract，因此所有应用入口集中使用 `Application::new_inaccessible` 只关闭
重复的 GPUI adapter，避免两套 `accesskit_macos` 在同一个 NSView 注册固定 Objective-C 类名并
触发 `SIGABRT`。本机 release 设置 smoke 已验证启动、项目桥接语义和有序退出；迁移到 GPUI
原生 element 语义及删除项目桥接/兼容构造仍是后续 Design System 工作，不据此勾选总项。
本次迁移已通过 macOS workspace format、Clippy、unit/doc tests、release check，以及 release
设置窗口与 Models 页面 smoke。macOS 到 `x86_64-pc-windows-msvc` 的交叉 check 会在
GPUI Kit 配套 HTTP/TLS 链编译 `aws-lc-sys`/`ring` 时因本机没有 Windows SDK headers 停止；
Windows 原生 build、UIA、设置窗口和 shutdown smoke 仍须由 `windows-latest` runner 验证。

- [ ] 定义颜色、排版、间距、圆角、边框、阴影和焦点 token。
- [ ] 实现 Button、IconButton、TextInput、NumberInput、Slider、Switch。
- [ ] 实现 Select、Menu、Tabs、Tooltip、Dialog、Toast。
- [ ] 实现 List、EmptyState、ErrorState、Progress 和 Skeleton。
- [ ] 控件具有 hover、active、focus、disabled、loading 和 error 状态。
- [x] 支持浅色、深色和系统主题。
  - 状态（2026-09-05）：正式 settings snapshot/command、Application 原子持久化和 GPUI Kit
    `Select` 已形成三态主题闭环；`Theme::change` 即时更新内容，显式模式同步原生窗口外观，
    system 模式恢复并跟随系统通知。项目辅助功能桥已将主题投影为 ComboBox role、当前值和 action；
    本机定向测试与完整 workspace 通过；commit `ac5dc70` 的 run `33871601685` 全绿，Windows
    job `101018640203` 与 macOS job `101018640280` 均通过 release settings/state smoke，完成
    证据由 `P5-APPEARANCE-THEME` 记录。
- [ ] 图标统一使用 Lucide 资源并提供 tooltip/accessibility label。
- [ ] 不直接复制 Zed 产品内部组件源码，除非许可证和维护边界明确。

### 6.4 页面

- [ ] 应用框架：导航、标题、主题、语言、更新状态和错误边界。
  - 状态（2026-09-04）：主题和语言已有 typed snapshot/command、即时应用和辅助功能语义；
    中英 shell/Appearance/runtime status 已接入，Models、Diagnostics、其余 General 文案、更新状态
    和完整错误边界仍待完成，因此保持未勾选。
- [ ] 通用：启动项、任务栏/菜单栏、语言、主题和日志。
  - 状态（2026-09-04）：启动项、任务栏/菜单栏可见性、主题和语言已有正式 UI/持久化闭环；
    日志设置和其余 General 文案本地化仍待完成，因此保持未勾选。
- [ ] 窗口：显示器、位置、缩放、透明度、置顶、穿透和显隐。
- [ ] 模型：预置/用户模型、导入、删除、切换和兼容诊断。
- [ ] 输入：键鼠、手柄、忽略鼠标、单键模式和校正状态。
- [ ] 快捷键：捕获、冲突、清除和恢复默认。
  - 状态（2026-09-01）：正式 `bongocat-config` 已加入平台无关的 typed chord 校验和 canonicalization；修饰键别名、顺序和多余空白会稳定化，重复修饰键、多 key、空片段和非法 key 会被拒绝，`commands` 与 `model_behaviors` 共享冲突命名空间。settings service 现以 typed command 完成 revision-checked 原子持久化、snapshot 投影、重启恢复和 `RestoreDefaultShortcuts` 恢复默认；空集合可清除全部绑定。平台输入 owner 已将匹配 target 投递到 runtime 或 settings handoff；UI 编辑入口、平台注册/捕获和实机证据仍待完成。
  - 状态（2026-09-01）：chord key 已收敛为 legacy 可录制键的闭合集合并映射到 USB HID usage；
    `ShortcutMatcher` 聚合左右 modifier、抑制重复 down；binding replace 保留 pressed set 防止
    held-key repeat 误触发，reset/reconcile 分别清除或校正 transient pressed state。Windows scan code 与 macOS keycode 的现有映射均有定向回归证明可命中
    同一 compiled chord；产品输入 worker 已投递 matcher target，active model 的 motion/expression
    会转成 typed runtime command。应用级 target 通过有界 typed handoff 进入 settings service，
    显隐/镜像/穿透/置顶会在唯一 Application owner 内按当前配置持久化切换；`open_settings` 经
    线程安全 signal 交给 GPUI frame source 重开设置窗口，服务关闭和队列满均有边界处理。注册/捕获 UI、
    GPUI 清除/恢复默认入口和 Windows/macOS 实机快捷键证据仍未完成。
- [ ] 动作/表情：绑定、预览 command 和错误状态。
- [ ] 权限：macOS 状态/跳转和 Windows 权限差异。
- [ ] 更新：检查、下载、验证、安装和回滚提示。
- [ ] 诊断：版本、renderer、GPU、输入、权限、模型错误和日志导出。
- [ ] About：许可证、Cubism attribution、第三方依赖和隐私说明。

### 6.5 UI 质量

- [ ] 迁移五种本地化并建立缺失 key 检查。
  - 状态（2026-09-04）：当前 v1 先支持 `system`、`zh-CN`、`en-US` 三种 typed 偏好；跟随系统
    只解析简体中文或英文，其它 locale 回退英文。GPUI Kit Select、窗口标题/导航/Appearance/
    runtime status 的中英文案及对应辅助功能语义已接入；Rust 单元测试防止当前 key 空值和中文
    整组英文 fallback。繁中、越南语、葡萄牙语以及 Models、Diagnostics、其余 General 动态/错误
    文案和统一的全量 key 漂移门禁后续迁移，因此保持未勾选。
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
- [x] 未知字段采用明确的拒绝、忽略或诊断策略。
  - 验收证据（2026-08-31）：JSON Schema 的所有对象使用 `additionalProperties: false`，正式
    Rust 配置类型逐层使用 `deny_unknown_fields`；共享 `invalid-unknown-field.json` 在嵌套
    application 域注入 `legacy_alias` 并由固定 Draft 2020-12 validator 拒绝，正式 crate 另有
    unknown/legacy 字段拒绝测试。
- [x] `next` 的当前完整 Native Rewrite schema 固定为 `schema_version: 1`，不包含迁移链。
  - 状态（2026-09-04）：正式 config、独立 config-store contract、JSON Schema 和全部 fixture
    已统一为完整 v1；模型来源、input dead-zone 与现有字段都直接属于首版结构。store 只接受 v1，
    非 v1 明确拒绝且不改写；迁移函数与迁移专用测试已删除。首次正式发布后的后续版本再以实际
    发布的 v1 为基线新增迁移，不在 `next` 预置兼容逻辑。
- [x] 不包含旧 Pinia store key、旧字段 alias 或自动导入逻辑。
  - 验收证据（2026-08-31）：Native schema/Rust 类型没有 serde alias 或 legacy 字段，严格未知
    字段 fixture 与单元测试拒绝 `legacy_alias`/`old_pinia_field`；产品 `ConfigStore` 只解析当前
    环境的完整 v1 `config.json`，不执行迁移或兼容转换。
- [ ] 独立 `state.json` v1 schema 只保存可恢复窗口布局，不进入用户配置事务。
  - 状态（2026-09-04）：正式 `StateStore` v1 保存 settings 与 overlay 的有限坐标/尺寸，
    settings 的 maximized、独立 writer lock、原子提交后验证/回滚、损坏/非 v1 schema 非阻塞回退和
    未知文件防覆盖已实现；settings worker 接收合并后的 GPUI bounds 更新和 overlay 几何变化并及时
    落盘，shutdown 仍强制 flush。更新后的双平台实机多显示器恢复证据尚未完成，因此保持未勾选，
    由 `P6-STATE-WINDOW-LAYOUT` 跟踪。

### 7.2 环境与持久化事务

- [x] 构建系统显式产生 Development/Production 元数据，发布构建拒绝默认值。
  - 验收证据（2026-08-31）：正式 app build script 已删除隐式 Development fallback；Native workspace
    Cargo config 与 CI 显式选择 Development，Production step 显式覆盖，macOS packaging 在 Cargo
    前拒绝缺失/空/未知值。commit `2810f4a` 的 pull request run `33383026191` 全绿；Windows/
    macOS/Ubuntu Native jobs `99459402028`/`99459402083`/`99459402181` 通过完整 workspace、
    Development/release、显式 Production 和拒绝隐式环境门禁，Windows/macOS GPUI jobs
    `99459402171`/`99459401995`、Windows input/config job `99459402076` 和 config-store job
    `99459402352` 同时通过。
- [x] path resolver 返回当前平台与环境的数据根，不能接受任意外部生产路径。
  - 验收证据（2026-08-31）：正式 `Application::start` 只使用不可变编译环境与平台 resolver；任意
    `StorageLayout` 注入只存在于显式 Development `storage-test-injection` 测试产物，默认 CLI/API
    不包含该入口，Production 组合在编译期拒绝。commit `696319e` 的 pull request run
    `33386401135` 全绿，三平台完整门禁、Windows 真实路径测试和双平台 recovery window smoke
    均通过，详见 `P6-STORAGE-LAYOUT-BOUNDARY`。
- [x] 实现 load -> parse -> validate current v1 -> atomic commit -> verify。
  - 验收证据（2026-09-04）：正式 store 在单一 writer lock 内严格检查 schema version 并执行
    typed validate，再经固定 temp、flush、原子替换和重读 typed config/revision 验证。底层事务与
    替换后破坏注入最初由 commit `fd0f1d2` 建立；当前 v1 实现会在验证失败时逐字节恢复原文件并
    清理 temp，不包含迁移步骤。本次重置已通过完整本地 Native workspace 与 config 定向测试。
- [x] backup 包含 Native schema 版本和时间，并限制数量与总大小。
  - 验收证据（2026-08-31）：正式 `bongocat-config` 在替换前生成 v1 envelope，保存真实墙上
    时间、源 schema/revision 和原始配置；每环境仅管理固定命名空间，按不受时钟回退影响的
    排序键保留最新 8 份且总计不超过 8 MiB。单元测试覆盖 v1 原文备份、12 次提交收敛、
    未知文件保留和时钟回退顺序；完整 Native workspace 门禁随当前队列提交验证。
- [x] spike 中途提交中断后可安全恢复或重试；失败不覆盖当前可用配置。
  - 状态（2026-08-29）：`ConfigStore::recover_interrupted_commit` 覆盖主配置有效/缺失/损坏与临时文件有效/无效组合，恢复在 OS writer lock 内执行并保留诊断副本；父进程强制终止已写入并 flush 临时配置的持锁子进程后，macOS 本机与 Windows runner 均验证 lock 自动释放、当前配置保留和 interrupted archive。
  - 状态（2026-08-31）：正式产品已实现固定 `config.json.tmp`、跨平台原子替换、current/temp
    状态机、有界 interrupted archive、启动锁重试和匿名 app action；本机定向测试已通过，三平台
    CI 与最终验收证据由当前执行队列 `P6-CONFIG-INTERRUPTED-COMMIT` 跟踪。
- [x] GPUI 显示错误摘要、备份位置和恢复默认 command。
  - 状态（2026-08-31）：成功从备份恢复时，正式 settings snapshot 已投影匿名的源 schema 与
    跳过候选数，Diagnostics 显示正常加载或恢复成功状态。
  - 状态（2026-08-31）：正式 Application/settings service 已实现无有效备份时的
    `RecoveryRequired` recovery-only 窗口、匿名候选计数和 `RestoreDefaultConfiguration` typed
    command；恢复前业务 command 被拒，恢复后标记需重启。
  - 状态（2026-08-31）：settings 已增加权限、空间不足和目标占用的独立匿名错误摘要；config
    crate 已加入权限/磁盘满阶段注入及真实目标占用测试。
  - 状态（2026-08-31）：Diagnostics 已增加当前环境 Backups 入口、typed command、pending/error、
    键盘与 accessibility 状态；路径只存在于 Application/platform adapter，成功不推进 revision，
    recovery-only 同样可用。commit `6b41808` 的 run `33381198560` 已通过三平台完整门禁、双平台
    GPUI smoke 与 Windows config job；`P6-CONFIG-BACKUP-LOCATION` 退出条件满足，因此总项完成。
- [x] 用户模型只通过显式、受验证的导入进入当前环境，不扫描旧应用目录。
  - 验收证据（2026-08-30）：`bongocat-app` 不再提供任意外部目录激活入口；模型必须
    先经 `ModelStore::import` 复制、复验和 commit，随后只能按已安装 `ModelId` 加载；
    runtime 激活 command 只接受 store 签发的 `InstalledModel`。Development/Production
    两个 app 同时存活并以相同 ID 导入的测试验证目录与 lock 均互不影响。

### 7.3 跨环境隔离

- [ ] Development 与 Production 的相对目录树和 JSON schema 完全一致。
  - 状态（2026-09-01）：`StorageLayout` contract test 现逐项比较 config、state、models、
    backups、logs 和 locks 的相对路径，并确认两个环境根目录互不包含；NativeConfig schema
    仍由同一 typed 定义生成，待独立 schema fixture 门禁后再勾选本项。
- [ ] 配置、state、模型、备份、日志、锁和单实例 namespace 均包含环境边界。
  - 状态（2026-08-31）：config、state、模型、备份、对应 writer lock 和 Windows 单实例均已按
    环境隔离；state 双环境 sentinel/restart/lock 测试已进入当前批次。日志 writer 与更新 channel
    仍未实现，因此总项保持未勾选。
- [ ] 两个环境可同时运行，不争用 writer lock、模型目录或日志文件。
  - 状态（2026-08-30）：config store 已通过双环境进程测试；正式 app 又以相同模型 ID
    同时写入两套环境，验证 `models/` 和 `locks/models.writer.lock` 分离。日志 writer
    尚未实现，因此保持未勾选。
  - 状态（2026-08-31）：state 进一步使用独立环境根和 `locks/state.writer.lock`，Development/
    Production 写入不同窗口布局并重启读回；日志 writer 缺失仍阻止总项勾选。
- [ ] 开发构建即使收到指向 Production 的 CLI 参数或进程环境变量也拒绝越界。
- [ ] Production 不自动复制 Development 数据；需要测试数据时使用显式导入。
- [ ] 更新 channel 与环境绑定，Development 不能安装 Production 更新或反向覆盖。

### 7.4 测试与门槛

- [x] 在平台无关 spike 中验证 Development/Production 根目录不同且相对结构一致，并在 Windows/macOS target-specific test 验证真实 resolver。
- [x] 两个环境写入不同 sentinel，重启和并发运行后仍只读取各自数据。
  - 验收证据（2026-08-29）：macOS 本机与 commit `cf16291e8cee027b6983abcf919a32fb5a0278a5` 的 Windows push run `33251410654`、job `99097619545` 均通过 `development_and_production_processes_commit_and_restart_independently`；产品 state/model/log 服务仍由各自阶段验证。
- [x] 覆盖损坏、截断、错误类型、越界值和未知字段。
  - 验收证据（2026-08-31）：正式 `ConfigStore::load_or_default` 产品测试逐项写入非 JSON、
    截断 JSON、错误布尔类型、越界 opacity 和嵌套未知字段，全部返回错误且逐字节保留当前
    `config.json`；没有有效备份时不创建 quarantine 或静默回落默认值。
- [ ] 覆盖无权限、磁盘满、目标占用和中途退出。
- [ ] 覆盖非 ASCII/超长路径、缺失和重复模型。
- [x] 当前 v1 连续读取 10 次结果一致且不会产生额外写入或备份。
- [ ] 失败注入不丢当前环境的配置或用户模型。
- [ ] 发布依赖和运行日志中没有旧 Tauri/Pinia 配置探测。
- [ ] Bundle ID 精确验证为 `com.ayangweb.bongo-cat`。

## 8. Phase 7：原生系统集成

### 8.1 应用生命周期

- [ ] 单实例唤醒已有进程并打开设置或显示 overlay。
- [x] GPUI 设置窗口按需创建，关闭不退出后台应用。
  - 验收证据（2026-09-04）：双平台设置窗口关闭/重开和后台 frame/input/runtime 生命周期已由
    产品 smoke 覆盖；正式入口无参数启动持续运行到显式 Quit，正数 `--run-seconds` 仅用于有界
    smoke/诊断。commit `7f799f7` 的 run `33867921771` 全绿；Windows job `101006895636` 与
    macOS job `101006895731` 均通过完整 workspace、release 产品 lifecycle、系统菜单 Quit 和
    shutdown smoke。
- [ ] 托盘/菜单栏 command 统一进入 runtime。
- [ ] 系统关机、注销和普通退出进入 shutdown coordinator。
- [ ] panic/crash 生成本地诊断并避免配置半写入。
- [ ] 定义正常退出、强制退出、崩溃和系统终止的恢复标记；下次启动可区分并避免无限恢复循环。

### 8.2 Windows

- [x] Shell_NotifyIcon + HMENU 托盘。
- [ ] named mutex + registered message/IPC 唤醒单实例。
- [ ] 当前用户启动项启用、禁用和状态检测。
- [ ] 文件选择、外部 URL 和剪贴板使用最小权限 wrapper。
  - 状态（2026-08-31）：模型目录 picker 已有共享稳定结果/错误和双平台最小 adapter；Windows
    使用 STA `IFileOpenDialog`、filesystem/folder/path-exists/no-recent flags 与 COM/TaskMem RAII，
    macOS 使用主线程单目录 `NSOpenPanel`。结果在 Rust 侧重新验证并 canonicalize，GPUI Models
    页面及双平台真实选择/取消 smoke 已通过；外部 URL、clipboard 仍待完成，因此总项不勾选。
- [ ] 选择并记录 MSIX、WiX 或 NSIS 打包 ADR。
- [ ] 对安装目录、用户数据目录和更新临时目录分别建模。

### 8.3 macOS

- [x] NSStatusItem + NSMenu 菜单栏。
- [x] NSApplication activation/reopen/single-instance 行为。
- [ ] SMAppService 启动项启用、禁用和状态检测。
- [ ] NSOpenPanel、NSWorkspace 和 pasteboard 最小权限 wrapper。
  - 状态（2026-08-31）：`NSOpenPanel` 模型目录 adapter 已实现并通过主线程边界、稳定错误
    contract 和 macOS 26.5.2 arm64 真实选择/取消交互 smoke；`NSWorkspace`、pasteboard 仍待完成。
- [ ] .app bundle、entitlements、Hardened Runtime 和 notarization 流程。
- [ ] TCC 权限状态变化可在 UI 实时刷新。

### 8.4 更新与诊断

- [ ] 设计纯 Rust 更新 client、manifest 和签名验证。
  - [x] 以环境独立 v1 状态保存最高已验证 manifest sequence。
    - 验收证据（2026-09-05）：`bongocat-update::UpdateSequenceStore` 以固定 channel、同目录 lock、
      原子替换和回读验证持久化单调 sequence；缺失 state 从 `0` 起始，低 sequence、损坏/未来 schema、
      cross-channel、symlink 和并发 writer 都稳定拒绝且不覆盖已有字节。该 state 不进入 config/state
      事务，尚待后续 update client 从 immutable environment layout 注入并在成功验签后调用。
  - 状态（2026-09-05）：ADR-0021 与 `bongocat-update` 已建立平台无关的 signed manifest trust
    boundary；Ed25519 先验签后解析，严格 v1 manifest、稳定错误码、artifact 流式完整性验证、环境内
    anti-rollback sequence store 和 24 小时调度 contract 均有自动化。网络 client、endpoint、手动
    dispatch 与下载/安装仍未实现，因此总项保持未勾选。
- [ ] 只允许 HTTPS，固定公钥来源和轮换流程。
  - 状态（2026-09-05）：manifest/release notes/artifact URL 已强制 HTTPS 且拒绝 credentials/fragment；
    信任公钥绑定 key ID、构建环境 channel 和 release sequence 有效窗。Production 公钥注入、签名
    envelope 与实际 endpoint 尚待发布基础设施确定，因此保持未勾选。
- [ ] 校验版本、target、arch、hash 和签名。
  - 状态（2026-09-05）：离线 verifier 已校验 SemVer、最低可升级版本、四个 target/arch 组合、精确
    artifact 长度、SHA-256 与 detached Ed25519 签名；操作系统包签名和真实发布产物仍待验证。
- [ ] 下载支持取消、断点/重试策略和失败清理。
- [ ] 安装前协调 runtime/renderer shutdown，失败可回滚。
- [ ] 测试断网、代理、中断、签名错误和降级攻击。
- [ ] 日志 rotation、总大小和保留天数有上限。
  - 状态（2026-09-01）：Cubism Core 日志 sink 已在单文件达到 1 MiB 时执行有界路径轮转，最多保留
    4 个轮转文件，活动文件和轮转失败均有有界 dropped 计数；测试覆盖触发轮转、保留上限和
    活动文件恢复写入。应用级 writer 现按 UTC 日分文件，单文件 1 MiB、总量 8 MiB、最多 8 个文件、
    保留最近 7 日，并覆盖日期切换、轮转、过期/总量清理和失败计数；Core 历史日志仍未纳入同一
    retention policy，因此本项保持未勾选。
- [ ] 记录 renderer/input/model/config/update 的稳定 error code。
  - 状态（2026-09-01）：runtime renderer 已为 model load/evaluation、motion/expression load、GPU
    prepare、platform、transport 和 overlay validation 定义 8 个固定 snake_case code，并以唯一性
    contract 防止诊断协议依赖 Rust `Debug` 名称；该 code 已投影到 SettingsSnapshot 和 Diagnostics
    页面。input/config/model 已有各自 typed code，但统一诊断导出和 update code 尚未完成，因此保持
    未勾选。
- [ ] 日志导出生成可预览的脱敏包。
  - 状态（2026-09-01）：settings service 已新增有界 `ExportDiagnostics` command，使用当前环境
    `logs/diagnostics.json` 的同目录原子写入生成 format v1 JSON。导出只包含稳定 runtime/input/
    configuration code、匿名聚合计数、模型来源计数和 settings/config revision；不包含模型 ID、
    路径、按键值、原始 JSON、时间戳或动态 I/O 文本。Diagnostics 页面提供键盘和 AccessKit 可访问
    的 Export 控件，并显示本次导出的字节数；app/ui 定向测试覆盖原子写入、聚合排序、隐私边界和
    typed command。应用级 writer 的匿名 written/dropped/rotated/pruned/bytes/retained_files
    统计现已并入导出，但导出仍不读取或合并 Core/应用原始日志正文，预览器和跨域历史日志打包
    仍待完成，因此本项保持未勾选。
- [ ] 更新 manifest 定义 `schema_version`、channel、最低可升级版本、发布时间和防回滚字段。
  - 状态（2026-09-05）：共享 Draft 2020-12 manifest v1 已定义 `schema_version`、环境 channel、
    release/minimum SemVer、`published_at_unix_seconds`、单调 `release_sequence` 和 target artifacts；
    Rust 对同一 accept fixture 验签解析，真实发布生成器与签名 envelope 尚未实现。
- [ ] 更新 helper/installer 的权限边界、替换原子性和失败恢复经过单独威胁建模。

### 8.5 Phase 7 退出门槛

- [ ] 托盘/菜单栏、单实例、启动项、更新、日志和退出双平台通过。
- [ ] 断网和系统服务失败不影响本地 overlay 运行。
- [ ] 安装包、权限和更新机制通过安全审查。

## 9. Phase 8：测试、性能与稳定性

### 9.1 自动化测试

- [ ] Runtime reducer、输入语义和动画单元测试。
- [ ] motion/expression priority 和可注入 clock 测试。
  - 状态（2026-08-30）：motion 已使用可注入 `MonotonicClock` 覆盖时间推进、真实 drawable
    变化、低优先级拒绝、force 抢占、同级替换、旧 stop 不影响新动作、GPU rejection
    保留及成功模型切换清理；expression 也使用同一 clock 覆盖淡入、替换、错误保留和
    模型事务边界。expression 产品协议采用 latest-set-wins，不另设 priority；motion 主动
    stop fade-out 和完整 fixture 对接仍未完成，因此总项保持未勾选。
- [ ] 配置 v1 schema、环境隔离和原子写入测试。
- [ ] 模型路径安全和损坏资源测试。
- [ ] Cubism safe wrapper 生命周期测试。
- [ ] 输入 fixture 和丢 release 恢复测试。
- [ ] GPUI Kit component、command 和窗口重建测试。
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
- [ ] Native 配置写入与损坏恢复可靠，模型显式导入无已知数据丢失路径。
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
   - 状态（2026-09-05）：已重新采集当前 macOS 26.5.2/Xcode 26.6/SDK 26.5/Rust 1.97.1/Metal
     Toolchain v17.6.109.0 证据，并明确开发机额外安装的 i686 target 不进入 Native Rewrite
     矩阵。Windows 实机、发布产物保留与最终 target freeze 仍是本项未完成门禁。
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

- [x] 完成平台无关 Rust model3/package parser、所有结构化 sidecar 静态 preflight、三个预置规范化索引与异常资源安全 contract；后续任务已完成产品 Core safe wrapper、预置 motion/expression 求值及 D3D11/Metal 绘制，真实 physics/pose 行为样本仍缺失。
- [x] 完成 6 个预置 motion3 与 15 个 exp3 的强类型结构、segment/Meta 计数、fade/parameter/blend 校验；这不代表 motion/expression 行为求值完成。
- [x] 完成 3 个预置 cdi3 的强类型 parameter/group/part 与 group 拓扑校验；这些字段直接属于规范化索引 schema v1，跨资源 ID 以未来 Core 表为准。
- [x] 完成 physics3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；13 个历史文件只作为本地结构覆盖，不作为可分发 fixture 或行为求值证据。
- [x] 完成 pose3 静态 preflight、匿名摘要 CLI 和合成错误 contract；没有授权真实样本或 fade/link 求值证据。
- [x] 完成 userdata3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；三个预置模型没有真实 userdata3。
- [x] 完成 macOS arm64 真实 r.5 sys binding/Core probe；三个预置 Moc 各 100 次生命周期、drawable 与 r.5 offscreen 数组边界、legacy count 对照和 `leaks` 0-byte 门禁通过。产品 safe wrapper、Windows x64 Core/D3D11 与 macOS arm64 Core/Metal 已进入正式链路；macOS x64 原生 ABI、非零 offscreen fixture 及真实 physics/pose Framework 求值仍未完成。
- [ ] 取得可分发授权的 physics3/pose3 fixture 后完成强类型结构和 Framework 求值；三个预置模型不含这两类资源，不得以合成样本冒充兼容证据。

14. [ ] `P0-GO-NO-GO`：汇总证据、阻塞和条件，形成完整功能与 stable 发布决议。
    - 状态（2026-08-31）：ADR-0011 已形成 `IMPLEMENTATION GO WITH RELEASE CONDITIONS`，允许建立正式 workspace；这不勾选完整 Phase 0 决议。标准 Native `5-r.5` ZIP/hash、产品 safe wrapper、Windows x64 D3D11 与 macOS arm64 Metal 三预置模型绘制已验证；真实 physics/pose Framework 样本、其他原生 ABI、GPUI 辅助功能/IME 与双平台物理输入/GPU 矩阵继续阻塞对应功能声明，最终合规清单只阻塞 stable 发布。

15. [x] `P1-RUNTIME-CONFIG`：建立正式 workspace，提升 runtime 生命周期、强类型 command/snapshot 与 Development/Production 配置隔离闭环。
    - 依赖：ADR-0011、`spikes/runtime-contract/`、`spikes/config-store/`。
    - 退出条件：workspace 默认命令通过；环境由构建产物固定；两个数据根无读取、写入或锁 fallback；runtime 正常启动、更新 snapshot、拒绝队列溢出并有序 shutdown。
    - 验收证据（2026-08-30）：`native/` 仅包含 app/runtime/config；11 项单元测试覆盖严格 schema、共享默认 fixture、原子写入、revision 冲突、双环境根、typed snapshot、队列满返回原 command 和 shutdown。Development 默认构建与 `BONGOCAT_BUILD_ENV=production` 构建使用同一代码、不同编译期常量；format、Clippy、test 和 release check 本机通过，三平台 CI 已配置。
16. [x] `P4-MOTION-AUDIO`：实现 motion UserData 与不阻塞 runtime 的单 voice 音效闭环。
    - 依赖：正式 model/live2d/runtime、ADR-0012、预置 model3/FLAC。
    - 退出条件：UserData 跨帧/loop 不重复且有界；accepted motion 才播放；抢占、无 sound、
      stop、disable、成功切模、故障、overflow 和 shutdown 行为有自动化证据。
    - 验收证据（2026-08-31）：`bongocat-audio`、runtime side-effect 接线、真实预置 FLAC
      decoder 与 motion event/audio contract 已进入正式 workspace；完整 Native format、
      Clippy、test、release check、双 Windows target check 和 CI 结果随对应提交记录。
17. [x] `P1-SETTINGS-WINDOW-LIFECYCLE`：设置窗口关闭后保持后台产品运行，并可从当前
        revisioned snapshot 重建窗口。- 依赖：正式 GPUI 设置窗口、app coordinator、runtime/render owner。- 退出条件：window close 不触发 shutdown；窗口隐藏/销毁期间 frame source 继续推进；
        macOS reopen 只创建一个新 GPUI Entity，Windows reopen 只重显保留的唯一 Entity，且都
        从当前 revisioned snapshot 刷新；显式 Quit 仍按既定顺序 join 全部 owner；
        Windows/macOS release smoke 与完整 Native workspace 门禁通过。- 状态（2026-08-31）：macOS release smoke 和 Windows platform target check 本机通过；
        Windows run `33328391234`、job `99302481796` 已证明普通 close 隐藏有效，但随后允许真实
        `WM_DESTROY` 的两阶段退出仍以 `0xC0000409` fast-fail。上游 commit
        `399258feeaf90ad8a3a208c99221ee87b6452f38` 保留同一同步重入回调，因此当前实现改为先
        有序停止并 join 全部 BongoCat owner，再由 Windows adapter 跳过最终 GPUI 窗口析构；
        Windows 原生 lifecycle CI 和完整门禁通过前保持未勾选。- 状态（2026-08-31）：run `33330226417`、Windows job `99307365560` 的编译、Clippy、
        测试和 release check 均通过，但 lifecycle script 的 `Process.MainWindowHandle` 选中了
        独立 overlay，导致错误地关闭猫窗口并报告 overlay/设置窗口双失败；macOS job
        `99307365568` 已通过。runner 现改为按标题和 PID 定位 GPUI 设置窗口并发送真实
        `WM_CLOSE`。- 状态（2026-08-31）：替代 run `33331197902`、Windows job `99309931267` 的 workspace
        门禁再次通过，唯一失败仍为 product lifecycle smoke；精确标题查找没有在内部 3 秒隐藏
        截止前取得 HWND，随后只报告产品已退出且遗漏重定向日志。runner 现按 PID 枚举可见顶层
        窗口、排除独立 overlay，并在所有失败路径输出 HWND 清单与产品 stdout/stderr；等待新的
        原生 Windows run 区分窗口发现问题与产品故障。- 状态（2026-08-31）：run `33332271286`、Windows job `99312838431` 证明外部枚举选中的
        fallback HWND 在延迟投递前已失效，`PostMessage(WM_CLOSE)` 因此失败，产品内部也未观察到
        settings close。smoke 现由 Windows platform adapter 从 GPUI 公共 raw-window-handle 精确
        取得设置 HWND 并异步投递真实 `WM_CLOSE`；CI 不再枚举或猜测产品窗口，待原生 run 复验。- 验收证据（2026-08-31）：commit `9365eda` 的 push run `33333789799` 全绿；Windows job
        `99316966532` 与 macOS job `99316966517` 均通过 release lifecycle smoke、完整 workspace
        tests 和有序 shutdown，Windows 真实 `WM_CLOSE` 后 frame source 继续、保留 Entity 重显并
        恢复 snapshot。Ubuntu job `99316966591` 通过共享 contract、Clippy、test 和 release check。- 状态（2026-08-31）：Models 页面提交的 PR run `33338726693`、Windows job
        `99330277028` 在 release lifecycle smoke 暴露 GPUI `AsyncApp::update` 时序重入并以
        `RefCell already borrowed`/`0xC0000409` 退出；同提交 push job 偶然通过，不足以维持完成
        声明。Windows frame tick、close/hide/reopen 检查和定时退出现改为经保留的唯一
        `WindowHandle` 使用 GPUI 可失败的 window update，并对短暂占用做有界重试；原生 CI 改为
        连续五轮 lifecycle smoke。等待新 run 全部通过后恢复勾选并记录证据。- 状态（2026-08-31）：push run `33340053848`、Windows job `99333935406` 的完整 backtrace
        将重入定位到 `ProductOverlaySession::tick -> pump_window_messages -> GPUI window proc ->
AsyncApp::update`，而非 close/reopen 本身。commit `7fe3d10` 将 Windows overlay tick 移出
        GPUI `App`/`Window` borrow，并把 Win32 pump 仅保留给 standalone `run_for`；显式退出改为
        原子请求，由唯一 frame owner 在 tick 边界执行有序 shutdown。首轮复验的 Windows Clippy
        只发现 cfg 后未使用的 async context，当前批已修正；真实五连跑通过前仍保持未勾选。- 验收证据（2026-08-31）：commit `b54080a` 的 push run `33342464726` 与 PR run
        `33342466529` 全绿；Windows jobs `99340456964`/`99340462222` 各自连续五轮通过真实
        `WM_CLOSE`、frame source 继续、唯一 Entity 重显、revisioned snapshot 刷新、Models 页面
        操作与显式有序 shutdown。macOS jobs `99340456930`/`99340462228` 通过 release close/reopen、
        Entity 重建、Models 页面与 shutdown smoke，Ubuntu jobs `99340456922`/`99340462194` 通过
        共享 contract、Clippy、workspace tests 和 release check。
18. [x] `P4-MODEL-CATALOG`：建立来源感知的预置/用户模型合并目录并投影到设置服务。
    - 依赖：正式 `bongocat-model`、环境 `ModelStore`、只读预置资源和 typed settings snapshot。
    - 退出条件：应用持有 preset catalog；preset/installed 的 ready/invalid 条目都可见且确定
      排序；重复 ID 保留 `(origin, id)` 复合身份；snapshot 只暴露稳定诊断而不泄漏路径；
      model/app/ui 单元测试、Clippy 与完整 Native workspace 门禁通过。
    - 验收证据（2026-08-31）：来源合并、无效条目、重复 ID、确定排序、路径脱敏与 typed
      snapshot 测试均进入 `next`；push run `33333789799` 的 Windows/macOS/Ubuntu workspace
      jobs `99316966532`/`99316966517`/`99316966591` 全部通过。
19. [x] `P4-MODEL-SELECTION`：以 `(origin, model_id)` 从设置服务事务切换并持久化模型。
    - 依赖：`P4-MODEL-CATALOG`、runtime/renderer model commit、config expected revision。
    - 退出条件：typed command 不靠字符串推断来源；preset/installed 同 ID 可分别选择；当前 v1
      配置直接保存成对的 origin/id；CPU/GPU/配置失败保留当前模型，GPU 拒绝恢复旧配置；
      重启重新加载所选来源；schema fixture、定向测试与完整 Native workspace 门禁通过。
    - 验收证据（2026-08-31）：复合身份选择、重启、CPU/GPU/config rollback 与 Windows/macOS
      renderer rejection 测试均进入 `next`；push run `33333789799` 的三平台 workspace jobs
      全部通过，Windows job 又通过 transactional D3D11 model switching smoke。2026-09-04 将当前
      完整结构重置为 v1 后，本地 workspace 回归继续覆盖同一行为。
20. [x] `P4-MODEL-IMPORT-COMMAND`：从设置服务显式导入用户确认的外部模型目录。
    - 依赖：环境 `ModelStore`、来源感知 catalog、typed settings command。
    - 退出条件：UI command 强类型携带 model ID/source root，文件 I/O 不在 UI executor；导入
      复制、复验并原子提交到当前环境且不隐式切换；成功 snapshot 刷新 installed 条目；非法
      ID、重复 ID、无效包、源变化/不支持项、store busy/I/O 映射为稳定且不泄漏路径的错误码；
      ui/app 定向测试与完整 Native workspace 门禁通过。
    - 验收证据（2026-08-31）：typed request、settings worker 接线和稳定错误映射已实现；系统
      文件选择 wrapper 及模型页面 loading/error/retry 属于后续任务。
      ui/app 定向测试与完整 Native format、Clippy、workspace test、release check、Linux shared
      contract check 本机通过；push run `33333789799` 的三平台 workspace jobs 全部通过。
21. [x] `P4-MODEL-IMPORT-OPERATION`：为模型导入提供可观测、可取消的长操作契约。
    - 依赖：`P4-MODEL-IMPORT-COMMAND`、环境 `ModelStore` staging transaction、typed settings
      command/reply。
    - 退出条件：所有 client clone 共享单调 typed operation ID；progress 只公开固定 stage、文件数
      和字节数且三者单调；复制期间 cancellation 无需 settings worker 消费第二条 command；提交前
      取消清理 staging、不创建目标、不刷新 catalog revision；成功保持既有不隐式选模语义；
      final result 携带原 operation ID，service shutdown/join 确定完成；model/ui/app contract test
      与完整 Native 本地门禁通过。
    - 验收证据（2026-08-31）：`ModelStore` 使用 64 KiB 有界分块复制并在准备、遍历、复制、复验
      和 rename 前检查取消；settings operation 以共享 atomic token 更新无路径 progress，并返回
      稳定 `ModelImportCancelled`。测试覆盖跨 clone ID、倒退 progress 拒绝、typed final result、
      中途取消清理、catalog revision 不变、成功四阶段及 shutdown/join；完整 format、Clippy、
      workspace test 和 release check 本机通过；push run `33335183755` 的 Ubuntu/Windows/
      macOS workspace jobs `99320715006`/`99320715016`/`99320715124` 全部通过。Windows job
      同时通过 D3D11 product overlay、missing-release recovery 与 transactional model switch
      smoke，macOS job 通过 release settings lifecycle smoke。
22. [x] `P4-MODEL-DELETE-COMMAND`：按来源身份安全删除未选择的 installed 模型。
    - 依赖：`P4-MODEL-CATALOG`、`P4-MODEL-SELECTION`、`ModelStore` rename-delete transaction。
    - 退出条件：typed command 携带 `(origin, id)`；preset 和当前 runtime/config 所选 installed
      均拒绝；激活同 ID preset 不阻塞删除 installed 副本；成功刷新 catalog/revision 且不切模
      或改配置；非法 ID、未安装、store busy/I/O 返回稳定无路径错误；app/ui 定向测试与完整
      Native workspace 门禁通过。
    - 验收证据（2026-08-31）：核心来源判断、typed client/service、全部 store diagnostic
      的稳定错误映射及 app/ui 定向测试已进入 `next`；push run `33333789799` 的 Windows/macOS/
      Ubuntu workspace jobs `99316966532`/`99316966517`/`99316966591` 全部通过。
23. [x] `P4-MODEL-PARSER-PROPERTY`：固定模型包解析的随机输入安全边界。
    - 依赖：`bongocat-model` package limits、路径规范化、model3 JSON 与 PNG header parser。
    - 退出条件：可收缩生成器覆盖畸形 JSON、数组位置、portable ID、平台路径和任意长度 PNG
      header/dimensions；接受路径保持包内相对且幂等，parser 不 panic/OOB；JSON/package/file/
      dimension 上限在无界解析或像素分配前失败；测试依赖版本/许可证/维护性/替换边界有记录；
      完整 Native 本地门禁和 license/source policy 通过。
    - 验收证据（2026-08-31）：6 组 property 每轮共执行 3,072 case，另有固定 limit/depth/
      symlink/oversized fixture；`proptest 1.11.0` 以最小 `std` feature 精确锁定，`cargo update`
      只新增其和两个缺失传递包。完整 format、Clippy、workspace test、release check 及
      `cargo deny --all-features check licenses sources` 本机通过；push run `33336116944` 的
      Ubuntu/Windows/macOS Native workspace jobs `99323200807`/`99323200905`/`99323200915`
      与 dependency policy job `99323200931` 全部通过。
24. [x] `P4-MODEL-FIXTURE-CONTRACT`：将共享自定义模型 fixture 提升为正式产品导入契约。
    - 依赖：`shared/fixtures/model-fixtures/cases.json`、`PreparedModel`、transactional
      `ModelStore`。
    - 退出条件：manifest 严格反序列化且每个 case 目录唯一注册；所有 accept/reject case 在
      隔离物化后由产品 parser 与 store 同时执行；拒绝诊断精确匹配声明 stage，不写目标或
      staging，不修改源；成功只提交一个来源感知 installed model；完整 Native 门禁通过。
    - 验收证据（2026-08-31）：正式 crate 已覆盖 6 个共享合成 package；`bongocat-model`
      33 项、旧 model-package spike 15 项与 Python fixture oracle 全部通过。完整 Native format、
      Clippy、workspace test 和 release check 本机通过；push run `33336496654` 与 PR run
      `33336497984` 全绿，push 的 Windows/macOS/Ubuntu Native workspace jobs
      `99324223865`/`99324223945`/`99324223963` 全部通过。
25. [x] `P7-MODEL-DIRECTORY-PICKER`：以原生最小权限目录选择器接入模型导入。
    - 依赖：`P4-MODEL-IMPORT-OPERATION`、AppKit `NSOpenPanel`、Shell `IFileOpenDialog`。
    - 退出条件：共享 API 区分 selected/cancelled 和稳定无路径错误；macOS 强制 AppKit 主线程，
      Windows 使用 STA、folder/filesystem/path-exists/no-recent 且 COM/TaskMem 成对释放；Rust
      重新验证并 canonicalize；GPUI Models 页面不阻塞执行文件复制，可消费取消与选择结果；
      双平台真实选择/取消 smoke 和完整 Native 门禁通过。
    - 状态（2026-08-31）：共享验证、双平台 adapter、macOS background-thread contract 和
      Windows x64/ARM64 platform cross-check 已通过。Models 页面现已接入真实导航、64-byte
      ASCII model ID 草稿、无路径 folder 状态、typed operation、100 ms progress、cancel、retry
      和 catalog refresh，全部命令支持 Tab 焦点及 Enter/Space 激活；UI 测试覆盖建议
      ID 的 portable/长度/保留名边界、输入过滤、键盘激活、状态脱敏，以及 operation 入队前的
      cancel 请求在 control 建立后立即生效。复制、解析和复验仍只在 settings worker 执行，不
      阻塞 GPUI executor。
      初次产品实机交互发现同步 `runModal` 会重入 GPUI 并触发 `RefCell already borrowed`；现已
      改用 AppKit completion handler，选择后的文件系统复验移至短生命周期 worker，Windows
      阻塞 COM dialog 也移至专用 STA worker。macOS 26.5.2 arm64 已通过真实 `NSOpenPanel`
      Cancel 和仓库预置 `standard` 目录 Select：页面分别显示 `Selection cancelled` 与
      `Folder selected`/建议 ID `standard`，进程未崩溃且最终经产品 Quit 正常退出；未触发导入。
      Windows release smoke 用 PID 限定的 Win32 controller 驱动真实 dialog 标准取消/确认路径，
      并由 callback 超时及 Rust 目录复验保护。commit `5f88fb8` 的 push run `33348859607` 全绿，
      Windows/macOS/Ubuntu Native jobs `99358134554`/`99358134575`/`99358134545` 均通过完整
      format、Clippy、workspace test、release/Production 和平台 smoke 门禁；commit `0e5072e`
      的 push run `33349095568`、Windows job `99358790654` 进一步通过真实 dialog cancel/select
      release smoke。结合本机 macOS 真实交互证据，双平台退出条件已满足。
      `block2 0.6.2`、`objc2 0.6.4`、AppKit/Foundation `0.3.2` 与 `windows 0.62.2` 均为当前
      最新稳定版并已在 workspace 锁定；最低 Rust 1.71/1.82、MIT/Zlib/Apache-2.0 许可证兼容
      workspace，替换边界仅为对应 OS 原生 API binding。完整 Native format、Clippy、workspace
      test、release/Production check、license/source policy、Linux workspace Clippy 与双 Windows
      target platform Clippy 本机通过；macOS 可重复 callback smoke example 已同步更新。
26. [x] `P4-MODEL-MANAGEMENT-UI`：在 Models 页面完成来源感知的激活与删除闭环。
    - 依赖：`P4-MODEL-CATALOG`、`P4-MODEL-SELECTION`、`P4-MODEL-DELETE-COMMAND` 和正式
      GPUI settings snapshot。
    - 退出条件：每行按 `(origin, model_id)` 保持身份，重复 ID 不混淆；ready 且非 active 的
      模型可激活，invalid 模型不可激活并显示稳定无路径诊断；preset 与 active installed 不提供
      删除，其他 installed 删除前需显式确认且可取消；操作期间其他模型命令禁用，成功只接受
      不倒退的 revisioned snapshot，失败保留 catalog/active model 并显示可重试错误；所有动作
      支持可见 Tab 焦点与 Enter/Space，定向 UI contract、完整 Native 门禁和双平台页面 smoke
      通过。
    - 状态（2026-08-31）：页面已按 `(origin, model_id)` 渲染 active/ready/invalid 状态，接入
      typed activation 与 installed delete，提供 Cancel/Confirm 且保护 preset/active installed；
      operation 期间禁用冲突命令，异步结果只接受不倒退 revision，错误保留当前 catalog，invalid
      诊断不含路径。动态 focus handle 覆盖每行 Enter/Space，并修正确认态 Cancel/Confirm 的视觉
      与 Tab 顺序。14 项 UI 测试覆盖复合身份、删除资格、稳定诊断、按键和焦点顺序；macOS
      release product smoke 已实际切换并渲染 Models 页面后完成 close/reopen/shutdown。Windows
      五连跑与完整三平台 CI 已由 commit `b54080a` 的 push run `33342464726` 和 PR run
      `33342466529` 验证；Windows jobs `99340456964`/`99340462222` 各自连续五轮通过 Models
      页面 release product smoke，macOS jobs `99340456930`/`99340462228` 通过对应页面 smoke，
      Ubuntu jobs `99340456922`/`99340462194` 通过共享 UI contract 与完整 workspace 门禁。
27. [x] `P7-SYSTEM-MENU-LIFECYCLE`：提供双平台后台产品的系统菜单恢复入口与显式退出。
    - 依赖：`P1-SETTINGS-WINDOW-LIFECYCLE`、app shutdown coordinator、平台 UI 主线程。
    - 退出条件：Windows `Shell_NotifyIcon` + `HMENU` 与 macOS `NSStatusItem` + `NSMenu` 由明确
      owner 管理；Open Settings 不创建重复窗口并恢复当前 revisioned snapshot；Quit 停止菜单
      事件后进入既定 input/runtime/config/frame/renderer/overlay shutdown；callback 只发送强类型
      有序事件；双平台 release smoke、Windows x64/ARM64 source check 与完整 Native 门禁通过。
    - 状态（2026-08-31）：共享 `OpenSettings`/`Quit` contract、macOS 主线程 target/action、Windows
      隐藏 HWND callback 与显式菜单/status item cleanup 已接入 app coordinator；不新增第三方 tray
      crate，继续使用已锁定的 `objc2 0.6.4`/AppKit `0.3.2` 与 `windows 0.62.2`。macOS 本机真实
      status item owner + Objective-C target/action smoke 及既有 settings/Models release smoke 通过。
    - 验收证据（2026-08-31）：commit `9e97704` 的 PR run `33344287629` 全绿；Windows job
      `99345364734` 与 macOS job `99345364649` 均通过原生菜单 callback -> typed action -> settings
      恢复 -> 显式 Quit 的 release smoke，Ubuntu job `99345364707` 通过完整共享 workspace 门禁。
      Windows x64/ARM64 platform Clippy、完整 Native format/Clippy/test/release check 本机通过；
      callback 只入队，菜单 owner 在 input/runtime/config/frame/renderer/overlay 之前停止。
28. [x] `P7-WINDOWS-SINGLE-INSTANCE`：按构建环境隔离 Windows 单实例并唤醒现有设置窗口。
    - 依赖：`P1-SETTINGS-WINDOW-LIFECYCLE`、ADR-0008、Windows GPUI message loop。
    - 退出条件：Development/Production 使用不同的 local named mutex、owner window class 和
      registered wake message；primary 在任何 config/model writer 前取得 owner，secondary 不启动
      配置/runtime/input/GPU，只通知 primary 后成功退出；primary 将消息转为强类型
      `OpenSettings`，不创建重复 Entity，恢复当前 snapshot；owner 在产品 shutdown 中显式释放；
      双进程 release smoke、Windows x64/ARM64 source check 与完整 Native 门禁通过。
    - 验收证据（2026-08-31）：commit `c889115` 的 push run `33345266089`、Windows job
      `99348057229` 与 PR run `33345268535`、Windows job `99348064645` 均通过真实双进程
      release smoke：secondary 只通知 primary 后成功退出，primary 保持 frame source、重显
      原 Entity、恢复当前 snapshot 并完成有序 shutdown。两次 run 的 macOS/Ubuntu workspace
      门禁也通过；本机完整 Native 门禁及 Windows x64/ARM64 platform Clippy 通过。
29. [x] `P7-MACOS-APPLICATION-REOPEN`：通过正式 `.app` 和 LaunchServices 唤醒后台产品。
    - 依赖：`P1-SETTINGS-WINDOW-LIFECYCLE`、GPUI `on_reopen`、ADR-0008、产品资源目录。
    - 退出条件：`.app` 固定 Bundle ID、最低系统和禁止多实例 metadata，内置三个预置模型且
      executable 从 `Contents/Resources` 加载；再次 `open` 只触发既有进程的 AppKit reopen，
      已销毁设置 Entity 只重建一个并恢复当前 snapshot，后台 frame source 持续；退出仍进入
      shutdown coordinator；ad-hoc strict codesign、release LaunchServices smoke 和完整 Native
      门禁通过。Distribution signing、Hardened Runtime/notarization 继续由发布门禁跟踪。
    - 验收证据（2026-08-31）：最小产品 `Info.plist`、可重复打包脚本、bundle resource resolver
      与 application-reopen smoke 已实现；本机 release `.app` 先销毁设置 Entity，再从外部执行
      第二次 `open`，验证进程数保持 1、新 Entity 恢复 revisioned snapshot、frame source 持续、
      ad-hoc strict codesign 和正常 shutdown。commit `2aba0e8` 的 push run `33347041829` 全绿，
      macOS Native job `99353029349` 的正式 `.app` LaunchServices smoke 明确报告 primary ready、
      application reopen callback、设置窗口恢复和正常 quit；同一 job 的 format、Clippy、workspace
      test、release、Production build 与系统菜单 smoke 均通过。Distribution signing、Hardened
      Runtime/notarization 仍由发布门禁跟踪，不计入本项完成声明。
30. [x] `P7-STARTUP-ITEM-PLATFORM`：实现环境隔离的双平台当前用户启动项 adapter。
    - 依赖：ADR-0008、ADR-0013、正式 build environment 和产品 executable identity。
    - 退出条件：共享稳定 state/error 区分 disabled/enabled/stale/requires-approval/unsupported；
      Windows HKCU Run value 按环境分名、精确匹配当前 executable + `--run-seconds 0` 且无需管理员；
      macOS 13+ Production 使用 `SMAppService.mainAppService`，macOS 12 与 Development 明确
      unsupported 且不触及生产登录项；读取不改变系统状态，显式启用/禁用可恢复原状态；双平台
      平台 smoke、Windows x64/ARM64 source check 与完整 Native 门禁通过。
    - 状态（2026-08-31）：ADR-0013 已接受；共享 state/error、Windows UTF-16 HKCU Run
      adapter、macOS runtime class availability/Production-only `SMAppService` adapter 和恢复型
      双平台 smoke 已实现。`objc2-service-management 0.3.2` 为当前最新稳定 binding，许可证、
      维护方与替换边界已审计；完整本地门禁通过。commit `b84c910` 的 push run
      `33351444078`、Windows job `99365495806` 已通过真实 HKCU disabled -> enabled -> stale ->
      disabled 恢复 smoke；commit `17f9a3c` 的 push run `33352737430`、macOS job
      `99369071727` 进一步证明复制到 `/Applications` 唯一目录并由 LaunchServices 启动的 ad-hoc
      bundle 初态仍为 `NotFound`，但旧 smoke 在注册前错误拒绝该可操作状态。本机 Production
      `.app` 已真实通过 `NotFound` -> register -> unregister -> `Disabled` 并清理安装目录。
      commit `62f8c8f` 的 push run `33354177622` 全绿；Windows job `99373058496` 再次通过真实
      HKCU lifecycle，macOS job `99373058428` 明确输出 `NotFound` -> register/unregister ->
      `Disabled`，并完成 `/Applications` 临时安装、LaunchServices 注销和目录清理。双平台
      workspace、Production build、平台 source check 与其余 release smoke 同时通过，退出条件满足。
31. [x] `P5-STARTUP-ITEM-UI`：以 typed settings command/snapshot 接入 General 启动项控件。
    - 依赖：`P7-STARTUP-ITEM-PLATFORM`、现有 revisioned `SettingsSnapshot` 和 settings worker。
    - 退出条件：状态读取与启用/禁用不阻塞 GPUI executor；控件覆盖 loading、enabled、disabled、
      stale、requires-approval、unsupported 和 retry；Development/macOS 12 不允许 mutation；窗口重建
      从新 snapshot 恢复，错误不改变 runtime/config；键盘、accessibility、双平台页面 smoke 与完整
      Native 门禁通过。
    - 状态（2026-08-31）：UI 自有 startup state/error、typed enable command、settings worker
      平台映射和 revision observation 已接入；General 控件覆盖 loading、disabled、enabled、stale、
      requires-approval、not-found、unsupported 与 read-error retry，操作支持 Tab 和 Enter/Space。
      模拟服务测试证明外部状态/read error 会递增 revision，变更失败和成功都不改 config/runtime，
      shutdown 保留最后状态；General product smoke 已进入双平台既有 settings lifecycle。完整 Native
      format/Clippy/test/release/Production、license/source policy 和 Windows x64/ARM64 platform Clippy
      本机通过；commit `62f8c8f` 的 push run `33354177622` 中 Windows/macOS jobs
      `99373058496`/`99373058428` 均通过 General 页面、窗口重建和 shutdown，macOS 同时通过安装态
      startup-item mutation smoke。2026-08-31 已在正式 `bongocat-platform` 接入项目自有
      AccessKit tree：General、Models、Diagnostics、overlay/audio/startup switches、Refresh 和
      Quit 均有稳定 role/label/value/toggle/focus/click 投影；loading/unsupported 状态不暴露
      mutation action，action 经容量 32 的 typed channel 回到 GPUI 并复用现有 focus/command 路径。
      `cargo test -p bongocat-platform -p bongocat-ui` 已通过 tree validation、toggle/value/action
      contract；本机 release product smoke 已从正式 AppKit AX 对象读取 startup 的
      `AXCheckBox`/`AXSwitch`、布尔值、enabled 和 press selector，且未触发登录项 mutation。
      commit `718e3f4` 的 pull request run `33364047140` 全绿；Windows job `99400940944`
      通过真实 UIA Button/switch、TogglePattern off/on 状态切换与恢复、enabled/focusable 和
      SetFocus，并同时通过完整 D3D11、输入和模型 smoke。macOS job `99400940878`、Ubuntu job
      `99400940817` 与其余 contract jobs 同时通过。真实 VoiceOver/Narrator 操作仍属于更宽的
      Phase 0 辅助技术门禁，不阻塞本项 typed UI 闭环完成。
32. [x] `P5-INPUT-DIAGNOSTICS-UI`：把 runtime 输入可靠性计数投影到真实 Diagnostics 页面。
    - 依赖：正式 `RuntimeSnapshot.input`、revisioned `SettingsSnapshot` 和双平台 settings lifecycle。
    - 退出条件：UI 协议只包含 pressed 数量及 captured/reconciled/reset、sequence、overflow 的匿名
      聚合计数，不含具体键值、原始事件、路径或平台类型；transport-only 变化推进 settings revision；
      页面覆盖 loading、service error 和 retry，导航/刷新支持 Tab 与 Enter/Space；双平台 release
      settings smoke 实际切换并渲染页面，定向 contract 与完整 Native 门禁通过。
    - 状态（2026-08-31）：`SettingsInputDiagnostics` 已逐字段投影 19 项 runtime/transport 计数，
      settings clock 独立观察该投影；侧栏占位已替换为可键盘访问的双列 Diagnostics 页面，既有
      Refresh 提供 loading/error/retry，双平台 settings lifecycle smoke 会先验证 General 再切换
      Diagnostics。本机 800x600 Production `.app` 可视检查证明 19 项指标与底部操作无重叠；
      Development release settings lifecycle、完整 Native format/Clippy/test/release/Production、
      license/source policy、Linux app Clippy 与 Windows x64/ARM64 platform Clippy 均通过。
      commit `62f8c8f` 的 push run `33354177622` 全绿；Windows/macOS jobs
      `99373058496`/`99373058428` 均实际通过 General -> Diagnostics 页面切换、close/reopen 和有序
      shutdown，Ubuntu job `99373058388` 通过共享 UI contract 与完整 workspace 门禁，退出条件满足。
    - 增量证据（2026-09-01）：Diagnostics 在配置状态与 25 项计数前新增平台输入服务状态带，
      stable status/单次尝试计数进入 settings 独立 revision；真实 Development permission-denied
      bundle 的 800px 宽可视与 AppKit accessibility tree 检查通过，未显示平台错误文本。
33. [x] `P6-CONFIG-BACKUP-RETENTION`：为正式配置提交建立有界、可审计的备份集合。
    - 依赖：正式 `ConfigStore`、当前 v1 schema 和环境 writer lock。
    - 退出条件：每份备份携带格式版本、墙上时间、源 schema/revision 和原始配置；按环境限制
      数量与总大小；系统时钟回退不误删新备份；不删除非自有文件；备份失败不替换当前配置；
      config 定向测试、Clippy 和完整 Native workspace 门禁通过。
    - 验收证据（2026-08-31）：backup envelope、12 次提交后的 8 份/8 MiB 收敛、未知文件保留、
      排序键时钟回退和 expected-revision 提交均有正式 crate 单元测试；commit `25d5030` 的
      pull request run `33364970646` 全绿，Windows/macOS/Ubuntu Native jobs
      `99403612087`/`99403611991`/`99403612068` 通过完整 format、Clippy、workspace test、
      release/Production 和平台 smoke，dependency policy、shared schema 与 config-store jobs
      同时通过。2026-09-04 重置为当前 v1 后，本地 config 与 workspace 测试继续覆盖这些契约。
34. [x] `P6-CONFIG-INVALID-LOAD`：固定无效配置保留和当前 v1 重复读取契约。
    - 依赖：正式 `ConfigStore` 和严格 Native v1 schema。
    - 退出条件：损坏、截断、错误类型、越界值和未知字段均返回错误且不覆盖/备份当前文件；
      有效 v1 连续加载 10 次结果和 revision 不变且不产生重复备份；config 定向测试、Clippy 与
      完整 Native workspace 门禁通过。
    - 状态（2026-09-04）：正式 crate 已加入五类无效输入逐字节保留测试和 10 次 v1 reload
      门禁；本机定向测试、Clippy 与完整 workspace 门禁通过。
35. [x] `P6-CONFIG-BACKUP-RECOVERY`：从验证通过的 Native 备份恢复损坏的正式配置。
    - 依赖：`P6-CONFIG-BACKUP-RETENTION`、`P6-CONFIG-INVALID-LOAD` 和正式 app 启动装配。
    - 退出条件：按新到旧验证格式/schema/revision/typed config，只提交首个有效候选；损坏 current
      逐字节进入有界环境内 quarantine；无有效候选或归档/验证失败时不默认覆盖；恢复重启幂等，
      Development/Production 隔离；app 暴露不含路径的恢复诊断；config/app 定向测试、Clippy 和
      完整 Native workspace/三平台 CI 门禁通过。
    - 验收证据（2026-08-31）：正式 store 从新到旧验证 backup format、源 schema/revision 与
      typed config，未来格式/schema 和 revision mismatch 均被跳过；损坏 current 逐字节进入每环境
      4 份/8 MiB quarantine，无候选、未来 current schema、重复启动和双环境隔离均有单元回归，
      app 集成测试确认恢复值进入 runtime 且只保留匿名诊断。commit `11f5509` 的 pull request run
      `33367819458` 全绿；Windows/macOS/Ubuntu Native jobs `99412066607`/`99412066610`/
      `99412066583` 通过完整 format、Clippy、workspace test、release/Production 与平台 smoke，
      Windows input/config job `99412066542` 也通过真实路径与存储测试。
36. [x] `P6-CONFIG-RECOVERY-DIAGNOSTIC`：把成功配置恢复投影到正式 Diagnostics 页面。
    - 依赖：`P6-CONFIG-BACKUP-RECOVERY`、revisioned `SettingsSnapshot` 和正式 Diagnostics 页面。
    - 退出条件：settings 协议只公开源 schema 与跳过候选数，不包含路径、原始 JSON、时间戳或
      I/O 文本；正常加载与恢复成功均有明确状态；refresh、shutdown snapshot 和 800x600 页面
      smoke 保持一致且无重叠；UI/app 定向测试、完整 Native 门禁和三平台 CI 通过。
    - 验收证据（2026-08-31）：协议、service 投影、Diagnostics 状态行、正常/恢复 presentation
      测试和 service refresh/shutdown 回归已实现；本机 800x600 release `.app` 可视检查通过。
      commit `260083d` 的 pull request run `33369531252` 全绿；Windows/macOS/Ubuntu Native jobs
      `99417224388`/`99417224402`/`99417224398` 通过完整 format、Clippy、workspace test、
      release/Production 与平台 smoke，Windows input/config job `99417224387` 同时通过。
37. [x] `P6-CONFIG-INTERRUPTED-COMMIT`：把强杀中断后的确定性配置恢复提升到正式产品 store。
    - 依赖：正式 `ConfigStore`、`P6-CONFIG-BACKUP-RECOVERY` 和环境 writer lock。
    - 退出条件：正式提交以固定同目录 `config.json.tmp` 执行 flush、备份、跨平台原子替换和提交后
      验证；有效/缺失/损坏 current 与有效/无效 temp 组合均保守恢复；非 v1 schema temp 原样保留；
      stale/invalid archive 每环境合计最多 4 份/8 MiB，未知文件与另一环境不受影响；强杀持锁
      子进程后 OS lock 释放且启动在 1 秒内有界重试；app 只公开匿名 action；config/app 定向测试、
      完整 Native workspace、三平台 CI 和 Windows input/config job 通过。
    - 验收证据（2026-08-31）：正式 store、状态机、有界归档、未知 schema 保留、强杀子进程
      回归和匿名 app action 已实现。commit `0b7b118` 的 pull request run `33371888571` 全绿；Windows/macOS/Ubuntu Native jobs
      `99424654786`/`99424654816`/`99424654950` 通过完整 format、Clippy、workspace test、
      release/Production 与平台 smoke，Windows input/config job `99424654701` 实际通过 Windows
      原子替换、强杀 lock 释放、启动恢复和真实存储路径测试。2026-09-04 的 v1 重置将同一路径
      收紧为拒绝全部非 v1 schema，并通过本地完整 Native workspace 与仓库策略门禁。
    - 补充证据（2026-08-31）：workspace 并行测试在 `File` 析构后紧接重入时观察到瞬时
      `LockUnavailable`，commit `a760ce0` 为 writer lock RAII guard 增加显式 `unlock()`；本机
      32 测试线程重复运行与完整
      workspace 均通过，后续 run `33381198560` 的独立 config-store job `99453718598` 和三平台
      workspace 再次全绿，普通 commit 的非阻塞竞争语义保持不变。
38. [x] `P6-CONFIG-SAFE-RECOVERY`：在无有效备份时进入受限设置并提供显式恢复默认 command。
    - 依赖：`P6-CONFIG-BACKUP-RECOVERY`、`P6-CONFIG-RECOVERY-DIAGNOSTIC` 和 typed settings command。
    - 退出条件：无有效候选时不覆盖 current、不启动 overlay/GPU，Application 进入 recovery-only
      settings；snapshot 公开匿名状态与候选计数，所有业务写入/模型/启动项操作被拒；显式恢复默认
      在 writer lock 内二次确认、quarantine 原字节、原子写入并验证 v1 默认配置，恢复后要求重启；
      非 v1 schema、归档/验证失败保留原文件并返回稳定错误；config/app/ui 定向测试、完整 Native
      workspace、三平台 CI 和 recovery window smoke 通过。
    - 状态（2026-08-31）：config/app/ui 定向测试已通过（config 23、app 29、ui 21）。commit
      `e2ced51` 的 pull request run `33374202985` 全绿；Windows/macOS/Ubuntu Native jobs
      `99431897523`/`99431897620`/`99431897612`、Windows input/config job `99431897588`、
      Windows GPUI jobs `99431897503`/`99431897512` 和 macOS GPUI job `99431897618` 均通过。
      显式 Development 测试产物另提供 `--configuration-recovery-smoke`，只在独立临时存储根
      写入损坏 current，验证匿名 recovery snapshot、真实 recovery-only GPUI 窗口、settings service
      有序停止与临时数据清理；本机 macOS smoke 通过。commit `175e7a4` 的 pull request run
      `33376471972` 全绿，
      Windows/macOS/Ubuntu Native jobs `99438972370`/`99438972328`/`99438972320` 通过完整门禁，
      Windows 与 macOS Native jobs 均实际通过新增 recovery window smoke；Windows input/config job
      `99438972066` 及双平台 GPUI spike jobs 同时通过，退出条件满足。
39. [x] `P6-CONFIG-WRITE-FAILURES`：稳定分类并投影配置写入的可恢复存储失败。
    - 依赖：`P6-CONFIG-INTERRUPTED-COMMIT`、正式 settings error contract 和原子 writer。
    - 退出条件：权限/只读、空间/配额不足和固定 temp 目标占用具有稳定匿名原因与独立 settings
      error；temp 创建前权限失败、创建后磁盘满和真实文件/目录占用均可重复注入，失败逐字节保留
      current、不推进 snapshot/revision，只清理本次调用创建的 partial temp，绝不删除预先/并发占用
      条目；config/app/ui 定向测试、完整 Native workspace、三平台 CI 和 Windows config job 通过。
    - 验收证据（2026-08-31）：config 25、app 31、ui 22 项定向测试覆盖阶段注入、真实文件/目录
      占用、current/占用条目保留、partial temp 清理、snapshot revision 不变和匿名 settings error。
      commit `0549f33` 的 pull request run `33378437342` 全绿；Windows/macOS/Ubuntu Native jobs
      `99445071780`/`99445071706`/`99445071635` 通过完整 format、Clippy、workspace test、release/
      Production 与平台 smoke，Windows input/config job `99445071726` 和 config-store job
      `99445071760` 同时通过，退出条件满足。
40. [x] `P6-CONFIG-BACKUP-LOCATION`：从 Diagnostics 安全打开当前环境配置备份目录。
    - 依赖：正式环境 `StorageLayout`、revisioned settings protocol、`P6-CONFIG-SAFE-RECOVERY` 和
      双平台 platform adapter。
    - 退出条件：无路径参数的 typed command 只打开 Application 派生的当前环境 `backups/`；UI、
      snapshot 和 error 不包含路径或原始 OS 文本；platform adapter 验证/canonicalize 绝对目录并以
      独立参数启动 Finder/Explorer，不使用 shell；成功不推进 revision，失败保留 snapshot 并返回
      稳定匿名错误，recovery-only 可用；Diagnostics 覆盖 pending、键盘和 accessibility 状态；
      platform/app/ui 定向测试、完整 Native workspace、三平台 CI 和双平台 GPUI smoke 通过。
    - 验收证据（2026-08-31）：typed protocol、Application capability、Finder/Explorer adapter、
      Diagnostics 控件与匿名错误/不变 revision/recovery-only 回归已完成；本机 platform 17、ui 23、
      app 33 项定向测试、严格 Clippy、完整 workspace、release/Production 与真实 recovery window
      smoke 通过。commit `6b41808` 的 run `33381198560` 全绿；Windows/macOS/Ubuntu Native jobs
      `99453718576`/`99453718477`/`99453718406` 通过完整门禁，Windows/macOS 分别执行 opener
      参数 contract；Windows input/config job `99453718404`、Windows/macOS GPUI jobs
      `99453718327`/`99453718079` 和 config-store job `99453718598` 同时通过，退出条件满足。
41. [x] `P6-BUILD-ENV-METADATA`：让正式构建和打包入口显式固定 Development/Production。
    - 依赖：ADR-0008、正式 app build script、Native workspace/CI 与 macOS packaging baseline。
    - 退出条件：build script 不含隐式 fallback，只接受精确的 `development`/`production` 并把结果
      编译为 immutable cfg；Native workspace 和 CI 显式选择 Development，Production check/package
      显式覆盖；packaging 在调用 Cargo 前拒绝缺失、空和未知值；运行时 CLI/env/settings 不能切换；
      解析 contract、缺失/未知失败 smoke、完整 Native workspace 与三平台 CI 通过。
    - 验收证据（2026-08-31）：严格解析器、workspace/CI 选择、packaging guard 与本机成功/拒绝
      路径已实现；commit `2810f4a` 的 pull request run `33383026191` 全绿。Windows/macOS/Ubuntu
      Native jobs `99459402028`/`99459402083`/`99459402181` 通过完整 workspace、Development
      release、显式 Production 和隐式环境拒绝门禁；macOS job 还在 Cargo 前覆盖缺失、空与未知
      packaging 值，双平台 GPUI、Windows input/config、dependency policy 与 config-store jobs
      同时通过，退出条件满足。
    - 补充证据（2026-09-05）：修复 macOS 打包脚本中 host target 的 `awk` 引号错误；
      `sh -n`、Production `.app` 打包、Bundle ID/最低系统版本、release provenance 字段和
      `codesign --verify --deep --strict` 均在本机 Apple Silicon 通过。
42. [x] `P6-STORAGE-LAYOUT-BOUNDARY`：隔离正式平台路径解析与临时测试存储注入。
    - 依赖：`P6-BUILD-ENV-METADATA`、ADR-0008、正式 `Application::start` 与 recovery window smoke。
    - 退出条件：默认产品 API/CLI 不接受 `StorageLayout`、根目录或 recovery storage override；
      正式启动只以 immutable build environment 调用当前平台 resolver；临时根注入必须显式启用
      Development-only feature，Production 组合在编译期失败；恢复窗口 smoke 使用独立测试产物且
      不覆盖默认 release binary；默认/feature 参数 contract、Production 拒绝、完整 Native workspace、
      三平台 CI、Windows input/config 与双平台 recovery window smoke 通过。
    - 验收证据（2026-08-31）：产品/测试 API、CLI feature gate、Production compile guard 和独立
      CI target 已实现；默认 release binary 不接受 recovery override，独立测试产物完成双平台窗口
      生命周期。commit `696319e` 的 pull request run `33386401135` 全绿；Windows/macOS/Ubuntu
      Native jobs `99469897044`/`99469896758`/`99469896811` 通过完整 format、Clippy、workspace
      test、release/Production 与平台 smoke，Windows/macOS jobs 均实际通过 recovery window；
      Windows input/config job `99469896784`、config-store job `99469896999`、双平台 GPUI 和依赖
      策略 jobs 同时通过，退出条件满足。
43. [x] `P6-CONFIG-TRANSACTION-PIPELINE`：验收正式配置加载、提交与最终验证闭环。
    - 依赖：正式 `ConfigStore`、当前 v1 schema、`P6-CONFIG-BACKUP-RETENTION` 和
      `P6-CONFIG-INTERRUPTED-COMMIT`。
    - 退出条件：current 在 writer lock 内按 load -> schema v1 check -> typed validate 执行；提交经固定
      同目录 temp、flush 和原子替换，最终重读比较 typed config/revision；替换后验证破坏可受控注入，
      失败逐字节恢复原 v1 并清理 temp；有效 v1 不重写，无效/非 v1 schema 不被覆盖；config 定向测试、严格 Clippy、完整
      Native workspace、三平台 CI、Windows input/config 和独立 config-store job 通过。
    - 验收证据（2026-09-04）：正式成功路径、有效 v1 重复读取、无效/非 v1 schema 保留，以及替换后
      验证破坏注入、原 bytes 回滚、temp 清理和重启重试均有正式 crate 回归。底层事务与故障注入
      最初由 commit `fd0f1d2` 建立；本次无迁移的 v1 实现已通过本地 format、Clippy、workspace test、
      release check、config-store contract 和 schema/fixture 门禁。
44. [x] `P6-STATE-WINDOW-LAYOUT`：以环境内 `state.json` 恢复所有产品窗口布局。- 依赖：`P6-STORAGE-LAYOUT-BOUNDARY`、正式 settings lifecycle、GPUI 公共 bounds API。- 退出条件：state 使用独立 v1 schema、`state.writer.lock` 和原子提交后验证，不进入 config
        revision/backup/recovery；settings 与 overlay 坐标/尺寸有界且支持负坐标，settings 另保存
        maximized；完全离屏或无已存状态时回到鼠标当前所在显示器居中；缺失、损坏、I/O 和非 v1 schema
        不阻塞 config/runtime，当前版本不覆盖未知 state；GPUI observer 合并变化后及时写入，overlay
        只在几何变化时写入，settings worker shutdown 强制 flush，配置更新、模型切换、macOS Entity
        重建与 Windows 隐藏/重显均保留最新几何，进程重启读回；config/ui/app 定向测试、严格
        Clippy、完整 Native workspace、三平台 CI 和双平台隔离 storage smoke 通过。- 验收证据（2026-08-31）：typed store、UI tracker、Application/settings worker 接线、损坏隔离、
        双环境、并发 lock、验证失败回滚、shutdown/restart 单测和 Development-only 双平台 smoke
        已实现。`cargo fmt --all -- --check`、`cargo test --workspace`、
        `cargo clippy --workspace --all-targets --all-features -- -D warnings`、
        `cargo check --workspace --release` 与 `python3 tools/validate-json-schema.py` 在本机通过；
        macOS Development release smoke `BONGOCAT_BUILD_ENV=development cargo run --manifest-path
native/Cargo.toml --locked -p bongocat-app --release --features storage-test-injection
--target-dir native/target/storage-test-injection -- --settings-window-state-smoke` 输出
        `settings window state restored after restart`。workflow `33395834870` 的 Native workspace
        jobs `99500010100`（Ubuntu）、`99500010122`（macOS）和 `99500010167`（Windows）以及
        Windows input/config job `99500010128` 全部通过；Windows 原生状态 smoke 输出与 macOS
        release smoke 一致。2026-09-02 增补运行中落盘、配置/模型更新不覆盖状态和 overlay 完整
        bounds 恢复；2026-09-04 将当前完整 state 结构重置为 v1。更新后的 Windows/macOS 实机
        显示器/DPI 热切换仍属于后续平台矩阵。2026-09-04 又将无已保存 bounds 时的
        `100%` 默认宽度统一为 `350px`，高度按当前模型 Canvas 宽高比自适应；完整 bounds 恢复、变化持久化和
        缩放时按比例更新的契约不变。
45. [ ] `P2-GAMEPAD-RUNTIME`：将双平台 GameController/XInput producer 接入正式 runtime。
    - 依赖：`InputControl::Gamepad` 按钮语义、Gamepad axis keyed latest-value contract、现有
      Windows/macOS 平台 producer spike。
    - 退出条件：按钮边沿与连接代次进入可靠 runtime 队列，六轴/trigger 使用独立 latest-value
      通道并应用 dead-zone/范围归一化；断开、重连、overflow 和 shutdown 不残留 pressed 或旧
      axis；三平台 contract、双平台 producer smoke、模型 Stick 参数回归和完整 Native 门禁通过。
    - [x] 建立正式 runtime 的 generation-keyed axis latest-value transport。
      - 状态（2026-08-31）：`bongocat-runtime` 新增六轴/trigger 强类型 key/sample、固定 24 key
        容量、按 key 合并、非单调时间/非有限/越界/过期 generation 拒绝、重连淘汰旧 pending
        样本和 shutdown 拒绝发布；transport 诊断进入 `RuntimeSnapshot`。
    - [x] 在 runtime 应用可配置 stick/trigger dead-zone，并投影到 ModelInputSnapshot。
      - 状态（2026-08-31）：`GamepadAxisSettings` 拒绝无效 dead-zone，stick 使用对称重映射、
        trigger 使用单侧重映射；`StickLeft/Right X/Y` 已进入 renderer 参数，Reset 会清空轴值。
    - [x] 将 stick/trigger dead-zone 纳入正式配置并接入 Application runtime 生命周期。
      - 状态（2026-09-04）：Native config v1 直接包含 `[0, 1)` 的
        `input.gamepad_stick_dead_zone`/`gamepad_trigger_dead_zone`，默认值为 `0.15`/`0.0`。
        Application 启动会在 runtime Ready 后发送强类型 settings，
        运行中更新先做 revision-checked 原子配置提交再重投影现有 axis；启动、更新、重启和 schema
        accept/reject 回归已覆盖。
      - 验收证据（2026-09-01）：正式 config 与独立 config-store spike 使用 `f64` 保存 JSON 数值，
        Application 在 runtime 边界受检转换为 `f32`，运行时更新按最短十进制表示写回；共享默认
        fixture 的 value-level 序列化回归覆盖 `0.15`，并校验当前 schema v1 contract。
        commit `c388cf2` 的 run `33414582196` 全绿，config-store job `99562067963` 与 Windows
        input/config job `99562067572` 均通过当时的 contract；当前已统一为 v1。
    - [x] 将同一 `GamepadAxisProducer` 从 Application 传递到独立 overlay/input service owner。
      - 状态（2026-08-31）：Windows/macOS 正式服务启动与所有 opt-in smoke 调用均持有 runtime
        producer；双平台服务现分别消费 XInput/GameController，无手柄启动行为不变。
    - [x] 接入 Windows XInput 和 macOS GameController producer 的连接、按钮、axis 生命周期。
      - 状态（2026-08-31）：Windows Raw Input owner 已在 16ms service tick 查询 XInput 0..3，
        将连接/断开、按钮边沿和六轴归一化送入同一 runtime producer；Windows
        物理手柄/多手柄热插拔仍待实机。
      - 状态（2026-08-31）：正式 Windows adapter 改为只从 System32 动态解析
        `xinput1_4.dll`，消除 Native workspace 测试对 SDK `xinput1_4.lib` 的链接依赖；backend
        缺失和 axis publish 拒绝分别计数。可注入 poll contract 覆盖首次连接按钮边沿、trigger
        `128/255` 阈值、多 slot、断开/重连 generation，以及 stopped axis 不伪装成可靠队列
        overflow；Windows CI 与物理设备证据仍待补齐。
      - 状态（2026-08-31）：run `33406476868` 的 Windows Native workspace 由 contract 发现
        trigger 合成位 8/9 与 XInput shoulder 原生位冲突，左 trigger 被重复发布为 left shoulder。
        adapter 内部 pressed mask 已扩为 `u32`，原生按钮保留低 16 位，trigger 改用位 16/17；
        Windows x64 all-target check/Clippy 通过；commit `119ea66` 的 run `33408664176`、Windows
        Native job `99542490478` 与 input job `99542490550` 已通过原生 unit/adapter smoke。
      - 状态（2026-08-31）：macOS 正式 input worker 使用最新稳定版
        `objc2-game-controller 0.3.2` 枚举至多四个 extended profile，连接/断开与 16 个按钮走
        可靠 runtime producer，六轴走 generation-keyed latest-value；callback 使用原子 pressed
        bitset，不持有 runtime 锁或执行 UI/文件工作。owner 启用并恢复后台投递、清除 copied
        handler、复用 slot、拒绝迟到 generation，并在 overflow recovery 后重播当前状态。
        synthetic runtime contract 与真实无设备 framework owner smoke 已通过；commit `119ea66`
        的 run `33408664176`、macOS Native job `99542490494` 与 dependency job `99542490215`
        已通过完整 workspace/许可证门禁。物理 controller、profile 差异和热插拔矩阵仍待实机。
    - [x] 将 producer overflow、断开/重连和 shutdown 诊断统一映射到 runtime snapshot。
      - 状态（2026-08-31）：共享 axis transport 现为每个 device id 分配跨 service restart
        单调 generation，并在 snapshot 统计连接、断开、discard 和各类拒绝；可靠 input reducer
        新增 typed connect/disconnect、active connection、stale event、scoped release 诊断，设置
        Diagnostics 页完整投影 25 个输入计数。Windows 断开不再用全局 Reset 清除其他手柄或
        键鼠，键鼠 reconcile 也不再错误释放手柄按钮。
      - 状态（2026-09-01）：稳定的 `PlatformInputDiagnostics` 与 latest producer 已归入 runtime
        contract；Windows 每个 service tick、macOS 每个 run-loop slice 分别发布 worker/callback/
        cursor 合并快照，且不占用 command 或可靠 edge 队列。overlay 正式路径共享该 producer，
        input stop 会在 runtime shutdown 前发布最终 `clean_shutdown` 快照；runtime stop 后拒绝更新并
        保留最后值。共享 transport、双平台合并 contract 和权限实机 smoke 的 final-snapshot 断言已覆盖。
    - 状态（2026-09-01）：共享 runtime、正式配置、运行期诊断与双平台正式 producer 已接线；
      物理 controller、多手柄/profile 和热插拔 smoke 仍未完成，未声称手柄功能完成。runtime
      现仅接受 active connection 的 axis sample，连接前缓存和陈旧 generation 不会在后续连接时回放，
      并有回归测试锁定该边界。
    - 状态（2026-09-05）：任何已成功提交到 runtime 的 input Reset 都会清空 active gamepad set；Windows
      XInput poller 在 overflow、生命周期 Reset 与系统查询失败后重播仍连接 slot 的
      `GamepadConnected`，macOS GameController owner 同样在 overflow、tap/session 和 callback Reset
      后先重播 attached connection 再重新采样，避免后续 button/axis 被当作 stale。Windows 首次连接的
      `GamepadConnected` 入队失败会立即释放尚未提交的 axis generation。macOS platform 33 项定向测试
      和 Windows x64 platform tests 交叉编译通过；新增 Windows synthetic reset/reseed 回归待 push CI
      执行，物理 controller 矩阵仍是总项的剩余门禁。

46. [ ] `P5-SHORTCUT-CONTRACT`：冻结快捷键 chord 的规范化与冲突校验前置契约。
    - 依赖：Native config v1、`InputEvent`/`PhysicalKey` 语义和后续 GPUI 快捷键编辑页。
    - 退出条件：字符串绑定在配置提交前解析为平台无关的单 key chord；别名/顺序规范化稳定，
      非法或重复绑定返回可重试错误；不把平台 keycode、窗口句柄或原始按键流带入 config；
      后续平台捕获、清除、恢复默认和动作触发必须复用该 canonical contract。
    - 状态（2026-09-01）：Rust parser、跨 `commands`/`model_behaviors` 冲突检测、闭合的
      application command 与 `motion:<group>:<index>`/`expression:<name>` behavior action
      解析、runtime typed shortcut dispatch、settings typed command、revision-checked 原子
      持久化、snapshot projection、重启恢复回归、`RestoreDefaultShortcuts` 清除/恢复默认
      command、canonicalization 回归、单元测试和
      `shared/config/native-config-contract.md` 已进入 `next`。2026-09-01 又增加
      `CompiledShortcuts` typed table 和 `Application::compiled_shortcuts()` 只读投影：配置提交
      后可一次性解析为闭合 command/model action。chord key 已冻结为 legacy 可录制键闭集并携带
      USB HID usage；平台 `ShortcutMatcher` 按左右 modifier 聚合 + HID identity 确定性匹配，拒绝
      重复 down，binding replace 保留 pressed set 防止 held-key repeat 误触发，reset/reconcile 分别
      清理或校正 transient pressed state。Windows scan code 与
      macOS keycode 映射已通过同一 compiled chord 回归；非法 action、非法 chord 和跨域冲突仍在
      编译边界拒绝。2026-09-01：产品 overlay 将同一 compiled table 和 runtime client 交给双平台
      input owner；边沿仍先进入可靠 `InputEvent`，随后在 worker 外匹配并将 active model 的
      motion/expression target 转成 typed runtime command，Reset 会清理 matcher transient state。
      应用级 target 目前通过有界 typed handoff 交给 settings service；显隐、镜像、穿透和置顶
      在 service owner 内执行并持久化，`open_settings` 经 settings service signal 交给 GPUI frame source；
      配置提交后共享 `ShortcutTable` 会在下一条边沿前原子替换，运行中的 input owner 无需重启
      即可读取新 compiled bindings；旧 pressed set 会按 matcher 规则保留或由 Reset 清除。
      `open_settings` 现经 settings service 设置线程安全请求位，由 GPUI frame source 消费并复用
      `ensure_settings_window` 重开窗口；forwarder 使用停止标志和有界轮询，避免 shutdown join
      卡住。注册/捕获 UI、GPUI 清除/恢复默认入口和 Windows/macOS 实机快捷键证据仍未完成。
      - 状态（2026-09-01）：Diagnostics 页面现展示当前 command/model shortcut 的匿名绑定文本，并
        提供带 expected config revision 的“Restore default shortcuts”和“Clear all” typed 操作；按钮
        复用 settings worker、稳定错误映射、可见焦点和 AccessKit tree，配置冲突或服务关闭时保持原
        快照。设置页现为每条 command 与 model behavior 提供 Capture 控件：捕获结果仅进入平台无关的
        canonical chord，提交前执行跨域冲突预览并复用 revision-checked `SetShortcuts`；非法键与冲突
        保持原快照并显示可重试诊断。新增 3 项 UI contract 测试覆盖 modifier/key 规范化、非法键和
        冲突检测。双平台实机触发证据仍待完成。
      - 状态（2026-09-01）：每条 Capture 控件现以 command 或 `(model_id, behavior_id)` 稳定身份
        持有独立焦点，不再共享一个 `FocusHandle` 或依赖可能重排的数组下标；Tab 可逐行导航，
        Enter/Space、pointer 与 AccessKit click 都进入同一捕获状态，Escape 可取消。动态 accessibility
        节点公开当前 chord 与 waiting 状态，配置恢复或模型导入期间会同时撤销 click/focus/action。
        纯 Rust 回归覆盖逐行 tab order、快照重排后精确更新、缺失目标拒绝和 accessibility target
        映射；双平台真实快捷键触发与屏幕阅读器操作仍待实机完成，因此总项保持未勾选。
      - 状态（2026-09-05）：Windows/macOS 系统状态校正成功后现在将同一 authoritative pressed-key
        snapshot 交给 `ShortcutDispatcher`，并在 Windows 查询失败导致的 Reset 清空 matcher；因此丢失
        release 经第二次状态校正后不会让后续同一 chord 被错误视为 repeat。平台 crate 33 项定向测试
        与严格 Clippy 在 macOS 通过；Windows 真实输入路径继续由 push CI 和实机矩阵验收。
47. [x] `P2-CURSOR-SMOOTHING`：在平台 latest-value 与模型参数之间恢复帧率无关的光标平滑。
    - 依赖：正式 cursor transport、可注入 `MonotonicClock` 和 display-relative normalization。
    - 退出条件：60 FPS 单帧保持 `0.75` 剩余距离，不同 tick 切分产生相同结果，逻辑距离
      `< 0.5` 后精确收敛；首样本与 viewport 变化不产生跨屏漂移；无新 sample 时周期 tick
      继续推进，raw cursor diagnostics 与可靠 edge 队列语义不变；runtime 定向测试与完整
      Native workspace 门禁通过。
    - 验收证据（2026-09-04）：`CursorSmoother` 使用注入单调时间换算指数衰减，runtime worker
      在 command、timeout 和 shutdown 边界统一消费/推进；单元测试覆盖帧率独立、首样本与
      viewport 切换，集成测试使用 `ManualClock` 验证连续 tick 的 `0.25 -> 0.4375` 参数轨迹。
48. [x] `P2-DYNAMIC-MAXIMUM-FPS`：让当前 v1 的目标帧率设置无需重启即可作用于完整产品链路。
    - 依赖：runtime typed command/snapshot、settings revision transaction 和 app-owned frame source。
    - 退出条件：`15..=240` 有统一 runtime contract；设置 UI 可读写并保留 keyboard/AccessKit
      语义；runtime evaluation、GPUI 产品 frame source 与独立 overlay loop 都使用最新值；有效值
      持久化并在重启后恢复，越界或 stale 请求不改变当前 runtime/config；完整 Native workspace
      门禁与 macOS release 产品 smoke 通过。
    - 验收证据（2026-09-04）：`SetMaximumFps`、`RuntimeSnapshot::maximum_fps` 和
      `SettingsCommand::SetMaximumFps` 形成强类型链路；GPUI Kit number field 提供 `15..=240`、步长
      `15` 的设置，并由项目 AccessKit tree 暴露增减动作。runtime worker 与两种 frame source 均按
      当前 snapshot 动态计算间隔；单元/服务测试覆盖边界、typed rejection、配置持久化、重启恢复
      和 stale revision；完整 macOS workspace 门禁、release 产品 lifecycle smoke 与 Windows x64
      overlay target check 通过。
49. [x] `P2-HIDDEN-FRAME-THROTTLE`：overlay 不可见时降低无效 runtime/frame-source 唤醒。
    - 依赖：runtime-owned overlay visibility、动态帧率间隔和 app-owned product frame source。
    - 退出条件：隐藏状态不再按用户目标 FPS 周期唤醒；runtime command queue 仍可立即响应；重新
      显示与应用快捷键的轮询延迟有明确上限；可见状态恢复用户目标 FPS；定向测试、完整 Native
      workspace 门禁和 macOS release 产品 smoke 通过。
    - 验收证据（2026-09-04）：共享 `frame_interval_for_runtime` 对所有合法目标 FPS 在隐藏时返回
      固定 `100 ms`，可见时恢复目标间隔，非法值仍拒绝；runtime `recv_timeout` 与 GPUI 产品 frame
      source 消费同一策略，可靠 command 到达会提前结束 runtime 等待。边界单测覆盖最低/最高 FPS
      的隐藏策略。
50. [x] `P2-HIDDEN-MODEL-COMMIT`：overlay 隐藏时模型切换仍须完成 GPU prepare/commit。
    - 依赖：可靠 model commit frame 槽、隐藏 `100 ms` 调度和首帧 presentation contract。
    - 退出条件：隐藏 tick 只消费可靠模型提交，保留同 generation 的 ordinary latest frame 并
      淘汰已被候选 supersede 的旧 generation data frame；候选完成一次隐藏
      draw/present 验证后提交且窗口保持隐藏；失败保留旧 GPU owner；重显先同步 latest frame 并
      present 后才显示；双平台产品 smoke、完整 Native workspace 门禁通过。
    - 验收证据（2026-09-04）：`RenderConsumer::take_model_commit` 已隔离 control/data 消费，双平台
      product tick 已实现隐藏候选验证和可见前再次 present。本机 macOS release 产品 smoke 已
      完成隐藏切模、GPU generation 前进、保持不可见、重显及恢复原模型。Windows runner 随后
      发现候选 overlay 与旧窗口重叠时把专用 Win32 class 已注册误判为创建失败；Windows owner
      现接受同进程 `ERROR_CLASS_ALREADY_EXISTS` 并以双隐藏窗口回归固定 prepare/rollback 所需的
      重叠生命周期。commit `7082ff3` 的修复由 run `33865854261` 的 Windows job `101000445117`
      通过隐藏切模、transactional D3D11 切模、release 产品 lifecycle 与完整 workspace 门禁；同一
      run 的 macOS job `101000445151` 通过对等隐藏切模和 Metal lifecycle。
51. [x] `P3-FRAME-SOURCE-SHUTDOWN`：产品退出必须确认 frame source 停止后再释放 renderer。
    - 依赖：app coordinator、双平台产品 frame source、runtime/config/audio shutdown owner。
    - 退出条件：先阻止新 tick 并停止 input producer；frame task 正常退出或被取消均发送 ack；
      未收到 ack 时产生稳定匿名失败而非静默继续；runtime/config/audio shutdown 及
      renderer/GPU/window 释放发生在 ack 等待之后；单元测试、双平台 release lifecycle smoke
      与完整 Native workspace 门禁通过。
    - 验收证据（2026-09-04）：共享 stop/ack 与 RAII run guard 已接入产品 coordinator 和 frame task；
      本机 format、严格 Clippy、workspace test、release check、macOS release settings/Models
      lifecycle 与隐藏切模 smoke 已通过。commit `99f0977` 随 commit `7082ff3` 和 CI race 修复进入
      run `33865854261`；macOS job `101000445151` 与 Windows job `101000445117` 均通过完整
      workspace、release 产品 lifecycle、隐藏模型提交和 shutdown smoke，退出条件满足。
52. [x] `P7-PRODUCT-LIFETIME-DEFAULT`：正式应用无参数启动不得按预览时长自动退出。
    - 依赖：双平台产品 lifecycle、系统菜单 Quit、shutdown coordinator 和有界 smoke CLI。
    - 退出条件：无参数解析为持续运行到显式 Quit；正数 `--run-seconds` 仍提供有界诊断且 `0`
      保持显式无界拼写；安装包/Finder/Explorer 启动不依赖额外参数；所有退出仍进入既有
      shutdown coordinator；入口 contract test、完整 Native workspace 与双平台 release lifecycle
      smoke 通过。
    - 验收证据（2026-09-04）：入口默认值、usage、contract test、Native README 与 Technical
      Design 已同步；本机 format、严格 Clippy、workspace test、release check、app 入口 13 个
      contract test 和 macOS release lifecycle smoke 通过。commit `7f799f7` 的 run
      `33867921771` 全绿；Windows job `101006895636` 与 macOS job `101006895731` 均通过完整
      workspace 和 release 产品 lifecycle，退出条件满足。
53. [x] `P5-APPEARANCE-THEME`：首版 `appearance.theme` 必须可修改、持久化并即时应用。
    - 依赖：当前 v1 config、settings revision/CAS、GPUI Kit Theme/Select 和项目辅助功能桥。
    - 退出条件：`system`、`light`、`dark` 通过强类型 snapshot/command 往返；Application owner
      原子提交且重启恢复，stale revision 不改配置；显式模式即时更新组件与原生窗口外观，system
      清除覆盖并继续响应系统变化；三种选项通过 Select 提供 ComboBox role、当前值、键盘和 action 语义；
      定向测试、完整 Native workspace 与双平台 release settings smoke 通过。
    - 完成（2026-09-04）：代码、Technical Design 和 smoke contract 已实现；本机 format、严格
      Clippy、workspace test、release check、默认 system 主题 release 产品 smoke，以及临时环境
      dark 主题 release 设置窗口/state 恢复 smoke 均通过。commit `ac5dc70` 的 run
      `33871601685` 全绿；Windows job `101018640203` 与 macOS job `101018640280` 均通过完整
      workspace、release settings/state smoke 和辅助功能 contract，退出条件满足。
54. [x] `P3-PANIC-DIAGNOSTICS-RELEASE`：以实际 release panic 验证本地诊断和恢复标记。
    - 依赖：app-owned bounded log writer、process panic hook、环境隔离 run marker、当前
      `panic = "abort"` release profile 与 Development-only storage injection 边界。
    - 退出条件：Windows/macOS 同一 release executable 的子进程在 Application owner 存活时
      panic 并非零退出；持久日志只含固定 `application/error/panicked` code，不含 payload 或路径；
      run marker 保留且 current config 字节不变；下一次启动记录一次 `previous_run_unclean`，正常
      shutdown 后清除 marker 并记录 `shutdown_completed`；默认产品 CLI 拒绝父/子测试参数；入口
      定向测试、完整 Native workspace 与双平台 release smoke 通过。
    - 验收证据（2026-09-04）：commit `8284176` 实现、Technical Design、CI 步骤、feature 参数
      边界与本机 debug 子进程闭环；run `33873937760` 全绿，Windows job `101026266475` 与 macOS
      job `101026266252` 均以同一 release executable 通过 `panic=abort` 子进程、固定匿名日志、
      config 字节不变、unclean 重启分类、marker 清理和正常 shutdown 验证。默认产品 CLI 继续拒绝
      两个私有测试参数，完整 Native workspace 门禁同时通过。
55. [x] `P5-STATUS-ICON-VISIBILITY`：让当前 v1 的菜单栏/托盘状态图标可即时隐藏和恢复。
    - 依赖：`P7-SYSTEM-MENU-LIFECYCLE`、当前 v1 `application.show_status_icon`、settings revision/CAS
      和 GPUI Kit switch。
    - 退出条件：配置值通过强类型 snapshot/command 往返；平台主线程先应用显隐，Application owner
      再原子提交，平台失败不改配置，配置失败回滚平台状态；Windows `NIM_DELETE`/`NIM_ADD` 与 macOS
      remove/recreate `NSStatusItem` 都保留唯一菜单事件 owner；启动恢复已保存值；General 控件具备
      keyboard/AccessKit switch 语义；定向测试、完整 Native workspace 与双平台 release system-menu
      smoke 通过。
    - 验收证据（2026-09-04）：commit `8632ae5` 完成强类型 command/snapshot、主线程平台桥、
      config commit/rollback、双平台 status-item owner、启动恢复、GPUI Kit switch 与 AccessKit 语义；
      本机 app/platform/UI 定向测试、macOS release 产品 smoke、Windows x64/ARM64 platform source
      check 和完整 Native 门禁通过。CI run `33877770376` 最终全绿；首次完整 macOS job
      `101038752799` 与 Windows job `101038752918` 都通过增强后的 release
      `Smoke native system menu lifecycle`。独立 macOS GPUI spike 首次因 tooltip 延迟、第二次因无显示
      runner 的 Metal drawable-pool 测量抖动失败，第三次 job `101044187976` 全部通过；两次重跑均未
      掩盖产品 job，且最终 run 保留完整成功证据。
56. [x] `P5-TASKBAR-ICON-VISIBILITY`：让当前 v1 的 Windows 设置窗口任务栏按钮可即时隐藏和恢复。
    - 依赖：当前 v1 `application.show_taskbar_icon`、GPUI 设置窗口 HWND、settings revision/CAS、
      platform main-thread adapter 和 GPUI Kit switch；macOS 不把该字段映射为 Dock 图标。
    - 退出条件：Windows-only 配置值通过强类型 snapshot/command 往返；平台主线程先修改窗口扩展
      样式，Application owner 再原子提交，平台失败不改配置，配置失败回滚 HWND；启动和窗口重建
      恢复已保存值，隐藏任务栏按钮不隐藏/销毁设置窗口；General 控件具备 keyboard/UIA switch
      语义；定向测试、完整 Native workspace 与 Windows release settings smoke 通过。
    - 验收证据（2026-09-04）：commit `8ad5c49` 完成 Windows-only typed command/snapshot、GPUI
      owner request/reply bridge、HWND 扩展样式切换与回读、config commit/rollback、启动/重建恢复、
      GPUI Kit switch 和 UIA 语义；本机定向测试、完整 format/Clippy/workspace test/release check、
      x64/ARM64 platform source check、共享 fixture/schema 门禁和 macOS release system-menu smoke 通过。
      CI run `33882985949` 全绿；Windows Native job `101055885362` 通过完整 workspace、release 产品
      smoke 并输出 `taskbar icon toggled and restored`，macOS job `101055885548` 同时证明该 Windows
      控件未泄漏且原有 system-menu 生命周期无回归，退出条件满足。
57. [x] `P5-APPLICATION-LANGUAGE`：建立当前 v1 应用语言设置和首批中英 Native 本地化闭环。
    - 依赖：当前 v1 `appearance.language`、settings revision/CAS、平台 locale API、GPUI Kit
      Select 和项目辅助功能桥。
    - 退出条件：`system`、`zh-CN`、`en-US` 使用闭合 enum 并拒绝未知持久化值；跟随系统仅解析
      简体中文或英文，其它 locale 回退英文且不覆写偏好；typed command 原子持久化且 stale revision
      不改配置；Select 从 snapshot 无回声同步；窗口标题、导航、Appearance、runtime status 和对应
      AX/UIA 语义即时切换；定向测试、共享 schema/fixture、完整 Native workspace 与双平台 release
      settings smoke 通过。其它三种历史语言和完整 Models/Diagnostics/General 文案仍由 UI 质量
      总项继续跟踪。
    - 验收证据（2026-09-05）：commit `74a9460` 完成当前 v1 闭合语言枚举、严格 schema/fixture、
      双平台系统首选 locale adapter、revision-checked typed command、原子持久化、GPUI Kit Select、
      窗口标题/导航/Appearance/runtime status 中英文切换和项目 AccessKit 语义；commit `9505fb2`
      同步修正隔离 config-store contract 的默认值。CI run `33893896502` 全绿；Windows/macOS/Ubuntu
      Native jobs `101091871906`/`101091871884`/`101091871820` 均通过语言解析、持久化、重启、
      stale revision、UI contract、完整 workspace test、严格 Clippy 与 release check，Windows/macOS
      release settings smoke 同时通过。其它三种历史语言和剩余页面文案仍由 UI 质量总项跟踪。
58. [x] `P5-GENERAL-LOCALIZATION`：完成当前 General 页面及辅助功能语义的中英本地化。
    - 依赖：`P5-APPLICATION-LANGUAGE`、GPUI Kit Settings 页面和项目 AccessKit tree。
    - 退出条件：Overlay、Model interaction、Input、Application 分组的当前可见标题、描述、动态
      启动项状态和 stepper action 均从同一闭合文案表读取；可见控件与 AX/UIA label/value 不漂移；
      中文 800x600 隔离 smoke 覆盖 General 页面、窗口状态恢复和有序 shutdown；删除未接入模块树的
      旧 General renderer；UI 定向测试、严格 Clippy、完整 Native workspace 与双平台 CI 通过。
    - 验收证据（2026-09-05）：实现提交 `f319556` 将主渲染、动态启动项状态和项目 AccessKit
      tree 收敛到同一中英文案表，删除未接入模块树的旧 `window/general.rs`；本机 UI 48 项测试、
      严格 Clippy、完整 Native workspace test、release check 与 macOS 隔离 release smoke 通过。
      CI run `33900420623` 的 macOS/Windows/Ubuntu Native jobs
      `101112924277`/`101112924307`/`101112924317` 全绿；Windows 与 macOS 日志均实际输出
      `Chinese General localization verified` 和 `settings window state restored after restart`，并继续
      通过配置恢复、shutdown 及各自剩余平台 smoke。
59. [x] `P5-MODELS-LOCALIZATION`：完成当前 Models 页面及模型管理状态的中英本地化。
    - 依赖：`P5-APPLICATION-LANGUAGE`、`P5-GENERAL-LOCALIZATION`、GPUI Kit Models 页面和
      模型 catalog/import typed contract。
    - 退出条件：页面、分组、导入控件、模型来源与资源计数、有效性诊断、空/错误/进度状态、
      激活与删除确认均从同一闭合文案表读取；Model ID placeholder 随语言更新；中文隔离 smoke
      覆盖预置模型 catalog、导入初态、800x600 窗口恢复和有序 shutdown；UI 定向测试、严格
      Clippy、完整 Native workspace 与双平台 CI 通过。
    - 验收证据（2026-09-05）：实现提交 `8714740` 将 Models 页面、模型/导入动态状态、全部稳定
      settings error、Model ID placeholder 与 shell footer 接入同一中英文案源；UI 50 项测试、
      严格 Clippy、完整 Native workspace test、release check 和 macOS 隔离 release smoke 本机通过。
      CI run `33905710597` 的 Ubuntu/macOS/Windows Native jobs
      `101130018327`/`101130018508`/`101130018510` 全绿；macOS/Windows 日志均实际输出
      `Chinese Models localization verified`，并继续通过 800x600 窗口重启恢复、shutdown 和剩余
      平台 smoke。

60. [x] `P5-DIAGNOSTICS-LOCALIZATION`：完成当前 Diagnostics 页面及辅助功能语义的中英本地化。- 依赖：`P5-APPLICATION-LANGUAGE`、`P5-GENERAL-LOCALIZATION`、`P5-MODELS-LOCALIZATION`、
        GPUI Kit Diagnostics 页面和现有 input/runtime/config/shortcut typed contract。- 退出条件：页面、分组、26 个输入指标、input service、renderer/command failure、配置恢复、
        导出状态、快捷键动作/捕获/错误及备份操作均从同一闭合文案源读取；可见动作与 AX/UIA
        label/value 不漂移；中文 800x600 隔离 smoke 覆盖 Diagnostics 页面、窗口状态恢复和有序
        shutdown；UI 定向测试、严格 Clippy、完整 Native workspace 与双平台 CI 通过。- 验收证据（2026-09-05）：中英文静态/动态文案、稳定快捷键捕获错误、用户可读 command 名称、
        AccessKit 语义、中文隔离 smoke 标记和双平台 CI 断言均已实现。`BONGOCAT_BUILD_ENV=development`
        下 Native workspace `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`、`cargo test --workspace --locked` 和
        `cargo check --workspace --release --locked` 全部通过；UI 定向测试 51 项、Diagnostics
        presentation/localization 回归均通过。macOS Input Monitoring/Accessibility 相关 4 项
        集成测试按设计保持 ignored，真实权限矩阵仍属于平台实机门禁，不影响本项文案闭环。

61. [x] `P5-BEHAVIOR-SHORTCUT-TOGGLE`：让当前 v1 的模型行为快捷键开关作用于正式输入链路。
    - 依赖：`P5-SHORTCUT-CONTRACT`、当前 v1 `model.enable_behavior_shortcuts`、settings
      revision/CAS、共享 `ShortcutTable` 和 GPUI Kit switch。
    - 退出条件：配置值通过强类型 snapshot/command 往返并在重启后恢复；禁用时仅从活动表移除
      motion/expression 绑定，保留配置中的绑定和所有应用级快捷键；重新启用无需重录即可恢复；
      stale revision 不改变配置或活动表；General 控件具备中英文本、keyboard/AccessKit switch
      语义；定向测试、完整 Native workspace 门禁和 macOS release settings smoke 通过。
    - 验收证据（2026-09-05）：Application 活动表过滤、settings typed command/snapshot、原子
      持久化、GPUI Kit switch、中英文案和项目 AccessKit 语义已实现；测试覆盖应用级绑定保留、
      模型行为绑定禁用、重启恢复、无需重录的重新启用、stale revision 和配置字节持久化。本机
      format、严格 workspace Clippy、完整 workspace unit/integration/doc tests、release check、
      共享 schema/fixture/input runner 与隔离 macOS release settings/state smoke 全部通过；4 项
      macOS Input Monitoring/Accessibility 实机测试按既有权限门禁保持 ignored。最终补强断言另以
      release test 实际执行通过；重复的 app-only debug Clippy 在无 CPU 的 rustc metadata 阶段被
      中断，生产代码与原测试此前已通过的严格 workspace Clippy 结果不受影响。

62. [x] `P2-KEY-RELEASE-FALLBACK`：让当前 v1 的按键释放兜底超时作用于正式 runtime。
    - 依赖：ADR-0004、可注入 runtime 单调时钟、可靠 input queue、平台 reconcile/reset、当前 v1
      `model.release_fallback_timeout_ms` 和 settings revision/CAS。
    - 退出条件：仅 captured keyboard control 在 normal release、reconcile 与 Reset 均未清理时按
      runtime 观察时间到期；repeat down 刷新期限，`0` 禁用，鼠标/手柄不超时，平台事件时间戳不
      跨时钟原点比较；fallback release 有独立匿名诊断；typed command/snapshot、原子持久化、重启
      恢复、stale/越界拒绝、GPUI Kit 数字控件、中英文案与 AX/UIA stepper 语义完成；定向测试、
      完整 Native workspace 门禁和双平台 CI 通过。
    - 验收证据（2026-09-05）：runtime、Application/settings、GPUI Kit 数字控件、中英文案、
      AccessKit stepper 和匿名 diagnostics 已接通；定向 release 测试、format、严格 release
      workspace Clippy、release check、共享 schema/fixture/input runner 与隔离 macOS release
      settings/state smoke 均通过。无 debuginfo 的完整 workspace unit/integration tests 全部通过；
      Rust 1.97.1 本机 `rustdoc bongocat_app` 复现既有零 CPU 停滞并中断。首次 Windows CI 发现专测
      reconcile 的 smoke 与默认 `500 ms` fallback 在第二次 `250 ms` 校正点竞争；commit `6107b6f`
      在该 smoke 显式禁用 fallback，避免用兜底释放冒充 reconcile。run `33928914217` 全部 23 个
      job 通过；Windows workspace job `101203309507` 与 macOS job `101203309682` 均通过完整
      workspace、release 产品 lifecycle 和设置窗口 AX/UIA smoke，Windows formal missing-release
      recovery 也以独立 reconcile 路径通过。

63. [x] `P6-REMOVE-STARTUP-CONFIG-FIELD`：从当前 v1 配置移除未被产品消费的登录启动布尔值。
    - 依赖：ADR-0013、正式 startup-item platform snapshot/command、`next` 首版 schema 边界。
    - 退出条件：Rust config、JSON Schema、默认 fixture 与 config-store spike 不再序列化或接受
      `application.launch_at_login`；启动项仍只由平台 snapshot 读取并仅由显式 typed command
      修改，外部系统变更可观察；不增加 migration、alias 或旧数据 fallback；共享 schema/fixture、
      config 定向测试、完整 Native workspace 门禁和双平台 CI 通过。
    - 验收证据（2026-09-05）：正式 Rust config、共享 JSON Schema/default/reject fixture 与离线
      config-store spike 已移除该字段；serde 与 Draft 2020-12 两条入口均把旧键当作 unknown field
      拒绝。config release 测试 46 项（1 项 crash-probe child 按设计 ignored）、config-store spike
      22 项、format、严格 release workspace Clippy、完整 release all-target tests、release check 和
      共享 schema/fixture validator 均通过。实现 commit `8945509` 的 CI run `33930711175` 全绿；
      Windows/macOS/Ubuntu workspace jobs `101208547975`/`101208547928`/`101208547908` 均通过完整
      workspace 门禁，双平台 startup-item lifecycle 继续从 platform snapshot 验证且无配置回归。

64. [x] `P6-REMOVE-DEFERRED-CORNER-RADIUS-FIELD`：从当前 v1 配置移除首发后才实现的窗口圆角字段。
    - 依赖：Phase 0 行为清单的 `P1 首发后` 决策、`next` 首版 schema 边界。
    - 退出条件：Rust config、JSON Schema、默认 fixture 与 config-store spike 不再序列化或接受
      `overlay.corner_radius_percent`；该 P1 功能仍留在行为清单且不误报为首发实现；不增加 migration、
      alias 或旧数据 fallback；共享 schema/fixture、config 定向测试、完整 Native workspace 门禁和
      双平台 CI 通过。
    - 验收证据（2026-09-05）：正式 Rust config、共享 JSON Schema/default fixture 与隔离 config-store
      已移除该字段；serde 和独立 Draft 2020-12 reject fixture 均明确拒绝旧键。config release 测试
      46 项（1 项 crash-probe child 按设计 ignored）、config-store 22 项、8 个共享 config fixture、
      format、严格 release workspace Clippy、完整 release all-target tests、release check 和共享
      fixture validator 均通过。实现 commit `d8991bb` 的 CI run `33932042669` 全部 23 个 job 通过；
      Windows/macOS/Ubuntu workspace jobs `101212465140`/`101212465166`/`101212465133` 均通过完整
      workspace 与对应产品 smoke，P1 行为清单保持不变。

65. [x] `P6-REMOVE-DEFERRED-HOVER-FIELDS`：从当前 v1 配置移除首发后才实现的指针悬停隐藏字段。
    - 依赖：Phase 0 行为清单的 `P1 首发后` 决策、`next` 首版 schema 边界。
    - 退出条件：Rust config、JSON Schema、默认 fixture 与 config-store spike 不再序列化或接受
      `overlay.hide_on_pointer_hover` 和 `overlay.hide_on_pointer_hover_delay_ms`；两个旧键各有独立
      reject contract；该 P1 功能仍留在行为清单且不误报为首发实现；不增加 migration、alias 或
      fallback；共享 schema/fixture、config 定向测试、完整 Native workspace 门禁和双平台 CI 通过。
    - 验收证据（2026-09-05）：正式 Rust config、共享 JSON Schema/default fixture 与隔离 config-store
      已移除两个字段；serde 与两个独立 Draft 2020-12 reject fixture 分别拒绝旧开关和旧延迟键。
      config release 测试 46 项（1 项 crash-probe child 按设计 ignored）、config-store 22 项、10 个
      共享 config fixture、format、严格 release workspace Clippy、完整 release all-target tests、
      release check 和共享 fixture validator 均通过。实现 commit `66163f2` 的 CI run
      `33933263642` 全部 23 个 job 通过；macOS/Windows/Ubuntu workspace jobs
      `101216083086`/`101216083118`/`101216083187` 均通过完整 workspace 门禁和对应产品 smoke。

66. [x] `P6-KEEP-OVERLAY-IN-WORK-AREA`：让当前 v1 的可见工作区约束作用于正式 overlay。
    - 依赖：当前 v1 `overlay.keep_inside_work_area`、runtime overlay settings、持久化窗口 bounds、
      Win32 monitor API、AppKit screen API 和 GPUI Kit switch。
    - 退出条件：配置值通过强类型 snapshot/command 往返并在重启后恢复，stale revision 不改变
      config/runtime；开启时 Windows 使用最近 monitor 的 `rcWork`、macOS 使用最大交叠或最近
      screen 的 `visibleFrame`，在启动、缩放/设置重建、模型重建和拖动后的 frame tick 收敛窗口
      原点；保留多显示器负坐标，窗口大于工作区时不改变用户尺寸；关闭时允许部分越界，完全离开
      显示器的 state 仍执行既有回退；General 开关具备中英文本、键盘和 AX/UIA switch 语义；纯几何、
      runtime/app/UI 定向测试、完整 Native workspace 门禁和双平台 release 产品 smoke 通过。
    - 验收证据（2026-09-05）：强类型 config/Application/runtime/settings/overlay options、GPUI Kit 开关、
      中英文案和项目 AccessKit 语义已接通；双平台创建/重建/tick 工作区收敛、纯几何测试、产品
      shutdown smoke 断言与 Windows 实际 HWND 收敛测试已实现。format、严格 workspace Clippy、
      release all-target tests、release check、共享 fixture/schema/locales、Windows x64/ARM64 overlay
      严格 Clippy、隔离 macOS release 设置窗口/state smoke 和 119 帧 Metal Live2D preview 均通过，
      已通过。实现 commit `22cd56e` 的 CI run `33935203737` 全绿；Windows/macOS/Ubuntu workspace
      jobs `101221672187`/`101221672371`/`101221672243` 均通过完整 workspace 门禁和对应产品 smoke。

67. [x] `P7-SIGNED-UPDATE-MANIFEST`：建立首发更新的离线信任判断核心。
    - 依赖：ADR-0021、不可变 Development/Production 环境、四个首发 target、发布版本与公钥流程。
    - 退出条件：平台无关且禁止 unsafe 的 verifier 先验签再严格解析 v1 manifest；拒绝 HTTP、跨环境、
      未知字段、错误 target/arch、无效 SemVer、过大 manifest/artifact、未知或过期 key、sequence 降级；
      只返回项目自有 verified 类型，并对下载流校验精确长度和 SHA-256；共享 Draft 2020-12 schema、
      accept/reject fixture、依赖许可证/来源检查、完整 Native workspace 门禁和三平台 CI 通过。
    - 验收证据（2026-09-05）：`bongocat-update`、ADR-0021、共享 schema/fixture 和稳定错误码已实现；使用
      固定测试 key 的 12 项 release 测试、共享 schema/fixture/locales、严格 workspace Clippy、完整
      release all-target tests、release check、依赖许可证/来源和 Linux/macOS Intel/Windows x64/ARM64
      定向 Clippy 已通过。实现 commit `a9371f6` 的 CI run `33936771710` 全绿；Windows/macOS/Ubuntu
      workspace jobs `101226118609`/`101226118560`/`101226118583` 均通过完整 workspace 门禁和对应
      产品 smoke。
    - 状态（2026-09-05）：环境根的 `StorageLayout` 现显式携带 immutable `BuildEnvironment` 并拥有私有
      `updates/` 目录；`UpdateSequenceStore::open_for_layout` 从该布局派生唯一 channel，调用方不能将
      Development sequence 写入 Production 根。目录形状测试逐项包含该目录；store 只在此目录写入
      `update-sequence.json` 和锁文件，不与 config/state 事务混用。

68. [x] `P7-AUTOMATIC-UPDATE-PREFERENCE`：让当前 v1 自动检查更新偏好进入正式设置链路。
    - 依赖：当前 v1 `application.check_for_updates_automatically`、settings typed command/snapshot、
      GPUI Kit switch 与 signed update manifest boundary。
    - 退出条件：配置值通过强类型 snapshot/command 往返并在重启后恢复，stale revision 不改变
      config 或 snapshot；General 开关具备中英文本、键盘焦点和 AX/UIA switch 语义；UI/Application
      定向测试、完整 Native workspace 门禁和双平台 CI 通过。该任务不包含 endpoint、24 小时调度、
      下载、安装或回滚，这些仍由后续更新任务完成。
    - 验收证据（2026-09-05）：Application 持久化、settings snapshot/command/client、GPUI Kit 开关、
      中英文案和项目 AccessKit 语义已接通；typed command、stale revision 与重启恢复测试已完成。
      format、严格 release workspace Clippy、完整 release all-target tests 和 release check 已通过。
      实现 commit `cdc2ec3` 已由后续 commit `1106807` 的 CI run `33938263954` 全量覆盖；Windows/
      macOS/Ubuntu workspace jobs `101230369276`/`101230369290`/`101230369343` 均通过完整 workspace
      门禁和对应产品 smoke。

69. [x] `P7-AUTOMATIC-UPDATE-SCHEDULE`：冻结自动检查的单调 24 小时调度契约。
    - 依赖：`P7-AUTOMATIC-UPDATE-PREFERENCE`、runtime 单调时钟原则与旧版首发行为清单。
    - 退出条件：启用时 startup 和重新启用各立即派发一次，之后从实际派发时间间隔 24 小时；
      关闭立即抑制待触发检查，重复 poll 不重复派发；时钟回退产生稳定匿名诊断并安全重建期限，
      期限溢出时停止后续自动调度且不 panic；平台无关定向测试、完整 Native workspace 门禁和三平台
      CI 通过。
      endpoint、网络 worker、手动检查和下载/安装仍由后续任务接入。
    - 验收证据（2026-09-05）：`bongocat-update` 已新增无 I/O 的可注入单调 scheduler 与强类型触发原因；
      startup/interval/reenable、disable、重复 poll、时钟回退和期限溢出回归已实现。update release 测试
      16 项、format、严格 release workspace Clippy、完整 release all-target tests 和 release check 已通过。
      实现 commit `1106807` 的 CI run `33938263954` 全绿；Windows/macOS/Ubuntu workspace jobs
      `101230369276`/`101230369290`/`101230369343` 均通过完整 workspace 门禁和对应产品 smoke。

70. [x] `P9-BLOCK-LEGACY-AUTO-RELEASE`：阻止 Native Rewrite 开发期间由 tag 自动发布历史 App。
    - 依赖：Phase 0 发布门禁、历史源码保留规则与尚未完成的 Native 签名/安装流水线。
    - 退出条件：历史 Tauri release workflow 保留用于考古和回滚，但只允许显式手动触发；任何
      `v*` tag 都不再自动构建或发布旧 Tauri、Linux 或 i686 artifact；不得据此声称 Native App
      已可发布，新的双平台签名、安装和更新流水线仍由 Phase 9 跟踪。
    - 验收证据（2026-09-05）：`.github/workflows/release.yml` 已移除 `push.tags`，名称明确标注为
      `Legacy BongoCat Release (manual only)`，历史 job/matrix 未删除；YAML 语法与 staged whitespace
      检查通过。Native release workflow 尚未建立，因此 Phase 9 发布准备保持未完成。

71. [x] `P9-NATIVE-PRODUCT-ICON`：让双平台 Native 应用与 Windows 托盘使用正式产品图标。
    - 依赖：正式 `bongocat-app` build script、macOS `.app` 打包入口与 Windows system-menu owner。
    - 退出条件：Native workspace 自有并校验 `.icns/.ico` 容器；macOS bundle 声明、复制并签名封装
      `BongoCat.icns`；Windows executable 编译至少一个 icon group，托盘从当前 module 加载同一固定
      资源且不回退通用图标；完整 Native workspace、依赖策略、macOS Production package 与三平台
      CI 通过。
    - 验收证据（2026-09-05）：实现、文档和 CI 产物断言已接入；本地 icon container 测试、format、
      严格 release workspace Clippy、完整 release all-target tests、release check、dependency policy、
      Windows x64/ARM64 platform Clippy 与 macOS Production `.app` 打包/资源逐字节比较/strict
      codesign 均通过。commit `300f470` 的 CI run `33945105437` 全部 23 个 job 通过；Windows job
      `101249657297` 从真实 `bongocat-app.exe` 提取到 product icon group，macOS job
      `101249657241` 验证 Production `.app` 中的图标字节、bundle metadata 与 strict codesign，
      Windows system-menu smoke 同时使用当前 module 的固定 icon resource 创建托盘图标。

## 13. 待决策清单

| 决策                                                          | 最迟完成              | 阻塞内容                           |
| ------------------------------------------------------------- | --------------------- | ---------------------------------- |
| Windows/macOS 首发 CPU 架构和 target triple                   | `P0-DOC-CONSISTENCY`  | CI、SDK 二进制、签名和安装包矩阵   |
| GPUI 默认 shader 构建工具链及上游 future-incompatibility 处置 | `P0-GPUI-PACKAGE-MAC` | 产品 workspace 和发布构建          |
| Cubism Core/Framework 版本、获取方式和再分发条款              | `P0-CUBISM`           | Live2D safe layer、CI 和公开安装包 |
| Windows 安装格式与更新 helper 权限模型                        | Phase 7 开始前        | 签名、升级、回滚和卸载             |
| macOS 最低系统、Intel 支持和 universal binary 策略            | Phase 1 开始前        | target、依赖、CI 和 notarization   |

每项决策必须落入 ADR 或对应设计文档，并从本表移除；不得只在聊天记录中形成结论。
