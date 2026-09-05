# BongoCat Native Rewrite - AI Working Agreement

本文件适用于整个仓库。任何 AI 在分析、修改或验证本项目之前，都必须先完整阅读本文件，并遵守更深层目录中可能存在的附加 `AGENTS.md`。

## 1. 项目目标

BongoCat Native Rewrite 是一个以 Rust 2024 edition 实现的桌面应用：

```text
Rust Application
├── GPUI settings UI
├── Product Runtime
├── Live2D Runtime
├── Windows: Raw Input + Win32 + D3D11
├── macOS: CGEventTap + AppKit + Metal
└── Shared schemas, resources and fixtures
```

首发平台是 Windows 10 1903+ 和 macOS 12+。Linux 只属于首发后的评估范围，不得阻塞 Windows/macOS 工作；共享业务模块仍须保持平台无关。

Windows Native Rewrite 只面向 `x86_64-pc-windows-msvc` 与 `aarch64-pc-windows-msvc`，不得新增 i686 构建、测试或安装包。Windows ARM64 虽是产品目标，但当前 Cubism Native R5 缺少 desktop ARM64 Core；在官方可授权 artifact 通过真实 ABI 与模型验证前必须保持发布阻塞，不得用 UWP DLL 或模拟结果冒充支持。

“纯 Rust”指 BongoCat 自有应用代码全部使用 Rust。官方 Cubism Core 平台二进制是唯一允许的厂商 FFI 例外。不得把 BongoCat 业务逻辑放入 SDK bridge。

Native Rewrite 的 Bundle ID 固定为 `com.ayangweb.bongo-cat`。Development 与 Production 使用相同数据结构和不同存储根；任何开发构建不得读取、写入或锁住生产数据。新配置使用自己的 `snake_case` 字段，不兼容或导入旧 Tauri/Pinia 配置。

## 2. 必读文档

开始任务前按顺序阅读：

1. `AGENTS.md`
2. `docs/BongoCat Native Rewrite Technical Design.md`
3. `docs/BongoCat Native Rewrite Implementation TODO.md`
4. 与当前任务直接相关的 ADR、fixture、schema 和源码

Technical Design 是目标架构的事实来源。Implementation TODO 是工作顺序、完成定义和验收门槛的事实来源。两者冲突时不得自行选择其一；先指出冲突并修正文档或请求用户确认。

Technical Design 只描述当前 Rust + GPUI 目标，不写入历史技术路线、候选方案比较或已放弃设计。历史实现细节只允许出现在迁移文档、ADR 背景和实施 TODO 中。

## 3. AI 每次任务的工作流程

### 3.1 开始前

1. 检查当前分支和 `git status`，保留所有用户修改。
2. 定位 TODO 中与请求对应的阶段、任务和退出门槛。
3. 阅读现有实现和测试，不凭文档标题猜测代码行为。
4. 明确本次最小交付范围、平台范围和验证方式。
5. 若任务会跨越阶段门禁，先完成前置 spike 或明确报告阻塞，不得绕过门禁。

### 3.2 实施时

1. 优先遵循仓库既有模块边界和命名。
2. 修改范围必须与用户请求和当前 TODO 项直接相关。
3. 先实现最小闭环，再扩展功能；不得同时铺开多个尚未验证的子系统。
4. 平台差异放入 platform adapter 或 `cfg` 模块，禁止扩散到 runtime、model、config 和 UI。
5. 任何新依赖都要说明用途、维护状态、许可证和替换边界。
6. 不进行无关重构，不批量改格式，不覆盖用户未提交修改。

### 3.3 完成前

1. 运行与改动风险相称的格式化、静态检查、单元测试和平台 smoke test。
2. 检查正常、错误、重启和 shutdown 路径。
3. 检查依赖方向、平台类型泄漏、`unsafe` 范围和日志隐私。
4. 只有满足 TODO 的完整完成定义并有证据时，才勾选对应 checkbox。
5. 若实现改变架构、行为协议或退出条件，同步 Technical Design、TODO 和 ADR。
6. 最终报告必须列出改动、验证结果、未运行的测试、已知风险和下一项未完成任务。

