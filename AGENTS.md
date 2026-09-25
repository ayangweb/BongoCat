# BongoCat AI Working Agreement

适用于整个仓库；分析、修改或验证前先完整阅读本文件与更深层目录中的 `AGENTS.md`。

## 1. 必读文档与项目事实

按顺序阅读：

1. `AGENTS.md`
2. `docs/technical-design.md`
3. `docs/implementation-todo.md`
4. 当前任务相关的 ADR、fixture、schema 和源码

- Technical Design 是目标架构的事实来源；Implementation TODO 是顺序、完成定义和验收门槛的事实来源。冲突时必须指出并修正文档或请求确认，不得自行选择。
- Technical Design 只描述当前 Rust + GPUI 目标，不写历史路线、候选方案比较或已放弃设计；历史细节只放在 migration、ADR 背景和 TODO。
- 项目是 Rust 2024 桌面应用：GPUI 设置 UI + Product/Live2D runtime；Windows 使用 Raw Input、Win32、D3D11；macOS 使用 CGEventTap、AppKit、Metal；共享 schema、资源与 fixture 保持平台无关。
- 首发平台为 Windows 10 1903+ 和 macOS 12+；Linux 仅作首发后评估，不阻塞 Windows/macOS。
- Windows 仅支持 `x86_64-pc-windows-msvc`；不得新增 i686 或 `aarch64-pc-windows-msvc` 的构建、测试或安装包。Windows on ARM 通过 x64 仿真运行。
- “纯 Rust”指自有应用代码全部使用 Rust；官方 Cubism Core 平台二进制是唯一允许的厂商 FFI 例外。业务逻辑不得放入 SDK bridge。
- Bundle ID 固定为 `com.ayangweb.bongo-cat`。Development 与 Production 共用数据结构、使用不同存储根；开发构建不得读取、写入或锁住生产数据。配置使用自定义 `snake_case` 字段，不兼容或导入旧 Tauri/Pinia 配置。

## 2. 任务流程

### 2.1 实现决策阶梯

先理解需求和现有流程，再按顺序选择第一个可行方案：

1. 现有产品行为或配置已满足时不新增能力，遵循 YAGNI。
2. 复用仓库已有 helper、contract、类型或模式。
3. 使用 Rust 标准库。
4. 使用操作系统原生能力。
5. 使用当前 workspace 已安装依赖。
6. 采用经调研、可靠、维护良好的第三方库或开源方案。
7. 以上都不满足，或引入成本明显更高时，才编写满足当前需求的最小自有代码。

- 第三方方案须通过第 6 节的版本、许可证、维护状态、平台支持、unsafe 面积和替换边界评审。
- 不复实现已有通用能力，不引入不可靠或不成比例的依赖；复用不得让第三方类型泄漏进项目公共 API。

### 2.2 开始前

1. 检查当前分支和 `git status`，保留全部用户修改。
2. 定位 TODO 中对应阶段、任务和退出门槛。
3. 阅读现有实现与测试，不从文档标题推断行为。
4. 明确最小交付范围、平台范围和验证方式。
5. 跨越阶段门禁时先完成前置 spike；否则明确报告阻塞，不得绕过。

### 2.3 实施

1. 遵循既有模块边界和命名。
2. 改动只覆盖请求和当前 TODO 项。
3. 先完成最小闭环，再扩展；不同时铺开多个未验证子系统。
4. 平台差异只放在 platform adapter 或 `cfg` 模块，不扩散到 runtime、model、config、UI。
5. 新依赖说明用途、维护状态、许可证和替换边界。
6. 不做无关重构、批量格式化；不覆盖用户未提交修改。

### 2.4 完成前

