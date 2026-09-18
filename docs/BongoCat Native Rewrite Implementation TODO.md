# BongoCat Native Rewrite Implementation TODO

状态：Phase 0 证据补齐与 Phase 1 渐进实现并行
最后更新：2026-09-14
当前分支：`next`
首发平台：Windows 10 1903+、macOS 12+
后续评估：Linux

> 执行基线：应用代码使用 Rust 2024 edition；GPUI 负责设置 UI；模型窗口由 Rust 平台模块直接创建，不嵌入 GPUI renderer；Windows 使用 Raw Input + D3D11，macOS 使用 CGEventTap + Metal；官方 Cubism Core 是唯一厂商二进制/FFI 例外。生产产物不包含 Tauri、WebView、Vue、React 或 JavaScript runtime。

> 应用与存储基线：Bundle ID 固定为 `com.ayangweb.bongo-cat`；Development/Production 使用相同 schema 和不同数据根；新配置使用 `snake_case` 自有命名，不读取或导入旧 Tauri/Pinia 配置。

> 初始版本基线：`next` 只开发全新的首版，当前完整配置、state 和内部持久格式统一从 v1 开始。
> 首次正式发布前不实现版本迁移、schema 兼容、旧数据转换或历史版本判断；新增字段直接修改当前
> v1。保留版本字段和严格的当前版本解析入口，首次发布后的后续版本再以实际发布基线设计迁移。

## 0. 执行规则

### 0.1 架构红线

- [ ] GPUI 只负责设置、模型管理、快捷键、权限、更新和诊断 UI。
- [ ] Rust runtime 是配置、输入、动画和当前模型状态的唯一事实来源。
- [ ] 模型窗口必须是独立原生 overlay，不直接接入 GPUI renderer 私有接口。
- [ ] 输入 callback、runtime tick 和 renderer 不得经过 GPUI 响应式状态链。
- [ ] 不引入 Tauri、WebView、Node.js、JavaScript 或第二套 UI framework。
- [ ] 不使用 rdev 或 monio 事件流作为 pressed state 的唯一依据。
- [ ] 除官方 Cubism Core 外，不新增长期 C/C++/Swift 业务模块。
- [ ] 平台 unsafe/FFI 必须集中在小型 wrapper，业务 crate 默认禁止 unsafe。
- [ ] Linux 不阻塞首发，但共享 crate 不得暴露 Win32/AppKit 类型。
- [ ] Development 不得读取、写入、锁住或 fallback 到 Production 数据。
- [ ] 不实现旧配置字段 alias、自动导入或旧目录探测。
- [ ] `next` 的配置、state 和内部持久格式保持 v1，不包含开发中间版本的迁移或兼容分支。
- [ ] 新功能实现遵守 ADR-0030：先复用现有代码、标准库、平台能力、已安装依赖和成熟第三方
      方案；只有现成方案无法满足需求或引入成本明显更高时才编写最小自有实现。

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
- [x] 新增 ADR-003：模型窗口使用独立 D3D11/Metal overlay。
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
- [x] 确认旧 Vue/Tauri 应用仍可构建和运行，保存命令与产物信息。
  - 验收证据（2026-09-06）：macOS 26.5.2 arm64 使用远端 `pre-refactor-tauri` 分支锁定的 `pnpm-lock.yaml` 运行
    `pnpm build` 与 `pnpm tauri build --debug --bundles app` 成功；Vite 完成 4,406 个模块的
    production bundle，`dist/` 生成 18 个资源文件，Tauri 编译并打包
    `target/debug/bundle/macos/BongoCat.app`、tar.gz 与签名文件。直接启动
    `target/debug/bongo-cat` 触发 `applicationDidFinishLaunching` 并创建主窗口和偏好窗口。
    Bundle ID/version 与 SHA-256、旧应用固定 hide-on-close 行为及无法自动优雅退出的限制记录于
    `docs/migration/legacy-build-baseline.md`；未将旧 updater key 警告或 `block 0.1.6`
    future-incompatibility 误记为 Native 问题。
- [x] 建立 `docs/adr/`、`docs/benchmark/`、`docs/migration/` 目录。
- [x] 建立依赖许可证清单，确认当前 Native spike crate graph 与项目 MIT 发布兼容。
  - 状态（2026-08-29）：最新稳定版 `cargo-deny 0.20.2` 以四个 Windows/macOS target 扫描 13 个独立 workspace，license/source policy 通过并接入 CI；依赖升级后 package 节点数由 lockfile 动态决定，不再把旧的 535 节点快照当作当前事实。Cubism 厂商许可、未来产品依赖、SBOM 和 notice bundle 仍由各自后续门禁处理。
- [x] 审计 Native Rewrite 所有直接 Rust 依赖并升级到 crates.io 最新稳定版。
  - 验收证据：`docs/phase-0/rust-dependency-versions.md` 记录 2026-08-29 的 21 个直接依赖家族、升级范围和命令。原 18 个家族中 8 个已升级、10 个原本已是最新；后续新增的最新稳定版 `bindgen 0.72.1`、`sha2 0.11.0` 与 `libc 0.2.189` 也已精确锁定。完整 `cargo update` 后，最新 `gpui 0.2.2` 仍约束旧 generation 的 Metal/CoreGraphics 和 5 个有兼容更新的传递版本；均已记录 owner path，未静默覆盖或 fork。Dependabot 每周仅扫描 13 个 Native workspace 并向 `next` 提交分组更新。
- [x] 冻结首发 target triple 和 CPU 架构矩阵，明确 Windows ARM64、macOS Intel 是否发布或仅测试。
  - 状态（2026-08-29）：ADR-0010 已固定 Windows 仅支持 x64/ARM64，i686 不再构建或发布。官方 Cubism Native R5 不提供 desktop Windows ARM64 Core，只有 experimental UWP ARM64 DLL，因此 ARM64 当前是发布阻塞；macOS Intel 和最终安装包形式仍待实机与发布链验证。
  - 状态（2026-09-07）：历史手动 release workflow 已移除 `i686-pc-windows-msvc` matrix entry，避免任何仓库发布入口继续构建 Native Rewrite 明确排除的 Windows x86 target；历史基线文档中的旧版 i686 产物记录仅保留为考古证据。
  - 状态（2026-09-07）：`tools/tests/test_native_release_target_matrix.py` 已接入 Phase 0 fixtures job，持续断言 release workflow 仅保留当时冻结的 Windows 目标；该 contract 不替代 macOS Intel、Windows ARM64 Core、实机和签名门禁，因此本项当时仍保持未勾选。
  - 状态（2026-09-14）：该 contract 现断言的是三个已发布 target（`x86_64-pc-windows-msvc`、`x86_64-apple-darwin`、`aarch64-apple-darwin`），并新增一项断言把 `tools/check-native-dependencies.sh` 的离线审计目标列表与 `deny.toml` 的 `[graph] targets` 钉在一起——该脚本此前仍在审计已删除的 `aarch64-pc-windows-msvc`。
  - 状态（2026-09-14，**本条取代以上判断**）：矩阵冻结为 `x86_64-pc-windows-msvc`、`x86_64-apple-darwin`、`aarch64-apple-darwin`。Windows ARM64 不再是产品目标（ADR-0010 已更新，理由见 ADR-0033）：没有官方可授权 desktop ARM64 Core 就没有真实 ABI/模型证据，而 Windows on ARM 走 Windows 自身的 x64 仿真，因此维持一个无法端到端验证的原生目标只增加成本；后续版本可按需重新开启。macOS Intel 与 Apple Silicon 都发布 `.app` + `.dmg`，首发安装包形式由 ADR-0033 固定。`deny.toml`、`bongocat-update::UpdateTargetTriple` 与 `crates/bongocat-packaging` 三处声明一致，由 `tools/tests/test_packaging_contract.py` 强制；CI 已删除 Windows ARM64 的 clippy/check 步骤。因此本项转为已勾选，剩余的是各平台实机证据而不是架构决策。
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
- [x] 将旧配置考古留在远端 `pre-refactor-tauri` 分支，不接入生产配置路径。
- [x] 为 standard、keyboard、gamepad 预置模型生成文件清单和 hash。
- [x] 建立缺文件、损坏 JSON、非 ASCII 路径、超大纹理等模型 fixture。
- [x] 记录 model3、moc、texture、motion、expression、physics、pose、cdi 和音频用法。
- [x] 记录 background、cover、left-keys、right-keys 的实际语义。

状态（2026-08-28）：ADR-008 和 `shared/config/native-config-contract.md` 已冻结应用身份、环境隔离与新字段命名。旧配置兼容已移出产品范围，历史考古仅保留在远端 `pre-refactor-tauri` 分支。六类合成模型包已覆盖缺失 moc、损坏 JSON、非 ASCII/空格路径、超大纹理、路径穿越和多 model3 入口，并由临时目录 validator 检查稳定诊断；Cubism 与 renderer 兼容仍待独立 spike。

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

状态（2026-08-30）：已在 `spikes/gpui-settings/` 建立隔离的 macOS 最小窗口，历史 spike 精确锁定 `gpui = 0.2.2` 并生成独立 lockfile；它只保留为 Phase 0 证据，正式 Native workspace 统一使用 `gpui-kit = "=0.6.1"`。默认预编译 shader、release `.app`、原生 Application/Edit/Window 菜单、窗口关闭/重开和 shutdown smoke 通过。当前 spike 还验证了 System/Light/Dark 主题、焦点边框、Tab/Shift-Tab、Unicode/grapheme 文本编辑、选择、剪切、复制和粘贴，以及 GPUI executor 上的 bounded typed command/revision snapshot/shutdown acknowledgement，并保存浅色/深色截图证据。marked-text 纯状态 contract 已覆盖连续中文组合、已有多字节前缀、surrogate pair 和异常 range；本机 WeType 拼音 2.2.3 进一步通过真实系统组合更新、候选提交和已有中文前缀后的再次组合。项目自有 AccessKit tree 已由 macOS AppKit AX API 读取 9 个语义节点，Dark radio 的系统 press 经强类型 channel 回到 GPUI；Reset tooltip 已通过双平台原生合成 mouse-move -> GPUI 500ms delay -> build -> hover exit 链路，modal AlertDialog 的 Cancel 初始焦点、Tab/Shift-Tab 陷阱、Escape 关闭和背景语义隐藏也已验证。Windows UIA runner 已读取基础 role/name、selected/action、dialog，并通过 loading -> error -> retry/revision 2 恢复门禁；`busy=true` 因 runner 托管 UIA client 缺少属性标识而仍未验证。Apple 拼音、Windows IME、物理键盘/pointer、tooltip 朗读、目标 DPI 和真实辅助技术操作仍未验证，详见 `docs/phase-0/gpui-settings-spike.md`。

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

- [x] 在 GPUI 应用生命周期内创建独立模型窗口。
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
  - 状态（2026-09-17）：实机探针（listen-only tap 采集真实 FlagsChanged 事件，macOS 26.5.2/
    Apple M1 Pro）发现右侧修饰键两类系统行为并已修复：session tail 位置收不到右 Shift 的
    释放事件且重复按下事件 flags 完全相同；`CGEventSourceKeyState` 对右侧键码（54/60/61/62）
    按住时也返回 false，导致周期校正误杀 ShiftRight/AltRight。对照 rdev 确认其能正确捕获
    右 Shift 释放的根因是 tap 创建在 `kCGHIDEventTap` + `kCGHeadInsertEventTap`：同机 HID
    head 位置实测收到全部修饰键的完整 press/release 对。tap 位置已切换为 HID head，
    `FlagsChanged` 方向采用设备位跳变 → 家族位跳变 → 前一边沿交替的 decoder（CapsLock 固定
    走交替）；周期校正对右侧键码追加家族主键码（55/56/58/59）查询，强制释放时同步清除
    decoder 记录；冒烟测试合成事件改投 HID 层。单元测试覆盖实测序列；修复后的物理键盘
    全键矩阵仍待实机验证。
- [x] 连续 start/stop/restart 输入服务 100 次，无资源泄漏。
  - 验收证据（2026-08-29）：release probe 现在严格校验每个 cycle 的 enabled 恢复、callback panic、queue overflow/closed event 和 NSWorkspace observer 成对注销，任一失败均非零退出。`leaks --atExit` 的 100-cycle 报告 `0 leaks for 0 total leaked bytes`、physical footprint `5232K`，`NSZombieEnabled=YES` 另完成 100/100；两次均为 `queue_overflows=0 callback_panics=0 clean_shutdown=true`，且每个 tap worker 都已 join。timeout/user-disable 各 20 次恢复已另行通过；权限故障循环留在 TCC 矩阵，不阻塞本 restart owner 子项。
- [x] 记录 monio 对照结果，但不引入生产依赖；`docs/phase-0/monio-comparison.md` 基于 commit `d1766e0dcd20dea0435be16cd80adaa749b86e30` 记录 Raw Input、channel、reconciliation、Reset、callback 和许可证差异。
- [x] 为 captured、reconciled、reset、duplicate、overflow 分别维护计数器，不记录具体键值。
  - 验收证据（2026-09-06）：产品 runtime 的 `InputDiagnostics` 维护 captured、reconciled、
    reset/released-by-reset 与 duplicate/unmatched 聚合计数，`InputTransportDiagnostics` 维护
    queue-full/recovered-after-overflow；两者进入 `InputSnapshot`。`SettingsInputDiagnostics` 与
    Diagnostics 页面只投影命名计数，`diagnostics_export_is_atomic_aggregated_and_path_free` 逐项
    断言五类 JSON 数值，且回归确认导出不含 model 私有名称或本地路径。平台 worker 另以
    `PlatformInputDiagnostics` 发布匿名 capture/overflow/recovery 计数，runtime shutdown 后冻结最终快照。
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
- [x] 对 raw binding 生成流程固定 header、生成器版本和输出审阅方式，禁止手改生成代码后失去可重复性。
  - 验收证据（2026-09-06）：`tools/cubism-bindgen` 精确固定 `bindgen 0.72.1`、生成选项、
    三个可用 R5 target 与 header SHA-256 输入；输出包含无路径 provenance，并要求新 staging
    目录和重复生成 hash 一致后才可审阅导入。合成 header 的三 target golden、拒绝覆盖/仓库内
    header/hash 不符测试，以及 format、Clippy、test、release check 均通过。真实 SDK 的第二人
    重生成、平台 ABI 和模型 smoke 仍由 P0 Cubism 发布门禁跟踪。

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

- [x] 仓库根目录是仅包含新 Rust 应用和 crate 的唯一产品 Cargo workspace；历史 Tauri workspace 已从当前工作树退役。
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
  - 状态（2026-09-01）：`rust-toolchain.toml` 已固定 Rust `1.97.1`、`clippy` 和 `rustfmt`，`native-toolchain` job 也验证当前 stable 与该版本；Windows ARM64 desktop Core、macOS Intel 发布形式和完整 target 发布矩阵仍待外部证据，因此保持未勾选。
- [x] 在 workspace manifest 声明 `rust-version`，CI 验证最低版本和当前 stable，不依赖开发机偶然安装的 nightly。
  - 验收证据（2026-09-01）：`Cargo.toml` 的 workspace package 声明 `rust-version = "1.97"`，全部 Native crate 继承该字段；`native-toolchain` job 对当前 stable 和 `1.97.1` 均执行 `cargo check --locked --workspace`。
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
    完整 SDK 保存在仓库外，普通构建、CI 和打包不联网或下载 artifact。`docs/product-runtime.md`
    同步说明固定 vendor 基线和离线构建边界。第二来源复核、Windows/macOS 全 ABI 以及最终
    再分发授权仍由 P0-CUBISM/stable 发布门禁跟踪，不扩大本项完成范围。
- [x] 构建脚本默认不联网；外部 SDK、shader compiler 和生成器必须先由显式 bootstrap 步骤准备。
  - 验收证据（2026-09-01）：正式 `bongocat-app`/`bongocat-live2d` build script 只读取显式
    build environment、本地 vendor header 和已提交资源，不执行下载或网络命令；macOS packaging
    只调用本地 Cargo、provenance 和 bundle 工具。Cubism inspector、bindgen 与 Core probe
    均是离线 CLI，要求维护者先准备并校验 SDK，CI 不下载 Cubism、shader compiler 或生成器。
    Native 三平台 locked format、Clippy、test、release/Production check 及依赖策略通过。
- [x] 定义 debug、release、profiling 三种 profile，profiling 产物不得误发布。
  - 验收证据（2026-09-01）：`Cargo.toml` 显式定义 dev（debug、incremental、unwind）、
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
  - 状态（2026-09-07）：新增 `tools/collect-native-failure-evidence.py`，原生 workspace、Cubism
    binding、contract、model package、macOS、Windows input 和 Windows GPUI jobs 的失败路径均
    收集 runner 临时目录中的有限日志与明确命名的 renderer/validation 截图；拒绝符号链接，限制
    单文件 256 KiB、总量 2 MiB、最多 100 个文件，并对绝对路径、按键/scan code、剪贴板和 pressed
    字段脱敏。artifact 保留 7 日且只上传脱敏目录；测试覆盖路径/按键清理、非白名单图片和 symlink。
    尚未覆盖未产生临时日志的纯 contract job，也未将真实平台截图接入 smoke，故保持未勾选。
  - 状态（2026-09-07）：新增 `test_native_failure_evidence_workflow.py` 静态 contract，持续检查
    失败 artifact 必须经收集器、使用 7 日保留且禁止直接 glob 上传 runner 原始日志。
  - 状态（2026-09-07）：修复收集器输入/输出同目录时的递归扫描边界，输出子树现在明确跳过；回归
    覆盖 workflow 实际 `$RUNNER_TEMP/bongocat-failure-evidence` 布局，避免 manifest 或已收集文件
    被重复上传。
  - 状态（2026-09-07）：失败收集步骤已移至 macOS spike 全部构建与 smoke 之后，并由 workflow
    contract 测试锁定顺序，确保后置的 GPUI/Metal smoke 失败同样产生脱敏证据。
  - 状态（2026-09-07）：Windows GPUI matrix job 的收集步骤也已移至 settings、Win32/D3D11
    overlay 和 100-cycle smoke 之后；workflow contract 同时锁定 macOS/Windows spike 的后置顺序。
  - 状态（2026-09-07）：`native-toolchain`、`fixtures` 和 `dependency-policy` 基础 job 也接入
    同一失败证据收集/上传步骤，当前 Phase 0 workflow 共 10 个 job 仅上传受限脱敏目录；静态
    contract 固定收集器与上传步骤一一对应。
  - 状态（2026-09-07）：收集器进一步拒绝未命名 JSON，并按匿名状态前缀保留文本行；未知行统一
    替换为 `<redacted-line>`，回归覆盖任意 JSON 和潜在用户模型文本，避免仅依赖字段名匹配隐私。
  - 验证（2026-09-07）：本机 `cargo run --manifest-path
Cargo.toml --locked -p bongocat-app --release --features storage-test-injection
--target-dir target/storage-test-injection -- --diagnostics-export-smoke` 通过，输出
    `bongocat-app: diagnostics export completed with a private preview bundle`。
  - 状态（2026-09-07）：文本脱敏字段扩展至相对 `path/file` 及 `message/detail/error` 值，新增
    相对用户模型路径回归，避免错误详情或相对路径绕过绝对路径清理。
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
  - 状态（2026-09-06）：`tools/check-native-dependencies.sh` 现对
    `x86_64/aarch64-pc-windows-msvc` 与 `x86_64/aarch64-apple-darwin` 分别执行
    `cargo tree --edges normal,build`，拒绝 Tauri、Wry/WebView、Node、Deno、QuickJS 和
    JavaScriptCore 包名。四个 target 当前均通过；`tauri-winrt-notification` 仅存在于
    Linux `gpui-pre-linux` 传递依赖，不进入首发树。批准清单逐包比对和最终发布 artifact
    审计仍待完成，因此本项保持未勾选。

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
  - 状态（2026-09-07）：补充 gamepad axis 单元回归，固定新 key 超过 latest-value 容量时
    返回原始 `CapacityExceeded` sample、增加匿名拒绝计数，并保持
    `published = coalesced + consumed + discarded + pending` accounting 不变量；消费 pending
    后仍可完整对账。平台实机手柄和跨平台产品证据仍待完成，因此总项保持未勾选。
  - 状态（2026-09-07）：runtime shutdown 现在与 cursor 使用相同的顺序，在停止 gamepad axis
    slot 前消费最后的 pending latest value；stopped snapshot 的 `pending` 归零、`consumed`
    计数完整且 active connection 的轴值仍投影到 model input。新增 shutdown flush 回归通过；
    平台实机手柄和跨平台产品证据仍待完成，因此总项保持未勾选。
- [ ] 队列溢出必须计数、记录并触发安全恢复。
  - 状态（2026-08-28）：`spikes/input-queue/` 的 `push_with_overflow_reset` 已固定溢出返回原事件、清空不可信缓存、注入 `Reset` 并记录恢复/丢弃计数；`spikes/runtime-contract/` 已将同一策略应用到 typed command queue 并通过 worker snapshot 暴露诊断；runtime producer、实际容量和输入/command sequence 仍待产品实现。
  - 状态（2026-08-30）：产品 `InputProducer` 已聚合 enqueued、queue full、overflow 后
    recovery 和 stopped 数量，所有 clone 共用 sequence；被拒事件消耗 sequence，使下一次
    成功 publish 在 runtime 触发 gap Reset，显式 recovery Reset 保留 `QueueOverflow`
    原因且只计一次。macOS 正式 callback 已改用该 producer；Windows 正式 callback 尚未
    接入，故保持未勾选。
- [x] 动画、长按和延迟统一使用可注入的单调时钟。
  - 验收证据（2026-09-06）：`bongocat-runtime` 以 `MonotonicClock` 作为唯一业务时间源，
    生产默认实现基于 `Instant`，所有动画求值、motion/expression fade、breath/blink、cursor
    平滑、key release fallback 和 frame 间隔均传递单调 `Duration`。`ManualClock` 定向回归
    固定 runtime tick 仅在注入时间到达 fallback deadline 后释放按键；rendering 单元测试固定
    自动效果的周期性和确定性。runtime 的状态、输入与动画代码不使用 `SystemTime` 或其他墙钟
    API；`Instant` 仅用于调用方 bounded wait/shutdown deadline，不参与产品状态求值。Live2D
    Core 日志的文件保留时间独立使用 `SystemTime`，不参与 runtime 或模型动画求值。
- [x] 实现可注入 clock 和确定性 tick。
  - 验收证据（2026-09-06）：正式 runtime 启动边界接收 `Arc<dyn MonotonicClock>`，生产
    owner 使用 `SystemMonotonicClock`，测试和 fixture 以 `ManualClock`/`FixtureClock` 精确推进
    `Duration`。`RuntimeCommand::Tick` 在可靠 command queue 中触发单次 input fallback、cursor
    smoothing、motion/expression/automatic-effect 求值与 immutable render snapshot 发布；生产
    worker 仍使用 maximum-FPS/hidden-overlay 间隔自行调度。共享输入 fixture 与
    `model-motion-expression-audio` fixture 均经同一 typed Tick 路径运行，shutdown 会先关闭
    producer/transport 并在 worker stopped 前完成 drain。`大`量时间相关单元回归覆盖 clock 推进、
    motion/expression fade、cursor smoothing 与 fallback deadline。
- [x] 实现 starting、ready、degraded、stopping、stopped 状态。
  - 验收证据（2026-09-06）：`RuntimeSnapshot` 从 `Starting` 发布到 `Ready`；renderer
    failure 进入 `Degraded`，首个成功 evaluation 恢复 `Ready`，重复相同 failure 不重复推进
    revision；shutdown 先发布 `Stopping`，释放 render transport 后发布 `Stopped`。runtime
    单元测试覆盖正常启动/停止及 failure/recovery 状态转换。
- [ ] 实现 shutdown drain、超时和错误聚合。
  - 状态（2026-09-01）：runtime shutdown 现在先关闭 command producer gate，避免关闭开始后新
    command 进入队列；worker 以非阻塞方式排空已接收 command 后处理 shutdown，即使命令队列已满
    也不会在 shutdown timeout 内卡在发送端。新增满队列拒绝/排空 contract 已通过；超时错误
    聚合和真实阻塞工作预算仍待完成。
  - 状态（2026-09-01）：显式 timeout 现在在 deadline 到达时立即返回 `TimedOut` 并放弃 join
    handle；worker 继续异步完成已接收队列的 drain/shutdown，避免 `RuntimeOwner::Drop` 在错误
    返回后再次无界等待。新增零时限回归确认调用方有界返回且 worker 最终进入 `Stopped`；
    超时错误聚合和真实阻塞工作预算仍待完成。
  - 状态（2026-09-07）：新增仅测试可用的 worker panic-after-stopped 注入，验证 runtime
    已发布 `Stopped` 后的 join panic 会返回 `WorkerPanicked` 并累计匿名计数；该回归与
    timeout/drain 测试通过。真实阻塞工作预算和平台线程故障注入仍待完成。
  - 状态（2026-09-07）：shutdown 超时现在将 worker join 转交给独立 watcher；调用方仍在
    deadline 内返回 `TimedOut`，但 worker 随后发生 panic 时会继续累计匿名
    `worker_panicked` 诊断，不再因丢弃 join handle 而静默丢失错误。新增超时后 late-panic
    聚合回归通过。真实阻塞工作预算和平台线程故障注入仍待完成。
  - 状态（2026-09-07）：新增仅测试可用的 shutdown 阶段阻塞注入，固定调用方在短 deadline
    内返回 `TimedOut`，已接收 command 仍被 drain，worker 延迟后进入 `Stopped`，且 watcher
    不产生误报 panic。该 contract 证明 runtime 的有界退出边界，但不代表真实模型解析、磁盘、
    音频或 GPU 阻塞已拆分到独立 worker；平台线程故障注入仍待完成。
- [x] command 定义幂等性和重复提交语义；有副作用的长操作使用 operation id 去重。
  - 验收证据（2026-09-06）：runtime command envelope 以单调 sequence 拒绝重复和乱序投递；
    `Set*` 与相同 active motion/priority 的重试保持状态，并且 duplicate motion 不重新启动
    renderer 或 motion audio，`StopMotion` 对非当前或已停止 motion 无副作用。`Tick`、
    `ApplyInput`、`ResetInput` 与 model prepare/commit 保持事件语义，不能由值相等合并；input
    本身另以 sequence 防重。模型导入使用 `SettingsOperationId`、共享 cancel token、单调
    progress 和同 ID final result，UI 仅接受当前 operation 的结果。runtime 单元回归验证
    duplicate motion 不增加 audio side effect，UI/app contract 覆盖 operation id、cancel、
    progress 与 final-result 关联。
- [ ] runtime tick 设置工作预算，模型解析、磁盘、音频初始化和 GPU 上传不得阻塞实时队列。
  - 状态（2026-08-28）：`spikes/runtime-contract/` 已通过 14 项测试，覆盖状态机、单调 tick、operation 去重、typed bounded worker、递增 snapshot revision、sequence gap/duplicate、overflow Reset、shutdown drain/timeout、command error 和 panic/join 诊断；产品 runtime 的输入、模型、配置服务、工作预算和真实线程 owner 仍待 Phase 1/2。
  - 状态（2026-09-07）：正式 runtime 新增 `RuntimeWorkDiagnostics`，按已验证的最大 FPS
    帧间隔一半计算匿名处理预算，并在 worker 实际处理段超预算时累计
    `budget_exceeded` 与 `last_over_budget_ms`；阻塞等待不计入预算，且计时不参与产品状态求值。
    63 项 runtime 单元测试、共享 fixture、Clippy 和 release check 通过。模型解析、磁盘、音频
    初始化和 GPU 上传仍未拆分到独立有界 worker，因此本项保持未勾选。
  - 状态（2026-09-07）：新增纯 Rust `record_work_budget` contract，固定等于预算不计数、
    超预算记录毫秒、非超预算样本不覆盖最近值以及计数饱和语义；定向 runtime test 与严格
    Clippy 通过。全局 format 检查仍仅受既有 `bongocat-update/src/check.rs` 差异影响，本次未
    修改该无关文件；真实阻塞工作拆分仍未完成。
  - 状态（2026-09-07）：`RuntimeWorkDiagnostics` 已投影到 `SettingsRuntimeDiagnostics`，并纳入
    匿名 `diagnostics.json` 的 runtime 分栏；app 导出与 UI runtime diagnostics contract 验证
    字段值往返，未导出路径或工作 payload。真实模型解析、磁盘、音频初始化和 GPU 上传仍未拆分
    到独立有界 worker，因此本项保持未勾选。
  - 状态（2026-09-07）：新增 app 纯 Rust projection contract，使用真实 `RuntimeOwner` snapshot
    覆盖 work diagnostics 后断言 `budget_exceeded` 与 `last_over_budget_ms` 原样投影到
    `SettingsRuntimeDiagnostics`；定向 app test、严格 Clippy 和格式检查通过。真实阻塞工作拆分
    仍未完成。
  - 状态（2026-09-07）：runtime 新增匿名 `RuntimeShutdownDiagnostics`，在显式 shutdown
    timeout 或 worker join panic 时累计稳定计数，并沿 app/UI snapshot 与 `diagnostics.json`
    投影；timeout 计数、app projection 和 JSON 字段回归均通过。模型解析、磁盘、音频初始化和
    GPU 上传仍未拆分到独立有界 worker，shutdown 的真实阻塞工作与 panic 注入仍待完成。
  - 状态（2026-09-07）：settings service 的成功 shutdown reply 现改用 runtime 返回的最终
    `RuntimeSnapshot` 投影 runtime/input diagnostics，而不是复用停止前 snapshot；因此最终
    状态和匿名 shutdown/输入计数不会被丢弃。service shutdown 与 projection 回归通过；音频
    shutdown 的复合错误聚合和真实阻塞工作仍待完成。
  - 状态（2026-09-07）：`Application::shutdown` 现在无论 runtime shutdown 成功或失败都会先
    尝试 audio shutdown，再按单一失败保留既有稳定错误；两者同时失败时返回包含 runtime 与
    motion-audio 原因的 `ApplicationShutdownError`。纯 Rust contract 覆盖单错、双错和稳定
    文案，app 全量测试 92+17 项与严格 Clippy 通过；真实阻塞工作预算与线程 panic 注入仍待完成。
  - 状态（2026-09-07）：`MotionAudioService::shutdown` 的显式 timeout 现在会放弃 worker join
    handle 并立即返回，避免 `Drop` 在超时错误后再次无界等待；已用阻塞 backend 验证零时限
    调用有界返回，释放 backend 后 worker 仍能完成 drain/stop 并进入 `Stopped`。audio 定向
    测试与严格 Clippy 通过；新增仅测试可用的 worker panic-after-stopped 注入，验证已发布
    `Stopped` 诊断后 join panic 返回 `WorkerPanicked`。真实输出设备阻塞和平台线程故障注入仍待完成。

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
- [x] 每个 pressed key 最终经 KeyUp、reconcile、Reset 或最终 fallback 释放。
  - 验收证据（2026-09-06）：runtime 在可靠 captured `KeyUp` 时立即清除 pressed record；
    reconcile 连续两次缺失后释放、任意 Reset 清除全部 candidate，且由单调 clock 驱动的
    keyboard fallback 会在配置期限后作为最后恢复路径释放。定向回归覆盖 issue #47 丢失
    release、Reset、重复 down 刷新 fallback deadline 和 runtime tick；fallback 明确不释放
    鼠标或手柄。PixPin、Win+L、UAC 和物理设备实测仍由独立 P0 发布回归跟踪。
- [ ] 实现 fixture runner 和规范化 snapshot 比较。
  - 状态（2026-08-29）：`spikes/fixture-runner/` 已用 Rust 强类型解析并执行全部 9 组共享 fixture，在 24 个 checkpoint 比较完整规范化 snapshot，且已接入 Phase 0 Linux contract matrix。
  - 状态（2026-09-07）：正式 runtime 的共享 fixture contract 新增 `shared/fixtures/manifest.json`，
    显式登记 9 组 input/expected 文件并校验 schema 版本、唯一 id、单组件文件名和目录覆盖；新增
    fixture 未同步清单或缺少配对文件时会明确失败。该项仍不替代双平台实机输入证据。
  - 状态（2026-09-07）：`tools/validate-fixtures.py` 现与 Rust contract 共用该清单，校验 id、文件名、
    输入/期望配对和目录覆盖后再运行既有 schema/语义 oracle，避免独立 runner 漂移；9 组输入、9 组期望
    fixture 校验通过。
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
  - 状态（2026-09-07）：runtime 集成测试现在自动枚举纯输入 fixture，并校验
    `input-sequences` 与 `expected-state` 的 stem 集合一致；新增 fixture 若未配套 expected snapshot
    或未进入测试集合会立即失败，模型/音频 fixture 仍由现有平台条件测试单独执行。

### 3.3 Windows 输入

- [ ] 独立消息窗口接收 Raw Input，不占用 renderer 热路径。
- [ ] 注册 keyboard/mouse 并处理设备热插拔。
- [ ] 完整处理 scan code、E0/E1、左右修饰和特殊键。
  - 状态（2026-09-07）：正式 adapter 的纯 Rust 回归新增 E0 导航/小键盘/GUI 键及 E1 Pause
    make/break 矩阵，确认 `RI_KEY_BREAK` 不改变物理 HID identity；已有左右修饰、PrintScreen
    和未知 scan code 断言继续通过。Windows 实际 WM_INPUT、物理键盘和特殊键设备矩阵仍待
    Windows 实机/CI 验收，因此总项保持未勾选。