## 4. 当前阶段与执行顺序

当前处于 Phase 0 证据补齐与 Phase 1 渐进实现并行阶段。ADR-0011 已授权在外部
证据尚未齐全时建立正式 workspace；除非用户明确改变顺序，按以下顺序推进：

1. 行为、配置和模型资源考古
2. fixture 与规范化 snapshot 格式
3. GPUI 设置窗口 spike
4. GPUI 与独立 overlay 共存 spike
5. Windows 输入可靠性 spike
6. macOS 输入权限与恢复 spike
7. Cubism + D3D11/Metal spike
8. Phase 0 go/no-go 评审

已由自动化 contract 验证且不依赖缺失外部证据的模块可以进入正式实现。每次只
推进一个最小产品闭环，并继续维护对应 Phase 0/发布证据。

Phase 0 退出条件未满足前：

- 不实现完整设置 UI。
- 不批量迁移历史功能。
- 不删除历史源码和行为对照。
- 不声称 Live2D、输入可靠性或双平台渲染已经完成。
- 不为了目录美观提前创建大量空 crate。

Phase 0 未完成不再阻止正式 workspace、runtime、config、model contract 或最小
产品窗口的实现。Cubism 书面授权、SDK 分发、Windows ARM64 Core、实机输入、
辅助功能、GPU、签名和 soak 证据是 stable 发布门禁；缺失时不得生成或公开分发
包含受限 artifact 的安装包。

### 4.1 `next` 初始版本原则

- 核心原则：`next` 分支只面向当前全新的初始版本，不考虑任何历史版本兼容和迭代迁移。
- 只要当前处于 `next` 分支开发，就不需要考虑任何迭代、版本兼容或数据迁移相关逻辑。
- `next` 视为一次全新的初始版本开发；当前完整数据结构统一使用 `schema_version: 1`，不兼容
  `next` 开发过程中曾出现过的中间结构或版本号。
- 删除并禁止新增版本迁移、schema 兼容、旧数据转换、历史版本兼容判断等迭代逻辑；新增字段时
  直接修改当前 v1 schema、默认值、fixture 和实现。
- 相关实现全部按当前完整的初始 v1 状态实现，不为了兼容任何开发中间版本额外增加分支、转换器
  或 fallback；“回到初始版本”不表示删除已经确定属于首版的产品字段。
- 后续在 `next` 分支新增功能时，也不得引入任何针对旧版本或开发中间版本的兼容代码。
- 保留显式 `schema_version` 字段和单一的当前版本解析入口，非 v1 数据明确拒绝且不得自动转换；
  这只是首版格式边界，不是兼容实现。
- `next` 首次正式发布后，后续版本才允许以该发布版本为迁移基线新增顺序、幂等迁移；相关设计、
  测试和发布门禁必须在当时单独建立，不得提前把兼容代码带入 `next`。

## 5. 架构边界

### 5.1 GPUI

- GPUI 只负责设置、模型管理、快捷键、权限、更新和诊断 UI。
- GPUI `Entity` 只保存表单草稿、选择、导航和其他临时视图状态。
- GPUI 不持有 pressed state、动画状态、Cubism model 或主猫 GPU 资源。
- GPUI 不驱动 Live2D frame loop，不接入 GPUI renderer 私有接口。
- UI 通过强类型 command 向 runtime 发请求，通过带 revision 的 snapshot 显示结果。
- UI executor 不执行阻塞文件、模型解析或 GPU 工作，也不持有 runtime 写锁。

### 5.2 Runtime

- Runtime 是配置、输入、动画和当前模型状态的唯一事实来源。
- 单一 runtime owner 管理可变业务状态。
- Command、InputEvent、RuntimeSnapshot 和 RenderSnapshot 必须是强类型接口。
- 禁止通用 `set_value(path, any)`、弱类型 JSON 业务消息或字符串事件协议。
- 时间相关逻辑使用可注入的单调时钟；不得用墙上时间驱动动画和输入延迟。