1. 运行与风险相称的格式化、静态检查、单元测试和平台 smoke test。
2. 检查正常、错误、重启和 shutdown 路径。
3. 检查依赖方向、平台类型泄漏、`unsafe` 范围和日志隐私。
4. 只有完成定义完整满足并有证据时才勾选 TODO checkbox。
5. 架构、行为协议或退出条件变化时同步 Technical Design、TODO 和 ADR。
6. 最终报告列出改动、验证、未运行测试、已知风险和下一项未完成任务。

## 3. 阶段与 `next`

- 当前为 Phase 0 证据补齐与 Phase 1 渐进实现并行；ADR-0011 授权在外部证据未齐时建立正式 workspace。除非用户改变顺序，按此顺序推进：行为/配置/模型资源考古 → fixture 与规范化 snapshot → GPUI 设置窗口 spike → GPUI 与独立 overlay 共存 spike → Windows 输入可靠性 spike → macOS 输入权限与恢复 spike → Cubism + D3D11/Metal spike → Phase 0 go/no-go。
- 经自动化 contract 验证且不依赖缺失外部证据的模块可进入正式实现。每次只推进一个最小产品闭环，并持续维护 Phase 0/发布证据。
- Phase 0 退出前：不实现完整设置 UI；不批量迁移历史功能；未经授权不删除历史源码和行为对照；不宣称 Live2D、输入可靠性或双平台渲染完成；不为目录美观预建大量空 crate。
- Phase 0 未完成不阻止正式 workspace、runtime、config、model contract 或最小产品窗口。stable 发布仍需 Cubism 书面授权、SDK 分发、实机输入、UI/主题、GPU、签名、Windows 实机安装/升级/卸载和 soak 证据；缺失时不得分发含受限 artifact 的安装包。
- `next` 只面向全新的首版，不考虑历史版本兼容和迭代迁移。当前完整数据结构统一使用 `schema_version: 1`；`next` 开发期的中间结构或版本号也不兼容。
- 删除并禁止新增迁移、schema 兼容、旧数据转换和历史版本判断。新增字段直接更新当前 v1 schema、默认值、fixture 和实现，不增加中间版本分支、转换器或 fallback。
- 保留显式 `schema_version` 和单一版本解析入口；非 v1 明确拒绝且不自动转换。`next` 首次正式发布后，后续版本才可基于该版本新增顺序、幂等迁移，并单独建立设计、测试和发布门禁。

## 4. 架构边界

### 4.1 GPUI

- 只负责设置、模型管理、快捷键、权限、更新和诊断 UI。
- `Entity` 只保存表单草稿、选择、导航等临时视图状态；不持有 pressed state、动画状态、Cubism model 或主猫 GPU 资源。
- 不驱动 Live2D frame loop，不接入 GPUI renderer 私有接口。
- UI 通过强类型 command 请求 runtime，通过带 revision 的 snapshot 显示结果。
- UI executor 不执行阻塞文件、模型解析或 GPU 工作，不持有 runtime 写锁。

### 4.2 Runtime

- Runtime 是配置、输入、动画和当前模型状态的唯一事实来源，由单一 owner 管理可变业务状态。
- Command、InputEvent、RuntimeSnapshot、RenderSnapshot 必须强类型；禁止 `set_value(path, any)`、弱类型 JSON 业务消息和字符串事件协议。
- 时间逻辑使用可注入的单调时钟；不得用墙上时间驱动动画或输入延迟。

### 4.3 Renderer 与 Overlay

- 主猫使用独立原生 overlay，不嵌入 GPUI renderer；Windows 使用 D3D11，macOS 使用 Metal。
- Renderer 只消费不可变 RenderSnapshot，不读配置、不决定动作、不访问 GPUI Entity。
- Overlay 的窗口、GPU、frame source 必须有明确 owner 和析构顺序。
- Shutdown 顺序：阻止新 frame tick → 停止输入生产者 → 确认 frame source 退出 → 停止 runtime → flush 配置 → 停止音频并 join → 释放 renderer/GPU → 销毁 overlay → 关闭 GPUI。

### 4.4 平台模块