- [ ] 去重 Raw Input、可选 hook 和合成事件。
- [ ] 对 pressed set 执行 GetAsyncKeyState 校正。
- [ ] 处理 power、session lock/unlock 和 input desktop 变化。
- [ ] 管理员权限差异产生诊断，但默认不要求提权。
  - 状态（2026-09-14）：ADR-0032 新增启动时的只读 `TokenElevation` 检查（`OpenProcessToken` +
    `GetTokenInformation`）与 `rfd 0.17.2` 原生提示，未提权时给出「属性 → 兼容性 → 勾选以管理员
    身份运行此程序」路径，并用 `opener 0.8.5` 的 reveal 定位当前 executable。产品不原地提权、不写
    HKCU/HKLM、不注册 service，提示不写配置或 state，每次启动重新读取令牌状态。自动化覆盖文案键、
    `rfd` 结果映射与运行选项解析；2026-09-15 起启用 `common-controls-v6`，Windows 按钮为
    「退出并前往设置」/「稍后设置」两个自定义文案（Task Dialog；ComCtl32 v6 manifest 由
    `gpui-pre` 静态库内嵌提供，缺失时回退 `MessageBoxW` 且结果按「稍后设置」处理）。reveal 成功后经
    `shutdown_requested` 标志走常规 shutdown 退出，reveal 失败保持运行。按钮显示、reveal 动作、
    点击后退出、勾选兼容性开关后不再提示、以及提权进程完全不提示仍需 Windows 10 1903+ 实机验收，因此总项保持
    未勾选。
- [ ] RegisterHotKey 冲突返回错误并保持旧绑定。
- [ ] issue #47 固定为发布回归项。
- [ ] 明确 Raw Input scan code 到可查询 virtual-key 的映射，无法可靠校正的键必须有 Reset/保险策略和诊断。
  - 状态（2026-09-07）：正式 Windows adapter 对无法映射的 scan code 继续累计匿名
    `unmapped_keys` 诊断，并立即排入 `ServiceRestart` Reset；该路径不发布边沿，避免
    无法通过 `GetAsyncKeyState` 校正的未知键永久残留在 pressed state。新增 contract 回归
    固定诊断、Reset 和零边沿；当前 macOS 主机仅能验证共享代码，Windows WM_INPUT、真实
    scan code/virtual-key 及设备矩阵仍待 Windows 实机/CI 验收，因此总项保持未勾选。
- [ ] 处理输入设备提供伪造、重复或异常长度 Raw Input 数据的边界，不信任设备名称和 handle 生命周期。
  - 状态（2026-09-07）：Windows Raw Input decoder 现在以纯字节 contract 回归覆盖伪造
    `dwSize`（小于 header 或大于实际 buffer）、过短 header 和未知输入类型；异常包只返回
    decode error，未知类型安全忽略，不生成业务边沿。真实设备伪造/重复消息和 handle 生命周期
    仍需 Windows 实机矩阵验证。

### 3.4 macOS 输入

- [x] 创建 listen-only CGEventTap 和专用 run loop/source。
  - 验收证据（2026-08-30）：正式 `MacInputService` 在独立 worker 上创建 session-level
    listen-only tap 和 CFRunLoop source；同进程合成 Shift down/up 集成测试通过后，stop
    禁用 tap、移除 source、发布最终 Reset 并 join worker。
- [x] 映射 keycode、flags changed 和左右修饰键。
  - 验收证据（2026-08-30）：macOS virtual keycode 映射为稳定 USB HID usage；
    `FlagsChanged` 结合事件 flags 和 callback-time modifier set 区分左右修饰键方向，
    unit test 与 left Shift callback→runtime 集成测试均通过。
  - 验收证据（2026-09-17）：实机探针发现旧方向判定在 macOS 26 上对右 Shift（session tail
    位置释放事件缺失、按下事件 flags 不变）与 CapsLock（flag 反映锁存状态）失效，`AltRight`
    被 `CGEventSourceKeyState` 右侧键码恒 false 的周期校正误杀；对照 rdev 定位 tap 位置为
    决定性差异，tap 切换到 HID head 后右 Shift 释放事件完整到达；decoder 改为设备位跳变 →
    家族位跳变 → 前一边沿交替，校正追加家族主键码查询，新增 7 项 decoder 单元测试
    覆盖实测序列，platform 测试通过。
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
- [x] 明确辅助功能与 Input Monitoring 各自真正需要的能力，避免请求不必要的 TCC 权限。
  - 验收证据（2026-09-05）：ADR-0024 固定 BongoCat 全局输入只使用 Input Monitoring 的
    `CGPreflightListenEventAccess`/用户显式 `CGRequestListenEventAccess` 边界；现有 AccessKit/AppKit
    bridge 只公开本应用 settings 语义，不读取或控制其它应用，因此不请求 Accessibility trust。权限
    snapshot 与 event-tap service status 必须保持独立，真实 TCC 状态变化 UI 刷新和授权/撤销实机矩阵
    仍由相邻未完成任务验证。
- [ ] 启动时检查 Input Monitoring 并在缺失时用原生弹框引导授权，且不持久化提示状态。
  - 状态（2026-09-14）：ADR-0032 固定提示使用 `rfd 0.17.2` 的无父窗口消息框（macOS 侧为
    `CFUserNotificationDisplayAlert`，不进入 `NSAlert::runModal`），检查只调用只读
    `CGPreflightListenEventAccess`；`CGRequestListenEventAccess` 仍只在用户点击引导按钮后调用，因此
    与 ADR-0024 的「启动、轮询、服务恢复不得弹出请求」一致。引导按钮通过 `NSWorkspace` 打开
    `x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent`。提示不新增配置项、
    不写 state、不缓存「稍后」，每次启动重新读取平台状态。单元测试固定 `rfd` 结果到选择的映射与中英
    文案键；`--startup-permission-smoke` 是只读可重复验收命令（本机当前输出
    `input_monitoring is missing`）。macOS 26.5.2 arm64 实机采样确认提示在 GPUI run loop 之前真实
    显示并阻塞等待用户选择（调用栈为
    `main → ensure_startup_permission → check_startup_permission → rfd → CFUserNotificationDisplayAlert`）；
    检查判定为已授权时产品可正常启动并在 6 秒后干净退出。
  - 修正（2026-09-15）：上述「GPUI run loop 之前同步执行」的检查点会把权限提示变成启动的第一个
    交互，阻塞模型窗口、菜单栏等正常窗口的创建。按 ADR-0032「非阻塞执行修正」改为：主线程解析
    语言后，在 GPUI run loop 内（overlay、设置服务、系统菜单、update worker 均已启动后）spawn
    专用 worker 线程 `bongocat-startup-permission` 执行检查与提示，线程刻意 detached（原生对话框
    无远程取消通道，退出时 join 会把阻塞搬到 shutdown 路径；线程不共享任何产品状态，进程退出时
    由 OS 回收）。提示内容、检查逻辑、只读查询与「用户点击后才 TCC request」的边界均不变。
    macOS 侧 `objc2-app-kit 0.3.2` 绑定核实 `NSWorkspace::sharedWorkspace`/`openURL` 未标记
    main-thread-only。非阻塞执行与「提示未应答不影响窗口」仍需实机人工验收。
  - 缺陷与修正（2026-09-14 实机验收）：首个实现用 `rfd` 的 macOS **同步**消息框，在真实打包产物上
    用户应答提示后进程 `EXC_CRASH (SIGABRT)`。用崩溃报告帧偏移 + `atos`（同源码 `-C strip=none`
    重新链接，`__text` 大小 `0xd901e4` 与产物一致）与重现得到同一结论：`MacPlatform::run` 的
    `set_ivar("platform")` 作用在共享的**基类** `NSApplication` 上，
    `objc-0.2.7` panic `Ivar platform not found on class NSApplication`，release 的
    `panic = "abort"` 将其变成 abort。根因是 `rfd` 同步路径的 `PolicyManager`/`FocusManager`
    会抢先创建共享 `NSApplication`，而 `gpui_macos` 要求该实例是自带 `platform` ivar 的
    `GPUIApplication` 子类。现改用同 crate 的无父窗口**异步**实现（只建 `CFUserNotification`，
    不触碰 AppKit）+ `async_io::block_on`；无点击验证：请求弹框但不等待答复时产品正常启动并干净退出
    （exit 0），且采样确认对话框线程阻塞在 `CFUserNotificationReceiveResponse`。已确认
    `NSWorkspace` 与只读 preflight 不创建共享 `NSApplication`；新增 macOS contract 测试禁止该模块
    出现 `rfd::MessageDialog::new`（含反向自检）。两条按钮路径（打开设置 / 稍后再说）、授权后不再
    提示，以及「稍后再说」后产品继续启动，仍需要人工实机点击，因此总项保持未勾选。

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
  - 状态（2026-09-14）：`bongocat-app` 使用默认 Development、显式 `production` Cargo feature
    编译环境；直接 `cargo check --workspace` 无需手工前缀。packaging 继续校验自身
    `--environment`，并只在 Production 时为子 Cargo 命令启用 `production` feature。环境变量不再
    参与选择，CI 与 packaging contract 已同步固定默认、Production 和互斥组合行为。
- [x] Windows 使用 `%APPDATA%\com.ayangweb.bongo-cat\<environment>\` 数据根。
- [x] macOS 使用 `Application Support/com.ayangweb.bongo-cat/<environment>/` 数据根。
  - 双平台 target-specific resolver test 已通过。
- [x] 两个环境的 `config.json`、`state.json`、`models/`、`backups/`、`logs/`、`updates/` 和 `locks/` 相对结构一致；spike 测试逐项比较相对路径。
- [x] 环境不能由 CLI、进程环境变量或设置项在运行时切换，也不能 fallback 到另一环境。
  - 验收证据（2026-09-14）：`bongocat-app` 只在编译期根据 `production` Cargo feature 选择
    `BuildEnvironment`；运行时 API 不接受环境选择，也无 CLI/设置切换或另一环境 fallback。
    Development 默认构建、Production feature 构建及 Production + storage-test-injection 失败
    均由 contract test 固定，Development/Production root 隔离和跨环境应用测试通过。
- [x] 在 spike 中实现同目录临时文件、flush、原子替换、提交后验证和上一份有效配置备份；双平台 OS file lock 与强制进程终止恢复已通过。
- [x] 在 spike 中拒绝损坏配置并保留原始文件；中断提交恢复会保守提升有效临时文件并归档无效/陈旧副本，隔离备份保留策略、默认恢复和 GPUI 用户诊断仍未完成。
- [x] 配置写入去抖，退出前强制 flush。
  - 验收证据（2026-09-07）：设置窗口 bounds 与 overlay drag 使用 `150 ms` 的稳定窗口并只提交
    最新几何；scale、opacity、gamepad stick/trigger dead-zone、maximum FPS 和 release fallback
    timeout 以强类型 `SettingsPatchDebouncer` 合并连续编辑。所有请求保留 expected config revision，
    仅在成功回包后清除 pending；失败或满队列时保留最新值并重试。设置窗口、系统菜单和产品退出
    在 shutdown 前按 revision 链逐项 flush，Windows frame source 等待 flush acknowledgement；窗口
    不可用时明确降级到既有 shutdown，而不是静默丢弃。纯 Rust contract 覆盖最新值合并、失败重试、
    shutdown flush 和 revision chaining；真实系统 close/termination 的平台实机矩阵继续由 Phase 7/8
    生命周期门禁覆盖，不属于配置事务完成定义。
- [x] GPUI 只通过 typed command 获取 snapshot 和提交 patch。
  - 验收证据（2026-09-07）：设置窗口的主题、语言、图标/overlay、motion audio、行为快捷键、
    FPS、release fallback timeout、模型、gamepad dead-zone、启动项和快捷键操作均通过
    `SettingsCommand` 有界 typed request/reply；UI 不直接读取或写入配置。配置变更携带
    `expected_config_revision`，成功回包携带新的 runtime/config revision，文件写入只在 app
    service worker 执行。bongocat-ui 65 项命令/UI contract、严格 Clippy 和 app typed-command
    定向测试通过。
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
- [ ] `P3-WINDOWS-SRGB-ENCODE`：Windows 后端的最终 linear -> sRGB 编码在实机与跨平台像素对照下成立。
  - 状态（2026-09-18）：`COMPOSITION_FORMAT` 保持 flip model 要求的 `B8G8R8A8_UNORM`，
    但 back buffer 的 render target view 改为同族的 `B8G8R8A8_UNORM_SRGB`（常量
    `COMPOSITION_RENDER_TARGET_FORMAT`），编码由硬件在写入时完成，与 macOS
    `BGRA8Unorm_sRGB` drawable 语义一致；alpha-only mask target 保持 linear UNORM。修复前
    Windows 把 linear 预乘值写进被 DirectComposition 当作 sRGB 的 surface，中间调 ≈ v^2.2，
    是两平台明显色差的唯一代码来源（详见 ADR-0046）。
  - 已核实（2026-09-18）：`cargo fmt -p bongocat-overlay -- --check`、`cargo clippy -p
    bongocat-overlay --all-targets --all-features -- -D warnings`、`cargo test -p
    bongocat-overlay` 通过；改动涉及的 D3D11 调用形态用独立探针在
    `--target x86_64-pc-windows-msvc` 下 check 与 clippy 通过后删除。本机
    （macOS 26.5.2 / Apple M1 Pro / toolchain 1.97.1）无法为 Windows 目标构建
    `bongocat-overlay`：`libdeflate-sys` 经 `oxipng` 进入依赖图，交叉构建缺 MSVC C 头文件。
  - 尚缺：`windows.rs` 的格式契约单元测试只在 Windows job 执行，本机未运行；Windows 实机首帧
    与同 snapshot 的两平台 readback 色值对照未做。因此本项保持未勾选。
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
- [x] 明确 sRGB/linear、预乘 alpha 和 texture color space，避免两平台颜色或边缘混合语义漂移。
  - 验收证据（2026-09-06）：`bongocat-overlay` 将模型、背景和按键 PNG 固定为 sRGB texture
    view（D3D11 `R8G8B8A8_UNORM_SRGB` / Metal `RGBA8Unorm_sRGB`），将最终预乘 alpha
    composition/drawable attachment 遵循平台约束（D3D11 DirectComposition 使用
    `B8G8R8A8_UNORM`，Metal 使用 `BGRA8Unorm_sRGB`），并保留 alpha-only mask 的 linear
    UNORM format。两端 shader 都在
    采样解码后的 linear RGB 执行 multiply/screen、mask 和预乘，format contract 单元测试覆盖
    两个实现。嵌入 ICC/wide-gamut profile 的转换尚未实现，v1 明确不依赖平台默认色彩管理。
  - 更正（2026-09-18）：本条的结论不成立于当时的实现。D3D11 侧只把 swapchain 固定为
    `B8G8R8A8_UNORM`，没有同时把 back buffer 的 render target view 设为 `_SRGB`，因此 Windows
    缺少最终 linear -> sRGB 编码，两平台颜色语义实际相反而不是一致。单元测试只断言了常量组合，
    没有断言“两端都执行编码”，所以当时无法发现；修复与证据见 §4.2 和 ADR-0046。
- [ ] present 失败、窗口隐藏和 drawable unavailable 时限流，不产生 busy loop 或日志风暴。
  - 状态（2026-09-06）：macOS 的 `CAMetalLayer::next_drawable == None` 已分类为临时
    presentation unavailable；产品 frame source 收到非错误的 deferred tick，以 `100 ms` 起、
    `1 s` 封顶的指数退避调度，成功 present 后重置。初始模型和候选模型的 commit token 会保留到
    实际 draw/present 成功，临时不可用不会停止 frame source、写入 failure 记录或误拒绝候选。
    hidden overlay 仍只消费可靠 model commit 并由既有 `100 ms` 调度唤醒。D3D11 Present/device-loss
    的 HRESULT 分类、Windows 实机恢复与跨平台 present 故障验证尚未完成，因此本项保持未勾选。
  - 状态（2026-09-06）：Windows `IDXGISwapChain::Present` 的
    `DXGI_STATUS_OCCLUDED` 已在 D3D11 FFI 边界从成功 HRESULT 显式分类为临时 presentation
    unavailable，避免将不可见帧计为已 present；产品 frame source 复用与 macOS 相同的
    `100 ms -> 1 s` 指数退避，并在下一次成功 draw 后重置。`DXGI_ERROR_DEVICE_REMOVED`、
    `DXGI_ERROR_DEVICE_RESET` 及其他失败仍保持致命路径，不能被 occlusion 延迟掩盖。Windows
    x64/ARM64 cross-check、overlay unit test 与 Clippy 通过；真实 occlusion/device-loss 注入和
    runner GPU 验证仍缺失，因此本项继续保持未勾选。

### 4.5 Phase 3 退出门槛

- [ ] 双平台透明、置顶、穿透、缩放和多显示器通过。
- [ ] resize/scale/显示器切换 30 分钟无 device loss 死循环。
- [ ] 窗口创建/销毁 100 次无资源增长。
- [ ] 空场景 frame time 和空闲 CPU 基线已记录。

## 5. Phase 4：Live2D、动画和音效

### 5.1 模型包

- [x] 解析 model3 并规范化相对路径。
  - 验收证据（2026-09-06）：正式 `PreparedModel` 对所有声明的 model3 资源使用 canonical
    root、portable relative path 和 typed sidecar preflight；三个预置包、非 ASCII/反斜杠/遍历
    拒绝与用户导入均由产品 crate 回归覆盖。`bongocat-model` 的 sidecar contract 测试证明所有
    允许的引用均在 prepare 前验证，失败不会产生可提交模型。
- [ ] 验证 moc、texture、motion、expression、physics、pose、cdi 和音频。
  - 状态（2026-08-30）：正式 crate 已验证所有引用存在于 canonical package root，
    moc/普通文件大小、PNG header/尺寸及关联 JSON object 可读性；motion、expression、
    physics、pose、cdi 的完整结构语义仍由 spike 覆盖，尚未全部提升到产品 crate。
  - 状态（2026-09-06）：产品 parser 现会在模型 prepare 前严格验证预置与用户包的
    `cdi3`、`exp3` 和 `motion3` 核心契约：v3 版本、非空/去重标识、display-info 分组引用、
    expression fade/value/blend 以及 motion duration/FPS/metadata count/curve 数值边界。
    三套预置模型与无效 sidecar 回归都通过；motion segment 求值继续由 `bongocat-live2d`
    严格处理，physics/pose 缺少可授权真实 fixture，故本总项保持未完成。
  - 状态（2026-09-06）：产品 parser 现也在 prepare 前验证声明的 `pose3` Type、finite
    non-negative FadeInTime、非空 group、跨 group 唯一 part Id，以及不为空、不重复、非 self
    的 Link。该项只是资源静态 preflight；缺少可分发真实 pose3 fixture 和 fade/link 求值证据，
    因此不影响 physics/pose 行为任务和本总项的未完成状态。
  - 状态（2026-09-06）：产品 parser 现也在 prepare 前严格验证声明的 `physics3` v3
    version、Meta FPS/force、setting/dictionary 标识和计数、normalization range、input/output
    权重与 target、vertex 索引及有限系数。该项只是资源静态 preflight；未实现 physics 求值，
    且缺少可分发真实 physics3 fixture 和行为证据，因此不影响本总项的未完成状态。
  - 状态（2026-09-06）：产品 parser 现也在 prepare 前验证声明的 `userdata3` v3 version、
    metadata entry count/UTF-8 value byte size，以及非空且唯一的 `(Target, Id)`。该项只校验
    model-level user data resource，不替代 motion `UserData` 的 evaluator/occurrence contract。
  - 状态（2026-09-06）：motion `Sound` 现只接受当前产品音频后端支持的 FLAC，并在 prepare
    阶段验证 signature、首个 34-byte STREAMINFO block、sample rate/channel/bit-depth 边界及非空
    frame 数据；错误格式或损坏 header 在导入前以 `model_resource_invalid` 拒绝。完整解码和输出设备
    错误仍由独立 audio worker 处理，physics/pose 的真实 fixture 阻塞不变。
  - 状态（2026-09-06）：motion `UserData` 的 metadata count、字节大小和相对时间范围也在 package
    prepare 阶段校验，避免无效事件在导入提交后才由 runtime 发现。segment 语义、physics/pose 的完整
    结构与求值仍各自保留在 Live2D 边界及真实 fixture 门禁内。
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
- [x] 建立预置只读索引和用户模型可写索引。
  - 验收证据（2026-09-06）：`PresetModelCatalog` 只接受真实 bundle root/direct child，
    将预置模型作为不可删除的 `Preset` origin 签发；`ModelStore` 以当前环境 `models/` 为
    唯一可写事实来源。`Application::model_catalog` 和 settings snapshot 合并两者并保留 origin，
    因此同名预置与用户模型不会混淆。产品测试覆盖预置 catalog 的 committed-only 路径、同名
    source identity、导入/删除和 selected-model 保护。

### 5.2 Cubism safe layer

- [ ] 封装 Core version、logging、Moc consistency 和 Model creation。
  - 状态（2026-08-30）：Core version、Moc consistency 和 Model creation 已进入正式
    safe wrapper；2026-09-01 已在 `bongocat-live2d::CoreLogHandle` 接入 Core logging
    callback。callback 以 panic-safe、最大 512 bytes 消息、路径脱敏和 1 MiB 单文件预算
    写入当前 Development/Production 环境的 `logs/cubism-core.jsonl`，超出预算计数丢弃，
    handle drop 先卸载 callback 再释放 sink；纯 Rust 测试覆盖过滤、容量、写入和卸载。
    Core logging 已完成，但该行仍等待 FFI 错误映射、完整 Moc/Model 资源矩阵和双平台
    实机证据后再勾选。
- [x] 用 Rust owner 保证 Moc、Model 和 buffer 析构顺序。
  - 验收证据（2026-09-06）：正式 `CoreModel` 以独占对齐 allocation 持有 revived Moc 与
    Model buffer，raw Core pointer 不离开 safe wrapper；`Drop` 固定按 Model buffer -> Moc
    buffer 释放，避免 Model 观察已释放的 Moc tables。三个预置 Moc 各 100 次正式
    `load -> update -> drop` 回归覆盖此顺序；Phase 0 real-Core probe 另已记录三个 Moc 各
    100 次同生命周期和 `leaks --atExit` 0-byte 结果。该内存测量仅为 macOS arm64 证据，
    不替代 Windows/macOS 长时 GPU/应用级 leak 门禁。
- [ ] 校验 parameter/part/drawable id、index 和范围。
  - 状态（2026-08-30）：正式 wrapper 已在 Model 创建时一次性验证 product parameter
    ID/range/default，按模型解析 stable index，并验证 drawable array、index、texture、
    mask、vertex、opacity/color；part 表和完整 custom parameter 诊断尚未完成。
  - 状态（2026-09-06）：Core safe wrapper 现解析并拒绝空、重复或非 UTF-8 part id，缓存稳定
    part index，并通过 `csmGetPartOpacities` 提供有限值读取/写入。`PartOpacity` motion 不再
    误写 parameter sink；预置 `standard` Core 回归确认 `Part` opacity 曲线更新且 `ParamAngleX`
    保持不变。完整 part parent/offscreen 诊断仍待完成，因此本项保持未勾选。
  - 状态（2026-09-06）：模型创建阶段现在同时校验 `csmGetPartParentPartIndices` 与
    `csmGetPartOffscreenIndices` 的 null、根节点 `-1` 和范围，拒绝越界关系后才建立 part 表；
    三个预置模型的 Core load 回归通过。parent/offscreen 关系尚未进入 RenderSnapshot 诊断，
    因此本项保持未勾选。
  - 状态（2026-09-06）：模型创建阶段现在也在 renderer 接触 snapshot 前读取
    `csmGetDrawableIds`，拒绝 null、空、重复或非 UTF-8 ID；渲染侧继续使用经过数组边界校验的
    source index，未改变 `RenderSnapshot` 的强类型资源 ID 契约。三个预置 Moc 的 100-cycle
    正式 Core load/update/drop 回归覆盖该 preflight。完整 custom parameter 与 part/offscreen
    诊断投影仍待完成，因此本项保持未勾选。
  - 状态（2026-09-06）：参数表 preflight 也拒绝 Core 返回的空 parameter ID，和已有的
    null、非 UTF-8、重复 ID 及非法 range 检查保持一致；三个预置 Moc 的正式 Core load 回归
    继续通过。完整 custom parameter 诊断投影与 part/offscreen RenderSnapshot 诊断仍待完成，
    因此本项保持未勾选。
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
  - 状态（2026-09-06）：`bongocat-runtime` 新增集中式 Live2D error boundary；模型加载、参数/
    motion/expression 求值和 Core snapshot 错误统一映射到阶段级稳定 `RuntimeRenderErrorCode`，
    并保留 `platform_unsupported`，不向 UI 泄漏 FFI detail、路径或裸类型。17 个当前 Core
    code 的映射回归已通过；精确 Core code 的 UI 诊断投影和跨平台完整错误矩阵仍待完成，
    因此本项保持未勾选。

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
  - 状态（2026-09-06）：runtime fade 回归测试已区分实际渲染内容与 Cubism 每帧
    `dynamic_flags`；停止命令同一时刻发布的首帧允许变更标记清零，但 opacity、顶点、
    绘制顺序和其他 RenderSnapshot 内容必须保持一致。此前因全快照比较造成的脆弱失败已
    修正，runtime 61 项测试和共享 fixture 2 项均通过。
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
  - 状态（2026-09-06）：`DrawableSnapshot` 现携带 Cubism visibility/opacity/draw-order/
    render-order/vertex-position/blend-color 六个 dynamic bits；Metal 与 D3D11 共同只在
    `vertex_positions_changed` 时上传 vertex buffer，并拒绝同一 model generation 内的 index
    topology 变化。三个预置模型真实 Core regression 覆盖 flags 译码、稳定 visual snapshot 与按键
    参数驱动的顶点变化；macOS release preview 和 Windows x64 cross-check 通过。Windows 实机
    present、其他 GPU 资源的 dirty 策略及跨 backend 像素比较仍待完成，因此保持未勾选。
- [ ] 实现 normal/additive/multiplicative blend。
  - 状态（2026-09-06）：`bongocat-overlay` 现以共享的纯 Rust pre-multiplied blend-factor
    contract 定义 Normal/Additive/Multiplicative 的 RGB/alpha source/destination factor，Metal 与
    D3D11 pipeline 分别只负责映射该 contract，避免两端独立常量漂移。三种模式均有 platform-
    independent unit test；本机 macOS release preview 成功呈现 57 帧（21 drawable、5 mask、3
    texture，GPU allocation 稳定）。Windows D3D11 物理 present 与跨 backend 像素对比尚未完成，
    因此本项保持未勾选。
- [x] 实现 clipping mask、inverted mask 和 mask texture 生命周期。
  - 验收证据（2026-09-06）：Core safe wrapper 将每个 drawable 的 validated mask source IDs 和
    `csmIsInvertedMask` flag 投影到 immutable `RenderSnapshot`，拒绝 null、negative 或越界 mask
    index。Metal 与 D3D11 分别为每个 clipped drawable 创建同尺寸线性 alpha target，先以 source
    drawable 累积 mask pass，再由主 pass 采样；inverted flag 在同一 shared shader contract 中将
    coverage 反转。无 mask 的 drawable 不进入 mask pass，model replacement 时 target 由旧 GPU
    model owner 一起释放；同 generation 的 clipping topology 改变被明确拒绝，避免复用错误资源。
    预置 `standard` preview 已实际报告 5 个 masked drawable；本次 `cargo test -p bongocat-render