### 5.3 Renderer 与 Overlay

- 主猫使用独立原生 overlay，不嵌入 GPUI renderer。
- Windows renderer 使用 D3D11；macOS renderer 使用 Metal。
- Renderer 只消费不可变 RenderSnapshot，不读取配置，不决定动作，不访问 GPUI Entity。
- Overlay 的窗口、GPU 和 frame source 必须具有明确 owner 和析构顺序。
- Shutdown 顺序是：阻止新的 frame tick -> 停止输入生产者 -> 确认 frame source 已退出 ->
  停止 runtime -> flush 配置 -> 停止音频并 join -> 释放 renderer/GPU -> 销毁 overlay -> 关闭 GPUI。

### 5.4 平台模块

- 共享业务 crate 不得导入 Win32、Objective-C、GPUI 或 GPU handle。
- Windows/macOS API 封装在 `bongocat-platform` 或明确的平台子模块。
- 平台 API 返回稳定的项目类型和 error code，不向上泄漏裸指针或平台消息结构。
- 主线程限定、COM apartment、run loop 和 callback 生命周期必须写入 wrapper 的安全不变量。

## 6. 输入可靠性是硬约束

Issue #47 的“收到按下但未收到释放”必须从架构上处理，不能只增加动画超时。

### 6.1 事件通道

- Key/button down/up、设备连接/断开和 command 使用可靠、有序队列。
- 队列溢出必须可观测、计数并触发安全恢复；禁止静默丢弃边沿事件。
- 鼠标移动和手柄轴可以合并为 latest value，但不得阻塞 key/button release。
- 每个 pressed key 最终必须由 KeyUp、状态校正或 Reset 释放。

### 6.2 Windows

- Raw Input 是键鼠主路径。
- 正确处理 scan code、E0/E1、左右修饰键和 `RI_KEY_BREAK`。
- 对 pressed set 使用 `GetAsyncKeyState` 做释放状态校正。
- 锁屏、睡眠、设备移除、输入桌面变化和服务重启必须 Reset。
- 低级 hook 只能是补充来源，不能成为 pressed state 的唯一事实来源。
- PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回是强制回归用例。

### 6.3 macOS

- 使用 listen-only CGEventTap 和明确的 TCC 权限状态。
- tap timeout、disable、权限变化和 session 变化必须可恢复。
- 对 pressed set 使用 `CGEventSourceKeyState` 校正。
- 锁屏、睡眠、快速用户切换和 tap 重建必须 Reset。

## 7. Live2D 与 Cubism

- 使用官方 Cubism Core 平台二进制，并记录版本、来源、hash、架构和许可证。
- Raw binding 只存在于 sys/wrapper 边界，原始指针不得离开 safe wrapper。
- Rust owner 必须保证 Moc、Model、buffer、texture 和 renderer 的存活/析构顺序。
- 模型切换使用 prepare -> validate -> commit；失败时保留当前可用模型。
- 模型、motion、expression、physics、pose 和 mask 行为必须由 fixture 验证。
- 未完成三个预置模型 spike 前，不得宣称 Cubism 兼容完成。
- 不得加入长期的非 Rust 业务 bridge 来绕过 Phase 0 go/no-go。
- 维护者已授权把固定版本的 Cubism Core、header、生成 bindings 和三个预置模型作为
  开发基线提交到 `native/vendor/` 与 `native/resources/`。授权手续不得再阻塞本地开发、
  功能实现或 `next` 提交；公开发布前仍须完成 attribution、再分发清单和最终合规核对。

## 8. `unsafe` 与 FFI

- `bongocat-runtime`、`bongocat-config`、`bongocat-model` 和 `bongocat-ui` 默认使用 `#![forbid(unsafe_code)]`。
- `unsafe` 只允许出现在平台 API、GPU 和 Cubism 边界。
- 每个非平凡 `unsafe` block 前必须说明调用方需要维护的安全不变量。
- 优先创建小型 RAII wrapper，不在业务代码传播裸 handle、裸指针或手工析构。
- 不用 `unsafe impl Send/Sync` 绕过线程模型；必须证明平台对象允许跨线程。
- FFI callback 不执行阻塞工作，不 panic 穿越 FFI 边界。