- 共享业务 crate 不导入 Win32、Objective-C、GPUI 或 GPU handle。
- Windows/macOS API 封装在 `bongocat-platform` 或明确的平台子模块，返回稳定的项目类型和 error code，不泄漏裸指针或平台消息结构。
- 主线程限定、COM apartment、run loop 和 callback 生命周期写入 wrapper 的安全不变量。
- Linux 仅作首发后评估，不得通过 cfg fallback 维持编译；禁止 `#[cfg(any(target_os = "macos", target_os = "windows"))]` 及其反向用法，共享桌面代码应使用直接 cfg、`test` 或单一平台条件，由 `tools/tests/test_supported_platform_cfg.py` 强制。

## 5. 输入可靠性

Issue #47 的“按下后无释放”必须从架构处理，不能只增加动画超时。

- Key/button down/up、设备连接/断开和 command 使用可靠、有序队列；溢出可观测、计数并触发安全恢复，不得静默丢失边沿事件。
- 鼠标移动和手柄轴可合并为 latest value，但不得阻塞 key/button release；每个 pressed key 最终必须由 KeyUp、状态校正或 Reset 释放。
- Windows：Raw Input 是键鼠主路径；正确处理 scan code、E0/E1、左右修饰键和 `RI_KEY_BREAK`；用 `GetAsyncKeyState` 校正 pressed set；锁屏、睡眠、设备移除、输入桌面变化和服务重启必须 Reset；低级 hook 只作补充，不得成为 pressed state 的唯一来源；回归 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回。
- macOS：使用 listen-only CGEventTap 和明确的 TCC 权限状态；tap timeout、disable、权限变化、session 变化必须可恢复；用 `CGEventSourceKeyState` 校正 pressed set；锁屏、睡眠、快速用户切换和 tap 重建必须 Reset。

## 6. Cubism、FFI 与依赖

- 使用官方 Cubism Core 平台二进制，并记录版本、来源、hash、架构和许可证。
- Raw binding 只在 sys/wrapper 边界，原始指针不得离开 safe wrapper；Rust owner 保证 Moc、Model、buffer、texture 和 renderer 的存活/析构顺序。
- 模型切换使用 prepare → validate → commit；失败保留当前可用模型。
- 模型、motion、expression、physics、pose、mask 行为由 fixture 验证；完成三个预置模型 spike 前不得宣称 Cubism 兼容完成。
- 不得加入长期非 Rust 业务 bridge 绕过 Phase 0 go/no-go。维护者已授权将固定版本的 Cubism Core、header、生成 bindings 和三个预置模型提交到根目录 `vendor/` 与 `resources/`；公开发布前仍需完成 attribution、再分发清单和最终合规核对。
- `bongocat-runtime`、`bongocat-config`、`bongocat-model`、`bongocat-ui` 默认 `#![forbid(unsafe_code)]`。`unsafe` 只允许在平台 API、GPU 和 Cubism 边界。
- 每个非平凡 `unsafe` block 前说明调用方安全不变量；优先小型 RAII wrapper，不传播裸 handle、裸指针或手工析构。
- 不用 `unsafe impl Send/Sync` 绕过线程模型；须证明平台对象允许跨线程。FFI callback 不阻塞，不 panic 穿越 FFI。
- GPUI 使用 Technical Design 指定的精确版本并提交 `Cargo.lock`。新增或升级 crates.io 依赖前，用 `cargo search <crate> --limit 1`、`cargo info <crate>` 或 crates.io API 核对最新非 yanked 稳定版；默认精确 pin，不因少改 API 主动选旧版。
- 最新稳定版若不支持既定 Rust toolchain、target、许可证或安全边界，在相关 Phase 文档记录阻塞版本、原因、上游 owner 和解除条件，不只写代码注释。
- 修改 manifest 后对整个 workspace 执行 `cargo update`，同步 `Cargo.lock` 中可解析的传递依赖；受上游约束保留的旧版本须可由 `cargo tree --invert` 解释。
- 依赖审计只覆盖正式 workspace 和离线工具，不引入历史 Tauri workspace 依赖。禁止 `version = "*"` 和未固定 revision 的 git dependency。
- 不依赖 Zed 应用内部 crate 或私有 GPUI renderer 接口。新依赖检查许可证、近期维护、平台支持、unsafe 面积和替换成本。
- 系统能力先寻找满足边界的成熟 crate，再考虑 `windows-rs`、`objc2` 等基础 binding；第三方事件、错误、配置、平台类型不得进入项目公共 API。
- Cubism artifact 必须来自已固定版本和 hash 的基线；升级、替换或新增时同步 provenance、目标 ABI 和模型验证。