--locked` 通过 12 项 transport/resource contract test。Windows hardware pixel comparison 仍由
    `D3D11/Metal 对相同 snapshot 行为一致` 与 release matrix 单独验收。
- [x] 实现 texture upload、sampler、过滤和颜色空间策略。
  - 验收证据（2026-09-06）：Metal 与 D3D11 均在 GPU upload 前重新 decode PNG RGBA 并核对已经
    preflight 的尺寸；模型、背景和按键纹理固定以 sRGB view 采样，最终 pre-multiplied composition
    attachment 固定为 sRGB，alpha-only clipping mask 保持 linear UNORM。两端均使用 linear
    min/mag filter 与 clamp-to-edge addressing，shader 在 decoded linear RGB 执行颜色和 blend
    运算。`color_formats_decode_assets_and_encode_the_composited_frame_as_srgb` 的双 backend unit
    contract 固定这组格式；Technical Design 已将嵌入 ICC/wide-gamut profile 的转换明确排除在 v1
    范围外，因此不依赖平台默认颜色管理。
  - 更正（2026-09-18）：“最终 pre-multiplied composition attachment 固定为 sRGB”只对 Metal 成立；
    D3D11 的 attachment 是 `B8G8R8A8_UNORM`，编码本应来自 render target view 而没有设置。修复见
    §4.2，两端编码语义现已由常量与单元测试固定。
- [x] 只在 dirty 时更新必要 GPU 资源。
  - 验收证据（2026-09-06）：`CoreModel::update_and_snapshot` 在 reset Core dynamic flags 前复制六个
    drawable change bits；immutable `DrawableSnapshot` 将这些 flags 交给两端 GPU owner。Metal 与
    D3D11 只在 `vertex_positions_changed` 时重写 vertex buffer，同时更新小型 CPU draw state；index
    topology、vertex buffer size 与 clipping topology 在同一 generation 内视为不可变，变化即拒绝
    snapshot 而非悄然重建或写错资源。三预置模型的 Core regression 证明手部输入标记并改变实际
    drawable vertices，单元测试还冻结所有 Core flag 的译码。实现提交 `bd29508` 已完成两端实现与
    运行时 Core 回归；其余跨 backend pixel equivalence 保留在独立任务。
- [ ] D3D11/Metal 对相同 snapshot 行为一致。
  - 状态（2026-09-06）：`bongocat-render::validate_render_snapshot` 现作为两个 GPU prepare 路径在
    任何纹理解码或 GPU 分配前的唯一平台无关 preflight。它以相同稳定错误拒绝无效 model opacity、
    重复 texture/drawable ID、缺失 texture/mask source、空 geometry、越界 index、非有限 vertex/
    blend color 与非法 drawable opacity；Metal 不再遗漏 D3D11 已拒绝的空 geometry。两项纯 Rust
    contract test 覆盖 accept 与全部 reject boundary，`cargo check -p bongocat-overlay --locked` 和
    `cargo clippy -p bongocat-render -p bongocat-overlay --all-targets --all-features --locked -- -D warnings`
    本机通过。Windows hardware readback、mask/blend golden 和跨 backend pixel tolerance 仍是本项
    的剩余退出证据。
- [ ] 建立非空帧、alpha、mask 和 blend 截图 smoke test。
  - 状态（2026-09-06）：`bongocat-overlay` 现以共享的 `17 x 17` completed-drawable readback
    contract 替代两端仅检查单个非透明像素的实现。它拒绝没有透明 overlay 背景、没有可见模型、
    没有半透明 coverage 或没有足够可见颜色变化的帧；Metal release `standard` preview 已通过，
    输出 57 帧、21 个 drawable、5 个 masked drawable 和 3 张 texture。D3D11 已接入相同
    byte-level contract，但 Windows hardware readback、针对 mask/blend 的独立 golden 和跨 backend
    像素比较仍缺，因此本项保持未完成。

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

状态（2026-09-08）：撤回将 audio 首样本消费放入动作关键路径的试验。renderer prepare 成功后，
audio worker 预解码候选模型的去重 FLAC；PCM 预热完成或稳定失败后才提交模型。output stream 继续由 audio owner 惰性打开，
但 runtime 不等待它。
已活动模型的 motion 只发布缓存 `Play` 并在同一 runtime command 启动，不再执行文件/解码/设备工作、
`get_pos()` 等待或 1 ms 轮询。`bongocat-audio` contract 覆盖 prepare -> activate -> play 顺序；真实
设备端到端延迟测量仍属于 Phase 8 性能门禁。

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
- [x] 禁止通用 set_value(path, any) API。
  - 验收证据（2026-09-06）：`SettingsCommand` 是封闭枚举；每个设置变更都使用显式
    variant、领域类型和 revision-checked reply，`SettingsClient` 只公开对应的强类型方法。
    对 UI、app、runtime 与 config Rust 源码的静态检查未发现 path/any 或 JSON-value 业务
    mutation API。GPUI `InputState::set_value` 仅在视图层同步控件文本，不穿过 settings
    service，也不修改配置或 runtime。
- [x] 不向 UI 发送逐帧数据、原始按键流或 GPU/model pointer。
  - 验收证据（2026-09-06）：`bongocat-ui` 不依赖 runtime、render 或 Live2D crate；其
    `SettingsSnapshot` 只包含配置投影、匿名聚合输入/transport 诊断和模型目录元数据。
    settings service 从 runtime snapshot 只投影这些值，未向 UI command/reply 传递
    `RenderSnapshot`、`InputEvent`、`ModelInputSnapshot`、GPU handle 或 Cubism model/原始指针。
    UI 内的 GPUI `InputEvent` 仅表示本地文本控件编辑，且不等同于平台原始按键流。
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

状态（2026-09-09）：已阅读官方 `v0.6.1` release notes，并对照 `v0.6.0...v0.6.1` 源码与
docs.rs/crates.io metadata 完成迁移评估。`gpui-kit 0.6.1` 使用 Apache-2.0 许可证并默认提供
component/assets；Native workspace 现以精确固定的 crates.io `gpui-kit = "=0.6.1"` 作为唯一
直接 GPUI 依赖，已删除 Zed 与旧组件
git source 及 `gpui`、platform、component、assets 的直接 manifest 依赖。完整 `cargo update`
解析到 `gpui-pre 0.3.3`；该同步包元数据对应 Zed `gpui 0.2.2` revision
`5b055fa789a8b8d38ac951a6e0cde272f66b4495`。设置窗口调用 `gpui_kit::init`，使用
`gpui_kit::component::Root` 并随系统外观同步 `Theme`；状态标签、开关、
按钮、模型 ID、overlay scale/opacity 与 gamepad dead-zone 已迁移到 `Tag`、
`Switch`、`Button`、`Input` 和 `NumberInput`。输入实体通过 `InputEvent` 与
`NumberInputEvent` 接入现有 typed command/draft，并从 snapshot 同步。`0.6.1` 没有普通 Card
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
本次迁移已通过源码/API 兼容审查；升级后的 format、Clippy、unit/doc tests、release check，以及 release
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
  - 状态（2026-09-05）：已使用 `gpui-kit = 0.6.1` 内置的 Lucide 资源迁移 Settings 底栏的
    Refresh 与 Quit，以及 Diagnostics 的 Open backups 工具操作；所有 icon-only control 均保留
    键盘焦点、悬停 tooltip 和显式 accessibility label。其余命令仍待按操作语义逐项迁移，故保持未勾选。
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
  - 状态（2026-09-18）：页面按 ADR-0047 重做为「网格首位导入卡片 + 封面卡片」：每张卡片显示包内
    `resources/cover.png`（无封面时占位）、标题与可用性，操作行提供选中、打开模型位置、编辑
    （改名/换封面）和删除（仅导入模型，两段确认）。新增
    `SetModelTitle`/`SetModelCover`/`OpenModelLocation` 三个 typed command、
    `SettingsModelEntry.directory`/`cover` 投影、`ModelStore::replace_cover` 与共享的包布局常量；
    打开模型位置经 `ModelLocationCapability` 注入，与配置备份目录同一 seam。双平台实机点击与
    `--settings-window-smoke` 仍未运行，因此总项保持未勾选。
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
  - 状态（2026-09-18）：模型页不再列出或预览行为（表情列表已在快捷键页，属重复入口），
    `PreviewModelBehavior` command、对应 client 方法、服务端处理、
    `SettingsErrorCode::ModelBehaviorPreviewUnavailable/Failed`、AccessKit preview node/action 与
    相关文案全部移除；行为目录仍由快捷键页的绑定行独占消费，行为标识作用域改由
    `shortcut_behavior_rows` 断言。runtime 侧 `Application::preview_motion`/`set_expression` 保留。
  - 状态（2026-09-05）：已验证模型目录现将实际声明的 `(motion group, index)` 与 expression name
    作为只读强类型 behavior 投影到 settings snapshot；标识只来自通过包验证的模型索引，不包含包路径。
    settings service 的无持久化 preview command 已仅允许当前 runtime model，并以 Force priority
    转发 motion 或 expression；无效/过期模型和 runtime 失败均有稳定、已本地化的错误 code。Models
    页面仅为当前模型列出已声明行为，预览操作支持键盘焦点和 pending/error 状态。Diagnostics
    快捷键编辑现从当前激活且 Ready 模型的已验证行为目录生成 action/expression 行；未绑定行为可
    Capture 后通过既有 CAS/冲突校验创建 binding，已有 binding 可单项 Clear，非激活或未声明行为
    不会生成编辑入口。键盘焦点和 AccessKit capture/clear actions 使用相同目录投影；Models 页的
    动态 Preview controls 现也由同一 active Ready behavior 目录投影 AccessKit node/action，完整
    platform accessibility 实机验证仍待接入。
- [ ] 权限：macOS 状态/跳转和 Windows 权限差异。
- [ ] 更新：检查、下载、验证、安装和回滚提示。
- [ ] 诊断：版本、renderer、GPU、输入、权限、模型错误和日志导出。
  - 状态（2026-09-05）：settings snapshot 现投影编译期 product version 与不可变构建环境；Diagnostics
    页面以本地化、只读文本显示该 build identity，不读取路径、设备信息或网络来源。renderer/runtime
    stable code、输入可靠性计数、macOS Input Monitoring 权限、模型目录诊断与匿名日志导出仍已各自接入；GPU
    细节、update 诊断、完整产品错误边界及双平台实机证据尚待完成，因此总项保持未勾选。
- [x] About：许可证、Cubism attribution、第三方依赖和隐私说明。
  - 验收证据（2026-09-05）：GPUI Settings 新增只读 About 页面，以现有编译期 build identity
    显示产品版本/环境，并提供中英文 MIT 应用许可证、第三方 Rust 依赖许可证策略、Cubism Native
    `5-r.5`/Core `06.00.0001` 与 `Copyright Live2D` attribution，以及本地输入/匿名日志隐私边界。
    该页明确声明公开再分发和最终 Cubism attribution 仍须 Live2D 批准及发布审核，不把
    `P0-CUBISM` 或 stable 发布门禁标记完成。UI 本地化回归、AccessKit navigation/focus contract、
    `bongocat-ui` 全量测试、`bongocat-app` 测试和本机 macOS settings-window smoke 均通过。

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
- [x] 模型扫描/导入具有 loading、empty、error、cancel 状态。
  - 状态（2026-09-18）：模型页的**错误呈现统一到通用 Notification 组件**（ADR-0047 决策 6）：
    源选择器失败、导入失败、封面选择器失败、目录读不出、改名/换封面/打开位置失败一律
    `NotificationType::Error`；内联 `Tag`/文本只表达进度与选择状态，失败态标签留空。目录读取
    失败只在「从可读转为不可读」时推送一次（`model_catalog_error_reported`），空状态占位符改为
    中性色，不再是第二处危险色错误文本。导入状态机与 cancel 语义不变。
  - 验收证据（2026-09-07）：Models 页面在 snapshot 缺席、catalog 不可用和空 catalog 时分别显示
    `LoadingModels`、`ModelCatalogUnavailable` 和 `NoModelsAvailable`；导入状态机覆盖 picker/source
    错误、start/running progress、取消请求和最终 cancelled/succeeded/failed，运行中 Import 控件切换为
    Cancel。纯 Rust 回归直接锁定三种目录状态及中文取消状态，既有 operation/service 回归覆盖取消不
    提交模型或刷新目录；`bongocat-ui` 全量测试与严格 Clippy 通过。
- [ ] 复杂列表和动态文本不会导致布局跳动。
- [ ] UI 中不出现开发说明、架构术语或操作教学段落。
- [ ] screen reader 可识别 label、value、role、错误和进度；颜色不是状态的唯一表达方式。
  - 状态（2026-09-07）：模型导入的选择目录、导入/取消按钮与状态已投影为项目 AccessKit tree；目录加载、空、不可用和导入进度/错误通过 `Status` role 及可本地化 value 暴露，运行中保留 Cancel action。纯 Rust contract 覆盖按钮可用性、取消进度和目录状态；真实 VoiceOver/Narrator 操作和朗读仍是 Phase 0 实机门禁，因此保持未勾选。
- [ ] 中文、英文、德文等长文本和系统字体 fallback 下仍满足布局约束。
- [ ] 降低动态效果/高对比度等系统辅助设置有明确支持或书面限制。

### 6.6 Phase 5 退出门槛

- [x] 所有 P0 设置可通过 GPUI 修改并由 Rust 原子持久化。
  - 验收证据（2026-09-07）：当前 v1 schema 的 application、appearance、overlay、input、model 与
    shortcuts 字段均由 GPUI Settings 控件覆盖；模型选择/导入、行为预览、快捷键 capture/clear/restore
    与其余 General 控件均通过 revision-checked typed command 进入 Application owner。settings service
    以同一 config writer 完成原子持久化，CAS、错误回滚、restart 恢复和 shutdown flush 均有回归；
    `cargo test -p bongocat-ui --lib --locked`（68 passed）与
    `cargo test -p bongocat-app --lib --locked`（94 passed）通过。
- [ ] 设置窗口销毁/重建后状态一致。
- [ ] 设置窗口关闭时 overlay CPU、帧率和输入不受明显影响。
- [ ] GPUI test、contract test 和双平台截图检查通过。

## 7. Phase 6：配置存储与环境隔离

### 7.1 Schema 与命名

- [x] 发布 `shared/config/config.schema.json`，并用有效/拒绝样本验证 schema 边界。
- [x] 实现 `shared/config/native-config-contract.md` 中的首版字段，调整时同步 contract 和测试。
  - 验收证据（2026-09-05）：完整 v1 的 application、appearance、overlay、input、model 与 shortcuts
    字段均由 `NativeConfig` 的 strict Rust 类型、`config.schema.json`、default/invalid shared fixtures
    和产品 settings snapshot/typed command 共同覆盖；contract 表已补全两项 input gamepad dead-zone
    字段及 `[0, 1)` 边界。`bongocat-config` contract 测试验证默认 fixture 与 Rust 默认值一致、schema
    拒绝未知或越界字段。后续新增首版字段仍必须同时更新本 contract、schema、fixture 与实现。
  - 状态（2026-09-07）：`bongocat-config` 测试读取共享 fixture manifest，并按 manifest 逐项执行
    Rust parser accept/reject 断言；新增或删除 config fixture 时若未同步 manifest，contract 测试会明确失败。
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
  - 状态（2026-09-06）：新增 shared reject fixture 在合法窗口布局旁注入 `overlay.visible` 配置
    字段；`state.schema.json` 与 `StateStore` parser 均拒绝它，固定 state 不得承载用户配置。该
    fixture 不影响 config.json 事务，双平台多显示器恢复证据仍是本项剩余门槛。
  - 状态（2026-09-07）：state contract 测试改为读取共享 `state-fixtures/manifest.json`，逐项执行
    accept/reject 断言并拒绝 manifest 内重复文件；新增 state fixture 未同步 manifest 时会明确失败。

### 7.2 环境与持久化事务

- [x] 构建系统显式产生 Development/Production 元数据，发布构建拒绝默认值。
  - 验收证据（2026-08-31）：正式 app build script 已删除隐式 Development fallback；当时的 Native
    workspace Cargo config 与 CI 显式选择 Development，Production step 显式覆盖，macOS packaging
    在 Cargo 前拒绝缺失/空/未知值。commit `2810f4a` 的 pull request run `33383026191` 全绿；Windows/
    macOS/Ubuntu Native jobs `99459402028`/`99459402083`/`99459402181` 通过完整 workspace、
    Development/release、显式 Production 和拒绝隐式环境门禁，Windows/macOS GPUI jobs
    `99459402171`/`99459401995`、Windows input/config job `99459402076` 和 config-store job
    `99459402352` 同时通过。2026-09-14 起环境选择由默认 Development 与显式 `production` Cargo
    feature 固定，packaging 继续校验 `--environment` 并转换该 feature。
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

- [x] Development 与 Production 的相对目录树和 JSON schema 完全一致。
  - 验收证据（2026-09-06）：`bongocat-config::environments_have_identical_shape_and_disjoint_roots`
    逐项比较两个环境的 config、state、models、backups、logs、updates 和 locks 相对路径，
    并确认根目录互不包含；两个环境共用同一严格 v1 `NativeConfig`/`ApplicationState` 类型与
    `config.schema.json`/`state.schema.json`。`python3 tools/validate-json-schema.py` 已通过
    9 个 input、9 个 expected、10 个 config 和 6 个 state fixture；原 4 个 update fixture 已随
    ADR-0029 删除，环境同构测试仍通过。
- [x] 配置、state、模型、备份、日志、锁和单实例 namespace 均包含环境边界。
  - 验收证据（2026-09-06）：`StorageLayout` 为 Development/Production 分别派生 config、state、
    models、backups、logs、updates 和 locks 根；config/state/model/update 的环境边界已有定向
    contract。`Application::start` 的双环境回归进一步断言两套 application writer 将独立事件写入
    各自 logs，内容不会交叉。Windows `SingleInstanceEnvironment` 使用按环境分开的 mutex、window
    class、window title 与 wake message；产品入口只由不可变 build environment 选择其 namespace。
- [x] 两个环境可同时运行，不争用 writer lock、模型目录或日志文件。
  - 验收证据（2026-09-06）：`development_and_production_applications_never_share_roots` 在同一
    进程中同时启动两套正式 Application，以同一 model ID 分别导入，确认 models 根彼此独立；两者
    又在并存期间各自写入不同 application log event 并断言日志目录和内容不交叉。config/state
    的跨环境 writer lock/restart contract 与 update channel 的独立 sequence store 已由各自定向测试覆盖。
- [x] 开发构建即使收到指向 Production 的 CLI 参数或进程环境变量也拒绝越界。
  - 验收证据（2026-09-14）：`bongocat-app` 仅在编译期根据 `production` feature 选择
    `BUILD_ENVIRONMENT`；运行期不读取环境变量，`Application::start` 只以该常量派生
    `platform_layout`。默认产品 CLI 的 `run_options_reject_missing_invalid_and_unknown_values`
    明确拒绝 `--environment production`、`--BONGOCAT_BUILD_ENV=production` 和
    `--storage-root /production`，测试存储注入又在 Production 组合下编译期失败，因此运行时输入
    无法把 Development 定向到 Production 根。
- [x] Production 不自动复制 Development 数据；需要测试数据时使用显式导入。
  - 验收证据（2026-09-06）：`bongocat-config::production_first_load_never_copies_development_configuration`
    先提交非默认 Development 配置，再首次创建 Production store；Production 仍只生成当前 v1
    默认值，Development 的原始字节保持不变，两个 `config.json` 字节不同。生产代码只从当前
    `StorageLayout` 打开 store；测试数据仍须经显式模型导入边界进入目标环境。
- [x] 更新 channel 与环境绑定，Development 不能安装 Production 更新或反向覆盖。
  - 验收证据（2026-09-13）：原 sequence store / verifier channel 断言随 ADR-0029 退役，本项改由
    `bongocat-update::ReleaseChannel::from_environment` 与 `UpdateRuntime` 的可用性门禁承担。
    `release::development_channel_is_disabled_and_production_is_enabled` 固定 Development 关闭、
    Production 启用的映射；`runtime::development_builds_never_reach_the_network` 断言 Development
    构建在发起任何请求前就返回 `EnvironmentDisabled` 且诊断记为
    `update_environment_disabled`；`runtime::availability_requires_a_production_channel_and_a_signing_key`
    断言更新入口只在 Production 且已注入签名密钥时可见。channel 只从不可变 `BUILD_ENVIRONMENT`
    派生，运行期输入无法切换。新实现已无跨环境 artifact 通路（sequence store、staging 与
    artifact channel 字段均已删除），替换目标由更新库从 `current_exe()` 推导（ADR-0034），
    不落在任一环境的 `StorageLayout` 根下，因此不存在把 Production artifact 写入 Development
    根或反向覆盖的路径。

### 7.4 测试与门槛

- [x] 在平台无关 spike 中验证 Development/Production 根目录不同且相对结构一致，并在 Windows/macOS target-specific test 验证真实 resolver。
- [x] 两个环境写入不同 sentinel，重启和并发运行后仍只读取各自数据。
  - 验收证据（2026-08-29）：macOS 本机与 commit `cf16291e8cee027b6983abcf919a32fb5a0278a5` 的 Windows push run `33251410654`、job `99097619545` 均通过 `development_and_production_processes_commit_and_restart_independently`；产品 state/model/log 服务仍由各自阶段验证。
- [x] 覆盖损坏、截断、错误类型、越界值和未知字段。
  - 验收证据（2026-08-31）：正式 `ConfigStore::load_or_default` 产品测试逐项写入非 JSON、
    截断 JSON、错误布尔类型、越界 opacity 和嵌套未知字段，全部返回错误且逐字节保留当前
    `config.json`；没有有效备份时不创建 quarantine 或静默回落默认值。
- [x] 覆盖无权限、磁盘满、目标占用和中途退出。
  - 验收证据（2026-09-06）：`bongocat-config` 的 `injected_permission_and_storage_failures_preserve_current_and_clean_temp`
    覆盖权限拒绝与磁盘满注入，`occupied_temp_file_or_directory_is_retained_and_never_replaces_current`
    覆盖文件/目录占用，`forced_process_exit_releases_writer_lock_and_recovers_synced_temp` 覆盖持锁
    子进程中途退出后的恢复。完整 `cargo test -p bongocat-config --locked` 通过（46 passed，1 个
    仅供父测试调用的 ignored child probe）；当前配置字节、临时文件和恢复结果均有断言。
- [x] 覆盖非 ASCII/超长路径、缺失和重复模型。
  - 验收证据（2026-09-06）：`bongocat-model` 覆盖非 ASCII 源目录与资源名的导入/解析，
    65 字符超长 model ID 在文件系统访问前拒绝，缺失 `.moc3`/model3 入口和重复 installed
    ID 均保持稳定诊断且不覆盖既有内容；portable ID property test 另覆盖任意字符串长度与
    Windows 保留名边界。`cargo test -p bongocat-model --locked` 通过，平台文件选择实机证据
    仍由 `P7-MODEL-DIRECTORY-PICKER` 跟踪。
- [x] 当前 v1 连续读取 10 次结果一致且不会产生额外写入或备份。
- [x] 失败注入不丢当前环境的配置或用户模型。
  - 验收证据（2026-09-06）：配置层的权限/磁盘满与替换后校验失败注入均逐字节恢复当前
    `config.json` 并清理 temp；模型层的取消/无效包导入不创建 destination 或 staging，重复
    ID 不覆盖已安装内容；Application 的 `rejected_gpu_model_switch_restores_the_previous_config_selection`
    回归验证 GPU 拒绝后旧配置选择与 active model 保持不变。配置与模型定向测试均通过，应用
    回滚测试已纳入既有 app test suite；失败路径不会跨环境读写。
- [x] 发布依赖和运行日志中没有旧 Tauri/Pinia 配置探测。
  - 验收证据（2026-09-06）：Native runtime、config、日志、打包脚本和 manifest 没有
    Tauri/Pinia 配置路径、字段 alias 或导入逻辑；`cargo tree --target all` 仅显示
    `tauri-winrt-notification` 作为 GPUI Linux notification 的传递平台依赖，不提供
    Tauri 应用或配置 API。源码中出现的 `old_pinia_field` 仅用于 strict config 拒绝测试，
    `resources/models` 是预置模型 parser 测试与发布运行时共用的唯一资源根。
- [x] Bundle ID 精确验证为 `com.ayangweb.bongo-cat`。
  - 验收证据（2026-09-05）：配置与存储根使用固定 `BUNDLE_ID` 常量；macOS 打包脚本在签名前
    读取 `CFBundleIdentifier` 并拒绝任何非预期值，release LaunchServices smoke 再次断言该值且
    通过 strict codesign。Windows 单实例 namespace 同样使用该固定应用身份。

## 8. Phase 7：原生系统集成

### 8.1 应用生命周期

- [x] 单实例唤醒已有进程并打开设置或显示 overlay。
  - 验收证据（2026-08-31）：Windows `P7-WINDOWS-SINGLE-INSTANCE` 以按环境隔离的 local
    named mutex、registered wake message 和隐藏 owner window 完成真实双进程 release smoke；secondary
    只通知 primary 后退出，primary 重开既有设置窗口并保持 frame source。macOS
    `P7-MACOS-APPLICATION-REOPEN` 以正式 `.app` 的 LaunchServices reopen 唤醒同一后台进程，
    重建一个设置 Entity 并恢复当前 snapshot，两个实现均进入既有 shutdown coordinator。
- [x] GPUI 设置窗口按需创建，关闭不退出后台应用。
  - 验收证据（2026-09-04）：双平台设置窗口关闭/重开和后台 frame/input/runtime 生命周期已由
    产品 smoke 覆盖；正式入口无参数启动持续运行到显式 Quit，正数 `--run-seconds` 仅用于有界
    smoke/诊断。commit `7f799f7` 的 run `33867921771` 全绿；Windows job `101006895636` 与
    macOS job `101006895731` 均通过完整 workspace、release 产品 lifecycle、系统菜单 Quit 和
    shutdown smoke。
  - 状态（2026-09-10）：正式无参数启动现在保持 `settings_window = None`，不会创建或显示
    GPUI 设置窗口；只有系统菜单、应用快捷键、单实例/应用重开或显式窗口 smoke 才触发按需创建。
    Windows frame source 不再依赖设置窗口实体，因此 overlay 在无设置窗口时继续运行。
- [x] 托盘/菜单栏 command 统一进入 runtime。
  - 验收证据（2026-09-05）：双平台菜单均提供 `Show/Hide BongoCat` action；平台 callback 只投递
    `SystemMenuAction`，GPUI frame owner 读取当前 revisioned snapshot 后经 typed
    `SetOverlayVisible` 进入 settings service/runtime，不直接修改 overlay 或 config。macOS
    release system-menu smoke 已通过原生路径切换与恢复 overlay visibility，并确认产生新的
    config revision；Windows 使用同一 action/command contract，仍待真实 Windows desktop 复验。
  - 状态（2026-09-13）：当前 macOS/Windows 实现已统一为 `tray-icon 0.25.0` 托盘 owner + 直接
    `muda 0.20.0` 菜单 owner，见 ADR-0031；上述历史 smoke 证据保留，但 Windows tray behavior
    不以 cross-compile 代替实机验证。
- [ ] 系统关机、注销和普通退出进入 shutdown coordinator。
  - 状态（2026-09-05）：Windows Raw Input owner 现将 `WM_QUERYENDSESSION` 与已确认的
    `WM_ENDSESSION` 转为无阻塞终止信号；GPUI frame owner 在下一帧复用已有 shutdown coordinator，
    不在 Win32 callback 析构 runtime/GPU。正常 Quit 已有双平台 release smoke；真实 Windows
    注销/关机与 macOS 系统终止的实机矩阵仍待完成，因此本项保持未勾选。
- [x] panic/crash 生成本地诊断并避免配置半写入。
  - 验收证据（2026-09-04）：`P3-PANIC-DIAGNOSTICS-RELEASE` 以同一 Windows/macOS release
    executable 的 `panic=abort` 子进程验证固定匿名 panic code、当前 config 字节不变、环境内
    run marker 保留及下一次启动的 unclean 分类；正常 shutdown 会收敛 marker。默认产品 CLI
    不暴露该测试入口。
- [x] 定义正常退出、强制退出、崩溃和系统终止的恢复标记；下次启动可区分并避免无限恢复循环。
  - 验收证据（2026-09-07）：`ApplicationRunMarker` 在当前环境私有日志目录建立运行标记；有序 shutdown 先记录开始，只有 runtime 与音频 owner 都完成后才记录完成并删除标记。下一次启动将异常退出、panic 和中断 shutdown 统一识别为前次未清理运行，不修改配置或自动重试恢复路径；`bongocat-app` 单元测试和 `--panic-diagnostics-smoke` 覆盖标记保留、分类、干净重启和配置字节不变。Windows/macOS 系统终止回调的实机矩阵仍由 Phase 7/8 发布门禁跟踪。
  - 状态（2026-09-05）：环境隔离 marker v1 现固定为 `running`、`shutting_down` 或 `panicked`；
    下一次启动仅投影匿名 `forced_or_unknown`、`shutdown_interrupted` 或 `panic`，随即写入新的
    `running` marker。正常 shutdown 才删除 marker，panic hook 不读取 payload 且使用非阻塞写入。
    app-log contract 已覆盖三个残留分类及收敛；系统终止 source 到 marker phase 的双平台实机矩阵仍待完成。

### 8.2 Windows

- [x] `tray-icon 0.25.0` 托盘（Windows 使用 `muda 0.20.0` 菜单与 `tray-windows.png`）。
- [x] named mutex + registered message/IPC 唤醒单实例。
  - 验收证据（2026-08-31）：`P7-WINDOWS-SINGLE-INSTANCE` 已使用按环境隔离的 local named
    mutex、隐藏 owner window 与 registered wake message；secondary 只通知 primary 后退出，
    primary 将消息转为 `OpenSettings` 并保持既有窗口/运行时。真实双进程 release smoke 与
    Windows x64/ARM64 source check 已通过，owner 在产品 shutdown 中显式释放。
- [x] 当前用户启动项启用、禁用和状态检测。
  - 验收证据（2026-08-31）：`P7-STARTUP-ITEM-PLATFORM` 已以环境隔离的稳定状态/错误
    contract 完成 Windows HKCU Run 与 macOS 13+ Production `SMAppService` lifecycle；Windows
    真实 HKCU disabled -> enabled -> stale -> disabled 和 macOS `/Applications` 安装态
    NotFound -> register -> unregister -> Disabled smoke 均通过。macOS 12 与 Development
    明确报告 unsupported 且不触及生产登录项；完整退出条件及 CI job 证据见该任务。
- [ ] 文件选择、外部 URL 和剪贴板使用最小权限 wrapper。
  - 状态（2026-09-13）：模型目录 picker 已有共享稳定结果/错误和双平台最小 adapter；两平台均使用
    `rfd 0.17.2` 单选目录，Windows 在专用 worker 的 STA 中调用 `FileDialog`，macOS 在主线程 sheet
    parent 可用时调用 `AsyncFileDialog`，缺少 sheet parent 时返回 `BackendUnavailable`。结果在
    Rust 侧重新验证并 canonicalize；macOS 26.5.2 arm64 上真实 Cancel 与已知仓库目录 Select smoke
    均已通过，Windows 真实 `rfd` 选择/取消 smoke 仍需由原生 job 复验。外部 URL 现由共享
    wrapper 严格限制为无 credentials 的 HTTPS，并通过 `opener 0.8.5` 交给系统默认程序。clipboard
    现通过私有 `arboard 3.6.1` adapter 读写最多 1 MiB 的无 NUL 纯文本、无文本返回空选项，
    错误不包含内容；x64/ARM64 target check 已通过，但尚无 Windows 实机 clipboard read/write
    smoke，因此总项保持未勾选。
- [x] 选择并记录 MSIX、WiX 或 NSIS 打包 ADR。
  - 验收证据（2026-09-05）：ADR-0023 选择 NSIS per-user installer，以当前用户 local application
    directory、无管理员权限、逐 target/arch 已签名 artifact 和不触及环境数据为首发约束。MSIX 的包
    activation/update 模型与既有 HKCU Run/startup 和独立 signed update helper 冲突；WiX 的机器级 MSI
    重点需要额外管理员权限与服务策略，均不作为首发路径。windows/installer/BongoCat.nsi 与
    scripts/package-windows.ps1 现固定 user-level、HKCU-only NSIS v3.11 packaging boundary：
    wrapper 拒绝非 x64 release provenance、缺少三预置模型、reparse point、未签名 PE 或既有 output，且不
    build/sign/network；script 只升级固定 product root，卸载拒绝其他路径。PowerShell/NSIS 不在当前 macOS
    host，installer 编译、签名、安装、升级、卸载、环境数据保留、helper 和 rollback smoke 继续由后续
    Windows 实机任务验证。tools/tests/test_windows_installer_contract.py 已由现有 unittest discovery
    静态锁定 user execution level、固定 product root/HKCU、uninstall root guard，以及 x64 provenance、
    Authenticode、reparse-point、NSIS version/hash 与禁止 build/sign/network 的包装器边界。
  - 状态（2026-09-14）：ADR-0023 关于 per-user 安装、权限面、卸载语义与数据隔离的决策不变，但安装器
    生成方式已由 ADR-0033 取代：`windows/installer/BongoCat.nsi`、`scripts/package-windows.ps1` 与
    `scripts/build-windows.ps1` 已删除，改由 `cargo-packager` 的 NSIS 模板 + `install-mode =
    currentUser` 生成，本机不再需要 NSIS 3.11 的 MD5 固定与 `BONGOCAT_NSIS_SETUP_PATH` /
    `BONGOCAT_MAKENSIS_PATH` 注入。`tools/tests/test_windows_installer_contract.py` 随之删除，其仍然
    有效的部分（per-user 模式、产物集合、target 集合）由 `tools/tests/test_packaging_contract.py`
    接替。**已接受的降级**：新路径不再要求 payload 具备有效 Authenticode 签名，签名验证降级为
    release workflow 的显式告警门禁；同时 `cargo-packager` 会无校验下载 NSIS ApplicationID plugin。
    Windows 安装器仍未在本机验证过（本机是 macOS），installer 编译、签名、安装、升级、卸载、环境
    数据保留和 rollback smoke 继续由 Windows 实机任务验证。
- [x] 对安装目录、用户数据目录和更新临时目录分别建模。
  - 验收证据（2026-09-05）：`StorageLayout` 继续独占按环境隔离的用户数据根，并显式包含私有
    `updates/staging/`；目录创建、Development/Production 同构与 Unix owner-only 权限测试逐项覆盖。
    `bongocat-platform::InstallationLayout` 仅表示 installer/update helper 的 product files root，
    不携带环境或用户数据路径。该模型未实现下载、清理、替换、installer 或 rollback，相关操作仍受
    后续更新/发布任务的独立边界约束。`bongocat-app` 现在在 macOS bundle 与 Windows executable
    相对的 product resource root 解析预置模型；未打包开发二进制才回退仓库资源，因此未来 installer
    payload 不依赖源码树。

### 8.3 macOS

- [x] `tray-icon 0.25.0` 菜单栏（macOS 托管 `NSStatusItem` 与 `muda 0.20.0` 菜单）。
- [x] NSApplication activation/reopen/single-instance 行为。
- [x] SMAppService 启动项启用、禁用和状态检测。
  - 验收证据（2026-08-31）：`P7-STARTUP-ITEM-PLATFORM` 已以 Production-only macOS 13+
    `SMAppService.mainAppService` 完成稳定状态映射与可恢复启用/禁用；macOS 12 和 Development
    明确为 unsupported 且不触及生产登录项。本机 `/Applications` 安装态 smoke 已通过
    NotFound -> register -> unregister -> Disabled，完整退出条件及 CI job 证据见该任务。
- [ ] 文件选择、NSWorkspace 和 pasteboard 最小权限 wrapper。
  - 状态（2026-09-13）：模型目录 adapter 已迁移到 `rfd 0.17.2` 的 macOS
    `AsyncFileDialog`，仅在 AppKit 主线程、应用已运行且存在 sheet parent 时启动；`rfd` 的
    `None` 按取消映射。macOS 26.5.2 arm64 上本次 `rfd` 版本的真实 Cancel 与仓库目录 Select smoke
    均已通过；历史 `NSOpenPanel` 证据仍见 `P7-MODEL-DIRECTORY-PICKER`。外部 HTTPS URL
    通过 `opener 0.8.5` 交给系统默认程序，并在项目边界拒绝 credentials、非 HTTPS 与超长值；pasteboard 现通过私有 `arboard 3.6.1`
    adapter 在 AppKit 主线程/auto-release-pool boundary 读写最多 1 MiB 无 NUL 纯文本、匿名返回
    无文本和错误。自动化只验证后台线程拒绝，避免改写用户 clipboard；隔离 pasteboard 的实机
    read/write smoke 及 `NSWorkspace` 其余能力仍待完成。
- [ ] .app bundle、entitlements、Hardened Runtime 和 notarization 流程。
- [x] TCC 权限状态变化可在 UI 实时刷新。
  - 验收证据（2026-09-05）：Settings snapshot 新增独立的 Input Monitoring
    `unsupported`/`denied`/`granted` 投影，不再把授权状态和 event-tap `service_status` 混同；可见
    settings 窗口以每秒一次的只读 preflight 刷新，变化会推进 snapshot revision 并更新中英 Diagnostics
    文案。轮询不会请求权限、创建新 input service 或重启 tap，窗口释放即停止。UI presentation 定向测试、
    app/UI 严格 Clippy 和 release check 通过；真实 TCC 授权/撤销矩阵仍由 Phase 8 macOS 实机门禁覆盖。

### 8.4 更新与诊断

- [x] 以第三方库承担更新流程，删除自研验证层。
  - 验收证据（2026-09-13）：ADR-0029 取代 ADR-0021、ADR-0022、ADR-0025 与 ADR-0026。
    `bongocat-update` 原有 9 个模块、4615 行实现全部删除，改为第三方更新库的薄封装：
    `release.rs` 的 `ReleaseConfiguration` 固定发行仓库、构建期 channel、target triple 与二进制名；
    `runtime.rs` 的 `UpdateRuntime` 承载 check/install/restart 并把库错误映射为自有稳定码；
    `diagnostics.rs` 保留 10 项匿名计数与 13 个稳定错误码。`cargo fmt --all --check`、严格 Clippy
    （`-D warnings`）、`cargo test --locked --workspace` 与 locked release check 通过；新增 13 个
    单元测试覆盖 channel 门禁、签名密钥失败关闭、错误码稳定性与诊断计数。
  - 状态（2026-09-14）：库选择与信任模型已由 ADR-0034 更新为
    `cargo-packager-updater 0.2.3` + detached minisign 签名。换实现的两条硬性理由：zipsign 只能签
    `.zip`/`.tar.gz`，裸 `.exe` 落入 `ArchiveKind::Plain(None)` 必然验签失败；且 `self_update` 的
    replace-and-verify 语义不适用于 NSIS 系统安装器（上游 `src/lib.rs:302`），Windows 会缺失安装
    步骤。新库自带该步骤（`UpdateFormat::Nsis`）。`bongocat-update` 的公开面
    （`ReleaseConfiguration`、`ReleaseChannel`、`UpdateTargetTriple`、`UpdateRuntime`、13 个稳定
    错误码、10 项匿名计数）未变；签名端与打包端同属 `cargo-packager`，签名器与验签器不会各自演进。
    原三个 `self_update` 专属能力测试（`archive_layout_capability.rs`、`local_install_rehearsal.rs`、
    `multi_file_install_capability.rs`）随该库退役删除，由 `release_manifest_capability.rs` 取代：
    它用 loopback HTTP 把**生产签名器**（`cargo_packager::sign::sign_file`）与**客户端验签器**放在
    一起跑，覆盖共享 manifest 及其平台键查找、篡改载荷、未知密钥、空公钥与 macOS 整包替换。
    `tools/tests/test_update_release_contract.py` 覆盖另一半——manifest 资产名与平台键必须与
    runtime 声明的一致。`cargo fmt --all --check`、严格 Clippy、
    `cargo test -p bongocat-update`（17 单元 + 8 能力）与 `tools/tests` 全部通过。
  - 状态（2026-09-14，同日修订）：manifest 形状由"每 target 一份 dynamic"改为"一份共享 static"
    （ADR-0034 §3 修订说明）。runtime 请求
    `https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json`，其
    `platforms` 映射按 `<os>-<arch>` 提供每个 target 的载荷。每个构建 job 仍只写自己的 fragment，
    发布前用 `just manifest` 合并——合并由 `crates/bongocat-packaging --merge-manifests` 承担，
    而不是在 CI 里拼 JSON，manifest 形状与资产名因此仍由一处拥有。合并产物被更新库自身的读取类型
    反序列化验证（`cargo-packager-updater` 作为该 crate 的 dev 依赖），比正则匹配源码更强。
- [x] 更新 worker 与设置 worker 分离，独占更新管线。
  - 验收证据（2026-09-15）：`bongocat-app::ApplicationUpdateService` 起一条
    `bongocat-update-service` 线程独占 `UpdateRuntime`，是唯一触碰网络、下载与安装的组件；命令通道
    有界且非阻塞，状态经 `UpdateStateHandle`（`Arc<Mutex<UpdateSnapshot>>` + revision）覆盖发布，
    窗口按 250 ms 轮询。不复用设置服务循环：设置循环是串行阻塞的，百 MB 级传输会让设置读写排队。
    worker 只发布状态不推送事件，因此窗口关闭、隐藏或渲染慢都不会反压 worker。11 个测试在真实
    worker 线程上用脚本化 engine 驱动状态机，覆盖 check 的三种结果、失败的 stage/code 归属、
    安装的四步顺序、无 release 时 install 不改状态、不可用构建不进入管线、重启请求只被消费一次，
    以及 drop 会停止并 join 线程。`cargo test --locked -p bongocat-app --lib update` 通过。
- [x] 更新窗口覆盖检查、下载进度、校验、安装与失败重试。
  - 验收证据（2026-09-15）：新增 `bongocat-ui::update_window`，单例窗口由系统菜单「检查更新」与
    设置页 About 的「检查更新」入口打开。窗口渲染 `Unavailable` / `Idle` / `Checking` / `UpToDate` /
    `Available` / `Downloading`（`Progress` 条 + 已下载/总量 + 百分比）/ `Verifying` / `Installing` /
    `Installed` / `Failed`（阶段 + 稳定错误码的本地化文案 + 重试）十种状态，显示当前版本、可用版本与
    「更新内容」。关闭窗口不取消任何操作（操作属于 worker），因此关闭始终可用；窗口在打开期间每秒
    跟随设置快照的语言变化。**下载进度与阶段可观测性靠拆分库调用实现**：runtime 改用
    `download_extended` + `install` 两次调用，因为库的 `download_and_install` 把验签与安装合成一次
    调用，无法区分 `Verifying` 与 `Installing`。`bongocat-ui` 的 12 个 update 测试与
    `bongocat-app` 的状态机测试通过。
  - 补充（2026-09-15）：检查按钮文案按检查次数区分——`Idle` 为「检查更新」；`UpToDate` 与全部
    `Failed` 阶段至少检查过一次，改为「重新检查」（en "Check Again"）。刻意不用「重试」：点击走的是
    完整 check → available → 重新下载管线，不存在续传，措辞必须与真实动作一致。新增
    `update.action.recheck` 双语文案与渲染测试 `the_check_action_label_follows_the_phase`；
    `bongocat-ui` 121 个测试通过（临时模拟面板删除后为 112 个）。
  - 补充（2026-09-15）：发布页按钮文案由「查看发布说明」改为「在 GitHub 上查看」
    （en "View on GitHub"）。原因：窗口「更新内容」区已直接渲染同一份说明，按钮的独特价值是
    GitHub 发布页这个载体（资产列表、评论区），原文案与窗口内容重复主张且范围不精确。
    行为不变：`release_page_url` 拼为 `releases/tag/v<version>`，系统浏览器打开。
- [x] 更新入口、平台差异与安装后重启。
  - 验收证据（2026-09-15）：系统菜单 `SystemMenuAction::CheckForUpdates` 从空操作改为打开窗口并
    发起检查（此前是 `Ok(true)`，点下去没有任何行为）；About 页新增入口，由
    `SettingsWindowRequest` 可选回调承载——恢复模式与 smoke 窗口没有更新 owner，因此不渲染这个
    控件，而不是渲染一个点不动的按钮。`restart_required_after_install()` 返回
    `cfg!(target_os = "macos")`：macOS 安装成功后自动重启（先按 §5.3 顺序 shutdown，再 `exec` 新构建），
    Windows 由安装器 `/R` 重启、`Installed` 在本平台不可观测。**自动重启由应用侧看门狗触发而不是由
    窗口触发**——窗口可以随时关闭，挂在窗口上会让关掉窗口的用户留下一个执行着已删除文件的进程；
    看门狗每 25 ms 观察发布阶段，看到 `Installed { restart_required: true }` 后等 1200 ms 再重启，
    窗口的「立即重启」只缩短这个等待，两条路径共用同一标志因此只重启一次。**本机无法验证 Windows
    路径**，见 ADR-0035 待验证项 2。`cargo fmt`、两种 feature 组合的严格 Clippy 与 workspace 测试通过。
- [x] 自动检查更新开关真正生效。
  - 验收证据（2026-09-15）：`check_for_updates_automatically` 此前只被写入配置、**没有任何代码读取**。
    现在由 GPUI 侧调度（开关值只有设置服务读得到）：启动后等 10 秒开始首次检查，之后每 24 小时一次；
    发现可用更新且窗口未打开时打开更新窗口。**间隔未持久化**，频繁重启的机器会退化为每次启动检查，
    见 ADR-0035 残余风险 6。
- [x] 发布说明随共享 manifest 一起发布。
  - 验收证据（2026-09-15）：`bongocat-packaging --merge-manifests` 新增 `--release-notes <file>`，
    把说明写进 `latest.json` 顶层 `notes`（上限 32 KiB，超长在字符边界截断并追加可见标记，空文件按
    无说明处理）；runtime 透传为 `UpdateRelease.notes`，窗口在「更新内容」区域渲染并另给
    `releases/tag/v<version>` 链接。说明由发布工作流用 `gh api .../releases/generate-notes` 生成，
    同一份文本同时喂给 manifest 与 `gh release create --notes-file`，发布页与客户端不会分叉；
    不新增第二次网络请求、不新增依赖。`bongocat-packaging` 16 个测试与 `just release-manifest`
    端到端冒烟通过。**真实发布尚未跑过**，见 ADR-0035 待验证项 4。
  - 补充（2026-09-16）：说明的**来源**改为双语 changelog，`gh api .../releases/generate-notes` 退役
    ——它给的是两次 tag 之间的提交摘要，不是本项目对外发布的 changelog。`bongocat-packaging` 新增
    第三种模式 `--extract-release-notes <file>`（`just release-notes <file>`）：按产品版本号从
    `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 各取出同一条目，按「英文正文 → `---` → 中文正文」合成
    一份文件。条目按**二级 ATX 标题的首个 token** 匹配，`## <version> - <date>`、`## [<version>] - <date>`、
    `## v<version>` 都认，`## <version>-rc.1` 与 `## <version>.1` 不认；正文里提到的版本、`###` 子标题、
    围栏代码块里的标题都不参与匹配；CRLF 在拼接前归一；版本标题本身被丢掉（发布页已带版本，保留会
    每种语言各印一次）。版本没有对应条目时命令**失败**，并在错误里列出该文件实际记录的版本——把
    "版本号改一处漏一处"变成一行诊断，而不是发出一份描述别的版本的说明。
    `release.yml` 改为在**下载产物之前**合成说明（缺条目是 tag 的错误，几秒内失败，不必先拉完整
    产物），同一份文件继续同时喂给 `just release-manifest` 与 `gh release create --notes-file`，
    发布页与更新窗口仍不会分叉；该步骤不再需要 `GH_TOKEN`。新增
    `tools/tests/test_release_changelog_contract.py`（6 个用例）固定接线：说明由 changelog 合成且
    不再走 `generate-notes`、发布正文与 manifest 读同一份文件、合成早于下载与合并、工具声明的两个
    文件名确实存在、双语 changelog 记录相同且非空的版本列表。
    `bongocat-packaging` 23 个测试通过（新增 7 个：条目匹配、标题写法、CRLF、缺失条目的错误信息、
    模式互斥、双语合成格式、仓库双语文档一致性），`just release-notes` → `just release-manifest`
    在本机端到端跑通
    （`latest.json` 的 `notes` 为 6168 字节，含 `---` 分隔的两段语言，远低于 32 KiB 上限）。
    真实 tag 发布仍未跑过。**待处理**：工作区版本是 `1.1.0` 而 changelog 记录的是 `2.0.0`，版本号
    升到 `2.0.0` 之前 `just release-notes` 按设计失败（构建任务的 tag 校验同样会拦住 `v2.0.0`）。