## 9. 依赖规则

- GPUI 使用 Technical Design 指定的精确版本，提交 `Cargo.lock`。
- 新增或升级 crates.io 依赖前，必须使用 `cargo search <crate> --limit 1`、`cargo info <crate>` 或 crates.io API 核对当时最新的非 yanked 稳定版；默认选择该版本并精确 pin，不得为了少改 API 主动采用旧版。
- 最新稳定版若不支持项目已确认的 Rust toolchain、target、许可证或安全边界，必须在相关 Phase 文档记录阻塞版本、原因、上游 owner 和解除条件；不能只在代码注释中静默降级。
- 修改任一 manifest 后，对该独立 workspace 执行完整 `cargo update`，使 `Cargo.lock` 中所有满足上游约束的传递依赖同步到最新可解析版本；由上游精确/兼容约束阻止的旧传递版本必须可通过 `cargo tree --invert` 解释。
- 依赖版本审计以 Native Rewrite workspace 和离线工具为范围，不得借机升级或修改仅作行为对照的历史 Tauri workspace。
- 禁止 `version = "*"` 和未固定 revision 的 git dependency。
- 不直接依赖 Zed 应用内部 crate 或私有 GPUI renderer 接口。
- 新依赖必须检查许可证、最近维护情况、平台支持、unsafe 面积和停止维护后的替换成本。
- 小型系统封装优先直接使用 `windows-rs`、`objc2` 等基础 binding，不为了减少少量代码引入不可靠抽象。
- 不把第三方 crate 的事件、错误、配置或平台类型扩散为项目公共 API。
- 不提交或下载来源、版本、hash 不明确的 Cubism 二进制。
- 仓库内 Cubism artifact 必须来自已固定版本和 hash 的开发基线。升级、替换或新增
  artifact 时同步 provenance、目标 ABI 和模型验证；不得混入来源不明的 SDK 文件。

## 10. 配置、文件与安全

- 配置包含显式 `schema_version: 1`；`next` 只接受当前完整 v1，不执行迁移或兼容转换。
- 不实现旧 Tauri/Pinia 配置探测、字段 alias、自动导入或目录 fallback。
- JSON key 使用 `snake_case` 和当前产品领域名称，不沿用旧 store 字段名。
- 构建产物携带不可变的 Development/Production 环境；运行时输入不得切换环境。
- 配置、状态、模型、备份、日志、锁、单实例命名和更新 channel 全部按环境隔离。
- 写入使用同目录临时文件、flush 和原子替换；失败保留原文件和备份。
- 模型导入防止路径穿越、符号链接逃逸、绝对路径注入、压缩炸弹和静默覆盖。
- 文件选择结果必须在 Rust 侧再次验证。
- 更新只允许 HTTPS，校验版本、target、arch、hash 和签名，并提供失败回滚。
- 日志不得记录真实按键序列、剪贴板内容、用户文件内容或密钥。
- 日志和备份必须有大小、数量和保留期限上限。

## 11. UI 要求

### 11.1 GPUI Kit 组件规范

Native GPUI 设置界面统一使用 `gpui-kit = "=0.6.0"`，并以它作为唯一直接 GPUI 依赖。
不得再直接声明 `gpui`、`gpui_platform`、`gpui-component` 或单独的 assets crate，也不得通过
git source 覆写 GPUI。GPUI Kit 当前通过 crates.io 的 `gpui-pre` 同步包提供 crate 名为 `gpui`
的实现，其元数据对应 Zed `gpui 0.2.2`。开发 UI 前必须先查阅官方仓库
<https://github.com/longbridge/gpui-kit>、组件文档 <https://gpui-kit.com/docs/components> 和
<https://docs.rs/gpui-kit/> 对应专题文档；不得根据其他组件库、旧版本记忆或猜测臆造 API。