## 7. 配置、文件安全与更新

- 配置显式包含 `schema_version: 1`，`next` 只接受当前完整 v1；不做迁移、兼容转换、旧 Tauri/Pinia 探测、字段 alias、自动导入或目录 fallback。
- JSON key 使用 `snake_case` 和当前产品领域名称。
- 构建产物固定携带 Development/Production 环境，运行时输入不得切换。
- 配置、状态、模型、备份、日志、锁、单实例命名和更新 channel 全部按环境隔离。
- 写入使用同目录临时文件、flush 和原子替换；失败保留原文件和备份。
- `config.json` 损坏时按“最新有效备份 → 默认配置”处理；无 backup 时写入并使用默认配置。不得新增恢复窗口、恢复提示、restore command、operational gate 或“需重启”流程（ADR-0054）；非 v1 仍走严格版本入口。
- 模型导入防止路径穿越、符号链接逃逸、绝对路径注入、压缩炸弹和静默覆盖；文件选择结果在 Rust 侧复验。
- 更新只允许 HTTPS，校验版本、target、arch 和签名并提供失败回滚。独立 hash 校验当前无实现，`update_checksum_mismatch` 无产出路径；恢复需新建 ADR，恢复前不得宣称满足。
- 日志不得记录真实按键序列、剪贴板、用户文件内容或密钥；日志和备份必须有大小、数量和保留期限上限。

## 8. GPUI Kit UI