- [x] 更新内容的 Markdown 渲染。
  - 验收证据（2026-09-15）：`notes` 是随 manifest 走网络的**不可信输入**，新增
    `bongocat-ui::update_markdown`（依赖 `pulldown-cmark =0.13.4`，当时最新稳定版，
    `default-features = false` 去掉用不到的 CLI 参数解析与 HTML 渲染器）。渲染分两层：`blocks()`
    把 CommonMark（含删除线与任务列表）解析为不含 GPUI 类型的中间表示，纯函数可单测；`render()`
    只消费该表示，不存在可被注入的 markup 层，原始 HTML 按字面文本显示。图片只渲染 alt 文本、
    不发请求；链接仅 HTTPS 且无空白/控制字符才可点击（与更新传输的 HTTPS-only 策略一致），其余
    渲染为普通文本。长度上限 32 KiB（字符边界截断），嵌套深度超过 8 层压平但保留内容。紧凑列表项
    不被解析器包 `Paragraph` 的缺陷已修：inline 内容缓冲到项结束时统一成段，列表内链接保留样式。
    覆盖 26 个解析单测与 4 个无头渲染测试；`cargo fmt`、严格 Clippy 与 `bongocat-ui` 120 个测试通过
    （临时模拟面板删除后 `bongocat-ui` 为 112 个）。
    不支持 GFM 表格（按 CommonMark 退化为普通段落）——release notes 实际不会包含表格，如需要再评估。
- [ ] 只允许 HTTPS，固定公钥来源和轮换流程。
  - 状态（2026-09-13，历史）：当时 `self_update` 方案的 `RELEASE_SIGNING_KEY` 为 `None`，
    因此 runtime 在发出任何请求前失败关闭并返回 `update_signature_key_missing`；公钥轮换窗
    随 ADR-0021 退役，key ID 与有效期概念不再存在。该库与签名模型已于 2026-09-14 被 ADR-0034
    取代，此行只保留当时状态。
  - 状态（2026-09-15）：`UpdateRuntime::unavailability()` 现在把"为什么不能更新"作为返回值
    （`UnsupportedHost` / `DevelopmentChannel` / `SigningKeyMissing`），worker 据此在发出任何请求前
    把 `Unavailable` 阶段发布给窗口，而不是让入口静默消失。HTTPS-only 由 `manifest_endpoint` 的
    `https://` 字面量与定向测试锁定。
  - 状态（2026-09-14，当前）：公钥形态改为 base64 的 minisign 公钥盒文本，并已内嵌发布公钥
    `DF5E2C9D255DD85E`；缺失、空串或纯空白时的失败关闭语义由 `runtime.rs` 的门禁与定向测试
    锁定。**轮换能力比 ADR-0029 时更弱**：
    新库的 `Config.pubkey` 是单个 `String`，没有 zipsign 那样的 any-of 多密钥语义，因此换钥匙必须
    先发布一个"认识新钥匙"的版本，否则已安装用户的更新链直接断裂。轮换流程尚未设计，见 ADR-0034
    待验证项 4。
- [ ] 校验版本、target、arch、hash 和签名。
  - 状态（2026-09-13，历史）：当时由 `self_update` 承担版本比较、target 资产匹配、SHA-256
    与归档签名，但未配置真实签名公钥，也未在任何真实产物上验证。该库与签名模型已于
    2026-09-14 被 ADR-0034 取代，此行只保留当时状态。
  - 状态（2026-09-14）：版本比较与 detached minisign 验签由新库承担，且验签发生在安装之前
    （`Update::download()` 读完载荷即校验，`install()` 只接受已验签的字节）。**两项能力随本次换
    实现消失**：独立 SHA-256 完整性校验（旧 `checksums` feature 用 GitHub 公布的 per-asset
    digest）无替代，`update_checksum_mismatch` 已无产出路径；per-platform 资产匹配无替代，
    `update_no_matching_asset` 同样无产出路径（两者均已在 `diagnostics.rs` 注明保留原因）。
    发布公钥已注入；但尚未在真实发布产物上完成签名 → 下载 → 验签验证，操作系统包签名验证也
    仍未实现，因此保持未勾选。
  - 状态（2026-09-15）：失败现在带明确的 stage（`UpdateError::stage()`）。库把下载与验签合成一条
    错误，因此下载阶段的错误按 code 再分一次（`SignatureInvalid` → Verify，其余 → Download，
    未识别的按 Download 处理，不宣称载荷已通过认证），窗口据此显示"下载失败"与"校验失败"两种
    不同文案。传输增加了 30 分钟上限（transport 自身无超时，不设界会让 worker 永久阻塞）。
    **仍未在真实产物上验证**，保持未勾选。
- [ ] 下载支持取消、断点/重试策略和失败清理。
  - 状态（2026-09-13）：随 ADR-0021 一并退役。自研 staging 目录、三次重试与 1 秒/2 秒退避、
    Unix `0600` 权限与 partial 文件清理不再存在；库自身的下载重试与失败清理语义**未经本项目验证**，
    因此保持未勾选。
  - 状态（2026-09-14）：同上，换库未恢复该能力。另需记录一条库行为：验签前把整个载荷
    `read_to_end` 进内存，因此存在一条随产物增长而增长的常驻内存路径（ADR-0034）。
  - 状态（2026-09-15）：**进度反馈已实现**（`UpdateEvent::Progress`，窗口显示百分比与已下载/总量），
    但**取消是刻意不做的**：库在 `download_extended` 内部读完全部载荷并验签，没有 abort 钩子，
    要做到真取消只能自研下载与验签——而自研验证层正是 ADR-0029/0034 删掉的东西。因此窗口不提供
    取消按钮，并改为始终允许关闭（关闭不取消操作，因为操作属于 worker）。断点续传、库自身的重试
    与失败清理语义仍未验证，保持未勾选。
- [ ] 安装前协调 runtime/renderer shutdown，失败可回滚。
  - 状态（2026-09-13）：随 ADR-0026 一并退役，`UpdateInstallCoordinator` 已删除。库在内部执行
    prepare -> rename 交换 -> best-effort 回滚，但**不与本项目 runtime/renderer 的 shutdown 顺序
    协调**，也未在真实安装链验证，因此保持未勾选。
  - 状态（2026-09-14）：仍不与本项目 shutdown 顺序协调。macOS 由库整包替换 bundle；Windows 由库
    运行下载到的安装器（`install_mode = Quiet` → NSIS `/S` `/R`）后 `process::exit(0)`，因此
    `UpdateOutcome::Installed` 只在 macOS 可观测，Windows 成功路径以进程退出结束。**该路径在本机
    （macOS）无法验证**，见 ADR-0034 待验证项 2。
  - 状态（2026-09-15）：**协调已实现，但顺序与本文条目措辞不同，需要维护者确认**（详见 ADR-0035
    的"需要维护者复核的一处偏差"）。实现顺序是 install 完成之后、替换进程之前按 §5.3 走完整
    shutdown 再 `exec` 新构建，而不是"安装前 quiescence"。三条理由：macOS 的整包 `rename` 对运行中
    进程是 inode 安全的；Windows 的安装路径本来就 `exit(0)`；而 `Application::shutdown(self)` 消耗
    自身且没有 restart，先 quiescence 会让安装失败（磁盘满、权限、`remove_dir_all` 后 `rename`
    失败）停在"overlay 已销毁、runtime 已停止"的不可恢复状态。当前顺序下安装失败时应用完好无损。
    **回滚仍未验证**，macOS 自动重启路径也未实测，因此保持未勾选。
- [ ] 测试断网、代理、中断、签名错误和降级攻击。
  - 状态（2026-09-13）：自研 coordinator 回归随实现一并删除。**降级攻击检测随 `release_sequence`
    退役而不再存在**——现在仅按 semver 比较，低 sequence 重放不再被拒绝。断网、代理、中断与签名
    错误的真实链路测试均未运行，因此保持未勾选。
  - 状态（2026-09-14）：签名错误路径已有 loopback 能力测试覆盖（篡改载荷、未知密钥、空公钥），
    但仍**不是**真实链路。共享 manifest 重新带回 per-platform 门禁（漏掉本机平台键时命中
    `update_no_matching_asset`），但 manifest 本身没有防降级保护：能替换 manifest 的攻击者仍可以
    把客户端指向一个旧但签名有效的载荷。断网、代理与中断均未测试，因此保持未勾选。
  - 状态（2026-09-15）：worker 状态机现在有**离线**覆盖——脚本化 engine 注入
    `ReleaseFetchFailed`、`SignatureInvalid` 等失败后，断言窗口收到的阶段与错误码确实归属正确的
    stage。这覆盖的是"失败如何被呈现"，**不是**真实断网/代理/中断链路，也不是降级攻击防护，
    因此保持未勾选。
  - 状态（2026-09-15）：**真实端点的失败形态已在 loopback 上固定**。首次在真实安装产物上检查更新
    报 `update_release_fetch_failed`，`curl` 定位后发现线上 `latest.json` 是旧 Tauri 产物（18 个
    条目全部缺库必需的 `format`，平台键为 `darwin-*`），因此是 `Error::Serialization`。据此把
    "取不到发布信息"与"取到了但读不懂"拆成两个稳定码，并新增两条能力测试：
    `a_release_without_a_manifest_is_a_fetch_failure`（无 manifest → `ReleaseNotFound`）与
    `a_manifest_from_another_pipeline_is_rejected_as_unreadable`（旧 Tauri 文档 →
    `Serialization`；对照实验证明只补 `format` 就会变成 `TargetNotFound`）。**仍不是**真实断网/
    代理/中断链路，因此保持未勾选。
- [ ] 更新 channel 按环境隔离。
  - [x] 构建期 channel 门禁。
    - 验收证据（2026-09-13）：`ReleaseChannel::from_environment` 从不可变 `BuildEnvironment` 派生
      channel，运行时输入无法改变它；Development 构建的 `check()` 与 `install()` 在发出任何请求前
      返回 `update_environment_disabled` 并记录稳定诊断码。系统菜单的「检查更新」入口由
      `bongocat_app::update_check_available()` 决定，仅当 production channel 与签名公钥同时具备时
      才显示。
  - [x] worker 与窗口层面的 channel 门禁。
    - 验收证据（2026-09-15）：worker 在进入管线前先查 `UpdateRuntime::unavailability()`，禁用构建
      把 `Unavailable` 阶段发布给窗口而不是发起请求；初始阶段也由它决定，因此入口缺失是有解释的
      而不是静默的。定向测试覆盖"不可用构建收到 Check 后不进入管线、只重新发布原因"。
      更新 channel 的独立 sequence store 仍由既有定向测试覆盖。
- [x] 日志 rotation、总大小和保留天数有上限。
  - 状态（2026-09-05）：Cubism Core 日志 sink 已在单文件达到 1 MiB 时执行有界路径轮转，最多保留
    1 个活动文件加 7 个轮转文件，总量不超过 8 MiB；活动文件和轮转失败均有有界 dropped 计数；测试覆盖触发轮转、保留上限和
    活动文件恢复写入。应用级 writer 现按 UTC 日分文件，单文件 1 MiB、总量 8 MiB、最多 8 个文件、
    保留最近 7 日，并覆盖日期切换、轮转、过期/总量清理和失败计数；Core 历史日志仍未纳入同一
    retention policy；Core rotation files 现也在初始化和成功轮转后按 7 日上限清理，且 CoreLogStats
    已暴露匿名 written/dropped/rotated/pruned/active-bytes/retained-files 指标并有 rotation 回归；两类
    历史日志现以 retained-bytes/files 的饱和总数聚合导出，但 retention enforcement 仍由两个
    隔离 writer 仍分别维护自身 rotation 计数，但已通过共享 helper 形成统一目录级 budget。
  - 验收证据（2026-09-07）：新增无平台依赖的 `bongocat-log` retention helper，统一扫描已知
    application/Core JSONL 文件，按 7 日 metadata 保留和 8 MiB 目录级 budget 清理最旧轮转文件；
    活动文件始终保留，symlink/未知文件不会被触碰。application 与 Core writer 均在初始化/写入后
    调用同一策略，跨 writer aggregate、过期轮转和活动文件保护回归通过；`cargo fmt`、定向
    workspace test、严格 Clippy 与 locked release check 通过。
    - 状态（2026-09-07）：补充未知文件与 Unix symlink 隔离回归；目录级清理仅处理已知 JSONL
      命名，非日志数据和链接均保留。
  - [x] `P7-CORE-LOG-DIAGNOSTICS`：将 Cubism Core retention 指标接入匿名 diagnostics export。
    - 依赖：`CoreLogStats`、应用 diagnostics export 和 ADR-0016 的隐私边界。
    - 退出条件：产品启动将只读 Core 指标 provider 注册到 `Application`；每次导出实时采样
      written/dropped/rotated/pruned/bytes/retained_files，并与 application logs 分栏序列化；不导出
      日志正文、路径或 Core message；provider、导出字段和隐私回归通过。
    - 状态（2026-09-06）：Core callback owner 通过 `CoreLogReporter` 提供只读匿名统计，正式产品入口
      注册 provider；settings worker 将当前采样写入 `core_logs`，无需读取或复制 `cubism-core.jsonl`。
      应用 provider 与原子 export 定向测试覆盖缺失/存在 Core owner 和字段值。
    - 状态（2026-09-06）：Core FFI callback 已改为只做 512-byte 有界复制与容量 128 的非阻塞入队；
      JSON、轮转和文件 I/O 只在专用 Rust worker 执行。global callback-slot contention、queue full 和
      stop 后迟到记录均只递增匿名 dropped，关闭先注销 callback 再排空并 join worker。`bongocat-live2d`
      39 项定向测试覆盖 contention、saturation、late callback 和 shutdown drain；macOS Development release
      诊断导出 smoke 与 `bongocat-app --run-seconds 4` 正常启动/退出均通过。跨域历史日志的统一
      retention/aggregate policy 已由 `bongocat-log` 共享 helper 接入。
    - 状态（2026-09-07）：`CoreLogReporter::stats()` 每次采样都会从当前文件集合刷新
      `retained_files`/`retained_bytes`，因此 application writer 清理 Core 轮转文件后，下一次
      diagnostics export 不会继续显示过期容量；跨 writer 统计刷新回归通过。
- [ ] 记录 renderer/input/model/config/update 的稳定 error code。
  - 状态（2026-09-01）：runtime renderer 已为 model load/evaluation、motion/expression load、GPU
    prepare、platform、transport 和 overlay validation 定义 10 个固定 snake_case code，并以唯一性
    contract 防止诊断协议依赖 Rust `Debug` 名称；该 code 已投影到 SettingsSnapshot 和 Diagnostics
    页面。input/config/model 已有各自 typed code，update diagnostics 现通过统一 catalog 过滤后
    进入匿名导出；真实 update worker 错误源和跨平台完整错误矩阵仍待完成，因此保持未勾选。
  - 状态（2026-09-05）：`ModelStoreDiagnostic` 现为 11 个来源无关、固定
    `model_store_*` code；枚举包含完整 `ALL` 集合与唯一性回归。settings service 继续按 import/
    delete 操作映射为既有可操作 `SettingsErrorCode`，不公开 model store 的资源名或 I/O detail。
    2026-09-16 新增压缩包来源后为 12 个（多出 `model_store_source_archive_unsupported`，见
    `P4-MODEL-ARCHIVE-SOURCE`）；settings 侧仍映射为既有 `SettingsErrorCode`，两个映射函数的
    回归测试新增"枚举 cases 必须覆盖 `ALL`"的断言，避免新码静默漏映射。
    renderer 的 `ModelCommitErrorCode` 现同样公开唯一的稳定
    `model_commit_resource_preparation_failed` code，runtime 仍将该拒绝投影为
    `gpu_preparation_failed`，不改变既有两阶段模型切换失败语义。
    `PlatformInputError` 的 13 个 service lifecycle/permission/backend 结果也已固定为
    `platform_input_*` code；overlay 继续只将其归类投影为 input service status，避免在
    Settings snapshot 或日志中包含 OS 原始文本。
    `UpdateErrorCode` 现注册全部 32 个 manifest/verifier/artifact 失败 code，并以唯一性回归
    固定命名；download、schedule、sequence store 和 staging 的公开离散 code 也各自提供完整 `ALL`
    catalog 和 namespace/唯一性回归，staging 的完整性 composite 明确保留 verifier code，调度回退固定为
    `update_schedule_monotonic_time_regressed`。更新 diagnostics 的匿名导出投影已接入；真实 worker
    错误源和安装回滚观测仍待发布链路。
  - 状态（2026-09-06）：当前 configuration recovery diagnostics 现导出既有
    `configuration_recovery_required` stable code，同时保留 checked-backup 聚合；正常配置和用户已
    恢复默认值但需要重启的状态不伪装为错误。其它 config write failure 与 update code 尚无持久的
    产品观测源，因此总项保持未勾选。
  - 状态（2026-09-06）：平台 input diagnostics 将启动失败的既有匿名
    `platform_input_*` code 与 service status 一起发布，settings/Diagnostics export 保留该 code；
    例如 `TapCreateFailed` 保持为 `platform_input_tap_create_failed`，不将其降级为无信息的
    `Failed` 或导出 OS 文本。
  - 状态（2026-09-07）：runtime 新增闭合的 `platform_input_*` 稳定码 catalog，Application
    projection 现在在进入 SettingsSnapshot 和匿名 diagnostics export 前过滤未注册 provider code；
    已知 tap/permission code 保留，私有 detail 或路径字符串丢弃，避免公开字符串绕过稳定错误协议。
  - 状态（2026-09-07）：平台 `PlatformInputError::ALL` 现在由 platform unit test 逐项核对 runtime
    catalog，新增平台生命周期错误若未同步 diagnostics 过滤边界会直接失败。
  - 状态（2026-09-07）：runtime shutdown timeout/worker panic 计数已加入 settings/UI
    diagnostics presentation，页面以中英文匿名文案显示累计失败数并将其标记为 actionable；
    UI localization/presentation contract 与严格 Clippy 通过。update worker、真实安装回滚和
    平台错误源仍待后续发布链路接入。
  - 状态（2026-09-07）：`bongocat-update::UpdateDiagnostics` 已作为可选 app-owned provider
    接入匿名 `diagnostics.json`；导出只包含稳定 update error code 与 check/download/install
    阶段计数，未注册 worker 时保持 `null`，不暴露 endpoint、artifact、版本或签名材料。app/update
    定向测试、严格 Clippy 与 release check 通过；真实 update worker、安装回滚和平台错误源仍待
    后续发布链路接入。
  - 状态（2026-09-07）：更新 diagnostics provider 的 `last_error_code` 现在经过
    `bongocat-update` 统一稳定码目录校验，覆盖 manifest、transport、download、staging、sequence、
    schedule 和 install 边界；未知字符串在进入 Application/diagnostics export 前被丢弃，保留计数。
    目录覆盖、Application/导出边界和隐私回归通过，未改变真实 update worker、安装回滚或平台
    错误源仍待接入的状态。
  - 状态（2026-09-07）：`UpdateDiagnosticsTracker` 提供 app-owned、可跨 worker clone 的原子阶段计数
    与稳定错误码记录；`Application::set_update_diagnostics_tracker` 将其接入现有匿名导出边界，
    未注册 tracker 时仍保持 `update: null`。共享事件、未知错误码脱敏和 Application 投影回归通过；
    真实 update worker、endpoint、下载/安装调度仍待发布链路接入。
  - 状态（2026-09-15）：**真实 update worker 已接入**。`ApplicationUpdateService` 用应用自己的
    `UpdateDiagnosticsTracker`（在 `Application` 移交给设置服务之前注册，因此与匿名导出边界共享
    同一实例），每次 check/download/install 都推进对应计数与最后稳定错误码。UI 侧另有独立目录
    `bongocat_ui::UpdateErrorCode`（14 项，含本次新增的 `update_release_manifest_invalid`），由
    `bongocat-app` 的穷尽映射与逐项字符串比对锁定；
    新增一个 code 会让映射编译失败，直到它被赋予本地化文案。跨平台完整错误矩阵与真实发布链路
    证据仍待完成，因此总项保持未勾选。
  - 状态（2026-09-15）：`update_release_fetch_failed` 不再兼任两件事。真实端点的首次运行暴露了它
    同时表示"取不到"与"读不懂"，现在拆为 `update_release_fetch_failed`（`Error::ReleaseNotFound`）
    与 `update_release_manifest_invalid`（`Error::Serialization` / `Error::Semver`）。`Error::Semver`
    从 `NotConfigured` 移到这里：本构建自己的版本在任何请求前就已解析，库返回的 semver 错误只可能
    来自 manifest 的 `version` 字段。按 ADR-0034，新增码向后兼容。
  - 状态（2026-09-07）：check、download、install coordinator 增加显式 diagnostics 包装入口，统一记录
    started/succeeded/failed 及稳定失败 code，旧无诊断 API 保持兼容。三阶段成功、失败和取消路径的
    tracker 回归通过；这些入口仍是 worker 调用边界，不代表已建立真实后台更新线程或发布 endpoint。
  - 状态（2026-09-07）：automatic update scheduler 增加 diagnostics 包装入口，在单调时钟回退时将
    `update_schedule_monotonic_time_regressed` 作为匿名 check failure 记录；调度器原有的 rebasing
    和无重试语义不变，回归通过。
  - 状态（2026-09-07）：`UpdateDiagnosticsTracker` 增加多 worker 并发事件和 `u64::MAX` 饱和计数
    回归，确认跨线程共享计数不丢失且不会溢出；稳定错误码仍保持匿名、可枚举边界。
  - 状态（2026-09-07）：install diagnostics 包装新增安装失败/回滚失败分类与取消优先级回归，确认
    失败始终计入阶段计数，取消不会触发 shutdown 或 install；三阶段 coordinator 的稳定观测边界完整。
  - 状态（2026-09-07）：稳定 update error-code contract 进一步校验 verifier、manifest transport、
    download、install、schedule、sequence 和 staging 七个 catalog 的全局唯一性；69 个公开 code
    无跨边界重名，未知 provider code 仍被丢弃。该检查只强化匿名 diagnostics 协议，不代表真实
    update worker、endpoint 或安装回滚链已接入。
  - 状态（2026-09-07）：目录打开与外部 HTTPS URL wrapper 的公开错误现在也统一为
    `directory_open_*`/`external_url_open_*` stable code；枚举 `ALL` 与逐项回归固定全部 code，
    不再把自然语言或底层启动失败文本传播到 app 层。真实 update worker、安装回滚和平台错误源仍待
    后续发布链路接入。
- [ ] 日志导出生成可预览的脱敏包。
  - 状态（2026-09-06）：ADR-0027 已冻结 preview bundle 为当前环境私有的 v1 ZIP，固定只包含
    `manifest.json`、匿名 `diagnostics.json` 和严格重新序列化的 application code event records；
    Cubism Core message/原始 `.jsonl` 明确排除，只保留现有匿名聚合统计。2026-09-06 已接入 app-owned
    writer：它只枚举严格命名的 regular application logs、逐条以 closed schema 重新序列化为固定 code
    record，输出后用 ZIP reader 复核唯一固定 entries、manifest、匿名 diagnostics JSON 和 event count；
    每个来源和匿名 diagnostics entry 均最多 1 MiB、最多 8 个 application 来源，ZIP 总量最多 10 MiB；未知字段/损坏来源
    跳过且不复制原 bytes。typed settings result 已投影 ZIP format、固定 entry 数、bundle bytes 和匿名
    skipped-source count；Diagnostics 页面以中英文显示这些稳定结果，不显示路径、archive entry 或日志正文。
    测试 writer 已拒绝 symlink/non-regular target，并在 temporary file 打开后、commit 前及受控
    target-replace failure 注入失败；失败清理只匹配 `atomic-write-file` 规定的固定前缀和六位 ASCII
    staging 名，避免误删同前缀普通文件。已有 preview 在可保留的失败路径不变且 staging 清理；macOS
    Development product smoke 已通过隔离 owner-only
    storage 运行 typed command，并验证 private JSON/ZIP、固定 ZIP entries 与干净 shutdown。Windows
    release smoke 和 OS-level sync/replace failure injection 仍待实现，本项保持未勾选。
  - 状态（2026-09-07）：Native Phase 0 workflow 的 macOS/Windows 隔离 storage job 均执行
    `--diagnostics-export-smoke`，断言稳定成功消息、私有 diagnostics JSON/preview ZIP 和固定
    archive entries；Windows 平台不再只有单元测试覆盖。OS-level failure injection 仍待产品
    smoke 覆盖，本项保持未勾选。
  - 状态（2026-09-07）：settings service 新增失败重试 contract，确认一次成功导出后，后续
    provider/文件系统失败返回稳定 `diagnostics_export_failed`，且 snapshot 保留上次成功的
    format/bytes/entry 结果，UI 可安全显示并重试而不会丢失最近一次成功状态。
  - 状态（2026-09-07）：新增 `--diagnostics-export-failure-smoke` 产品级失败路径。macOS/Windows
    均以真实目录目标触发 diagnostics 原子 open/replace 失败，macOS 另以不可写 `logs/` 目录
    触发同步/写入失败；两平台均验证稳定 `diagnostics_export_failed`、既有 preview 保留和
    staging 清理，并已接入 Phase 0 隔离 storage workflow。该 smoke 不替代真实磁盘满、ACL/UAC
    和系统级电源故障注入，故总项仍保持未勾选。
  - 状态（2026-09-01）：settings service 已新增有界 `ExportDiagnostics` command，使用当前环境
    `logs/diagnostics.json` 的同目录原子写入生成 format v1 JSON。导出只包含稳定 runtime/input/
    configuration code、匿名聚合计数、模型来源计数和 settings/config revision；不包含模型 ID、
    路径、按键值、原始 JSON、时间戳或动态 I/O 文本。Diagnostics 页面提供键盘和 AccessKit 可访问
    的 Export 控件，并显示本次导出的字节数；app/ui 定向测试覆盖原子写入、聚合排序、隐私边界和
    typed command。应用级 writer 的匿名 written/dropped/rotated/pruned/bytes/retained_files
    统计与 Core retention 指标现已并入导出，但导出仍不读取或合并 Core/应用原始日志正文，预览器和
    跨域历史日志打包仍待完成，因此本项保持未勾选。