- GPUI 类型从 `gpui_kit` 根导出使用，平台、组件和资源分别从 `gpui_kit::platform`、
  `gpui_kit::component` 与 `gpui_kit::assets` 使用；只有明确的业务特殊行为或组件库没有等价
  primitive 时才保留项目内薄封装。
- 每个 GPUI 应用在创建组件前调用 `gpui_kit::init(cx)`，并以
  `gpui_kit::component::Root` 作为窗口根视图；系统外观变化通过
  `Theme::sync_system_appearance(Some(window), cx)` 同步。
- 组件语义色通过 `ActiveTheme::theme()` 读取。不在业务代码重复硬编码默认颜色、字号、间距、
  圆角或控件高度；除非有明确产品需求，让组件默认值和 `Theme` 生效。
- 常用组件及核心 API：`Button::new(id).label(label)`、`Switch::new(id).checked(bool)`、
  `Checkbox::new(id).checked(bool)`、`Radio::new(id)`、`InputState::new(window, cx)`、
  `Input::new(&state)`、`NumberInput::new(&state)`、`Select`、`Slider`、`TabBar`、
  `Separator::horizontal()`、`Badge::new()`、`Progress`、`Icon`、`Tooltip`、`Dialog` 和 `Menu`。
- `Input`/`NumberInput` 绑定 `Entity<InputState>`；文本编辑订阅 `InputEvent`，数字步进另订阅
  `NumberInputEvent`。范围、步长和最终校验仍来自业务 schema，组件事件不得绕过 typed command。
- 迁移组件后删除不再使用的直接 GPUI 依赖、自定义绘制和模块导出；更新 TODO 记录已迁移组件、剩余
  特殊控件和官方文档依据。

- 设置界面应安静、紧凑、适合重复操作，不使用营销页式布局。
- 建立项目自己的 design tokens 和基础控件，不直接依赖产品私有组件。
- 使用熟悉的图标表达工具操作，并为不熟悉图标提供 tooltip/accessibility label。
- 控件必须包含 hover、active、focus、disabled、loading 和 error 状态。
- 表单必须支持键盘导航、可见焦点和合理的辅助功能语义。
- 支持浅色、深色、系统主题和现有本地化语言。
- 在 800x600、Windows 125/150/200% 和 macOS Retina 下不得出现文本重叠、裁剪或布局跳动。
- 页面必须包含真实 loading、empty、error、cancel 和 retry 状态，不用静态占位冒充功能完成。

## 12. 测试与验收

### 12.1 默认验证命令