GPUI 设置界面统一使用上游 `longbridge/gpui-kit` 的固定 revision `500852f449c05dc01920ec82f3ae2656a61d0387`（当前 package 版本 `0.6.5`），并作为唯一直接 GPUI 依赖；不得直接声明 `gpui`、`gpui_platform`、`gpui-component` 或单独 assets crate，也不得混入其它 GPUI git source。该 revision 是上游合并 `SettingGroup::variant()` 与 `Popover::arrow()`、但尚未发布对应 crates.io release 时的临时精确来源；上游 release 包含这两项能力后必须切回 crates.io 精确 pin 并删除 git source。GPUI Kit 通过 crates.io 的 `gpui-pre` 同步包提供 crate 名 `gpui`，元数据对应 Zed `gpui 0.2.2`。开发前必须查阅 [gpui-kit](https://github.com/longbridge/gpui-kit)、[组件文档](https://gpui-kit.com/docs/components) 和 [docs.rs](https://docs.rs/gpui-kit/)，不得凭记忆或猜测 API。

- 类型从 `gpui_kit` 根导出，平台、组件、资源分别从 `gpui_kit::platform`、`gpui_kit::component`、`gpui_kit::assets` 使用；仅业务特殊行为或无等价 primitive 时保留薄封装。
- 创建组件前调用 `gpui_kit::init(cx)`；窗口根视图使用 `gpui_kit::component::Root`；系统外观变化用 `Theme::sync_system_appearance(Some(window), cx)`。
- 短暂反馈使用 `gpui_kit::component::notification::Notification`；`Root` 自动挂载 dialog、sheet 与 notification layer，业务根视图不得再手动渲染这些 layer；通过 `Theme::global_mut(cx).notification.placement = Anchor::BottomRight` 固定右下角。通知须含可操作上下文，例如快捷键冲突写出实际 chord，不用页面内临时错误文本替代。
- 语义色通过 `ActiveTheme::theme()` 读取。业务代码不重复硬编码默认颜色、字号、间距、圆角或控件高度；无明确产品需求时使用组件默认值和 `Theme`。
- 常用组件 API：`Button::new(id).label(label)`、`Switch::new(id).checked(bool)`、`Checkbox::new(id).checked(bool)`、`Radio::new(id)`、`InputState::new(window, cx)`、`Input::new(&state)`、`NumberInput::new(&state)`、`Select`、`Slider`、`TabBar`、`Separator::horizontal()`、`Badge::new()`、`Progress`、`Icon`、`Tooltip`、`Dialog`、`Menu`。
- `Input`/`NumberInput` 绑定 `Entity<InputState>`；文本编辑订阅 `InputEvent`，数字步进订阅 `NumberInputEvent`。范围、步长和最终校验来自业务 schema，组件事件不得绕过 typed command。
- 组件已在其输入边界内限制范围、步长或格式时，不要再为同一不可达条件新增专用错误码、国际化错误文案或描述；保留 schema/typed command 的防御性校验，只有非 UI 路径可触发且用户需要不同处理时才增加独立错误码和本地化提示。
- 迁移组件后删除无用的直接 GPUI 依赖、自定义绘制和模块导出；在 TODO 记录已迁移组件、剩余特殊控件和官方文档依据。
- 设置界面安静、紧凑、适合重复操作；建立项目 design tokens 和基础控件；图标表达常用工具并提供可见 tooltip；控件覆盖 hover、active、focus、disabled、loading、error。
- 表单支持键盘导航和可见焦点。采用 visual-first 契约：项目不维护 screen-reader/AccessKit tree、辅助桥、隐藏 label/action 或仅供辅助技术的文案；除 `gpui-kit` 传递实现外不新增直接 AccessKit 依赖（ADR-0054）。
- Development、Production、smoke 的设置窗口使用同一套可见组成；smoke 只改变驱动方式。
- 支持浅色、深色、系统主题和现有本地化语言。文案遵循 `docs/localization-copy-conventions.md`：省略号只用单个 `…`（U+2026），禁止 `……` 和 ASCII `...`，由 `tools/validate-locales.py` 强制。
- 800x600、Windows 125/150/200%、macOS Retina 下不得重叠、裁剪或布局跳动；页面必须有真实 loading、empty、error、cancel 和 retry 状态，不用静态占位冒充完成。

## 9. 测试、构建与文档维护

### 9.1 默认验证

按改动范围运行：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace --release
```

平台功能必须在对应平台运行 smoke test；不得用 macOS 编译推断 Windows Raw Input/D3D11，也不得反向推断 macOS CGEventTap/Metal。

### 9.2 构建与打包

`just` 是唯一构建入口，`build` recipe 只把参数转发给 `crates/bongocat-packaging`。该 crate 是产品构建、打包和发布的唯一事实来源，负责编译、build provenance，并将 bundle/installer 交给 `cargo-packager`。

```text
just build                                              # Production，本机 target，全部产物
just build --target x86_64-apple-darwin                 # 指定 target
just build --environment development --formats app       # 只要 Development .app
just manifest target/package target/package/*.json      # 合并 per-target manifest fragment
just keygen ~/.bongocat/release.key                     # 一次性生成更新签名密钥对
just version                                             # 唯一产品版本号来源
just schema                                              # 从 Rust 类型生成当前 JSON Schema
```

- 不在 `Justfile`、CI、文档中重实现平台判断、目录复制、`Info.plist` 注入、`.app`/`.dmg`/NSIS 组装或版本解析。
- 不重新引入 `scripts/` 或自维护 `.nsi`；`windows/installer/` 已删除。
- 版本号仅来自 `[workspace.package].version`。平台与产物集合由 `crates/bongocat-packaging` 声明，且与 `tools/tests/test_packaging_contract.py`、`deny.toml`、`bongocat-update::UpdateTargetTriple` 保持一致。
- 更新载荷组装与签名属于该 crate。`SIGNING_PRIVATE_KEY` 和 `SIGNING_PRIVATE_KEY_PASSWORD` 是唯一注入点；未设置则跳过签名，release workflow 断言发布构建已签名。签名必须是最后一步，签名后不得改名、重压缩或 strip。载荷形状、平台键和 manifest 键须与 `bongocat-update` 运行时常量一致，由 `tools/tests/test_update_release_contract.py` 强制。
- 签名密钥通过 `just keygen <file>` 使用同一份精确 pin 的 `cargo-packager` 离线生成，不在 CI 运行，不需要全局 `cargo install`；私钥不得进入源码、产物或日志，公钥写入 `bongocat-update::RELEASE_SIGNING_KEY`。
- 每次 `just build` 只写当前 target 的 fragment（`<os>-<arch>.json`）；运行时只请求共享 `latest.json`，多 target 发布必须先执行 `just manifest`。不在 CI 或脚本拼 manifest JSON；fragment 是构建输入，不是发行资产。
- 细节与退出条件见 `docs/adr/0033-build-packaging-and-release-toolchain.md` 和 `docs/adr/0034-detached-minisign-update-trust-model.md`。

### 9.3 不变量与完成定义

- 压力测试 key/button edge 丢失计数为 0；任何 pressed state 最终由 release、reconcile 或 reset 清除。
- Renderer 不阻塞 runtime，鼠标移动不阻塞释放边沿。
- 模型加载/切换失败不破坏当前可用模型。
- 配置写入或损坏恢复不丢失当前环境的可用配置或用户模型。
- 退出不依赖强杀，所有 worker 在超时内 join。
- 8 小时 soak 无持续内存、GPU、handle、线程或日志增长。
- scaffold 不等于功能完成；编译通过不等于平台行为完成；单个模型通过不等于模型兼容完成；增加超时不等于修复 issue #47；未验证签名、权限和回滚不等于可发布。
- 无法运行的测试必须在最终报告说明原因和残余风险。

### 9.4 文档与 TODO

- 架构决策、约束变化和 go/no-go 结果写入 `docs/adr/`；Benchmark 方法和结果写入 `docs/benchmark/`；历史考古写入 `docs/migration/`。
- Technical Design 只描述当前目标架构；不得把旧配置兼容重新纳入产品范围。
- 面向用户或贡献者的根级文档同时维护英文与简体中文版本：`README.md` / `README.zh-CN.md`、
  `CONTRIBUTING.md` / `CONTRIBUTING.zh-CN.md`，以及 `CHANGELOG.md` / `CHANGELOG.zh-CN.md`。
  英文文件不带语言后缀；修改一份文档时必须同步另一份对应版本。
- 公开文档只说明产品行为、用户流程和必要操作，不展示内部项目代号、迁移叙事、实现技术栈的重复
  强调或不必要的源码文件名。开发背景和实现约束由 `AGENTS.md`、Technical Design、TODO 与 ADR
  承载；公开文档仅保留受保护的 `pre-refactor-tauri` 分支链接，供历史参考。
- `CHANGELOG.md` / `CHANGELOG.zh-CN.md` 是面向用户的版本通知，必须保留替换、迁移、重写、默认行为
  变化和升级注意事项等事实，不得按公开文档的精简规则删除实现替换说明。
- 仓库自有文件名以及用户、贡献和运维文档不使用历史重写阶段的名称、kebab-case 变体或同义的
  内部项目限定词；文件名直接表达职责。普通文案中的 `native` 仅用于操作系统原生能力或 Cubism
  Native 官方产品名；既有 Rust 类型与 API 边界不因文档清理顺带重命名。
- TODO checkbox 仅在完成定义完整满足后改成 `[x]`；部分完成保持 `[ ]`，并在其下写明状态和剩余工作。
- 新任务放入正确 phase，标明依赖和退出条件，不在文档末尾堆放无归属事项。

### 9.5 CHANGELOG 编写规则

- 当前分支为 `next` 时，编写 changelog 前必须先对比重构前后的代码与功能，确认功能的实际新增、修改或修复；不得把重构前已经存在且没有实质变化的功能重复写入 changelog，只记录本次重构带来的变化。
- 当前分支不是 `next` 时，无需进行上述重构前后对比，相关新增、修改或修复内容直接写入 changelog。
- changelog 中没有对应的下一个版本标题时，统一将内容写入 `Unreleased`，不得自行创建版本标题。

## 10. 历史源码、Git 与交付

- 历史源码只作行为考古和模型兼容参考；TODO 10.3 后只保留在受保护的远端
  `pre-refactor-tauri` 分支。`next` 合并进入 `master` 后，`master` 承载当前代码，不作为旧实现参考。
- 不在远端历史实现上扩展 BongoCat，不重新接入当前工作树或产品依赖图；读取配置和模型样本不得原地修改；结论须由实际代码、配置或实机行为证明。
- 上游 [MMmmmoko/Bongo-Cat-Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver) 是输入、模型装配、Live2D 更新顺序和产品行为的固定参考。先查阅 `docs/migration/bongo-cat-mver-reference.md` 的 commit 和关键文件，再结合 Technical Design 与 ADR-0030；不得直接复制 C++ 业务代码或让旧架构覆盖当前边界。
- 当前目标分支是 `next`。未经用户要求不创建、删除、重命名或切换分支；不 reset、checkout、覆盖或格式化无关文件。
- 未经用户明确要求，不 commit、push 或 release。要求 commit 时先检查分支：
  - `master`：禁止直接提交；按实际改动创建 `<type>/<short-topic>` 语义分支后提交，例如 `feat/native-overlay`、`fix/input-release`、`docs/phase-0-plan`。
  - `next`：直接在 `next` 提交，不询问切换。
  - 其他分支：用户未明确当前分支或新分支时，先询问；得到答复前不得 commit。用户已指定时照做，不重复询问。
- Commit message 必须依据实际 staged diff，遵循 Conventional Commits 的 `<type>: <summary>`；只有 scope 有明确区分价值时使用 `<type>(<scope>): <summary>`。
- 常用 type：`feat`、`fix`、`docs`、`test`、`refactor`、`perf`、`build`、`ci`、`chore`；不得使用 `update`、`changes`、`misc`。summary 用简洁英文祈使句、不加句号、建议不超过 72 字符；不兼容变更加 `!`，正文写 `BREAKING CHANGE:`。提交前核对 staged diff，排除未暂存或无关改动。
- 执行 `git push` 前必须先完成 CHANGELOG 检查。判断范围为待推送 commit（`git log @{u}..HEAD`；无上游时用新增 commit）及其 diff：存在用户可见的新增、移除、默认值/行为/性能/UI/平台/本地化/配置格式变化、不兼容或升级注意事项时，同步更新 `CHANGELOG.md` 和 `CHANGELOG.zh-CN.md`，沿用现有 section 和文案风格；纯内部重构、测试、CI、格式化、注释、构建、依赖调整和未落地实现跳过，不新增临时 section，不为每次 push 强行添加。
- CHANGELOG 判断结论必须写入最终报告：说明更新内容，或明确“本次跳过 CHANGELOG”及理由。CHANGELOG 修改必须先提交，再 push。
- 不修改无关 lockfile、生成文件或资产 metadata。删除用户数据、历史源码、大型资源或构建产物前，确认任务明确授权并核对精确目标。
- 最终回复引用实际修改文件，说明验证命令和结果；文档任务无需声称运行代码测试。