- [ ] 更新 manifest 定义 `schema_version`、channel、最低可升级版本、发布时间和防回滚字段。
  - 状态（2026-09-13）：**本项随 ADR-0029 作废**。manifest v1 schema、单调 `release_sequence`
    防回滚、`minimum_upgradable_version` 与 target artifact 列表不再属于本项目契约；
    `shared/update/` 下的 Draft 2020-12 schema 与 accept/reject fixtures 已删除，
    `tools/validate-json-schema.py` 中的 update 校验入口同步移除。发行元数据现由更新库从发行页
    读取，本项目不再解析或校验 manifest。
  - 状态（2026-09-14）：换库（ADR-0034）后结论不变，且发行元数据的形状由库决定：共享 manifest 是
    库的 *static* 形状（顶层 `version` + `platforms` 映射），每个 target 的 fragment 是
    `version` + `url`/`signature`/`format`。本项目只负责写出这两种形状并合并，
    不引入 `schema_version`、channel 或防回滚字段。资产名由
    `tools/tests/test_update_release_contract.py` 固定，形状由
    `crates/bongocat-packaging` 的合并测试喂给更新库自身的读取类型验证，
    `crates/bongocat-update/tests/release_manifest_capability.rs` 覆盖库侧的解析与验签。
- [ ] 更新 helper/installer 的权限边界、替换原子性和失败恢复经过单独威胁建模。
  - 状态（2026-09-13）：**原验收证据随 ADR-0026 作废，本项从已完成回退为未完成**。独立 helper、
    prepare -> validate -> atomic same-volume rename -> launch/health acknowledgement 的契约不再由
    本项目实现；替换改由更新库内部完成，其权限边界、原子性与失败恢复**未经本项目威胁建模或实机
    验证**。OS package signature、故障注入和双平台实机 smoke 仍未实现。
  - 状态（2026-09-14）：换库未改变结论，并新增两条待建模的事实（ADR-0034）：macOS 走整包替换
    bundle；Windows 走"运行下载到的 NSIS 安装器 + `process::exit(0)`"，因此更新后的重启由安装器
    的 `/R` 承担而不是本项目的 `restart()`。Windows 路径在本机（macOS）无法验证。

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
  - 状态（2026-09-06）：macOS `bongocat-overlay` paced preview 已输出有界 `draw` 调用
    p50/p95/p99（nearest-rank，微秒）、完整主线程循环 missed-deadline 数、样本数和溢出数；
    初始 draw 也进入样本。`90e0aa7` 的 M1 Pro 单机 30 秒 release raw baseline 已保存；该工具
    不测输入/runtime/sleep，且尚无跨设备结果或 Instruments/Metal System Trace 原始证据，因此本项保持未勾选。
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

- [x] 删除 Tauri、Vue、Pinia、Pixi.js 和 easy-live2d 依赖。
- [x] 删除 src/ Web 前端、src-tauri runtime 和旧 plugin。
- [x] 删除 rdev 和旧 device emit/listen 路径。
- [x] 删除 gilrs 高频 IPC 路径；保留与新手柄方案无关的有效 fork 修复需单独评估。
- [x] 删除旧不安全 updater 配置和宽泛 asset scope。
- [x] 删除旧模型复制代码前确认 Native 显式导入覆盖支持的模型格式。
- [x] 更新 README、开发环境、贡献指南和架构图。
- [x] 保留远端 `master` 和不可覆盖的 `pre-refactor-tauri` 分支作为历史行为与模型资源参考，不重写历史。

状态（2026-09-10）：历史 Vue/Tauri workspace、Web 资源、Node manifests、旧 updater/release
workflow、legacy config inspector 及其本地 fixture 已从当前工作树删除。Native 产品、测试与
共享 preset fixture 统一使用 `resources/models`；`master` 与 `pre-refactor-tauri` 是
唯一的历史源码参考。Phase 0、稳定性和发布验收门槛仍按各自未完成项跟踪，代码退役不代表 stable 发布就绪。

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
   - 状态（2026-09-07）：新增 `docs/migration/legacy-release-assets-v1.1.0.md`，记录公开
     `v1.1.0` tag、发布时间、Windows/macOS 历史资产大小与 GitHub SHA-256，并明确旧 x86/ARM64
     资产不能改变 Native 目标与 Cubism 发布门禁。Windows 实机、Native 发布产物保留、签名和
     最终 target/toolchain freeze 仍待完成。
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
- 状态（2026-09-13）：run `34743931898`、job `103688224078`（PR #1030、`next` @ `f7c20a2`）的
  transactional D3D11 切模 smoke 以 `process thread count exceeded the warmup high-water mark
  12 with 13 threads during model switching` 失败。该提交与上一个绿灯提交 `5554bd5` 的差异仅为
  `bongocat-update` 新增集成测试与 ADR 措辞，overlay 不依赖该 crate，且前 7 次 `next` 运行同一
  步骤均通过，故判定为线程门禁误报而非回归：进程全局 D3D11/DXGI/线程池 worker 可在预热 settle
  窗口之后才出现并长期驻留，而原实现把测量前快照当作硬上限，零容忍比较会把一次性 `+1` 判成泄漏
  （真实逐 switch 泄漏应为 `+309`）。产品探针现采用有界容差 `THREAD_GROWTH_LIMIT = 2`（与既有
  `HANDLE_GROWTH_LIMIT = 4` 对称，稳定性仍由 `settle_process_threads` 保证），新增
  `thread_growth_exceeded` 判定谓词与 Windows 单元回归，并让 `PreviewReport` 报告
  `warmup_thread_high_water`/`threads_after`，使成功路径也能看到剩余余量。本机 macOS format、
  完整 workspace Clippy/workspace test/release check 与 `x86_64-pc-windows-msvc` 交叉 Clippy 已通过；
  新增 Windows 单元测试与 runner smoke 证据待本次推送后的 CI 运行，该项与发布门槛不变。

11. [ ] `P0-INPUT-WINDOWS`：完成 Raw Input + pressed set + `GetAsyncKeyState` 校正并实测 issue #47 场景。

- [x] 完成平台无关 pressed-set contract 和 issue #47 恢复测试；Windows 采集与校正仍未完成。
- [x] Windows 系统合成 input -> `WM_INPUT` -> 故意丢 release -> `GetAsyncKeyState` reconcile 闭环已通过 push run `33249296927`、job `99092066404`；PixPin、Win+L、UAC 和物理设备矩阵仍待完成。

12. [ ] `P0-INPUT-MAC`：完成 CGEventTap 权限拒绝/授予/恢复、状态校正、GameController 和 100 次 restart。

- [x] 完成权限/tap 生命周期 contract、只读 preflight、真实 callback 和受控 disable 恢复；TCC 权限矩阵与系统自然 timeout 仍未完成。
- [x] 完成候选 pressed set 到 `CGEventSourceKeyState` 校正快照的边界和周期调度；真实 callback release 受控丢弃后的 20-cycle 闭环已通过，正式 `MacInputService` 又完成 left Shift down/up 到 runtime `ModelInputSnapshot` 的同进程集成测试。物理输入、系统自然丢事件和生命周期实测仍未完成。
- [x] 完成 GameController extended-profile producer、可靠按钮边沿、keyed axis、连接 generation、background delivery 和 handler shutdown contract；framework 无设备 smoke 已通过，物理 controller/profile/热插拔矩阵仍待完成。
- 状态（2026-09-06）：macOS 26.5.2 arm64 本机以当前 Development release artifact 运行
  `bongocat-overlay standard 4 --interactive`，`MacInputService::start` 成功创建 listen-only
  event tap，并在 4 秒后正常 stop/join（exit 0）；报告 238 帧、1 个 platform cursor sample、0 条
  platform input edge。这只证明当前已授权会话的 tap 创建与有序停止，不替代真实键鼠边沿、TCC
  拒绝/撤销、系统 timeout、锁屏/睡眠或物理 GameController 矩阵。

13. [ ] `P0-CUBISM`：确认 SDK/许可证/binding 生成，三个预置模型完成 Core、资源和 renderer spike。

- [x] 完成平台无关 Rust model3/package parser、所有结构化 sidecar 静态 preflight、三个预置规范化索引与异常资源安全 contract；后续任务已完成产品 Core safe wrapper、预置 motion/expression 求值及 D3D11/Metal 绘制，真实 physics/pose 行为样本仍缺失。
- [x] 完成 6 个预置 motion3 与 15 个 exp3 的强类型结构、segment/Meta 计数、fade/parameter/blend 校验；这不代表 motion/expression 行为求值完成。
- [x] 完成 3 个预置 cdi3 的强类型 parameter/group/part 与 group 拓扑校验；这些字段直接属于规范化索引 schema v1，跨资源 ID 以未来 Core 表为准。
- [x] 完成 physics3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；13 个历史文件只作为本地结构覆盖，不作为可分发 fixture 或行为求值证据。
- [x] 完成 pose3 静态 preflight、匿名摘要 CLI 和合成错误 contract；没有授权真实样本或 fade/link 求值证据。
- [x] 完成 userdata3 v3 静态 preflight、匿名摘要 CLI 和合成错误 contract；三个预置模型没有真实 userdata3。
- [x] 完成 macOS arm64 真实 r.5 sys binding/Core probe；三个预置 Moc 各 100 次生命周期、drawable 与 r.5 offscreen 数组边界、legacy count 对照和 `leaks` 0-byte 门禁通过。产品 safe wrapper、Windows x64 Core/D3D11 与 macOS arm64 Core/Metal 已进入正式链路；macOS x64 原生 ABI、非零 offscreen fixture 及真实 physics/pose Framework 求值仍未完成。
- 状态（2026-09-06）：Apple Silicon host 通过 Rosetta 实际运行 x86_64 Mach-O 的
  `bongocat-live2d` release tests，并链接固定 macOS x64 static Core；三个预置模型的 100-cycle
  lifecycle 与稳定 drawable snapshot 均通过，Core 返回 `6.0.1`。该 cross-ABI smoke 不替代 Intel
  原生主机/GPU/签名验证，故 P0-CUBISM 总项与 macOS Intel 发布门槛保持未完成。
- [ ] 取得可分发授权的 physics3/pose3 fixture 后完成强类型结构和 Framework 求值；三个预置模型不含这两类资源，不得以合成样本冒充兼容证据。

14. [ ] `P0-GO-NO-GO`：汇总证据、阻塞和条件，形成完整功能与 stable 发布决议。
    - 状态（2026-08-31）：ADR-0011 已形成 `IMPLEMENTATION GO WITH RELEASE CONDITIONS`，允许建立正式 workspace；这不勾选完整 Phase 0 决议。标准 Native `5-r.5` ZIP/hash、产品 safe wrapper、Windows x64 D3D11 与 macOS arm64 Metal 三预置模型绘制已验证；真实 physics/pose Framework 样本、其他原生 ABI、GPUI 辅助功能/IME 与双平台物理输入/GPU 矩阵继续阻塞对应功能声明，最终合规清单只阻塞 stable 发布。

15. [x] `P1-RUNTIME-CONFIG`：建立正式 workspace，提升 runtime 生命周期、强类型 command/snapshot 与 Development/Production 配置隔离闭环。
    - 依赖：ADR-0011、`spikes/runtime-contract/`、`spikes/config-store/`。
    - 退出条件：workspace 默认命令通过；环境由构建产物固定；两个数据根无读取、写入或锁 fallback；runtime 正常启动、更新 snapshot、拒绝队列溢出并有序 shutdown。
    - 验收证据（2026-08-30）：正式 workspace 仅包含 app/runtime/config；11 项单元测试覆盖严格 schema、共享默认 fixture、原子写入、revision 冲突、双环境根、typed snapshot、队列满返回原 command 和 shutdown。当时的 Development 默认构建与 `BONGOCAT_BUILD_ENV=production` 构建使用同一代码、不同编译期常量；format、Clippy、test 和 release check 本机通过，三平台 CI 已配置。该环境变量入口已于 2026-09-14 改为默认 Development 与显式 `production` Cargo feature。
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
        独立 overlay，导致错误地关闭模型窗口并报告 overlay/设置窗口双失败；macOS job
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
    - 依赖：`P4-MODEL-IMPORT-OPERATION`、双平台 `rfd`、macOS AppKit sheet、Windows STA。
    - 退出条件：共享 API 区分 selected/cancelled 和稳定无路径错误；macOS 强制 AppKit 主线程，
      Windows 使用专用 STA worker；Rust 重新验证并 canonicalize；GPUI Models 页面不阻塞执行
      文件复制，可消费取消与选择结果；双平台真实选择/取消 smoke 和完整 Native 门禁通过。
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
    - 状态（2026-09-13）：按依赖审计结论将 macOS 私有 adapter 从手写 `NSOpenPanel` 迁移到
      `rfd 0.17.2`（MIT），使用 `default-features = false` 并精确锁定；随后按产品决策放宽
      Windows 选择器的 `FOS_FORCEFILESYSTEM`、`FOS_PATHMUSTEXIST`、`FOS_NOCHANGEDIR`、
      `FOS_DONTADDTORECENT` 要求，Windows 也从自研 `IFileOpenDialog` 迁移到同一 `rfd` 私有
      adapter。macOS adapter 仍要求 AppKit 主线程并强依赖已有 sheet parent，避免 `rfd` 同步
      `runModal` 重入 GPUI；Windows adapter 在专用 worker 的 STA 中调用 `FileDialog`。后台 worker
      负责等待/执行选择、重新验证和 canonicalize。`rfd` 把取消与后端失败统一为 `None`，当前
      按取消映射。macOS 26.5.2 arm64 上 Cancel 与仓库目录 Select smoke 均已通过；完整 workspace
      门禁及 Windows x64 target Clippy 通过，Windows 实机 smoke 仍由对应原生 job 验证。
    - 状态（2026-09-16）：`P4-MODEL-ARCHIVE-SOURCE` 增加压缩包来源后，本项职责由"选目录"扩展为
      "选模型来源"，模块改名 `model_source_picker`，类型改名 `ModelSourcePickerOutcome`/
      `ModelSourcePickerError`，稳定码前缀改为 `model_source_picker_*`（示例同步改名
      `examples/model_source_picker_smoke.rs`）。本项已通过的退出条件与实机证据不变，只换了名字
      与新增一个入口；归档选择器的实机 smoke 尚未执行。
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
    - 当前退出条件：macOS/Windows 的 `tray-icon 0.25.0` 与 `muda 0.20.0` 菜单由明确 owner 管理；
      Open Settings 不创建重复窗口并恢复当前 revisioned snapshot；Quit 停止菜单事件后进入既定
      input/runtime/config/frame/renderer/overlay shutdown；callback 只发送强类型有序事件；双平台
      release smoke、Windows x64/ARM64 source check 与完整 Native 门禁通过。
    - 历史状态（2026-08-31）：初期实现以 macOS 主线程 target/action 与 Windows 隐藏 HWND
      callback 管理菜单和 status item cleanup。该实现已由 ADR-0031 的 `tray-icon` 托盘 owner
      与直接 `muda` 菜单 owner 替换；历史 macOS smoke 及既有 settings/Models release smoke
      仍作为迁移前证据保留。
    - 验收证据（2026-08-31）：commit `9e97704` 的 PR run `33344287629` 全绿；Windows job
      `99345364734` 与 macOS job `99345364649` 均通过原生菜单 callback -> typed action -> settings
      恢复 -> 显式 Quit 的 release smoke，Ubuntu job `99345364707` 通过完整共享 workspace 门禁。
      Windows x64/ARM64 platform Clippy、完整 Native format/Clippy/test/release check 本机通过；
      callback 只入队，菜单 owner 在 input/runtime/config/frame/renderer/overlay 之前停止。
    - 状态（2026-09-13）：overlay 右键已改为通过 overlay session 的真实 Windows HWND / macOS
      content `NSView` 直接调用 `muda::ContextMenu`，不再借用托盘隐藏窗口；macOS 本机 release
      system-menu smoke 通过底层托盘显隐、状态恢复与 shutdown。实机右键弹出、cursor 定位、DPI、
      窗口层级、点击外部关闭和 action 派发仍待 Windows 10 1903+ 与受支持 macOS 复验，ADR-0031
      继续将上述行为列为发布门禁。Windows 右键失败的具体根因是 overlay 的 HTCAPTION 命中测试使
      Windows 发送 WM_NCRBUTTONUP 而非 WM_CONTEXTMENU；现已统一转发两类消息并增加映射单测，
      但仍需在实际 Windows overlay 上完成右键弹出复验。
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
      snapshot 和 error 不包含路径或原始 OS 文本；platform adapter 验证/canonicalize 绝对目录并
      通过 `opener 0.8.5` 交给系统默认程序；成功不推进 revision，失败保留 snapshot 并返回
      稳定匿名错误，recovery-only 可用；Diagnostics 覆盖 pending、键盘和 accessibility 状态；
      platform/app/ui 定向测试、完整 Native workspace、三平台 CI 和双平台 GPUI smoke 通过。
    - 验收证据（2026-08-31）：typed protocol、Application capability、Finder/Explorer adapter、
      Diagnostics 控件与匿名错误/不变 revision/recovery-only 回归已完成；本机 platform 17、ui 23、
      app 33 项定向测试、严格 Clippy、完整 workspace、release/Production 与真实 recovery window
      smoke 通过。commit `6b41808` 的 run `33381198560` 全绿；Windows/macOS/Ubuntu Native jobs
      `99453718576`/`99453718477`/`99453718406` 通过完整门禁，Windows/macOS 分别执行 opener
      参数 contract；Windows input/config job `99453718404`、Windows/macOS GPUI jobs
      `99453718327`/`99453718079` 和 config-store job `99453718598` 同时通过，退出条件满足。
    - 状态（2026-09-13）：目录和外部 URL 的重复平台命令构造与 reaper 已删除；
      `directory_opener` 和 `url_opener` 两个私有 adapter 保留原模块边界，并改用
      `opener 0.8.5` 的 `opener::open`；目录验证/canonicalize、HTTPS 校验、稳定匿名错误和
      settings 失败语义保持不变。依赖启用
      `reveal` feature，预留平台文件管理器的定位选中能力；当前没有 reveal 业务调用点，也未新增公共 API。
41. [x] `P6-BUILD-ENV-METADATA`：让正式构建和打包入口显式固定 Development/Production。
    - 依赖：ADR-0008、正式 app build script、Native workspace/CI 与 macOS packaging baseline。
    - 退出条件：默认 Development 无额外手工配置；Production check/package 显式启用 `production`；
      packaging 在调用 Cargo 前拒绝未知 `--environment` 值并映射 feature；运行时 CLI/env/settings
      不能切换；feature 组合 contract、完整 Native workspace 与三平台 CI 通过。
    - 验收证据（2026-09-14）：`bongocat-app` 以默认 Development、显式 `production` feature
      编译环境，Production 与 `storage-test-injection` 组合在编译期失败；packaging 校验
      `--environment` 后选择 feature，CI 覆盖默认、Production 和拒绝组合。直接
      `cargo check --workspace`、app feature Clippy、workspace tests、release check 和 packaging
      contract 在本机通过。
    - 历史证据（2026-08-31）：严格环境变量解析器、workspace/CI 选择、packaging guard 与本机
      成功/拒绝路径曾通过 commit `2810f4a` 的 pull request run `33383026191`；该变量入口已于
      2026-09-14 由上述 Cargo feature 方案替代。
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
        macOS Development release smoke `cargo run --manifest-path
Cargo.toml --locked -p bongocat-app --release --features storage-test-injection
--target-dir target/storage-test-injection -- --settings-window-state-smoke` 输出
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
      - 状态（2026-09-06）：新增 `runtime_rejects_late_gamepad_generation_after_reconnect` 集成回归，
        覆盖断开后重连、旧代次按钮边沿拒绝、旧代次 axis sample 拒绝、当前代次 axis 保留及
        pressed/model 状态不被迟到事件污染；`bongocat-runtime` 定向测试与严格 Clippy 通过。
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
    - 状态（2026-09-07）：共享 `InputState` 新增跨平台回归，锁定 Reset 后旧 generation 的
      gamepad edge 被拒绝，只有新的连接代次才能重新建立 pressed state；runtime 定向测试通过。
      Windows synthetic reset/reseed 仍需 push CI 复验，物理 controller/profile/热插拔矩阵继续作为
      总项剩余门禁。

46. [x] `P5-SHORTCUT-CONTRACT`：冻结快捷键 chord 的规范化与冲突校验前置契约。
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
        快照。设置页现为每条 command 与 model behavior 提供 Capture 控件，并可单项 Clear：捕获结果仅进入平台无关的
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
      - 状态（2026-09-08）：修复 `open_settings` 请求位只重开窗口而未切换可见性的行为；已有设置
        窗口时现在隐藏，Windows 保留 hidden entity，macOS 关闭当前窗口并由下一次请求重建。新增
        `Meta+O` HID 匹配到 application handoff 的平台回归，`cargo run -p bongocat-app -- --settings-window-smoke`
        在本机 macOS 通过一次隐藏和一次重显。Windows 原生窗口 smoke 与真实快捷键实机证据仍待完成。
    - 验收证据（2026-09-06）：核对正式实现已覆盖本项全部退出条件。GPUI Diagnostics 页面为
      application command 与 current active ready model behavior 提供 Capture/Clear，并提供 Clear all
      与 Restore defaults；Capture 将 key event 归一化为同一 `ShortcutChord` canonical form，
      捕获框持续显示当前按住的任意按键组合；modifier-only、无修饰单键（F1--F12 除外）、unsupported key 与跨域冲突均保持录入状态且不弹出错误。合法组合才提交并自动失焦，成功提交经
      revision-checked typed `SetShortcuts` 原子写入并替换活动 `ShortcutTable`。每一 target 使用稳定
      identity、keyboard tab stop 和 AccessKit capture/clear action。`bongocat-config` 的 canonicalization/
      conflict/HID mapping tests、`bongocat-platform` matcher tests、`bongocat-ui` capture tests 与
      `bongocat-app` persistence/stale-revision/restore tests 共同覆盖该 platform-neutral contract；
      Windows/macOS 真实 global input 是各平台发布矩阵的独立证据，不阻塞本项。
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
    - 当前退出条件：配置值通过强类型 snapshot/command 往返；平台主线程先应用显隐，Application
      owner 再原子提交，平台失败不改配置，配置失败回滚平台状态；macOS/Windows 共用同一个长期存活的
      `tray-icon 0.25.0` 托盘 owner 与直接 `muda 0.20.0` 菜单，`set_visible` 后两平台仍保留唯一
      菜单事件 owner；启动恢复已保存值；General 控件具备 keyboard/AccessKit switch 语义；定向测试、
      完整 Native workspace 与双平台 release system-menu smoke 通过。
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
        AccessKit 语义、中文隔离 smoke 标记和双平台 CI 断言均已实现。当时在 `BONGOCAT_BUILD_ENV=development`
        下运行的 Native workspace `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`、`cargo test --workspace --locked` 和
        `cargo check --workspace --release --locked` 全部通过（该变量入口已于 2026-09-14 改为默认
        Development 与显式 `production` Cargo feature）；UI 定向测试 51 项、Diagnostics
        presentation/localization 回归均通过。macOS Input Monitoring/Accessibility 相关 4 项
        集成测试按设计保持 ignored，真实权限矩阵仍属于平台实机门禁，不影响本项文案闭环。

61. [ ] `P5-JSON-LOCALIZATION-CONSOLIDATION`：将 Native UI 文案统一迁移到 `rust-i18n` JSON 资源。
    - 依赖：`P5-APPLICATION-LANGUAGE`、`P5-GENERAL-LOCALIZATION`、`P5-MODELS-LOCALIZATION`、
      `P5-DIAGNOSTICS-LOCALIZATION`。
    - 状态（2026-09-10）：已完成首批 Native locale 的信息架构重构：`en-US.json` 与 `zh-CN.json`
      使用 `_version: 1` 和 `navigation`、`settings`、`models`、`shortcuts`、`diagnostics`、
      `about`、`actions`、`status`、`errors` 领域层级；静态文案目前在实际 UI、可访问性和 smoke
      消费点直接引用稳定 snake_case 路径，不再维护 `UiText` enum 到 key 的集中映射。动态摘要、错误、
      快捷键冲突和输入指标也统一通过 JSON 占位符资源读取，并加入递归 key/占位符一致性测试。
      `bongocat-i18n` 继续是唯一的 rust-i18n catalog owner，因此保留显式 locale 查询而不在 UI
      crate 重复初始化 `t!`。三种历史前端语言仍不在
      Native v1 支持范围；完整 UI 编译、双平台 smoke 和 CI 语言资源门禁尚未完成，因此不得勾选。

62. [x] `P5-BEHAVIOR-SHORTCUT-TOGGLE`：让当前 v1 的模型行为快捷键开关作用于正式输入链路。
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

63. [x] `P2-KEY-RELEASE-FALLBACK`：让当前 v1 的按键释放兜底超时作用于正式 runtime。
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

64. [x] `P6-REMOVE-STARTUP-CONFIG-FIELD`：从当前 v1 配置移除未被产品消费的登录启动布尔值。
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

65. [x] `P6-REMOVE-DEFERRED-CORNER-RADIUS-FIELD`：从当前 v1 配置移除首发后才实现的窗口圆角字段。
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
    - 状态（2026-09-16，历史）：该决策已被第 78 项 `P0-OVERLAY-CORNER-RADIUS` 推翻。维护者要求
      新版同步支持旧版已有的窗口圆角配置，圆角因此从 `P1 首发后` 上调为 `P0 首发`，字段以
      `overlay.corner_radius_percent` 重新进入当前 v1。此行只保留当时状态。

66. [x] `P6-REMOVE-DEFERRED-HOVER-FIELDS`：从当前 v1 配置移除首发后才实现的指针悬停隐藏字段。
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
    - 状态（2026-09-16，历史）：该决策已被第 79 项 `P0-OVERLAY-HOVER-HIDE` 推翻。维护者要求新版
      同步支持旧版已有的「鼠标移入隐藏 + 悬停延迟」配置，该能力因此从 `P1 首发后` 上调为
      `P0 首发`，两个字段以 `overlay.hide_on_pointer_hover` 与
      `overlay.hide_on_pointer_hover_delay_ms` 重新进入当前 v1（延迟字段的存储单位在同日的第 79 项
      单位修订中由毫秒改为整秒，当前字段名为 `overlay.hide_on_pointer_hover_delay_seconds`）。
      此行只保留当时状态。

67. [x] `P6-KEEP-OVERLAY-IN-WORK-AREA`：让当前 v1 的可见工作区约束作用于正式 overlay。
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
    - 状态（2026-09-18，历史）：该决策已被第 83 项 `P0-OVERLAY-SCREEN-BOUNDS` 取代。维护者要求
      约束范围从工作区改为屏幕范围（允许覆盖任务栏等区域），并把“拖出屏幕后立即拉回”改为延迟
      收敛；`overlay.keep_inside_work_area` 已随语义改名为 `overlay.keep_inside_screen`。本行只
      保留当时状态。

68. [ ] `P7-SIGNED-UPDATE-MANIFEST`：建立首发更新的离线信任判断核心。
    - 依赖：ADR-0021、不可变 Development/Production 环境、四个首发 target、发布版本与公钥流程。
    - 退出条件：平台无关且禁止 unsafe 的 verifier 先验签再严格解析 v1 manifest；拒绝 HTTP、跨环境、
      未知字段、错误 target/arch、无效 SemVer、过大 manifest/artifact、未知或过期 key、sequence 降级；
      只返回项目自有 verified 类型，并对下载流校验精确长度和 SHA-256；共享 Draft 2020-12 schema、
      accept/reject fixture、依赖许可证/来源检查、完整 Native workspace 门禁和三平台 CI 通过。
    - 验收证据（2026-09-05，**已作废**）：`bongocat-update`、ADR-0021、共享 schema/fixture 和稳定错误码
      曾于 commit `a9371f6` 通过 12 项 release 测试与三平台门禁。该实现已于 2026-09-13 随 ADR-0029
      整体删除（`shared/update/`、manifest v1 schema、sequence store、verifier、staging 与下载/安装
      coordinator 均不存在），因此本项从已完成回退为未完成。
    - 状态（2026-09-13）：**本项随 ADR-0029 作废，不再作为交付目标**。更新信任判断改由第三方更新库
      承担：本项目不再解析或校验 manifest，发行元数据从发行页读取，载荷真实性由归档签名校验。
      ADR-0021 与 ADR-0025 已标记「已被 ADR-0029 取代」。被放弃的能力
      （单调 `release_sequence` 防降级、manifest 未知字段拒绝与 1 MiB 上限、公钥轮换窗、HTTPS-only /
      禁 redirect / 15s deadline / 禁透明压缩、32 个稳定错误码收敛为 13 个）已逐项记入 ADR-0029 的
      损失表，本项目当前**没有**替代实现。若后续要求恢复其中任一项，须新建 ADR 并重新立项，
      不得把本项直接勾选。
    - 状态（2026-09-14）：库与签名方案已由 ADR-0034 再换一次（`cargo-packager-updater` + detached
      minisign）。结论不变，并**新增两项损失**：独立 SHA-256 完整性校验与 per-platform 资产匹配
      均无替代，对应错误码已无产出路径；公钥轮换能力比 ADR-0029 时更弱（单公钥，无 any-of）。

69. [x] `P7-AUTOMATIC-UPDATE-PREFERENCE`：让当前 v1 自动检查更新偏好进入正式设置链路。
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

70. [x] `P7-AUTOMATIC-UPDATE-SCHEDULE`：冻结自动检查的单调 24 小时调度契约。
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

71. [x] `P9-BLOCK-LEGACY-AUTO-RELEASE`：阻止 Native Rewrite 开发期间由 tag 自动发布历史 App。
    - 依赖：Phase 0 发布门禁、历史源码保留规则与尚未完成的 Native 签名/安装流水线。
    - 退出条件：历史 Tauri release workflow 保留用于考古和回滚，但只允许显式手动触发；任何
      `v*` tag 都不再自动构建或发布旧 Tauri、Linux 或 i686 artifact；不得据此声称 Native App
      已可发布，新的双平台签名、安装和更新流水线仍由 Phase 9 跟踪。
    - 验收证据（2026-09-05）：`.github/workflows/release.yml` 已移除 `push.tags`，名称明确标注为
      `Legacy BongoCat Release (manual only)`；`.github/workflows/upgradelink.yml` 也已移除 release
      event，只允许显式手动上传旧 Tauri update metadata。历史 job/matrix 未删除；YAML 语法与 staged
      whitespace 检查通过。Native release workflow 尚未建立，因此 Phase 9 发布准备保持未完成。
    - 状态（2026-09-07）：`test_native_release_target_matrix.py` 现同时锁定 legacy workflow 的
      `workflow_dispatch` 手动触发和无 `push`/`tags` 自动发布入口，并继续保留历史 Linux 矩阵供考古，
      不将其误判为 Native 首发产物。

72. [x] `P9-NATIVE-PRODUCT-ICON`：让双平台 Native 应用与 Windows 托盘使用正式产品图标。
    - 依赖：正式 `bongocat-app` build script、macOS `.app` 打包入口与 Windows system-menu owner。
    - 退出条件：Native workspace 自有并校验 `.icns/.ico` 容器；macOS bundle 声明、复制并签名封装
      `BongoCat.icns`；Windows executable 编译至少一个 icon group，托盘从当前 module 加载同一固定
      资源且不回退通用图标；完整 Native workspace、依赖策略、macOS Production package 与三平台
      CI 通过。
    - 当前契约（2026-09-13）：Native product `.icns/.ico` 仍由 build script/测试验证并进入对应
      bundle/executable；Windows tray 改用 Native 自有 `resources/icons/tray-windows.png`，其
      容器、尺寸、RGBA 由 `product_icon_contract` 测试和 build script 固定，字节 hash 与来源记录在
      ADR-0031，Windows RC 只保留 product icon，不再嵌入 tray ICO。macOS tray 继续使用
      `tray-macos.png`。
    - 历史验收证据（2026-09-05）：实现、文档和 CI 产物断言已接入；本地 icon container 测试、
      format、严格 release workspace Clippy、完整 release all-target tests、release check、dependency
      policy、Windows x64/ARM64 platform Clippy 与 macOS Production `.app` 打包/资源逐字节比较/
      strict codesign 均通过。commit `300f470` 的 CI run `33945105437` 全部 23 个 job 通过；
      Windows job `101249657297` 从真实 `bongocat-app.exe` 提取到 product icon group，macOS job
      `101249657241` 验证 Production `.app` 中的图标字节、bundle metadata 与 strict codesign。