在相关工程骨架建立后，按改动范围运行：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace --release
```

平台功能必须在对应平台运行 smoke test。不能在 macOS 上仅凭编译推断 Windows Raw Input/D3D11 正常，也不能反向推断 macOS CGEventTap/Metal 正常。

### 12.2 必须维护的不变量

- 正常压力测试 key/button edge 丢失计数为 0。
- 任何 pressed state 最终都能由 release、reconcile 或 reset 清除。
- Renderer 不阻塞 runtime，鼠标移动不阻塞释放边沿。
- 模型加载/切换失败不会破坏当前可用模型。
- 配置写入或损坏恢复不会丢失当前环境的可用配置或用户模型。
- 退出不依赖进程强杀，所有 worker 在超时内完成 join。
- 8 小时 soak 无持续内存、GPU、handle、线程或日志增长。

### 12.3 不得虚报完成

- 只完成 scaffold 不等于功能完成。
- 只通过编译不等于平台行为完成。
- 只运行单个模型不等于模型兼容完成。
- 只增加超时不等于 issue #47 修复完成。
- 没有签名、权限和回滚验证不等于可发布。
- 无法运行的测试必须在最终报告中明确说明原因和残余风险。

## 13. 历史源码处理

当前仓库中的历史源码是行为考古和模型兼容的参考输入：

- 未到 TODO 的发布切换/退役阶段，不删除历史源码、资源或构建入口。
- 不在历史实现上继续扩展 Native Rewrite 功能。
- 可以添加最小诊断或导出工具来冻结行为，但必须与重构实现隔离。
- 已有 legacy config inspector 只作为历史考古工具保留，不得接入产品启动、设置或发布依赖。
- 读取历史配置和模型样本时不得原地修改。
- 对历史源码的结论必须以实际代码、配置文件或实机行为为证据。
- 上游原版 [MMmmmoko/Bongo-Cat-Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver)
  是输入、模型装配、Live2D 更新顺序和产品行为的固定参考。遇到相关问题时先查阅
  `docs/migration/bongo-cat-mver-reference.md` 记录的 commit 和关键文件，再结合当前
  Technical Design 独立实现；不得直接复制其 C++ 业务代码或让旧架构覆盖当前边界。

## 14. 文档与 TODO 维护

- Technical Design 只描述当前目标架构，不记录被放弃的技术路线。
- 架构决策、约束变化和 go/no-go 结果写入 `docs/adr/`。
- Benchmark 方法和结果写入 `docs/benchmark/`。
- 历史考古记录写入 `docs/migration/`，但不得把旧配置兼容重新加入产品范围。
- TODO checkbox 只有在完整完成定义满足后才能从 `[ ]` 改为 `[x]`。
- 部分完成的任务保持 `[ ]`，在其下增加简短状态和剩余工作，不得用模糊措辞标记完成。
- 新增任务放入正确 phase，并标明依赖和退出条件，不在文档末尾堆放无归属事项。

## 15. Git 与交付纪律

- Native Rewrite 的当前目标分支是 `next`。未经用户要求，不创建、删除、重命名或切换分支。
- 工作区可能包含用户修改；不得 reset、checkout、覆盖或格式化无关文件。
- 未经用户明确要求，不提交 commit、不 push、不创建 release。
- 用户要求创建 commit 时，必须先检查当前分支；该要求适用于代码、文档、配置和测试等所有提交内容：
  - 当前分支为 `master` 时，禁止直接提交。根据实际改动创建语义明确的新分支，再在新分支提交；分支名使用 `<type>/<short-topic>`，例如 `feat/native-overlay`、`fix/input-release` 或 `docs/phase-0-plan`。
  - 当前分支为 `next` 时，直接在 `next` 创建用户要求的 commit，不询问是否改用新分支。
  - 当前分支既不是 `master` 也不是 `next` 时，若用户尚未明确指定提交到当前分支还是新分支，必须先询问用户选择；得到答复前不得创建 commit。
  - 若用户已经明确指定提交到当前非 `master` 分支或指定了新分支，则按该选择执行，无需重复询问。
- Commit message 必须根据实际 staged diff 生成，并遵循 Conventional Commits。默认格式为 `<type>: <summary>`；scope 确有区分价值时才使用 `<type>(<scope>): <summary>`。
  - 常用 type 为 `feat`、`fix`、`docs`、`test`、`refactor`、`perf`、`build`、`ci` 和 `chore`；不得用含糊的 `update`、`changes` 或 `misc` 代替准确类型。
  - scope 是可选信息，不得机械添加。单一主题、仓库级改动或 summary 已足够明确时必须省略 scope。
  - 只有改动明确局限于一个稳定模块，且省略后容易与其他模块混淆时才使用 scope，例如 `fix(input): recover missing key releases`。不得仅为了强调 Native Rewrite 而固定使用 `native` scope。
  - summary 使用简洁英文祈使语气，不加句号，建议不超过 72 个字符；存在不兼容变更时使用 `!` 并在正文写明 `BREAKING CHANGE:`。
  - 提交前核对 staged diff，确保 message 描述的是实际提交内容，不包含未暂存或无关改动。
- 不修改无关 lockfile、生成文件或资产 metadata。
- 删除用户数据、历史源码、大型资源或构建产物前必须确认任务明确授权并核对精确目标。
- 最终回复引用实际修改文件，说明验证命令和结果；文档任务无需声称运行代码测试。