73. [x] `P4-MODEL-LIBRARY-METADATA`：多模型导入去单模型限制并建立标题元数据与启动回退。
    - 依赖：正式 `ModelStore` 多模型目录、v1 配置 store、settings service 导入/删除长操作契约。
    - 退出条件：同一环境可连续导入多个自定义模型且互不覆盖；导入 ID 由 service 分配并与标题
      解耦；元数据（id + title）进入 v1 配置并在删除时同步；启动恢复选中模型，缺失/损坏时回退
      `standard` 预置并持久化修正；全部失败路径不阻塞启动。
    - 当前契约（2026-09-15）：`ModelStore::allocate_unique_id` 以建议值 + `-2`/`-3` 后缀分配
      唯一可移植 ID（非法建议回退 `custom-model`），`import` 底层 `AlreadyExists` 防覆盖语义保留
      作为并发兜底；v1 配置新增 `model.installed_models`（`deny_unknown_fields`，id 唯一、title
      非空 ≤128 字符），schema、default fixture、两个新 reject fixture 与 `native-config-contract.md`
      已同步；导入成功以来源文件夹名为默认 title 登记、超长/缺失降级为模型 ID，元数据提交失败按
      导入失败报告且已安装目录保留；`delete_model` 同步移除记录；`Application::restore_startup_model`
      在启动时激活配置选择、缺失/损坏时记录匿名 `model_selection_fallback` 事件并持久化
      `(Preset, standard)`，未配置选择默认激活 standard，operational 启动还清理指向不存在目录的
      元数据记录；`SettingsModelEntry.title` 投影到 Models 页，无记录条目回退显示 ID。
    - 验收证据（2026-09-15）：`bongocat-config` 48 测试（含两个新 reject fixture 的 manifest 契约）、
      `bongocat-model` 46 测试（含 4 个 `allocate_unique_id` 测试：建议直用/占用后缀/非法回退/超长
      截断）、`bongocat-app` 118 测试（新增导入分配唯一 ID 并登记标题、启动回退持久化 standard、
      启动清理缺失目录元数据）与 `bongocat-ui` 112 测试全部通过；`cargo fmt --check` 与三组 clippy
      `-D warnings` 门禁通过；`cargo check --locked --workspace --release` 通过。跨平台重启恢复的
      实机 smoke（macOS/Windows 产品入口）仍属既有实机门禁，未在本任务运行。

74. [x] `P4-CDI3-COMBINED-PARAMETERS`：修复第三方模型被误判「模型包无效」的回归，并让导入
       建议显示所选目录名。
    - 依赖：`P4-MODEL-LIBRARY-METADATA` 建立的 service 端唯一 ID 分配与标题元数据。
    - 退出条件：官方 Cubism cdi3.json 字段 `CombinedParameters` 不再导致整个模型包被判无效；
      含 `CombinedParameters` 的真实第三方模型（送葬人 · 标准模式）可完成导入；选择目录后
      导入输入框显示实际目录名而非固定 `custom-model` 兜底。
    - 根因（2026-09-15 用户报告）：`RawDisplayInfo` 使用 `deny_unknown_fields`，但未声明
      Cubism 5 官方 cdi3 字段 `CombinedParameters`，第三方模型的 `DisplayInfo` sidecar 解析
      失败并让整个包被拒（`model_resource_invalid`），与重构前（不解析 cdi3）行为形成回归。
    - 当前契约（2026-09-15）：`RawDisplayInfo` 接受 `CombinedParameters: [[parameter id, ...], ...]`；
      校验要求每个组合非空、id 非空白且引用已声明的 Parameter，否则按既有
      `model_resource_invalid` 拒绝。目录选择的导入建议改为来源文件夹原名
      （`suggested_model_title`），手输仍经 `sanitize_model_id_input` 过滤为可移植 ASCII；
      最终存储 ID 由 service 端 `allocate_unique_id` 分配，标题元数据取来源文件夹名，显示
      名称与存储身份彻底解耦。
    - 验收证据（2026-09-15）：真实模型目录经产品校验器由 `model_resource_invalid
      (demomodel.cdi3.json): unknown field CombinedParameters` 变为通过；model fixture 契约
      新增 `combined-parameters-accepted`（accept）与 `combined-parameters-invalid`
      （reject，`model_resource_invalid`）两用例；`bongocat-model` 46 测试、`bongocat-ui` 112
      测试（建议显示目录名 4 断言）、`bongocat-app` 118 回归测试、fmt 与
      model/ui clippy `-D warnings` 全部通过。

75. [x] `P4-MODEL-ID-UUID`：已安装模型 ID 改为 UUID v4 生成，导入输入框改为可编辑标题。
    - 依赖：`P4-MODEL-LIBRARY-METADATA`、`P4-CDI3-COMBINED-PARAMETERS` 建立的标题元数据与
      service 端 ID 分配。
    - 退出条件：导入不再从文件夹名或用户输入派生存储 ID；ID 由成熟库生成的 UUID v4 承担，
      与 title 完全解耦；原「建议值 + 后缀」路径及其测试全部替换。
    - 依赖评估（2026-09-15，§9）：`uuid 1.26.1`（精确 pin，`std + v4` features，v4 经
      `getrandom 0.4.3` 取系统熵）。标准库无 RNG、手写跨平台熵读取不符合架构边界；`uuid`
      是 Rust 生态事实标准（Apache-2.0 OR MIT、多维护者、rust-version 1.85 低于项目
      1.97、纯 Rust 无平台 FFI），停止维护时可直接替换为 `rand` 生成 16 字节后手工格式化，
      替换边界收敛在 `ModelStore::allocate_unique_id` 单函数内。
    - 当前契约（2026-09-15）：`ModelStore::allocate_unique_id()` 在 store writer lock 内
      生成 UUID v4 并检查目录占用（碰撞概率工程上为零，重试上限 16 次）；ID 仍是
      `ModelId` 可移植 store key（36 字符连字符形式天然通过校验），并以 ID 作为安装目录名。
      导入 command 的字段更名为 `title`：UI 输入框语义改为「模型名称」（默认来源文件夹名，
      手输经 `sanitize_model_title_input` 过滤控制字符并截断 128 字符），service 以该值
      作为元数据标题（空白降级为来源文件夹名再到 ID）。`FALLBACK_MODEL_ID` 常量删除；
      `(origin, model_id)` 身份与预置/已安装同 ID 共存契约改由直接播种 store 目录的测试覆盖。
    - 验收证据（2026-09-15）：`bongocat-model` 43 测试（allocate 测试改为 32 次 UUID 唯一性
      与可移植性断言）、`bongocat-app` 118+21 测试（导入/删除/重启/环境隔离测试全部改为
      从导入返回值捕获 UUID id，同 ID 共存场景用 `seed_installed_model` 直接播种）、
      `bongocat-ui` 112 测试、`bongocat-config` 48 测试全部通过；fmt 与三组 clippy
      `-D warnings` 通过；真机 `--run-seconds 6` 完整运行干净退出。

76. [x] `P4-MODEL-CATALOG-DEGRADE`：修复单个非模型条目使整个已安装模型目录不可用的缺陷。
    - 依赖：`P4-MODEL-CATALOG` 建立的来源感知合并目录与 `P4-MODEL-ID-UUID` 建立的 UUID 目录名。
    - 退出条件：store 根目录出现文件管理器或系统元数据时不使整表失败；其余非自有条目被跳过
      且不影响可见模型；跳过数量进入 settings snapshot 并由 Models 页面呈现；符号链接始终
      不被跟随；缺失模型的元数据裁剪恢复正常。
    - 根因（2026-09-15 用户报告）：用户在 Finder 里手动删除已导入模型 `f93ca918-…` 后，Models
      页面整表显示「模型列表不可用」。`ModelStore::list` 对任何非目录条目（Finder 在 store
      根目录留下的 `.DS_Store`）返回 `StoreEntryUnsupported`，`Application::model_catalog`
      以 `?` 上抛，`settings_model_catalog` 于是清空全部条目并把整个 catalog 标为不可用。
      同一早退还让 `prune_missing_installed_metadata` 直接返回，被删模型的 `installed_models`
      元数据记录永远无法清理。
    - 当前契约（2026-09-15）：`ModelStore::list` 返回 `InstalledModelCatalog { entries,
      skipped_entries }`，只在 store 根目录不可读或 writer lock 竞争时失败；`.DS_Store`、
      `.localized`、`Thumbs.db`、`desktop.ini` 与 AppleDouble `._*` 按文件名忽略且不计数；
      其余非自有条目（非常规目录、符号链接、非 UTF-8 名称、非法 `model_id` 目录名）静默跳过
      并计数；带合法 `model_id` 但包校验失败的目录继续签发 `Invalid` 条目保持可见。
      过滤完全在 store 内部完成：`skipped_entries` 是 store 的自有扫描结果，不进入 settings
      snapshot、不产生用户可见文案与无障碍输出，`Application::model_catalog` 仍只返回模型
      条目集合，用户只看到可用模型；符号链接始终不被跟随。
    - 验收证据（2026-09-15）：修复前新增回归测试复现 `ModelStoreError { code:
      StoreEntryUnsupported, detail: "model store contains an entry not owned by the catalog" }`。
      修复后 `bongocat-model` 45 测试（`.DS_Store` 不使整表失败、外部文件与非法 ID 目录被跳过
      并计数 2、合法 ID 空目录仍为 `Invalid`、symlink 不跟随且 `skipped_entries == 1`）、
      `bongocat-app` 119 测试（新增端到端用例：手动删除模型 + `.DS_Store` 时合并目录仍返回
      `standard` 预置、过期元数据被裁剪）、`bongocat-ui` 112 测试、`bongocat-i18n` 4 测试通过；
      `cargo fmt --check` 与 workspace 测试通过。另以用户真实 `development/models/` 目录副本
      （含 `.DS_Store` 与存活的 `9e9f5a59-…`）只读验证，扫描结果为 1 条 installed 条目、
      `skipped_entries == 0`。
      真机门禁：对用户真实 Development 数据目录运行 `--run-seconds 6 --models-page-smoke`
      （该 smoke 断言 `model_catalog.error.is_none()`、条目非空、active 模型在 catalog 中、
      行操作权限与本地化），退出码 0 且无 `product run failed` 输出；同一次运行后
      `config.json` 中已手动删除模型的 `installed_models` 记录被裁剪，只剩存活模型记录。
      反向对照：以 `flock` 持有 `models.writer.lock` 后同一 smoke 以
      `ModelStoreError { code: StoreBusy }` 退出码 1 失败，证明真正的 store 失败仍然失败关闭、
      smoke 结果非空转。后续按用户要求把跳过条目改为纯内部过滤，移除 Models 页面状态行、无障碍
      status 节点分支与 `models.catalog.skipped_entries` 文案（两个 locale 均已删除），
      `SettingsModelCatalog.skipped_entries` 与 `Application::ModelCatalog` 一并撤销。
      未运行：Windows 路径与 Windows/macOS CI 门禁。

77. [x] `P4-MODEL-ARCHIVE-SOURCE`：让模型来源同时支持文件夹与 `.zip` 压缩包。
    - 依赖：`P4-MODEL-IMPORT-COMMAND`/`P4-MODEL-IMPORT-OPERATION`、`P7-MODEL-DIRECTORY-PICKER`
      建立的来源选择面、`ModelStore` staging 事务与 `ModelPackageLimits`。
    - 退出条件：来源类型按内容识别（目录 / zip 签名），不看扩展名、也不靠调用方标志；压缩包解压
      写进 store 自己的 staging 目录，仍共用同一个 `PreparedModel` 校验与原子 rename 提交；归档在
      **解压前**按中央目录校验条目名/条目类型/压缩方法/加密标志/条目数/深度/声明字节，容器另有
      独立字节上限；包装目录被剥离；解压计入既有 copy stage；任何拒绝都不留 staging 或目标；
      文件夹与压缩包两个来源入口、状态文案与 AccessKit 节点齐备；完整 Native 门禁通过。
    - 决策记录：ADR-0036。
    - 当前契约（2026-09-16）：`ModelStore::import_with_observer` 先 `detect_source_kind`
      （目录 → 就地读取；常规文件且以 `PK\x03\x04`/`PK\x05\x06`/`PK\x07\x08` 开头 → 压缩包；其余
      存续文件 → `SourceArchiveUnsupported`），再 `plan_archive`（只读中央目录，不解压）或
      `PreparedModel::prepare`，之后才创建 staging；压缩包走 `extract_archive`（`create_new` 写入
      + 逐条目复核名字/声明大小/实际字节数，CRC32 由 `zip` 默认校验），随后与目录来源一样
      `PreparedModel::prepare(staging)` + `rename` 提交。
      `ModelPackageLimits` 新增 `maximum_archive_bytes`（默认 1 GiB），与包字节上限分开，在解析
      中央目录前挡住输入；条目数另有 `maximum_file_count × 4` 的结构上限。
      `ModelStoreDiagnostic` 新增 `SourceArchiveUnsupported`
      （`model_store_source_archive_unsupported`），`ALL` 由 11 增至 12；它映射到既有
      `SettingsErrorCode::ModelImportSourceUnsupported`，因此没有新增用户可见错误码；路径穿越
      / 重复条目 / 文件目录冲突复用 `SourceEntryUnsupported`，符号链接复用
      `SourceSymlinkUnsupported`，超限与声明不符复用 `SourceChanged`。
      包装目录反复剥离直到剩余条目不再共享同一首段；规则收窄为"任一**文件**位于归档根即不剥离、
      等于该首段的**目录**条目不取消剥离"（后者是必要条件，否则真实"压缩文件夹"产物永远无法识别）。
      `__MACOSX/**` 与 `._*`/`.DS_Store`/`.localized`/`Thumbs.db`/`desktop.ini` 按文件名丢弃，
      既不参与包装判定也不解压。
      `bongocat-platform` 的 `directory_picker` 模块改名 `model_source_picker`，
      `DirectoryPickerOutcome`/`DirectoryPickerError` 改名 `ModelSourcePickerOutcome`/
      `ModelSourcePickerError`，稳定码前缀改为 `model_source_picker_*`，新增 `pick_model_archive`
      （`pick_file` + `.zip` 便利过滤，只要求"绝对路径 + 常规文件"），示例改名
      `examples/model_source_picker_smoke.rs` 并支持 `--kind directory|archive`。
      UI 的 Models 页面新增「选择压缩包」按钮、tab index 22 与
      `ACCESSIBILITY_MODEL_CHOOSE_ARCHIVE`（节点 50），导入按钮移到 23；归档状态文案落在
      `models.import.archive.*`，与来源无关的三条挑选文案上移到 `models.import.picker.*`。
      建议标题规则统一在 `bongocat_ui::model_source_display_name`（归档去掉 `.zip`，目录保留原名），
      UI 预填与 service 兜底共用它。
    - 依赖评估（2026-09-16，§9）：`zip =8.6.0`（已在 workspace 依赖中，供诊断包写归档；本次打开
      `deflate-flate2`，MIT）+ `flate2 =1.1.10`（显式后端 `rust_backend`/miniz_oxide，纯 Rust）。
      两者均为当次核对的 crates.io 最新非 yanked 稳定版；不给 `bongocat-model` 打开 AES/bzip2/
      zstd/lzma/ppmd/deflate64，未开启的压缩方法以稳定诊断拒绝。理由与替换边界见 ADR-0036 §9。
    - 验收证据（2026-09-16）：`bongocat-model` 60 测试（新增 15：`store.rs` 10 + `archive.rs` 5），
      新增用例覆盖"同一包以目录与压缩包两种来源导入后 `ModelPackageIndex` 逐字段相等"、按内容
      识别（无扩展名归档、`Stored` 归档、名为 `*.zip` 的目录）、嵌套包装剥离与深度上限、
      `__MACOSX`/`.DS_Store` 被丢弃且不干扰包装识别、非归档/截断/空/只有目录的归档、路径穿越
      （`../`、绝对、平台前缀）与重复条目（`猫//model.moc3` 与 `猫/model.moc3` 归一化后相撞）与
      文件目录冲突、符号链接条目、四类上限在解压前拒绝、进度单调 + 取消后无 staging 残留且不影响
      已装模型。`bongocat-app` 120+21 测试（新增文件夹与压缩包双来源导入：两个 UUID id、
      索引一致、目录侧标题 `非 ASCII 模型`、归档侧标题为归档名去掉 `.zip`），`bongocat-ui` 115
      测试（新增标题 UTF-8 边界/大小写/目录名为 `*.zip` 保留原名、归档状态文案、两个按钮的
      AccessKit 节点与禁用传播），`bongocat-platform` 50 测试（新增归档选择复验与稳定码唯一性）。
      **真实压缩包验证**：`BONGOCAT_MODEL_ARCHIVE_SAMPLES=/tmp/bongocat-samples cargo test
      -p bongocat-model imports_the_archive_samples` 通过，样本即用户提供的两个真实归档：
      `经典小键盘 · 标准模式.zip` → 入口 `cat.model3.json`、moc `demomodel.moc3`、3 张 1024x512
      纹理、cdi3、3 个表情、2 组动作（各 2 条，1 条带 FLAC 音轨）、31 文件 / 1 218 791 字节；
      `送葬人 · 标准模式.zip`（内部目录名为 `图弟 · 标准模式`，与归档名**不一致**）→ 入口
      `demomodel.model3.json`、1 张 1024x512 纹理、cdi3、61 文件 / 791 595 字节。两者包装目录均被
      剥离（安装根目录下直接是 `.model3.json`），源归档未被修改。`cargo fmt --all --check`、
      三组 clippy（workspace `--all-targets --all-features` 与 `bongocat-app` 的
      `storage-test-injection`/`production`）、`cargo test --locked --workspace`、
      `cargo check --locked --workspace --release` 全部通过。
      按 §9 执行了完整 `cargo update`，只升了 4 个与本功能无关的传递依赖补丁版本
      （`synstructure` 0.13.2 → 0.14.0，连带切到 `syn 3`；`yoke-derive` 0.8.2 → 0.8.3；
      `zerofrom-derive` 0.1.7 → 0.1.8；`zlib-rs` 0.6.7 → 0.6.8）。
      **顺带修复的既有缺陷**：`bongocat-platform` 的示例 `model_source_picker_smoke` 在 `next`
      上本来就无法编译（已用 `git stash` 在 HEAD 上复现 `objc2_app_kit::NSBackingStoreType`
      未解析），因为 `NSBackingStoreType` 属于未被打开的 `NSGraphics` feature；本次因需要修改该
      示例而补上 `"NSGraphics"`。这是与本功能无关的既有缺陷，需要单独复核。
      **未运行**：Windows 实机（无 Windows 机器）、UI 真实点击两个按钮与真实
      `NSOpenPanel` 选 zip 的手工 smoke（需人工运行示例）。

78. [x] `P0-OVERLAY-CORNER-RADIUS`：让当前 v1 支持旧版已有的窗口圆角配置，并作用于正式 overlay。
    - 依赖：`P3-*` 建立的 macOS Metal / Windows D3D11 renderer 与 `Uniforms` alpha 路径、
      `P2-*` 建立的 `OverlaySettings` 与强类型 command/snapshot 链路、`P5-*` 建立的 overlay
      设置页与 debounced patch 机制、`P6-*` 建立的当前 v1 schema 边界。
    - 退出条件：`overlay.corner_radius_percent` 作为当前 v1 字段在 Rust config、JSON Schema、
      默认 fixture 与共享 manifest 中一致存在；取值范围 `0..=50`、默认 `0`，越界在 config
      `validate()`、runtime `is_valid()`、JSON Schema 与两个 renderer 的 options 校验四处都失败
      关闭；该值经强类型 command/snapshot 往返并在重启后恢复；GPUI overlay 设置页有可编辑数字
      输入与中英文案；两个平台在同一片元着色器路径上按 drawable 像素位置裁剪；改变该值触发原生
      窗口重建；不引入旧版兼容、migration、alias 或 fallback。
    - 决策记录（2026-09-16）：本项由维护者要求直接决定，推翻第 65 项
      `P6-REMOVE-DEFERRED-CORNER-RADIUS-FIELD`，并把行为清单「主窗口/圆角」由 `P1 首发后`
      上调为 `P0 首发`。不新增 ADR：该配置不改变架构边界，只是 overlay 窗口的既有渲染属性。
    - 旧版契约（`pre-refactor` 分支考古）：`src/stores/cat.ts` 的 `window.radius: number`
      默认 `0`；`src/pages/preference/components/cat/index.vue` 的 `<InputNumber :min="0">`
      只有下界、无上界；`src/pages/main/index.vue` 主窗口内容容器
      `:style="{ borderRadius: `${catStore.window.radius}%` }"` 配合 `overflow-hidden`。
      CSS 百分比 `border-radius` 是椭圆：`N%` 表示水平半轴为窗宽 `N%`、垂直半轴为窗高 `N%`；
      `50%` 时四角弧线相接得到窗口内切椭圆，更大值被 CSS 缩放回同一椭圆。首版据此把范围收敛为
      `0..=50`（对旧版"无上界"输入的收窄，属首版契约决定），`0` 保持直角。
    - 当前契约（2026-09-16）：`OverlayConfig::corner_radius_percent: u8`
      （`crates/bongocat-config/src/lib.rs:264`），`NativeConfig::default()` 为 `0`（:918），
      `validate()` 以 `!(0..=50).contains(..)` 拒绝越界（:953）；共享 schema
      `shared/config/config.schema.json` 的 `overlay.required` 与
      `{"type":"integer","minimum":0,"maximum":50}` 同步。
      `OverlaySettings::corner_radius_percent` 与 `is_valid()` 的 `<= 50` 约束
      （`bongocat-runtime`）、`OverlaySessionOptions::corner_radius_percent`
      （`crates/bongocat-overlay/src/lib.rs:83`）与 `requires_window_recreation`（:109）三处同构。
      行号注（2026-09-16）：第 79 项在 `crates/bongocat-config/src/lib.rs` 的同一结构体/函数内、
      以及 `crates/bongocat-overlay/src/lib.rs` 的 `OverlaySessionOptions` 内插入悬停字段，因此
      本条目的行号已整体下移：config 的 `NativeConfig::default()` 原 :918 → 现 :943、
      `validate()` 原 :953 → 现 :980（`OverlayConfig::corner_radius_percent` 仍为 :264）；overlay 的
      `OverlaySessionOptions::corner_radius_percent` 原 :83 → 现 :89、
      `requires_window_recreation` 原 :109 → 现 :124。此处保留写入当时的行号，第 79 项记录了当前行号。
      共享 helper `MAXIMUM_CORNER_RADIUS_PERCENT = 50`（:44）与
      `corner_radius_uniform(percent, width, height) -> [f32; 4]`（:55）把百分比换算为
      `(radius_fraction, drawable_width, drawable_height, 0)`。
      两个 renderer 的 `Uniforms` 增加 `float4 corner_radius`（Rust 侧 `corner_radius: [f32; 4]`，
      结构体 96 字节，布局由断言锁定），片元着色器新增 `corner_coverage`：在归一化 uv 上求内切
      圆角矩形的有符号距离并转成 0..1 coverage，乘进既有
      `texture_color.a * opacity * mask` 路径。background、mesh（Live2D）与 key 三处绘制使用真实
      圆角，clipping mask pass 显式传 `[0.0; 4]`，避免圆角削弱 mask 覆盖。
      macOS 侧 `crates/bongocat-overlay/src/macos.rs`（shader :93-147、uniform 计算 :1493、
      绘制 :1527/:1560/:1602/:1646），Windows 侧 `crates/bongocat-overlay/src/windows.rs`
      （shader :235-281、:752、:879/:904/:964/:985）；Windows 的 `Renderer::create`/`create_inner`
      签名由 `opacity_percent: u8` 改为 `options: OverlaySessionOptions`。
      UI：`SettingsOverlay::corner_radius_percent`（`bongocat-ui`）、
      `SettingValue::OverlayCornerRadius` 与 `PendingOperation::OverlayCornerRadius`、
      `set_overlay_corner_radius_value`（`crates/bongocat-ui/src/window/settings.rs:326`，
      `raw.round().clamp(0.0, 50.0)`）、overlay 设置页数字输入
      `NumberFieldOptions { min: 0.0, max: 50.0, step: 5.0 }`
      （`crates/bongocat-ui/src/window/render.rs:485-518`）与
      `settings.overlay.corner_radius.{label,description}` 中英文案
      （`bongocat-i18n/locales/{zh-CN,en-US}.json`）。
      共享 fixture：`accept-corner-radius.json`（值 25，accept）与 `invalid-corner-radius.json`
      （值 51，reject）取代原 `invalid-deferred-corner-radius.json`。
    - 验收证据（2026-09-16）：`cargo test --locked --workspace` 退出码 0，共 651 passed /
      0 failed / 5 ignored（含 config 120、runtime 40、overlay 21+10、ui 115、app 120+21、
      i18n 4 等）；`cargo check --locked --workspace --release` 通过（5.88s，仅有与本次无关的
      `block v0.1.6` future-incompat 警告）；`cargo fmt --all --check`、三组 clippy
      （按 CI 逐字命令：`--workspace --all-targets --all-features --exclude bongocat-app`、
      `-p bongocat-app --all-targets --features storage-test-injection`、
      `-p bongocat-app --all-targets --features production`，均 `-D warnings`）通过；`tools/validate-json-schema.py` 输出
      `validated 9 input, 9 expected, and 13 config, 6 state fixture(s)`，其中
      `corner-radius (accept)` 与 `corner-radius-out-of-range (reject)` 均符合预期；
      `tools/validate-locales.py` 输出 `validated 2 locale(s), 374 key(s) each`；
      `tools/validate-fixtures.py`（9 input fixture + 8 model package case）、
      `tools/run-input-fixtures.py`（9 input fixture）、`tools/tests` 契约测试 63 项与
      `tools.tests.test_native_release_target_matrix` 4 项、`git diff --check` 均通过。
      Metal 着色器以 `xcrun -sdk macosx metal -std=metal3.0 -c` 离线编译通过（退出码 0）。
      **未运行**：Windows HLSL 编译与 Windows 实机渲染（本机无 Windows、无 `dxc`/`fxc`，且
      `naga` 不提供 HLSL frontend，离线校验路径不可用）；macOS 实机圆角视觉 smoke（需人工
      运行产品观察边缘抗锯齿与多层重叠）；双平台 CI 门禁。因此 Windows 侧 `corner_coverage`
      只经过与 Metal 逐行同构的代码审查，未经过任何编译或运行验证。

79. [x] `P0-OVERLAY-HOVER-HIDE`：让当前 v1 支持旧版已有的鼠标悬停隐藏与悬停延迟，并作用于正式 overlay。
    - 依赖：第 78 项建立的 overlay 呈现参数与窗口重建路径、`P2-*` 建立的 `OverlaySettings` 与强类型
      command/snapshot 链路、`P2-CURSOR-SMOOTHING` 建立的 `RuntimeSnapshot.cursor.sample` 与
      `PlatformInputServiceStatus`、`P5-*` 建立的 overlay 设置页与 debounced patch 机制、`P6-*`
      建立的当前 v1 schema 边界。
    - 退出条件：`overlay.hide_on_pointer_hover` 与 `overlay.hide_on_pointer_hover_delay_seconds` 作为
      当前 v1 字段在 Rust config、JSON Schema、默认 fixture 与共享 manifest 中一致存在；延迟范围
      `0..=60` 秒、默认 `0`，开关默认 `false`，越界在 config `validate()`、runtime `is_valid()`、
      JSON Schema 与两个平台的 options 校验四处都失败关闭；两值经强类型 command/snapshot 往返并在
      重启后恢复；开启后指针停留满延迟即淡出并强制穿透，离开后按同样时长淡回并恢复
      `overlay.click_through`，窗口本身不隐藏、不销毁，`overlay.visible` 不受影响；开关与延迟在
      frame tick 内原地生效，不触发原生窗口重建；指针采样缺失或平台输入服务未运行时按「不在窗口
      内」处理；GPUI overlay 设置页有可编辑开关与数字输入、中英文案与 AX/UIA 语义；不引入旧版
      兼容、migration、alias 或 fallback。
    - 决策记录（2026-09-16）：本项由维护者要求直接决定，推翻第 66 项
      `P6-REMOVE-DEFERRED-HOVER-FIELDS`，并把行为清单「主窗口/hover 延迟隐藏」由 `P1 首发后`
      上调为 `P0 首发`。不新增 ADR：该配置不改变架构边界，只是 overlay 窗口的呈现属性。三项首版
      契约决定经维护者确认：延迟上界收窄为 `0..=60000` 毫秒；淡入淡出复刻旧版 300ms 过渡；延迟
      输入控件始终显示（旧版只在开关开启时展开）。
    - 决策记录（2026-09-16，单位修订）：维护者要求把悬停隐藏延迟从毫秒改成整秒，理由是秒更容易
      操作。这推翻本项同日「延迟上界收窄为 `0..=60000` 毫秒」的契约决定，字段改名为
      `overlay.hide_on_pointer_hover_delay_seconds`，范围 `0..=60` 秒、步进 1 秒，与旧版 UI 的
      整秒单位一致；`60000` 毫秒的上界在数值上等于 `60` 秒，因此本次只改单位与名字，不改语义
      边界。不新增 ADR：同上，不改变架构边界。`next` 仍是全新首版，按 §4.1 直接改当前 v1 schema、
      默认值、fixture 与实现，不引入迁移、alias 或兼容分支；overlay frame loop 与两个平台的
      options 继续以毫秒计时，秒到毫秒的换算集中在 `bongocat_runtime::hover_hide_delay_ms` 一处，
      平台源码在本次修订中未改动。设置页数字输入的步进由 250 毫秒改为 1 秒，AX value 增加 `s`
      单位后缀，中英文案与共享契约文档同步。
    - 旧版契约（`pre-refactor` 分支考古）：`src/stores/cat.ts` 的 `window.hideOnHover: boolean`
      默认 `false`、`window.hideOnHoverDelay: number` 默认 `0`；
      `src/pages/preference/components/cat/index.vue` 的 `<Switch>` 与
      `<InputNumber :min="0">`（只有下界、无上界，单位为秒，`SpaceAddon` 显示 `s`），延迟控件仅在
      开关开启时展开（`w-28 opacity-100` / `w-0 opacity-0`）。
      `src/composables/useDevice.ts` 的 `onHideOnHover`：先取 `appStore.windowState[MAIN]` 的
      `x/y/width/height`，缺失即返回；用 `inBetween`（两端闭区间）判断指针是否在窗口内；`isInWindow`
      与上一次相同时直接返回（边沿触发）；进入时 `setTimeout(delay * 1000)` 后把
      `document.body.style.opacity` 设为 `'0'` 并 `appWindow.setIgnoreCursorEvents(true)`；离开时
      立即把 opacity 设为 `'unset'` 并 `setIgnoreCursorEvents(catStore.window.passThrough)`。
      `src/assets/css/global.scss` 的 `body` 带 `transition-opacity-300`，即 300ms CSS 过渡。
    - 首版对旧版的收窄与修正（均为契约决定，不是遗留行为）：
      1) 延迟上界收窄为 `60` 秒。旧版 UI 无上界，`hideOnHoverDelay * 1000` 可给出任意长的
         `setTimeout`；首版保留旧版 UI 的整秒单位并显式限界。存储单位与设置页输入单位一致，
         overlay frame loop 仍按毫秒计时，秒到毫秒只在 overlay options 边界换算一次。
      2) 恢复也走同一延迟。旧版只有「隐藏」经过 `setTimeout`，「恢复」在离开时立即生效；首版两侧
         对称，理由是旧版的不对称无法从代码意图解释，且会让快速划过的指针产生闪烁。
      3) 命中测试改为半开区间（`x >= left && x < left + width`，纵向同理）。旧版 `inBetween` 两端
         闭区间，相邻显示器共享的边界像素会同时落在两个窗口内。
      4) 旧版的降级路径不复刻：`windowState` 缺失、负 `winX`/`winY` 或丢失离开事件在旧版都会让内容
         停在全透明；首版把「无指针采样」和「平台输入服务未运行」都当作不在窗口内。
    - 当前契约（2026-09-16，含同日单位修订）：`OverlayConfig::hide_on_pointer_hover: bool`
      （`crates/bongocat-config/src/lib.rs:272`）与 `hide_on_pointer_hover_delay_seconds: u32`（:285），
      共享常量 `MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS = 60`（:293），`NativeConfig::default()`
      为 `false` / `0`（:946-947），`validate()` 以 `>` 比较拒绝越界（:985-991）；共享 schema
      `shared/config/config.schema.json` 的 `overlay.required` 与
      `{"type":"integer","minimum":0,"maximum":60}` 同步。
      `OverlaySettings::hide_on_pointer_hover` / `hide_on_pointer_hover_delay_seconds`
      （`crates/bongocat-runtime/src/lib.rs:227/230`）、秒上界常量（:190）与 `is_valid()` 的 `<=` 约束
      （:257）同构。`OverlaySessionOptions`（`crates/bongocat-overlay/src/lib.rs:96/101`）刻意保留
      毫秒，由 `with_runtime_settings`（:108-118）经 `bongocat_runtime::hover_hide_delay_ms`
      （runtime :207）换算一次；毫秒上界常量 `MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS`（runtime :197）
      由秒上界派生，平台 options 校验与两平台测试继续引用它，因此
      `crates/bongocat-overlay/src/{macos,windows}.rs` 在本次单位修订中未改动。这两个字段刻意
      **不**进入 `requires_window_recreation`（overlay :129），因为 alpha 与指针路由在每帧应用。
      可移植状态机 `crates/bongocat-overlay/src/hover.rs`：`HOVER_FADE_DURATION = 300ms`（:24）、
      半开区间命中测试 `pointer_inside_window`（:33）、`PointerHoverObservation`（:47）、
      `PointerHoverHide`（:66）及其 `observe`（:101）与 `advance`（:147）。`advance` 把 alpha 表示为
      `fade_from + (target - fade_from) * clamp(elapsed / 300ms)`，即从过渡起点起算的绝对时间函数，
      因此帧率无关（60 FPS 与 240 FPS 在 150ms 处都得到 `0.5`，由
      `the_fade_is_a_function_of_elapsed_time_and_never_overshoots` 断言）。
      两个平台 owner 对称实现：`apply_presentation(alpha, click_through)`（macOS
      `crates/bongocat-overlay/src/macos.rs:1504` 写 `panel.setAlphaValue`，Windows
      `crates/bongocat-overlay/src/windows.rs:1173` 写 `renderer.opacity`），
      `update_hover_presentation`（macOS :667、Windows :1514）在 frame tick 内每帧应用，
      `create_overlay`（macOS :697、Windows :1542）在窗口替换时预置当前淡出值以免闪一帧全不透明；
      macOS 额外用 `appkit_cursor_position`（:922）把 `CursorSample` 的 CoreGraphics 坐标镜像到
      AppKit 屏幕坐标（以 AppKit frame 原点为 `(0,0)` 的主显示器高度为轴，找不到主显示器时返回
      `None` 而不是猜测），Windows 侧 `GetCursorPos` 与 `GetWindowRect` 同为虚拟屏幕像素、无需换算。
      两平台 `validate_options`/`validate_product_options` 均以 `MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS + 1`
      作为拒绝边界。
      UI：`SettingsOverlay::hide_on_pointer_hover` / `hide_on_pointer_hover_delay_seconds`
      （`crates/bongocat-ui/src/lib.rs:526/529`）、`SettingValue::OverlayHoverHideDelay` 与
      `PendingOperation::OverlayHoverHideDelay`（`crates/bongocat-ui/src/window.rs:1426`、:245）、
      `schedule_overlay_hover_hide_delay_flush`（:725）、flush 分支（:938）与失败重排（:1304）、
      `set_overlay_hover_hide_delay_value` / `adjust_overlay_hover_hide_delay`
      （`crates/bongocat-ui/src/window/settings.rs:374/418`，`raw.round().clamp(0.0, MAXIMUM_…)`，
      上界直接引用 `bongocat_config` 常量而非本地字面量）、overlay 设置页开关与数字输入
      `NumberFieldOptions { min: 0.0, max: MAXIMUM_…, step: 1.0 }`
      （`crates/bongocat-ui/src/window/render.rs:397-465`）、辅助功能节点
      `ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER` / `…_HOVER_DELAY_DECREASE` /
      `…_HOVER_DELAY_INCREASE`（`crates/bongocat-ui/src/window.rs:188/191/194`，节点 ID 51/52/53）
      与 `settings.overlay.hide_on_pointer_hover{,_delay}.{label,description}`、
      `shortcuts.actions.{decrease,increase}_hide_on_pointer_hover_delay` 中英文案
      （`bongocat-i18n/locales/{zh-CN,en-US}.json`，共 6 个新 key）。延迟节点的 AX value 带 `s`
      单位后缀（`accessibility.rs:449/463` 的 `format!("{hover_hide_delay_seconds}s")`），与相邻的
      `%` 节点一致；增减按钮的步进为 1 秒（:1211/:1214）。
      app 侧映射：`overlay_settings_from_config`（`crates/bongocat-app/src/lib.rs:1464-1465`）、
      `set_overlay_settings` 回写（:749-751）、启动 `OverlaySessionOptions` 字面量经
      `bongocat_runtime::hover_hide_delay_ms` 换算（`crates/bongocat-app/src/main.rs:2187-2189`）、
      `SettingsCommand::SetOverlaySettings` 映射与快照投影
      （`crates/bongocat-app/src/settings.rs:632-633`、:1196-1197）。
      共享 fixture：`accept-hover-hide.json`（开关开、延迟 0，accept）、`accept-hover-hide-delay.json`
      （开关开、延迟 3 秒，accept）与 `invalid-hover-hide-delay.json`（延迟 61 秒，reject）取代原
      `invalid-deferred-hover-toggle.json` / `invalid-deferred-hover-delay.json`。
    - 验收证据（2026-09-16）：`cargo test --locked --workspace` 退出码 0，共 659 passed /
      0 failed / 5 ignored（config 51、runtime 72、overlay 28、ui 115、app 120+21、i18n 4 等；
      `hover.rs` 新增 7 项状态机测试全部通过）；`cargo check --locked --workspace --release` 通过
      （仅有与本次无关的 `block v0.1.6` future-incompat 警告）；`cargo fmt --all --check`、
      `git diff --check` 与三组 clippy（按 CI 逐字命令，含 `--manifest-path Cargo.toml --locked`：
      `cargo clippy --manifest-path Cargo.toml --locked --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings`、
      `cargo clippy --manifest-path Cargo.toml --locked -p bongocat-app --all-targets --features storage-test-injection -- -D warnings`、
      `cargo clippy --manifest-path Cargo.toml --locked -p bongocat-app --all-targets --features production -- -D warnings`）
      均退出码 0，且三组都强制重编后复跑确认（非缓存命中，仅剩与本次无关的 `block v0.1.6`
      future-incompat 警告）；
      `tools/validate-json-schema.py` 输出
      `validated 9 input, 9 expected, and 14 config, 6 state fixture(s)`，其中
      `hover-hide (accept)`、`hover-hide-delay (accept)` 与
      `hover-hide-delay-out-of-range (reject)` 均符合预期；`tools/validate-locales.py` 输出
      `validated 2 locale(s), 380 key(s) each`；`tools/validate-fixtures.py`
      （9 input fixture + 8 model package case）、`tools/run-input-fixtures.py`（9 input fixture）、
      `tools/tests` 契约测试 63 项与 `tools.tests.test_native_release_target_matrix` 4 项均通过。
      Metal 着色器以 `xcrun -sdk macosx metal -std=metal3.0 -c` 离线编译通过（退出码 0，产物
      `/tmp/bongocat_overlay.air` 7568 字节）；本次未改动任何着色器源，该步骤只是回归确认。
      **未运行**：Windows HLSL 编译与 Windows 实机悬停（本机无 Windows、无 `dxc`/`fxc`，且 `naga`
      不提供 HLSL frontend）；macOS 实机悬停视觉 smoke（需人工运行产品观察淡出观感、穿透时序与
      多显示器边界）；双平台 CI 门禁。因此 Windows 侧 `update_hover_presentation` 只经过与 macOS
      逐行同构的代码审查，未经过任何编译或运行验证；macOS 侧的坐标镜像只由代码审查与单元测试
      覆盖，未在真实多显示器与负坐标布局下验证。
    - 验收证据（2026-09-16，单位修订）：`just check` 退出码 0，即 `cargo fmt --all -- --check`、
      三组 clippy（`--workspace --all-targets --all-features --exclude bongocat-app`、
      `-p bongocat-app --all-targets --features storage-test-injection`、
      `-p bongocat-app --all-targets --features production`）、`cargo test --locked --workspace` 与
      `cargo check --locked --workspace --release` 全部通过（仅剩与本次无关的 `block v0.1.6`
      future-incompat 警告）。`cargo test --locked --workspace` 为 **660 passed / 0 failed /
      5 ignored**，比修订前的 659 多 1 项，即新增的
      `tests::hover_hide_delay_converts_whole_seconds_to_the_frame_clock`
      （`crates/bongocat-runtime/src/lib.rs:4881`），断言 0/1/3 秒的换算、`60` 秒等于毫秒上界，
      以及越界秒值经饱和乘法后仍大于毫秒上界而不是回绕成小延迟。
      `tools/validate-json-schema.py` 输出 `validated 9 input, 9 expected, and 14 config,
      6 state fixture(s), with Draft 2020-12`，其中 `hover-hide-delay (accept)` 与
      `hover-hide-delay-out-of-range (reject)` 符合预期；`tools/validate-locales.py` 输出
      `validated 2 locale(s), 380 key(s) each`（key 数不变，只改文案）；
      `tools/validate-fixtures.py`（9 input fixture + 8 model package case）、
      `tools/run-input-fixtures.py`（9 input fixture）与 `tools/tests` 契约测试 63 项均通过。
      **未运行**：Windows 与 macOS 实机悬停、Windows HLSL 编译、双平台 CI。本次未改动任何平台
      源码、着色器或输入处理，因此沿用第 79 项原有的平台未验证清单；但设置页在真实产品里按秒
      显示与步进、AX value 的 `s` 后缀、以及 `config.json` 的 `schema_version: 1` 新键名，都只在
      单元测试与 schema 校验层面验证过，没有人工打开设置窗口确认。
      `spikes/config-store` 的隔离 contract 也在本次一并同步。该 spike 自带一份冻结的
      `NativeConfig` 副本（`spikes/config-store/src/lib.rs:137`），却有一个
      `default_serialization_matches_shared_config_fixture`（:1031）拿它去比对会漂移的
      `shared/config/fixtures/default.json`；因此第 78 项（窗口圆角）与第 79 项（悬停隐藏）把字段
      写进共享 fixture 之后，这个用例就已经失败，而它在 CI 里由 `contract-spikes` job 的
      config-store 矩阵项和 `windows-input-spike` job 执行（`.github/workflows/native-rewrite-phase0.yml:1051-1053`、:1489-1490），
      属于既有红项，不是本次改动引入。本次补齐 `corner_radius_percent`、
      `hide_on_pointer_hover`、`hide_on_pointer_hover_delay_seconds` 三个字段、默认值与
      `validate()` 上界；把 `serialized_keys_follow_native_snake_case_contract` 中这三个键的断言由
      「必须缺席」改为「必须在场，且旧版 camelCase 拼写必须缺席」；把
      `unknown_fields_are_rejected_like_the_json_schema` 的候选换成 `hideOnHover`、
      `hideOnHoverDelay` 与 `hide_on_pointer_hover_delay_ms`（后者同时把本次改名记录为拒绝项）；
      并新增 `overlay_presentation_fields_follow_the_shared_range_contract`。
      验证：`cargo fmt --manifest-path spikes/config-store/Cargo.toml -- --check`、
      `cargo clippy --manifest-path spikes/config-store/Cargo.toml --locked --all-targets -- -D warnings`
      与 `cargo test --manifest-path spikes/config-store/Cargo.toml --locked` 均退出码 0，lib 21 项
      （含此前失败的 `default_serialization_matches_shared_config_fixture`）与 2 项进程恢复集成
      测试全部通过。**这是把冻结副本重新对齐到当前 v1 的同步，不是把该 spike 改成依赖正式
      `bongocat-config`；下次再改配置字段它仍会漂移**，是否让它复用正式 crate 需要单独决策。

80. [x] `P5-DEAD-SETTINGS-INPUT-CLEANUP`：删除设置窗口中从未被渲染的 5 个输入实体及其订阅。
    - 依赖：`P5-*` 建立的 overlay 与 gamepad 设置页、`gpui-component` 的 `SettingField::number_input`。
    - 退出条件：`SettingsView` 不再持有 `overlay_scale_input`、`overlay_opacity_input`、
      `overlay_corner_radius_input`、`stick_dead_zone_input`、`trigger_dead_zone_input`；只为它们
      存在的实体创建、`sync_component_inputs` 回填与 10 个输入事件订阅一并删除；
      `model_id_input` 保留，因为它是唯一被真实渲染的设置文本框；四个数字设置项的编辑路径
      （`set_overlay_scale_value` / `set_overlay_opacity_value` /
      `set_overlay_corner_radius_value` / `set_gamepad_dead_zone_value`）仍由 render 层可达，
      行为与用户可见结果完全不变；不引入新依赖、不改配置契约、不改任何测试期望。
    - 决策记录（2026-09-16）：维护者在第 79 项报告后要求清理。这是**删除死代码**，不是行为变更，
      因此不新增 ADR，也不改 Technical Design 的架构描述。
    - 死代码成因：`gpui-component` 的 `SettingField::number_input` 在渲染时用
      `Window::use_keyed_state` 自建 `InputState` 并直接调用传入的 setter，所以设置视图不需要
      再持有一份数字输入实体。这 5 个实体是在改用组件库 `NumberField` 之前留下的：它们被创建、
      被 `sync_component_inputs` 回填、并各自订阅了 `NumberInputEvent::Step` 与
      `InputEvent::Change`，但没有任何 render 引用它们，因此订阅永不触发，回填也无人读取。
      rustc 不会报 `dead_code`，因为这些字段确实被读取了，只有跨文件的渲染点缺失才能看出问题。
    - 当前契约（2026-09-16）：`crates/bongocat-ui/src/window.rs:546-553` 只保留 `model_id_input`，
      并用文档注释说明为什么不得再为数字字段保留实体副本；`window.rs:27` 的 import 收窄为
      `input::{Input, InputEvent, InputState}`（`NumberInputEvent` 与 `StepAction` 已无引用）。
      `crates/bongocat-ui/src/window/view_state.rs` 的 `sync_component_inputs` 从第 28 行起只剩
      `model_id_input`、语言选择与主题选择；构造函数只创建 `model_id_input`、`language_select`、
      `theme_select`；订阅只剩 `model_id_input`、`language_select`、`theme_select` 三处。
      `model_id_input` 的渲染点仍是 `crates/bongocat-ui/src/window/models.rs:428` 的
      `Input::new(&view.model_id_input)`。净删除 163 行（2 个文件，+8 / −171）。
    - 验收证据（2026-09-16）：清理前后 `cargo test --locked --workspace` 均为退出码 0 且
      **659 passed / 0 failed / 5 ignored**，逐项数字完全一致，符合「纯删除死代码不改变任何测试
      结果」的预期；`cargo fmt --all --check`、`cargo check --workspace --all-targets`、
      `cargo check --locked --workspace --release`（仅有与本次无关的 `block v0.1.6`
      future-incompat 警告）与 `git diff --check` 通过；三组 clippy 按 CI 逐字命令
      （`--manifest-path Cargo.toml --locked`，含
      `--workspace --all-targets --all-features --exclude bongocat-app`、
      `-p bongocat-app --all-targets --features storage-test-injection`、
      `-p bongocat-app --all-targets --features production`，均 `-D warnings`）退出码 0，
      并对 `bongocat-ui` 强制重编复跑确认（非缓存命中）；
      `tools/validate-json-schema.py`（14 config fixture）、`tools/validate-locales.py`
      （380 keys each）、`tools/validate-fixtures.py`（9 input + 8 model package case）、
      `tools/run-input-fixtures.py`（9 input fixture）、`tools/tests` 63 项与
      `tools.tests.test_native_release_target_matrix` 4 项均通过。删除前用全仓搜索确认这 5 个名字
      只出现在 `window.rs` 的声明与 `view_state.rs` 的创建/回填/订阅/构造器，且
      `crates/` 下只有 `Input::new(&view.model_id_input)` 一处真实渲染输入框。
      **未运行**：Windows 与 macOS 实机设置页 smoke（需人工打开设置窗口逐项编辑 overlay 缩放、
      不透明度、圆角与手柄死区，确认组件库的 `NumberField` 仍能提交数值）；双平台 CI 门禁。
      因此「用户可见行为不变」这一结论目前由测试数字一致、setter 调用点未变与编译期检查支持，
      尚未经过实机点击验证。

81. [x] `P4-MODEL-LEGACY-SOURCE`：让模型导入直接识别 BongoCatMver 模型并触发转换，无需外部工具。
    - 依赖：`P4-MODEL-ARCHIVE-SOURCE`（`detect_source_kind` 的来源识别与压缩包规划）、
      `P4-MODEL-ID-UUID`/`P4-MODEL-LIBRARY-METADATA`（UUID 存储键与标题元数据）、
      `P4-MODEL-IMPORT-OPERATION`（typed 导入操作与进度契约）、ADR-0030（先复用既有方案）。
    - 退出条件：Mver 源按内容识别且需要两条独立证据（根 `config.json` 可解析成 legacy section
      形状 + 它命名的模式里至少一个在 `<资源根>/<模式>/cat_model/` 下恰好有一个
      `.model3.json`），缺任一条回退到普通包导入；一个源产出每种模式一个模型，各自独立 UUID 与
      元数据、独立原子提交；转换写进 store 自己的 staging 并与目录/归档来源共用校验与 rename
      尾部；归档来源不解压；输出键位图使用产品自己的名字词汇表；合成图做无损重编码；跨模型进度
      单调；完整 Native 门禁通过。
    - 决策记录：ADR-0037。
    - 当前契约（2026-09-17）：`bongocat-model` 新增 `mver` 模块与公开类型 `MverInputMode`、
      `ModelSourceContent`。`ModelStore::inspect_source` 复用 `detect_source_kind` 后按上述两条
      证据判定；`ModelStore::import_mver_with_observer` 把选中的模式转换进自己的 staging；导入
      尾部抽成 `commit_installed_staging`，目录复制、归档解压与转换三条路径共用
      `PreparedModel::prepare` + 单次 `rename`。归档来源通过 `ArchivePlan` 新增的
      `contains_file`/`file_references`/`file_size`/`read_file` 按需读取被命名的条目（逐条目复核
      名字/声明大小/实际字节数），**不解压整个源**；转换期读取有独立上限
      `LEGACY_RESOURCE_MAXIMUM_BYTES`（64 MiB），与约束"可被安装的包"的
      `maximum_file_bytes` 分开。
      键位表按模式分属两个编码空间：`standard`/`keyboard` 是 Windows 虚拟键码，`gamepad` 是
      XInput 序号；输出名分别对应 `bongocat-live2d` 从 HID usage 解析的键盘名与随包预置
      gamepad 模型已装载的手柄名（`0x08` 采用产品拼写 `Backspace`；无法命名的控制码不产出
      图片也不报错）。`gamepad` 的右手图集下标从左手的长度继续，不是从 0 重新开始。
      合成为"最小画布上的 Porter-Duff over"，整数实现只在抗锯齿边缘像素四舍五入一次；模式
      没有 `keyboard/` 图集时按字节安装 paw 图；缺 paw 或配套键帽的绑定被跳过而不失败。
      合成图交 `oxipng 10.2.1` 无损重编码（`optimize_alpha` 只改写全透明像素颜色通道）；
      有损量化库 `imagequant` 因 GPL 许可证排除，Zopfli 后端因"多 5% 体积换 15 倍时间"不采用。
      诊断上新增 `ModelStoreDiagnostic::SourceConversionFailed`
      （`model_store_source_conversion_failed`，`ALL` 由 12 增至 13），映射到既有
      `SettingsErrorCode::ModelImportSourceUnsupported`，未新增用户可见错误码。
      `Application::import_model*` 被 `Application::import_models*` 取代（返回 `Vec<InstalledModel>`），
      避免留下一条会绕过转换的旁路；标题为「用户标题 · 本地化模式名」，模式名在截断之后拼接；
      跨模型进度由 `ImportProgressAccumulator` 折叠（累计已完成模型的总量、stage 取最大值）。
      UI 无新增控件：只改写 `models.installed.description` 说明自动转换，新增两个 locale 的
      `models.legacy.mode.*` 供标题使用。
    - 依赖评估（2026-09-17，§9）：`image =0.25.10`（已在 workspace 依赖中，供 Live2D 纹理读取；
      本次复用同一 pin 与同一 `png`-only feature 集）+ `oxipng =10.2.1`
      （lossless PNG 优化器，MIT，MSRV 1.88 ≤ 项目 1.97；只开库入口，`binary`/`parallel`/`zopfli`
      三个 feature 全关）。两者均为当次核对的 crates.io 最新非 yanked 稳定版。`oxipng` 传递引入
      `libdeflater 1.26.0`（MIT，用 `cc` 编译 libdeflate 的 C 源码）。被排除的候选：
      `imagequant 4.4.1`（GPL-3.0-or-later，与 `deny.toml` 白名单冲突）。取舍与替换边界见
      ADR-0037 §6。
    - 验收证据（2026-09-17）：`bongocat-model` 83 测试（`mver.rs` 新增 18 + `store.rs` 新增 5），
      覆盖两条检测证据与"配置了模式却没有模型"的跳过、模式文件夹摊在根上的布局、左右手共用
      键盘图集的下标、没有 `keyboard/` 图集时按字节复制、合成后包根布局与
      `[128, 0, 127, 255]` 合成像素、`over` 算子边界、无损重编码在缩小文件的同时保持**每个
      alpha > 0 的像素逐通道不变**、鼠标键与非法码被跳过、缺 layer 时跳过绑定、重复绑定只写
      一次、符号链接被拒绝、归档来源不解压即转换、超限资源被拒绝、两个编码空间的键名、
      切换模式与取消不留下 staging。`bongocat-app` 124 测试（新增 4：一个源导入出 3 个模型与
      3 个 UUID、标题「我的猫 · 标准模式/键盘模式/手柄模式」、三种模式的键位图落位与标准模式
      没有 `right-keys`、源目录未被写入、合并目录出现 3 条 installed、跨模型进度单调且终值等于
      三模型之和、标题拼接模式名后仍不超上限、进度折叠的单元语义）。`bongocat-i18n` 4 测试
      （两 locale 键与占位符一致）。
      **真实模型验证**：`cargo run -p bongocat-model --example model_conversion_smoke -- --source
      /Users/ayang/Downloads/bongo_cat_mver_0.1.6_64`（同一路径也可交给新用例
      `converts_the_legacy_sample_named_by_the_environment`，经 `BONGOCAT_MVER_SAMPLE` 指定）。
      逐模式结果：`standard` → 3 张纹理、15 张键位图（`Num1..Num7`/`KeyQ`/`KeyE`/`KeyR`/`Space`/
      `KeyA`/`KeyD`/`KeyS`/`KeyW`，与 `config.json` 的 15 条绑定逐个对应）、31 文件 / 1 081 672
      字节 / 0.87 s；`keyboard` → `left-keys`{`Control`,`KeyR`,`Shift`} +
      `right-keys`{`LeftArrow`,`RightArrow`,`UpArrow`,`DownArrow`}、23 文件 / 1 015 987 字节 /
      0.46 s；`gamepad` → `left-keys`{`DPadDown`,`DPadLeft`,`DPadRight`,`DPadUp`,`LeftTrigger`,
      `LeftTrigger2`} + `right-keys`{`East`,`North`,`RightTrigger`,`RightTrigger2`,`South`,`West`}、
      28 文件 / 1 082 753 字节 / 0.74 s。gamepad 的两个集合与仓库内
      `resources/models/gamepad/{left-keys,right-keys}` 的文件名**逐字符一致**，源目录未被修改。
      压缩取舍实测（15 张 612×354 合成图，release）：仅 `image` 编码 160 641 字节 →
      oxipng/libdeflate 91 470 字节（−43%，0.68 s）→ 再加 Zopfli 86 605 字节（0.68 → 10.25 s）。
      `cargo fmt --all --check`、三组 clippy（workspace `--all-targets --all-features` 与
      `bongocat-app` 的 `storage-test-injection`/`production`）、`cargo test --locked --workspace`
      （全部测试二进制全绿）、`cargo check --locked --workspace --release` 全部通过。
      按 §9 执行了完整 `cargo update`，只升了 3 个与本功能无关的传递依赖
      （`granit-parser` 1.2.1 → 1.3.0、`serde-saphyr` 1.2.0 → 1.3.0、`redox_users` 0.5.2 → 0.5.3）。
      **未运行**：Windows 编译与实机导入（`libdeflater` 的 C 工具链未在 Windows/交叉编译下验证）、
      UI 实机点击（本次未改动 Models 页面的控件集合，只改了说明文案）。
      **已知缺口（既有，不由本项引入）**：①`bongocat-runtime` 只对键盘按键产生 `KeyPress`，手柄
      按键仅置 hand-down/stick 标志，因此 gamepad 键位图与预置 gamepad 模型一样目前不可达；
      ②`standard.mouse_left/right/side` 与 `mouse*.png` 没有可映射的 overlay 通道，与参考实现
      一致地不转换；③转换按 `F1`…`F12` 命名以保留模型区分度，但
      `bongocat-live2d::key_name_candidates` 当时把功能键统一回退到 `Fn`，逐键 F 图不可达
      （预置模型只有 `Fn.png`、真实样本键位表也不含功能键，当时缺少可验证数据）。
      **③ 已于 2026-09-17 闭合**：HID 功能键布局收敛为 `bongocat-render` 的单一表
      `FUNCTION_KEY_USAGES` / `FUNCTION_KEY_NAMES`（`0x3a..=0x45` 与 `0x68..=0x73` 两段，共
      F1-F24），`key_name_candidates` 由它派生精确名、`Fn` 仍为末位候选，专属图存在即生效、
      缺失即回退到该模型共享的 `Fn.png`。同一张表也让 `bongocat-app::input_bindings_for_model`
      把整行功能键绑到左手——**这一步是本次发现的必要前提**：`InputState::model_snapshot`
      对 `hand_for == None` 的按键走 `None => {}` 直接丢弃，而当时的手表只有 0x04-0x27、0x28-0x2c
      、0x35、0x38、0x39、0x4c 和修饰键，功能键一个都没有，所以 F1-F12 的 `Fn.png` 在实机上也
      从未画出来过。逐键 F 图现在可达。
      验收证据：`bongocat-render` 15 测试（新增 1：两段区间总数与名字数量一致、F1/F12/F13/F24
      命名、PrintScreen 0x46 / 数字键盘 0x67 / Execute 0x74 不享受功能键语义）；
      `bongocat-live2d` 43 测试（候选顺序与逐键回退覆盖两段区间、出厂 `standard`/`keyboard` 模型的
      F1-F24 全部解析到磁盘上的 `resources/left-keys/Fn.png`、磁盘上同时存在 `F13.png` 时优先选中
      它）；`bongocat-app` 126 测试（新增 2：三种键盘模型的整行功能键左手绑定且 PrintScreen 不绑定、
      platform-gated 端到端用例发布 F1/F13 的 Down/Up 后 `model_input.key_presses` 出现
      `side = Left` 的对应 press）。反向对照：临时把绑定循环置空后这两条用例均失败，证明手表缺失
      是真实阻塞而非推断。**仍未覆盖**：Windows 的 `map_key_code` 没有 0x68-0x73 入口
      （UEFI 表的 AT 101/102 列对 F13 及以上为 N/A，缺可依据的扫描码，未猜值），
      `legacy_virtual_key_name` 的 VK 表也仍止于 F12（0x70-0x7B），因此 Legacy 源里绑定 F13+
      的条目依旧会被跳过。
    - 键位图 Alt 左右侧修正与旧名兼容（2026-09-17，ADR-0038）：`rdev` 时代的模型把左右 Alt 命名为
      `Alt`/`AltGr`，而运行时按 HID usage 派生的 `AltLeft`/`AltRight` 查资源，于是右 Alt 落到
      `Alt.png`（画成左 Alt 的图）、模型自带的 `AltGr.png` 完全不可达（预置模型中两者 SHA-256
      不同，是肉眼可见的错误，不是"反正一样"）。修正分三处：①预置 `standard`/`keyboard` 的
      `left-keys` 用 `git mv` 改名为 `AltLeft.png`/`AltRight.png`，字节不变，
      `preset-model3-index.json` 冻结快照同步（重跑 `spikes/model-package` 解析器逐字段相等）；
      ②`bongocat-live2d::key_name_candidates` 中右 Alt 的候选改为 `AltRight`→`AltGr`→`Alt`，
      `AltGr` 是唯一保留的旧名且只对右侧生效，使**已经安装**的旧模型不回退成共用图（`next` 无
      迁移，导入归一化不会回头改写用户数据根里的已有模型）；③`bongocat-model` 新增 `key_names`
      模块，在 store 自己的 staging 上把 `resources/{left-,right-}keys/` 的 `Alt.png`/`AltGr.png`
      改名为 canonical 名，调用点在目录复制与归档解压之后、共用的 `commit_installed_staging`
      之前，因此两种来源结果一致、用户选中的源始终只读、文件数与字节数不变（进度计数不虚报）；
      canonical 文件已存在时保留它、旧名文件原样留下，不猜作者意图。
      Mver 侧 `mver::legacy_key_names` 取代单值 `legacy_key_name`：`0x12` `VK_MENU`（上游教程图
      把左右 Alt 都编号为 `18`，C++ 侧用 `GetKeyState` 查询，旧格式本身没有"哪一侧"这一位信息）
      展开为 `AltLeft` + `AltRight` 两个目标名，同一份合成图，不再产出共用的 `Alt` 名；
      `0xA4`/`0xA5`（`VK_LMENU`/`VK_RMENU`）分别映射 `AltLeft`/`AltRight`，与
      `bongocat-platform` 对同一码的处理一致。`0x10`/`0x11` 继续输出 `Shift`/`Control` 家族名：它们
      与 `0x12` 属同一类歧义，但真实样本的 `left-keys{Control, KeyR, Shift}` 已被 ADR-0037 记为
      验证证据，改动会同时作废那份证据，因此留作单独改动。
      验收证据：`bongocat-live2d` 45 测试（新增 2：三组候选与命中结果；真实预置模型断言左 Alt 命中
      `resources/left-keys/AltLeft.png`、右 Alt 命中 `.../AltRight.png`、两者字节不同、预置模型里
      不再存在 `Alt`/`AltGr`）；`bongocat-model` 92 测试（`key_names` 3 + `store` 3 + `mver` 2，
      其中 `store` 的一个是 env 门禁的真实样本用例
      `imports_the_bongo_cat_sample_named_by_the_environment`，由 `BONGOCAT_PACKAGE_SAMPLE` 指定）。
      **真实社区模型验证**（目录与 `.zip` 两种来源各跑一遍）：`送葬人 · 标准模式`（left-keys 56
      张、2 个旧名）→ `Alt.png`/`AltGr.png` 改名后字节相同；`经典小键盘 · 标准模式`（15 张、0 旧名）
      → 无改名且逐字节相同；`Bongo Cat v0.16/BongoCat - 标准模式`（50 张、1 个旧名）→
      `Alt.png`→`AltLeft.png`。用例同时断言导入后 key 目录的**完整文件集合与逐文件字节**等于
      "源集合按规则改名后的期望"，并**重新读取源**确认与导入前快照完全一致。真实 Mver 样本回归：
      `model_conversion_smoke --source /Users/ayang/Downloads/bongo_cat_mver_0.1.6_64` 三个模式的
      文件数/字节数/键位图名与 ADR-0037 记录**完全一致**，说明本次展开没有改变真实样本产物。
      冻结快照用 `spikes/model-package` 解析器对仓库预置模型重新生成后逐字段相等（工作树里的
      未跟踪 `.DS_Store` 需排除，否则会多出一个 unreferenced 文件）。
      **未运行**：Windows 编译与实机按键、已有旧安装数据上的 `AltGr` 兼容路径、UI 实机点击。
      **既有缺口（不由本项引入）**：`preset-models.json` 与
      `docs/phase-0/model-resource-inventory.md` 的 `contentManifestSha256` 仍是
      `c4103c7`（PNG 无损重压缩）之前的取值，仓库内没有生成该清单的脚本，算法无法从现有数据
      反推（文件数与字节数也已与该 commit 不符），因此本项只报告、不臆造新值。
    - 小键盘键位图命名与完整键位词表（2026-09-17，ADR-0040 + ADR-0041）：闭合 ADR-0039 记录的
      后续项「把 `Kp0..Kp9`、`KpMultiply` 等接入运行时候选」，并把键位词表补成完整集合。
      ① `key_name_candidates` 增加小键盘整块精确名（`NumLock`、`KpDivide`、`KpMultiply`、
      `KpMinus`、`KpPlus`、`KpDecimal`、`Kp1..Kp9`、`Kp0`）与回退候选（`Kp1..Kp9,Kp0` →
      `Num1..Num9,Num0`、`KpEnter` → `Enter`、`KpDivide` → `Slash`、`KpMinus` → `Minus`、
      `KpDecimal` → `Dot`），新增 `KEYPAD_DIGIT_NAMES` 与既有 `KEY_NUMBERS` 逐下标对齐。
      ② 补齐主键盘缺失的 22 个 arm：标点 `Minus`/`Equal`/`LeftBracket`/`RightBracket`/`BackSlash`/
      `IntlHash`/`SemiColon`/`Quote`/`Comma`/`Dot`，`PrintScreen`/`ScrollLock`/`Pause`，导航
      `Insert`/`Home`/`PageUp`/`Delete`/`End`/`PageDown`，以及 `IntlBackslash`/`Apps`/`KpEqual`。
      **词表范围由两个平台 adapter 实际能产出的 usage 界定**（`0x04..=0x65` ∪ `{0x67}` ∪
      `0x68..=0x73`），**不以预置模型当前是否有图为前提**——命名是与模型作者的契约，
      `0x66`（`Power`）两边都不产出故保持无名。③ `input_bindings_for_model` 收敛为一条循环绑定
      `0x04..=0x65`（方向键除外）+ `F13..F24` + `0x67` → 左手：`InputState::model_snapshot`
      会丢弃没有 hand 归属的按键，只加名字不加绑定等于死代码（ADR-0040 的 `KpEnter` 已踩过）。
      `PrintScreen` 改为绑定，但 `FUNCTION_KEY_USAGES` 边界不变（它仍不是功能键）。
      **修复的既有缺陷**：`0x4c`（`Delete`）一直绑在左手表里、`Delete.png` 也一直随两个预置模型
      出厂，但没有名字 arm ⇒ **`Delete.png` 从出厂起就永远画不出来**；逐张核对 55 张预置键位图，
      它是唯一不可达的一张，与功能键逐键图曾经的缺口同类。
      验收证据：`bongocat-live2d` 50 测试（新增 2：
      `every_key_the_platform_adapters_can_report_has_a_name` 断言 `0x04..=0x65`/`0x67`/
      `0x68..=0x73` 每个 usage 的候选列表非空且 `0x66` 保持无名；
      `a_model_providing_a_named_key_image_draws_it` 用合成资源断言 27 个新命名键在模型提供对应
      PNG 时全部命中，并断言只提供出厂词汇的模型对这些键仍解析为空）；`bongocat-app` 124 测试
      （`keyboard_models_bind_every_named_key_of_the_standard_layout` 断言整块布局的绑定覆盖，
      功能键用例改为断言 `function_key_name(0x46) == None` + `hand_for(0x46) == Some(Left)`）。
      `just check` 六道门全过（fmt、三组 clippy、`cargo test --workspace`、release check）。
      **未运行**：Windows/macOS 实机按键、UI 实机点击。
      **行为变化（尚未实机确认观感）**：标点键、`PrintScreen`、导航键现在都会让左爪下压，
      此前完全无反应；除 `Delete.png` 外，本次补的名字都还没有美术，需要模型补图才有视觉效果。
      （该行为变化已由下一项 ADR-0042 闭合：缺图按键不再产生任何动作。）
    - 缺图按键不再触发按键动作（2026-09-17，ADR-0042）：修正 ADR-0041 残余风险 1 记录的缺陷
      ——预置 `standard` 没有 `Dot.png` 等资源，按下这些键时左爪仍会下压，用户看到的是"爪子按下去
      却什么都没出现"。① `bongocat-live2d` 新增 `KeyImageInventory`（`read`/`provides`/`can_draw`）：
      不解码图片地列出 `resources/left-keys`/`right-keys` 的资源名，并按 `key_name_candidates` 的
      候选顺序（精确名、`Fn`/修饰键家族图、`AltGr`/`Return` 旧名、`Kp*`→主键盘回退）回答"这个键
      能不能画出来"；`load_key_assets` 与它共用同一个私有目录扫描 `key_image_files`，因此清单与
      渲染实际加载的资产不可能漂移。② `bongocat-app::input_bindings_for_model` 只把 `can_draw`
      为真的键写进 `InputBindings`：写入 runtime 的每模型绑定 = 静态 hand 表 ∩ 该模型键位图。
      于是 `InputState::model_snapshot` 在 `hand_for == None` 处丢弃缺图按键，既不置
      `left_hand_down`/`right_hand_down`（不驱动 `CatParamLeftHandDown`/`CatParamRightHandDown`），
      也不产生按键层——按键层与爪部反馈从此一致。判断放在绑定层而非 renderer：renderer 不得决定
      动作，runtime 也不该为了解图片资源而依赖 Cubism/图片解码；`prepare_model` 与 `select_model`
      两条激活路径都传入 `CommittedModel::root()` 的清单，启动恢复与设置页切换同规则。
      **保持不变的既有行为**：`Delete.png`（ADR-0041 恢复）继续可画，小键盘数字/Enter/`KpDivide`
      继续回退 `Num*`/`Enter`/`Slash`，功能键继续优先专属图并回退 `Fn`，修饰键继续精确名+家族图；
      模型补图后绑定自动恢复，命名不以美术存在为前提这条契约不变。**作用范围仅键位图**：鼠标
      （`ParamMouseLeftDown`/`ParamMouseRightDown` 是指针状态）与手柄按钮（无键位图词表，其美术
      本就不进入按键层）不在本规则内。
      验收证据：`bongocat-live2d` 52 测试（新增 2：
      `key_image_inventory_lists_exactly_the_assets_the_renderer_loads` 对三个预置模型断言清单与
      `load_key_assets` 的 (side, name) 集合逐侧相等；
      `a_shipped_model_can_draw_only_the_keys_it_ships_artwork_for` 逐键断言 `standard` 能画
      `KeyA`/`Delete`/功能键/小键盘数字与小键盘 Enter，不能画 `.`/`-`/`PrintScreen`/`NumLock`/
      小键盘 `.`/小键盘 `=` 与方向键）；`bongocat-app` `--lib` 125 测试（新增 1、改写 3：
      `a_key_the_active_model_cannot_draw_never_moves_the_paw` 启动真实渲染应用并激活预置
      `standard`，断言按下 `A` 置 `left_hand_down` 且进入 `key_presses`、按下 `.` 两者皆无；
      `keyboard_models_bind_every_drawable_key_of_the_standard_layout` 断言绑定等于"静态表 ∩
      模型键位图"并逐条抽查 9 个键；功能键用例改为断言 `PrintScreen` 不再绑定；
      `installed_models_get_default_keyboard_bindings` 断言 `gamepad` 预置不绑任何键盘键、
      手柄按钮映射不变）。`just check` 六道门全过（fmt、三组 clippy、`cargo test --workspace`、
      release check）。
      **未运行**：Windows/macOS 实机按键与观感确认、UI 实机点击、真实社区模型回归。
      **行为变化（尚未实机确认观感）**：`gamepad` 预置不再响应任何键盘键；完全没有键位图的导入模型
      键盘输入完全无动作（模型仍由指针、呼吸、眨眼驱动）；只有另一手有图时该键仍不可达（side
      严格性，与按键层一致）。**既有缺口（不由本项引入）**：`bongocat-overlay` 的
      `preview_input_bindings` 仍是 ADR-0041 之前的独立静态表，不感知键位图，人工预览 `gamepad`
      时行为与产品不同。
    - 修饰键失去 hand 归属的回归修复（2026-09-17）：ADR-0041 的绑定循环照着一个**不完整的并集**
      重写，`0xe0..=0xe7` 八个修饰键 usage 落空（`0x04..=0x65` 止于 `0x65`，功能键表跳到
      `0x68`，小键盘 `=` 是单点），而 ADR-0041 之前的旧表里 `0xe0..=0xe7` 是显式列出的。后果：
      `InputState::model_snapshot` 对 `hand_for == None` 直接丢弃，于是按下 `Shift`/`Control`/
      `Alt`/`Meta` 既不置 `left_hand_down`/`right_hand_down`，也不产生按键 press ⇒ 模型画不出
      `ShiftLeft.png`、`AltLeft.png`、`AltRight.png`、`AltGr.png`、`Control.png`、`Meta.png`
      等修饰键美术，爪子也不动——ADR-0038 为左右 Alt 单独出图的工作被同时作废。**快捷键不受影响**：
      `bongocat-platform::ShortcutMatcher` 自己维护 pressed 集合（平台 adapter 对每个键边沿同时
      喂 runtime 与 dispatcher），与 `InputBindings` 无关；但**单独按一个修饰键永远不会触发快捷键**
      是 `ShortcutMatcher::apply` 的既有设计（`modifier_bit(key).is_some()` 直接返回 `None`），
      不是本次回归。
      修复与防回归：①`input_bindings_for_model` 补回 `0xe0..=0xe7` → 左手（仍受 ADR-0042 的键位图
      门禁约束）；②`bongocat-app` 的绑定契约测试改为**遍历 adapter 产出并集**（`0x04..=0x65` ∪
      `{0x67}` ∪ `0x68..=0x73` ∪ `0xe0..=0xe7`，逐 usage 断言绑定等于"该模型能画则绑"），不再只
      遍历实现恰好用到的区间——修复前该用例在 `custom-model 0xe0` 上以 `left: None / right:
      Some(Left)` 失败，是本次回归的直接证据；③`bongocat-live2d` 的
      `every_key_the_platform_adapters_can_report_has_a_name` 同步补上修饰键段，使其名称与断言
      范围一致（原先声称覆盖 adapter 全部产出，实际漏掉 `0xe0..=0xe7`）；④端到端用例
      `a_key_the_active_model_cannot_draw_never_moves_the_paw` 增加 `0xe1`（`ShiftLeft.png`）与
      `0xe7`（无 `MetaRight.png`，走共享 `Meta.png`）两个 drawable 断言。
      验收证据：`bongocat-app` `--lib` 125 测试全过、`bongocat-live2d` 52 测试全过、`just check`
      六道门全过。**未运行**：Windows/macOS 实机按键确认。**CHANGELOG 判定为跳过**：该回归在
      `v1.1.0` 之后、`2.0.0` 发布之前引入并修复，从未随任何发布版本出厂，因此 2.0.0 的变更日志
      无需新增条目（详见本次报告）。

82. [x] `P7-STARTUP-ITEM-AUTO-LAUNCH`：双平台启动项后端统一为 `auto-launch`，补齐 macOS 12 支持。
    - 依赖：ADR-0043（取代 ADR-0013）、`P7-STARTUP-ITEM-PLATFORM`（被替换的 contract 与 UI 闭环）。
    - 退出条件：macOS 12+ 全版本与 Windows 当前用户启动项由 `auto-launch =0.6.0` 提供；
      环境隔离保持（Development/Production 不同 `app_name`，macOS 为不同 plist Label、
      Windows 为不同 HKCU value name）；启动命令携带 `--run-seconds 0`；共享
      `StartupItemState`/`StartupItemError` contract 不变；Development 构建从此支持启动项；
      完整 Native 门禁通过。
    - 实现说明（2026-09-17）：`bongocat-platform` 新增 `startup_item_native.rs` 统一后端
      （macOS `MacOSLaunchMode::LaunchAgent` 写 `~/Library/LaunchAgents/{app_name}.plist`，
      Windows `WindowsEnableMode::CurrentUser` 写 HKCU Run 并同步 `StartupApproved\Run`），
      删除 `startup_item_macos.rs`（SMAppService/objc2）与 `startup_item_windows.rs`
      （HKCU raw binding）两个手写后端，`Cargo.toml` 移除 `objc2-service-management =0.3.2`。
      后端现只产生 `Enabled`/`Disabled` 与错误；`Stale`/`RequiresApproval`/`NotFound`/
      `Unsupported(OperatingSystem|BuildEnvironment)` 保留为契约变体、无生产者，UI 处理分支
      作为防御性路径保留。Windows stale 检测随 ADR-0013 退役（安装位置变化后 `is_enabled`
      仍为 enabled，重新开关一次即修复）。
    - 依赖评估（2026-09-17，§9）：`auto-launch =0.6.0` 为当次核对的 crates.io 最新非 yanked
      稳定版（MIT，上游 2026-09-16 仍有提交；本机阅读其 `macos.rs`/`windows.rs`
      源码核实：Windows 命令行按 MSVCRT 规则加引号并写 StartupApproved 启用标记、
      macOS LaunchAgent plist 含 `Label`/`AssociatedBundleIdentifiers`/`ProgramArguments`/
      `RunAtLoad` 且 enable/disable/is_enabled 为纯文件操作）。传递依赖 macOS 侧
      `dirs 6.0.0`/`os_info 3.15.0`/`smappservice-rs 0.1.3`、Windows 侧
      `windows-registry 0.6.1`，许可证均在本仓库 `deny.toml` 白名单内。
    - 验收证据（2026-09-17）：`bongocat-platform` 57 单测 + 1 opt-in smoke 全过
      （新增环境命名隔离、双环境后端构造；`startup_item_lifecycle_smoke_restores_original_state`
      在 macOS 实机驱动 Development 环境 disabled -> enabled -> disabled 并恢复原状态，
      期间断言 plist 文件随之出现/消失，且 Production 环境状态前后不变）；
      `cargo fmt --all -- --check`、workspace 与 `bongocat-app` 两组 feature 的严格 Clippy、
      `cargo test --locked --workspace` 全部通过（app 128、ui 115、platform 57 等）。
      按 §9 执行完整 `cargo update`（新增 auto-launch 及其传递依赖，另有 4 个无关传递
      小版本升级）。
    - **未运行**：Windows 实机注册表 lifecycle（CI Windows job 承担）与 macOS
      `/Applications` 安装态 `--startup-item-smoke`（需 Production 签名构建，留给发布门禁）；
      macOS 13+ 系统设置后台项目列表的可见性为文档预期、未实机核验。
    - 决策记录：ADR-0043（ADR-0013 标记 Superseded，Technical Design 启动项段落同步）。

83. [x] `P0-OVERLAY-SCREEN-BOUNDS`：把 overlay 放置约束从工作区改为屏幕范围，并改成延迟收敛。
    - 依赖：ADR-0045、被取代的第 67 项、当前 v1 `overlay.keep_inside_screen`（由
      `keep_inside_work_area` 改名）、`bongocat-overlay` 的 `placement` 模块、runtime overlay
      settings、持久化窗口 bounds、Win32 `EnumDisplayMonitors`、AppKit `NSScreen`。
    - 退出条件：约束区域从单块显示器的 `rcWork` / `visibleFrame` 改为所有显示器矩形
      （`rcMonitor` / `NSScreen.frame`）的并集；“完整显示”按并集覆盖判定，跨显示器摆放不纠正；
      只在窗口真正离开桌面时纠正，目标为交叠面积最大（无交叠取中心最近）的显示器且不改变窗口尺寸；
      创建/缩放重建/模型重建立即收敛，拖动后的收敛延迟 `PLACEMENT_SETTLE_DELAY`（1s）执行并在每次
      观测到位移时重新计时，因此不影响跨显示器拖拽；放置检查缓存不超过
      `PLACEMENT_INSPECTION_INTERVAL`（500ms），使显示器变化在静止窗口下仍被纠正；判定与倒计时为
      平台无关可测代码；字段、中英文案、共享 schema/fixture、契约表与 AX/UIA 语义同步；完整 Native
      门禁与双平台 CI 通过。
    - 实现说明（2026-09-18）：新增 `bongocat-overlay/src/placement.rs`——`bounds_inside_screens`
      按显示器边界切片做并集覆盖判定（可处理 L 形排列、重叠显示器与负坐标），
      `correction_for_screens` 选出最大交叠（无交叠取最近）显示器并复用不改变尺寸的
      `OverlayWindowBounds::clamp_to`，`OverlayPlacementConstraint` 实现“静止 1 秒后才纠正 + 每次
      位移重新计时 + 500ms 重评估”。Windows 用 `EnumDisplayMonitors` + `GetMonitorInfoW(rcMonitor)`
      枚举显示器，`centered_position` 改为按 `rcMonitor` 居中；macOS 用 `NSScreen::frame` 枚举并按
      `frame` 居中；两平台的 `ensure_inside_work_area` 换成只移动原点的 `set_origin`，tick 里改为
      喂约束状态机；会话原 `hover_started` 改名 `session_started`，作为 hover 淡出与放置延迟共用的
      单调时钟（一个会话只读一次墙上时钟）。`ProductOverlayReport.work_area_constraint_satisfied`
      改为 `placement_fully_visible` 并按并集判定。字段改名同步 config/runtime/ui/app、i18n 中英
      （标签改为“保持在屏幕内”并重写描述）、共享 JSON Schema 与 14 个 config fixture、契约表以及
      隔离的 config-store spike。
    - 验收证据（2026-09-18）：`cargo test --locked --workspace` 全绿（overlay 43 项，其中放置约束
      新增 15 项、原 28 项；app 128 + bin 21、ui 115、platform 58、runtime 73、model 93、
      config 51、live2d 53、update 36、packaging 23、render 15、i18n 4、shared input fixtures 2）；
      workspace 严格 Clippy 与 `bongocat-app` 两组 feature 的严格 Clippy 全过；
      `cargo check --locked --workspace --release` 通过；`tools/validate-json-schema.py`
      （14 config + 6 state + 9/9 input/expected）、`tools/validate-fixtures.py`（9 输入 + 8 模型用例）、
      `tools/validate-locales.py`（383 键 × 2）、`tools/tests` 63 项通过。
    - 实机证据（2026-09-18，macOS）：`cargo run --locked -p bongocat-app --release -- --run-seconds 4
      --settings-window-smoke` 退出码 0，stderr 无任何 `bongocat:` 失败行，即开放放置约束的产品
      overlay 会话在默认 `keep_inside_screen = true` 下未触发 `placement_fully_visible == false`
      的 shutdown 断言，窗口在 4 秒运行期间保持完整可见。
    - Windows 目标验证（2026-09-18）：本机无法交叉构建 `bongocat-overlay --target
      x86_64-pc-windows-msvc`（传递依赖 `libdeflate-sys` 的 C 构建在 macOS 主机上以
      `stdlib.h file not found` 失败，属既有工具链限制，与本次改动无关）。因此用一个独立的
      `windows = 0.62.2` 形态探针 crate 在 `--target x86_64-pc-windows-msvc` 下通过
      `cargo clippy -- -D warnings` 验证了本次用到的全部 Win32 形态：`EnumDisplayMonitors(None,
      None, Some(cb), LPARAM)`、回调 `unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM)
      -> BOOL`、`GetMonitorInfoW` + `MONITORINFO` 初始化、let-chain 里的元组模式绑定、
      `BOOL` 取自 `windows::core`（不在 `Win32::Foundation`）以及 `SetWindowPos` 收敛调用形态。
      真实 Windows 编译、严格 Clippy 与实机行为仍由 CI Windows job 承担。
    - **未运行**：两平台真实鼠标拖拽观感、多显示器实机跨屏拖拽、显示器热插拔（拔掉外接屏后的自动
      纠正）、macOS 覆盖菜单栏/程序坞的观感、Windows 实机 `EnumDisplayMonitors` 行为。
    - **既有门禁问题（不由本项引入）**：HEAD `95a0b7e` 上 `cargo fmt --all -- --check` 已经不通过，
      差异只在 `crates/bongocat-platform/src/lib.rs` 与 `src/shortcut.rs`：用本仓库固定工具链的
      rustfmt 1.9.0-stable（rustc 1.97.1）对 HEAD 内容重新格式化即产生这些 hunk。按 §3.3.6 不在本项
      内顺手格式化这两个无关文件，留待独立提交处理。
    - CHANGELOG 判定（2026-09-18 修正）：写入 `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的 `2.0.0`
      →「界面与体验」/「UI and Experience」。最初按第 81 项口径判为跳过（`git tag` 最新为 `v1.1.0`，
      旧行为从未出厂）；复核 `docs/phase-0/behavior-inventory.md` 后确认旧版 v1.1.0 的「保持在屏幕内」
      是「移动/缩放后按光标所在显示器边界 clamp」，即本次同时包含面向用户的行为改变（窗口可覆盖
      任务栏；返回时机由松手即回改为停止拖拽后约 1 秒），按 §15 必须记录。配置字段改名本身不构成
      升级注意事项：v1.1.0 的配置从不被导入。
    - 决策记录：ADR-0045。

84. [x] `P0-MODELS-PAGE-CARDS`：模型管理页按旧版交互重做为封面卡片，并统一元数据编辑与错误呈现。
    - 依赖：ADR-0047、ADR-0036（导入边界）、ADR-0037（Mver 转换写入的 `cover.png`）、
      当前 v1 `model.installed_models[].title`、`bongocat-model` 包布局、`bongocat-platform` 的
      `open_directory` 与 `opener`、GPUI 的 `img(PathBuf)` 本地文件加载。
    - 退出条件：模型页展示每个模型的 `cover.png` 与标题并提供「打开模型位置」；表情入口移除；
      页面只支持切换/选择模型与编辑标题/封面；错误提示统一走通用 Notification 组件。
    - 实现说明（2026-09-18）：`bongocat-model` 新增 `PACKAGE_RESOURCES_DIRECTORY`、
      `PACKAGE_COVER_FILE`、`package_cover_path`（Mver 转换改用同一组常量，删除私有
      `OUTPUT_RESOURCES`/`OUTPUT_COVER`）与 `ModelStore::replace_cover`（同目录临时文件 + rename，
      失败清理暂存文件）；`bongocat-platform` 新增 `pick_model_cover`（PNG 过滤，选择语义与模型来源
      选择器共用）；`bongocat-app` 新增 `Application::model_directory`/`set_model_title`/
      `set_model_cover` 与 `PresetModelMetadata`/`ModelNotInstalled`/`ModelTitleInvalid`/
      `ModelCoverInvalid` 错误，快照投影 `directory`/`cover`，服务端新增三个 command 处理与
      `ModelLocationCapability`；`bongocat-ui` 的 `SettingsModelEntry` 增加两字段，新增
      `SetModelTitle`/`SetModelCover`/`OpenModelLocation`，移除 `PreviewModelBehavior` 全链路与
      AccessKit preview node，模型页重写为固定宽度卡片网格（封面 `object_fit: Cover` 裁切、
      无封面占位、两段确认删除只对导入模型开放、内联编辑标题与封面），封面替换成功后显式
      `ImageSource::remove_asset` 失效按路径命中的图像缓存。文案删除 12 键、新增 15 键并使
      `models.behaviors.*` 只保留快捷键页仍在用的 `empty`。
    - 验收证据（2026-09-18）：`cargo test --locked --workspace` 全绿（app 130 + bin 21、ui 115、
      model 95、platform 58、runtime 73、config 51、live2d 53、overlay 43、update 36、packaging 23、
      render 15、i18n 4 等）；三组严格 Clippy 与 `cargo check --locked --workspace --release` 通过；
      `tools/validate-locales.py`（386 键 × 2）、`tools/validate-json-schema.py`、
      `tools/validate-fixtures.py`、`tools/tests`（63 项）通过；`cargo fmt --all -- --check` 通过。
      新增回归：store 封面替换原子性与缺失模型、平台封面选择器校验、服务端改名/换封面/打开位置的
      成功与全部拒绝路径、模型行动作与 tab 顺序（含预设只读）。
    - 语义漂移核查：以脚本比对 Rust 中 203 个字面量文案键与目录，发现并修复一处
      `models.edit.cover.selected` → `models.edit.cover.replace` 的静默失配；`models.` 与
      `errors.settings.` 命名空间无孤立键。该项应成为后续文案改动的固定检查步骤。
    - **未运行**：双平台实机点击（选中/编辑/换封面/打开文件夹）、`--settings-window-smoke`、
      模型页 opt-in smoke、Windows 实机 `open_directory`、真实社区模型回归。
    - 决策记录：ADR-0047。Technical Design §10 模型元数据段落已同步。

85. [x] `P1-PRESET-SCAN-STRAY-FILES`：预置模型目录扫描对陌生条目的容忍。
    - 背景（2026-09-18）：`just check` 的 app 测试在主树失败、在干净 worktree 通过，二分定位到
      `resources/models` 里被 Finder 写入的 `.DS_Store`：`PresetModelCatalog::list` 对无法解析为
      `ModelId` 的目录项直接 `?` 中止，一个杂散文件就让整个预置目录不可用。这不是测试环境问题，
      而是真实产品缺陷——用户在 Finder 打开过模型目录后，模型列表就会挂掉。
    - 修复：预置扫描与 store 扫描语义对齐——非 UTF-8 名称、无法解析为模型 ID 的条目一律跳过；
      仍是合法 ID 但内容损坏的目录照旧以 `Invalid` 条目呈现。新增回归
      `a_stray_file_in_the_preset_root_never_takes_the_catalog_down`。
    - 验收证据（2026-09-18）：`just check` 六道门全绿（model 96 项测试）。

86. [ ] `P7-NATIVE-THEME-SURFACES`：让窗口框、系统弹框、右键菜单、托盘菜单和文件选择框跟随主题。
    - 依赖：`P5-APPEARANCE-THEME`（三态偏好已闭环）、`P7-SYSTEM-MENU-LIFECYCLE`（托盘与菜单
      owner）、`P7-MODEL-DIRECTORY-PICKER`（原生 picker 入口）、ADR-0030、ADR-0031、ADR-0020。
    - 背景（2026-09-18）：用户要求窗口标题栏、系统弹框、右键菜单、托盘菜单、文件选择框跟随应用
      主题，不支持跟随应用主题的退化为跟随系统主题。调研（
      `docs/theme-mode-native-surface-research.md`）确认**不存在可直接使用的现成 crate**：能力
      已在依赖树内的 `windows 0.62.2` 与 `objc2-app-kit 0.3.2` 中；`muda::MenuTheme` 上游明确不覆盖
      popup，`dark-light` 只检测不设置，`tao`/`winit` 是完整窗口库。根因是结构性的——
      `bongocat-platform` 没有主题入口，主题知识只存在于 UI 层，而原生表面的 owner 在平台层。
    - 退出条件：`bongocat-platform` 提供唯一的原生主题入口（`apply_theme` / `init_native_theme` /
      macOS `system_appearance`），`AppTheme` 为已解析取值、`System` 不在平台层出现；`System` 的
      解析口径统一到平台层而不再读 gpui 的窗口外观缓存；macOS 以进程级 `NSApplication.appearance`
      覆盖窗口框/弹框/菜单/面板，Windows 以 `DWMWA_USE_IMMERSIVE_DARK_MODE` 覆盖窗口框并在创建
      任何窗口前以 `SetPreferredAppMode(AllowDark)` 让弹框/菜单/文件框跟随系统主题；主题失败一律
      降级为系统外观且不阻止启动、不改配置；更新窗口携带应用外观而非硬编码 `System`；不新增第三方
      依赖；定向测试、完整 Native workspace 与**双平台实机**主题切换（含托盘菜单、文件面板、
      弹框、标题栏）通过。
    - 状态（2026-09-18，**未完成**）：代码、ADR-0048 与平台能力矩阵已落地。
      `crates/bongocat-platform/src/theme.rs` 新建（4 类型 + 2 函数 + macOS `system_appearance`，
      非 mac/win 平台为有文档的 no-op）；`window.rs` 的 `apply_component_theme` 改为先落原生主题
      再推导组件模式，`System` 分支在 macOS 走平台查询以避开 gpui 的窗口外观缓存；System 解析
      收敛为 `resolved_theme_mode` 单一入口，render 路径与 `smoke.rs` 断言共用（smoke 原先用
      `component_theme_mode(theme, cx.window_appearance())` 独立推导期望值，与产品实际路径不是
      同一条，已消除）；`update_window.rs` 的 `UpdateView` 增加 `appearance_theme` 字段与
      `open_update_window` 参数，`start_language_polling` 扩为 `start_settings_polling`，外观回调
      只在偏好为 `System` 时响应；`main.rs` 在解析 RunOptions 后、任何窗口前调用一次
      `init_native_theme()`，并在创建 overlay 前从持久化 `appearance.theme` 调用一次
      `apply_process_theme()`；`objc2-app-kit` 增加 `NSAppearance` feature、`windows` 增加
      `Win32_Graphics_Dwm` feature。模型窗口及右键菜单不再依赖设置窗口曾经打开。
    - 附带发现：gpui 的原生外观映射（`gpui-pre-macos-0.3.5/src/window_appearance.rs`）只识别
      `Aqua`/`DarkAqua`/`VibrantLight`/`VibrantDark`，其余一律打印到 stdout 并回退成 `Light`；
      macOS 开启「提高对比度」后 AppKit 报 `AccessibilityHighContrastDarkAqua`，gpui 会把暗色系统
      判成浅色。这是 gpui 的既存缺陷，产品不复用该映射（§3.1 评估记录见 ADR-0048 决策 2）。
    - 验收证据（2026-09-18）：`just check` 六道门全绿（format、三组严格 Clippy、workspace
      **748 passed / 0 failed**、release check）；`cargo check --locked --workspace --all-targets`
      通过且三个受影响 crate 无告警；`bongocat-platform` 新增 2 个单测（error code 唯一性、
      `is_dark` 语义），`bongocat-ui` 新增 2 个不变量测试（`only_the_system_choice_lets_the_system_decide`
      覆盖四种系统外观、`the_native_and_component_halves_pin_together` 覆盖两半同时固定），两者均以
      变异测试确认有牙齿后还原；Windows 分支经隔离 crate `/tmp/theme-win-check`
      （`raw-window-handle 0.6.2` + `windows 0.62.2`，同 features）按
      `--target x86_64-pc-windows-msvc` 单独 `cargo check` 与 `cargo clippy -- -D warnings` 通过
      （以临时注入 `compile_error!` 确认文件确实被编译后还原）——workspace 无法交叉编译到该目标，
      `libdeflate-sys`（来自 `oxipng`）需要 Windows C 工具链。
    - **未运行**：Windows 实机 DWM 暗色边框与暗色弹框/菜单/文件框；macOS 实机肉眼确认标题栏、
      弹框、托盘菜单、文件面板的深浅色；运行中切换系统主题后的跟随行为；`SetPreferredAppMode` 在
      Windows 10 1903 / 11 各版本上的行为。**类型正确不等于运行时正确**，因此本项保持未勾选。
    - macOS 运行时冒烟（`just dev-smoke`）已跑通且 exit 0：设置窗口打开、辅助功能桥挂上、主题代码
      在真实主线程上执行完且无 panic。**但该结果不作为主题正确性的证据**：实测确认这个 smoke 在
      macOS 上无法失败（见第 87 项），且其主题断言只验证一致性（见 ADR-0048 残余风险 11）。
    - 已知取舍：Windows 的弹框/菜单/文件框**不自绘**，接受跟随系统主题（`AllowDark` 而非
      `ForceDark`）；`SetPreferredAppMode` 是未文档化的 `uxtheme.dll` 序号 135 导出，缺失时静默
      退化；picker/prompt 的 `set_parent`（调研文档 B5）是模态归属缺陷而非主题缺陷，本次不改；
      设置页下拉切换主题时原生表面滞后一个 snapshot 轮询周期。
    - 决策记录：ADR-0048。调研报告 `docs/theme-mode-native-surface-research.md`；主题色的 crate
      边界评估见 `docs/theme-color-extraction-evaluation.md`（结论：不拆 crate）。

87. [ ] `P7-MACOS-SMOKE-EXIT-CODE`：macOS 上被记录的 smoke 失败不影响进程退出码。
    - 背景（2026-09-18，做 `P7-NATIVE-THEME-SURFACES` 时顺带发现）：`--settings-window-smoke` 在
      macOS 上无论记录多少失败都以 0 退出，因此 CI 的
      `Smoke macOS settings window lifecycle`、`Smoke hidden overlay model switching`、
      `Smoke native system menu lifecycle` 三个步骤**在 macOS 上无法失败**——它们都是裸
      `cargo run`，只靠退出码判定。这不只影响本项，而是影响所有以 macOS smoke 为证据的条目。
    - 根因（已用探针逐步确认，非推测）：`App::quit()` 在 macOS 上走
      `msg_send![NSApplication, terminate: nil]`（`gpui-pre-macos-0.3.5/src/platform.rs:557-575`），
      AppKit 的终止流程调用 `applicationWillTerminate:` → gpui 的 `will_terminate`
      （同文件 `1392`）→ `App::shutdown()`（`gpui-pre-0.3.5/src/app.rs:944`）→ 跑 `on_app_quit`
      观察者并 `block_with_timeout(SHUTDOWN_TIMEOUT)` 等待，然后 AppKit 直接 `exit(0)`。
      `NSApplication::run()` 不返回，所以 `bongocat-app/src/main.rs` 末尾的失败汇总
      （`let failures = match Arc::try_unwrap(failures)`）对 macOS **不可达**。
    - 实现（2026-09-18）：新增 macOS-only `exit_after_automated_smoke`。automated verification
      在 `on_app_quit` 调 `begin_product_shutdown` 后、等待可能无法完成的异步 `finish()` 前，检查
      `ProductShutdown.coordinator.failures`；已有失败则写入固定的 `product run failed: ...` 并
      `std::process::exit(1)`。正常产品启动（没有 smoke/diagnostic 参数）不走该路径；Windows 仍
      使用原有的 `windows_product_exit_code`。
    - 变异证据（本机 macOS arm64 release）：故意在 settings smoke 分支记录
      `forced smoke failure`，`--run-seconds 4 --settings-window-smoke` 得到 **exit 1** 且 stderr
      输出 `product run failed: forced smoke failure`；还原后干净 smoke 得到 **exit 0**。此前
      已用探针确认失败确实进入 accumulator；所有探针与变异均已还原。
    - 当前边界：如果失败只在 `ProductShutdown::finish()` 内新产生，而 AppKit 在 future 完成前终止，
      仍可能无法反映到退出码；当前 smoke 断言和绝大多数 runtime failure 都在 quit 之前已记录。
      因此本项仍保持 `[ ]`，直到 Windows cfg 编译/CI 与 finish 内失败传播有证据。
    - 退出条件：macOS 上被记录的失败使进程以非零码退出；用变异确认退出码变化；Windows 行为不回归；
      finish 内失败传播策略明确；相关 ADR/TODO 中所有以 macOS smoke 为依据的证据重新核对。
    - 依赖：无（可独立实施）。与 ADR-0048 的关系：ADR-0048 残余风险 10 已更新为当前边界，
      残余风险 11（smoke 只证明一致性，不证明解析正确）仍然有效。

## 13. 待决策清单

| 决策                                                          | 最迟完成              | 阻塞内容                           |
| ------------------------------------------------------------- | --------------------- | ---------------------------------- |
| Windows/macOS 首发 CPU 架构和 target triple                   | `P0-DOC-CONSISTENCY`  | CI、SDK 二进制、签名和安装包矩阵   |
| GPUI 默认 shader 构建工具链及上游 future-incompatibility 处置 | `P0-GPUI-PACKAGE-MAC` | 产品 workspace 和发布构建          |
| Cubism Core/Framework 版本、获取方式和再分发条款              | `P0-CUBISM`           | Live2D safe layer、CI 和公开安装包 |
| Windows 安装格式与 installer 权限模型                          | Phase 7 开始前        | 签名、升级、回滚和卸载             |
| macOS 最低系统、Intel 支持和 universal binary 策略            | Phase 1 开始前        | target、依赖、CI 和 notarization   |

每项决策必须落入 ADR 或对应设计文档，并从本表移除；不得只在聊天记录中形成结论。
