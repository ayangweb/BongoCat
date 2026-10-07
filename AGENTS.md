# BongoCat AI Working Agreement

适用于整个仓库；分析、修改或验证前先完整阅读本文件与更深层目录中的 `AGENTS.md`。

本文件只保留**长期有效的约束**。事实来源分工如下，冲突时指出并请求确认，不自行选择：

| 主题                         | 事实来源                          |
| ---------------------------- | --------------------------------- |
| 目标架构                     | `docs/technical-design.md`        |
| 单项决策背景与取舍           | `docs/adr/`                       |
| 依赖与工具校验               | `deny.toml`、`tools/`、`justfile` |

## 1. 项目事实

- Rust 2024 桌面应用：GPUI 设置 UI + Product/Live2D runtime。Windows 用 Raw Input、Win32、D3D11；macOS 用 CGEventTap、AppKit、Metal；共享 schema、资源与 fixture 保持平台无关。
- 首发平台是 Windows 10 1903+ 与 macOS 12+；Linux 只作首发后评估，不阻塞首发。
- Windows 只支持 `x86_64-pc-windows-msvc`，不得新增 i686 或 `aarch64-pc-windows-msvc` 的构建、测试或安装包；Windows on ARM 走 x64 仿真。
- “纯 Rust”指自有应用代码全部用 Rust；官方 Cubism Core 平台二进制是唯一允许的厂商 FFI 例外，业务逻辑不得放进 SDK bridge。
- Bundle ID 固定 `com.ayangweb.bongo-cat`。Development 与 Production 共用数据结构、使用不同存储根；开发构建不得读取、写入或锁住生产数据。
- 不引入 Tauri、WebView、Node.js、JavaScript 或第二套 UI framework。

## 2. 任务流程

### 2.1 实现决策阶梯（ADR-0030）

按顺序取第一个可行方案：现有行为已满足（YAGNI）→ 复用仓库已有 helper、contract、类型或模式 → Rust 标准库 → 操作系统原生能力 → workspace 已安装依赖 → 经调研的成熟第三方方案 → 最小自有代码。

- 第三方方案按 §6 审查版本、许可证、维护状态、平台支持、unsafe 面积和替换边界。
- 复用不得让第三方类型泄漏进项目公共 API；不复实现已有通用能力，不引入不可靠或不成比例的依赖。

### 2.2 开始前

1. 检查当前分支和 `git status`，保留全部用户修改；先 `git fetch` 把上游同步到最新（见 §10）。
2. 定位 TODO 中对应阶段、任务和退出门槛；读现有实现与测试，不从文档标题推断行为。
3. 明确最小交付范围、平台范围和验证方式；跨阶段门禁先完成前置 spike，否则明确报告阻塞。

### 2.3 实施

1. 遵循既有模块边界与命名，改动只覆盖请求和当前 TODO 项；先完成最小闭环再扩展，不同时铺开多个未验证子系统。
2. 平台差异只放在 platform adapter 或 `cfg` 模块，不扩散到 runtime、model、config、UI。
3. 新依赖说明用途、维护状态、许可证和替换边界。
4. 不做无关重构、批量格式化，不覆盖用户未提交修改，不动无关 lockfile、生成文件与资产 metadata。

### 2.4 完成前

1. 按 §9 跑与风险相称的格式化、静态检查、测试和平台 smoke test。
2. 复查正常、错误、重启和 shutdown 路径，以及依赖方向、平台类型泄漏、`unsafe` 范围和日志隐私。
3. 完成定义全部满足并有证据时才勾选 TODO checkbox；架构、行为协议或退出条件变化时同步 Technical Design、TODO 与 ADR。
4. 最终报告列出改动、验证命令与结果、未运行测试、已知风险、CHANGELOG 判断和下一项未完成任务。文档任务无需声称运行代码测试。

## 3. 阶段与数据版本

- 每次只推进一个最小闭环；新增能力先有自动化 contract 或 ADR，再进入实现。ADR-0011 允许已通过自动化 contract 且不依赖缺失外部证据的模块先进入正式实现。
- 门禁未过的能力不得宣称完成（Live2D、输入可靠性、双平台渲染等）；未取得 Cubism 书面授权、SDK 分发、实机输入/UI/GPU、签名、Windows 实机安装升级卸载与 soak 证据时，不得分发含受限 artifact 的安装包。
- 不批量迁移历史功能，不为目录美观预建空 crate，未经授权不删除历史源码和行为对照。
- `config.json` 与 window state 显式带 `schema_version: 1`，保留单一严格解析入口：非 1 明确拒绝，不静默转换。
- 不做旧 Tauri/Pinia 格式的兼容、探测、字段 alias、自动导入或目录 fallback，也不为开发期中间版本写迁移分支。
- 历史源码只保留在远端受保护分支 `pre-refactor-tauri`，仅作行为考古与模型兼容参考，不重新接入工作树或依赖图；上游参考实现是 [MMmmmoko/Bongo-Cat-Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver)，见 `docs/migration/bongo-cat-mver-reference.md`。结论须由实际代码、配置或实机行为证明。

### 3.1 配置与数据兼容性

新增或修改配置项、持久化数据、状态结构时，前提是**已存在的数据仍能正常读取并正常启动**。新版本不得因为配置或数据结构变化导致旧数据读取失败、启动失败或行为异常。

- 新增字段必须兼容“旧数据里没有该字段”的情况：用 `#[serde(default)]`、`Option<T>` 或等价默认值，**不得**把新字段设为必填，否则旧文件解析失败并落入恢复流程甚至启动失败。
- 收紧取值范围、改变字段语义、枚举成员或 ID 生成规则前，先确认旧数据仍合法；确实不兼容时采用迁移方案（顺序、幂等、失败可回滚），并单独建立 ADR、设计、测试与发布门禁，不得直接改坏已有数据。
- 结构变化后同步更新 `Default`、`validate`、生成的 JSON Schema（`just schema`）、`shared/config/` 下的 fixture 与 manifest，并补一条“旧数据 → 新代码”读取的回归测试。
- 同一原则适用于 window state、日志、备份、model store 和更新 manifest 等所有持久格式。
- 边界不变：`schema_version` 非 1 仍明确拒绝；ADR-0054 的“最新有效备份 → 默认配置”是读取失败后的兜底，不是版本迁移。

## 4. 架构边界

### 4.1 GPUI

- 只负责设置、模型管理、快捷键、权限、更新和诊断 UI；不驱动 Live2D frame loop，不接入 GPUI renderer 私有接口。
- `Entity` 只保存表单草稿、选择、导航等临时视图状态；不持有 pressed state、动画状态、Cubism model 或主猫 GPU 资源。
- UI 通过强类型 command 请求 runtime，通过带 revision 的 snapshot 显示结果；executor 不做阻塞文件、模型解析或 GPU 工作，不持有 runtime 写锁。

### 4.2 Runtime

- Runtime 是配置、输入、动画和当前模型状态的唯一事实来源，由单一 owner 管理可变业务状态。
- Command、InputEvent、RuntimeSnapshot、RenderSnapshot 必须强类型；禁止 `set_value(path, any)`、弱类型 JSON 业务消息和字符串事件协议。
- 时间逻辑使用可注入的单调时钟，不得用墙上时间驱动动画或输入延迟。

### 4.3 Renderer 与 Overlay

- 主猫使用独立原生 overlay（Windows D3D11、macOS Metal），不嵌入 GPUI renderer；renderer 只消费不可变 RenderSnapshot，不读配置、不决定动作、不访问 GPUI Entity。
- Overlay 的窗口、GPU、frame source 必须有明确 owner 和析构顺序。
- Shutdown 顺序：阻止新 frame tick → 停止输入生产者 → 确认 frame source 退出 → 停止 runtime → flush 配置 → 停止音频并 join → 释放 renderer/GPU → 销毁 overlay → 关闭 GPUI。

### 4.4 平台模块

- 共享业务 crate 不导入 Win32、Objective-C、GPUI 或 GPU handle；平台 API 封装在 `bongocat-platform` 或明确的平台子模块，返回稳定的项目类型和 error code，不泄漏裸指针或平台消息结构。
- 主线程限定、COM apartment、run loop 和 callback 生命周期写入 wrapper 的安全不变量。
- 不靠 cfg fallback 让 Linux 编译，也禁止 `#[cfg(any(target_os = "macos", target_os = "windows"))]` 及其反向用法；共享桌面代码用直接 cfg、`test` 或单一平台条件，由 `tools/tests/test_supported_platform_cfg.py` 强制。

## 5. 输入可靠性

Issue #47 的“按下后无释放”必须从架构处理，不能只增加动画超时。

- Key/button down/up、设备连接/断开和 command 走可靠、有序队列；溢出可观测、计数并触发安全恢复，不得静默丢失边沿事件。
- 鼠标移动和手柄轴可合并为 latest value，但不得阻塞 key/button release；每个 pressed key 最终必须由 release、状态校正或 Reset 清除。
- Windows：Raw Input 是键鼠主路径，正确处理 scan code、E0/E1、左右修饰键和 `RI_KEY_BREAK`，用 `GetAsyncKeyState` 校正 pressed set；锁屏、睡眠、设备移除、输入桌面变化和服务重启必须 Reset；低级 hook 只作补充，不得成为 pressed state 的唯一来源；回归 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen 和 UAC 返回。
- macOS：listen-only CGEventTap 加明确的 TCC 权限状态；tap timeout、disable、权限变化、session 变化必须可恢复，用 `CGEventSourceKeyState` 校正 pressed set；锁屏、睡眠、快速用户切换和 tap 重建必须 Reset。

## 6. Cubism、FFI 与依赖

- 使用官方 Cubism Core 平台二进制并记录版本、来源、hash、架构和许可证；artifact 必须来自已固定基线，升级、替换或新增时同步 provenance、目标 ABI 和模型验证。
- Raw binding 只在 sys/wrapper 边界，原始指针不得离开 safe wrapper；Rust owner 保证 Moc、Model、buffer、texture 和 renderer 的存活/析构顺序；模型切换使用 prepare → validate → commit，失败保留当前可用模型。
- 模型、motion、expression、physics、pose、mask 行为由 fixture 验证；三个预置模型 spike 通过前不得宣称 Cubism 兼容完成，也不得加入长期非 Rust 业务 bridge 绕过 go/no-go。维护者已授权把固定版本的 Cubism Core、header、生成 bindings 和三个预置模型提交到 `vendor/` 与 `resources/`；公开发布前仍需完成 attribution、再分发清单和最终合规核对。
- 业务 crate 默认 `#![forbid(unsafe_code)]`，`unsafe` 只允许在平台 API、GPU 和 Cubism 边界；每个非平凡 block 前说明调用方安全不变量，优先小型 RAII wrapper，不传播裸 handle/指针或手工析构；不用 `unsafe impl Send/Sync` 绕过线程模型；FFI callback 不阻塞，不 panic 穿越 FFI。
- crates.io 依赖默认精确 pin。新增或升级前用 `cargo info <crate>` 或 crates.io API 核对最新非 yanked 稳定版，不因少改 API 主动选旧版；最新版若不支持既定 toolchain、target、许可证或安全边界，在相关 Phase 文档记录阻塞版本、原因、上游 owner 和解除条件，不只写代码注释。
- 修改 manifest 后对整个 workspace 执行 `cargo update` 同步 `Cargo.lock`；受上游约束保留的旧版本须能由 `cargo tree --invert` 解释。禁止 `version = "*"` 和未固定 revision 的 git dependency。
- 不依赖 Zed 应用内部 crate 或私有 GPUI renderer 接口；依赖审计只覆盖正式 workspace 与离线工具，不引入历史 Tauri workspace 依赖。系统能力先找满足边界的成熟 crate，再考虑 `windows-rs`、`objc2` 等基础 binding；第三方事件、错误、配置和平台类型不得进入项目公共 API。
- 不在 `Cargo.toml` 写注释。理由、取舍、许可证判断和踩坑记录属于 ADR 或代码注释，依赖文件只保留声明本身；确有必要时指向对应 ADR 编号而不是就地解释。
- 移除功能时同步删除其遗留的未使用依赖与零调用公开方法，不用注释或 `#[allow]` 掩盖；未使用依赖不是 `cargo clippy -D warnings` 能发现的问题，删除前用全仓引用搜索确认。

### 6.1 手柄后端（`ayangweb/gilrs`）

- 手柄 driver、mapping 与 backend 生命周期修复**直接推送到该 fork 的 `master`**（可审计的 commit、线性历史、`--ff-only` 合入），`bongocat-platform` 只保留强类型 adapter；不得用 `[patch]` 或 path 依赖在本仓库长期改第三方代码。临时克隆放在项目目录内（`~/Downloads` 受 macOS TCC 保护，非交互 shell 通常无权进入），验证完即删除。
- 固定 commit 以 `Cargo.toml` 和 ADR-0066 为准。完成定义：上游已推送、本仓库重新精确 pin、`Cargo.lock` 同步、`cargo deny check sources` 通过，并复测**空闲 CPU** 与控制路径延迟（`reset` ack、`shutdown` join 是否仍在既有超时预算内）。只看事件是否到达不算完成。
- 评估手柄改动必须包含空闲 CPU 复测：macOS IOHID worker 曾把 null mode 传给 `CFRunLoopRunInMode`，CoreFoundation 立即返回而不等待，使空闲 backend 占满一个 CPU 核；回归测试是 `gilrs-core` 的 `idle_backend_does_not_spin`。
- 更新固定 commit 后核对 `cargo tree --target x86_64-pc-windows-msvc -p bongocat-platform` 只出现 `Cargo.lock` 里的单一 `windows` 版本，必要时在 lock 中把 `gilrs-core` 的依赖指回该版本，并用 `cargo metadata --locked` 确认 lock 无需重解析。

## 7. 配置、文件安全与更新

- 配置只接受当前完整 v1，JSON key 用 `snake_case` 与当前产品领域名称；字段演进规则见 §3.1。
- 构建产物固定携带 Development/Production 环境，运行时输入不得切换；配置、状态、模型、备份、日志、锁、单实例命名和更新 channel 全部按环境隔离。
- 写入使用同目录临时文件、flush 和原子替换；失败保留原文件和备份。
- `config.json` 损坏时按“最新有效备份 → 默认配置”处理；无 backup 时写入并使用默认配置。不得新增恢复窗口、恢复提示、restore command、operational gate 或“需重启”流程（ADR-0054）；非 v1 仍走严格版本入口。
- 模型导入防止路径穿越、符号链接逃逸、绝对路径注入、压缩炸弹和静默覆盖；文件选择结果在 Rust 侧复验。
- 更新只允许 HTTPS，校验版本、target、arch 和签名并提供失败回滚。独立 hash 校验当前无实现，`update_checksum_mismatch` 无产出路径；恢复需新建 ADR，恢复前不得宣称满足。
- 日志不得记录真实按键序列、剪贴板、用户文件内容或密钥；日志和备份必须有大小、数量和保留期限上限。

## 8. GPUI Kit UI

设置界面唯一直接 GPUI 依赖是 crates.io 的 `gpui-kit`，精确版本以 `Cargo.toml` 为准；不得直接声明 `gpui`、`gpui_platform`、`gpui-component` 或单独 assets crate，也不得混入 GPUI git source。开发前必须查阅 [gpui-kit](https://github.com/longbridge/gpui-kit)、[组件文档](https://gpui-kit.com/docs/components) 与 [docs.rs](https://docs.rs/gpui-kit/)，不得凭记忆或猜测 API。

- 类型从 `gpui_kit` 根导出，平台、组件、资源分别用 `gpui_kit::platform`、`gpui_kit::component`、`gpui_kit::assets`；仅业务特殊行为或无等价 primitive 时保留薄封装。
- 创建组件前调用 `gpui_kit::init(cx)`；窗口根视图用 `gpui_kit::component::Root`；系统外观变化用 `Theme::sync_system_appearance(Some(window), cx)`。
- 短暂反馈用 `gpui_kit::component::notification::Notification`，右下角固定为 `Theme::global_mut(cx).notification.placement = Anchor::BottomRight`；`Root` 已挂载 dialog、sheet 与 notification layer，业务根视图不得重复渲染。通知须含可操作上下文（如快捷键冲突写出实际 chord），不用页面内临时错误文本代替。
- 语义色通过 `ActiveTheme::theme()` 读取；不重复硬编码颜色、字号、间距、圆角或控件高度，无明确产品需求时用组件默认值与 `Theme`。
- 常用组件：`Button::new(id).label(label)`、`Switch::new(id).checked(bool)`、`Checkbox::new(id).checked(bool)`、`Radio::new(id)`、`InputState::new(window, cx)` + `Input::new(&state)`、`NumberInput::new(&state)`、`Select`、`Slider`、`TabBar`、`Separator::horizontal()`、`Badge::new()`、`Progress`、`Icon`、`Tooltip`、`Dialog`、`Menu`。
- `Input`/`NumberInput` 绑定 `Entity<InputState>`，分别订阅 `InputEvent` 与 `NumberInputEvent`；范围、步长和最终校验来自业务 schema，组件事件不得绕过 typed command。组件已在自身输入边界限制的条件不要再新增错误码或本地化文案。
- 迁移组件后删除无用的直接 GPUI 依赖、自定义绘制和模块导出，并在 TODO 记录已迁移组件、剩余特殊控件和官方文档依据。
- visual-first 契约（ADR-0054）：不维护 screen-reader/AccessKit tree、辅助桥、隐藏 label/action 或仅供辅助技术的文案，也不新增直接 AccessKit 依赖。
- 设置界面安静、紧凑、适合重复操作；图标表达常用工具并提供可见 tooltip；控件覆盖 hover、active、focus、disabled、loading、error；Development、Production 与 smoke 使用同一套可见组成，smoke 只改变驱动方式。
- 支持浅色、深色、系统主题和现有本地化语言；文案遵循 `docs/localization-copy-conventions.md`（省略号只用单个 `…`，由 `tools/validate-locales.py` 强制）。
- 800x600、Windows 125/150/200%、macOS Retina 下不得重叠、裁剪或布局跳动；页面必须有真实 loading、empty、error、cancel 和 retry 状态，不用静态占位冒充完成。

## 9. 测试、构建与文档

### 9.1 默认验证

```text
just check          # fmt + clippy（全 target/feature 组合）+ workspace 测试 + release check
just test           # 仅 workspace 测试
```

`just check` 覆盖 `cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`（`bongocat-app` 按 feature 分组单独跑）、`cargo test --locked --workspace` 与 `cargo check --locked --workspace --release`。平台功能必须在对应平台跑 smoke test（`just dev-smoke`、`just preview` 等），不得用 macOS 编译推断 Windows Raw Input/D3D11，也不得反向推断 macOS CGEventTap/Metal；无法运行的测试必须在最终报告说明原因和残余风险。

### 9.2 构建与打包

`just` 是唯一构建入口，`build` recipe 只把参数转发给 `crates/bongocat-packaging`；该 crate 是产品构建、打包和发布的唯一事实来源。

```text
just build                                              # Production，本机 target，全部产物
just build --target x86_64-apple-darwin                 # 指定 target
just build --environment development --formats app      # 只要 Development .app
just manifest target/package target/package/*.json      # 合并 per-target manifest fragment
just release-notes release-notes.md                    # 从两份 changelog 合成发布说明
just keygen ~/.bongocat/release.key                    # 一次性生成更新签名密钥对
just version                                            # 唯一产品版本号来源
just schema                                             # 从 Rust 类型生成当前 JSON Schema
```

- 不在 `Justfile`、CI 或文档中重实现平台判断、目录复制、`Info.plist` 注入、`.app`/`.dmg`/NSIS 组装或版本解析；不重新引入 `scripts/` 或自维护 `.nsi`。
- 版本号只来自 `[workspace.package].version`；平台与产物集合由 `crates/bongocat-packaging` 声明，并与 `tools/tests/test_packaging_contract.py`、`deny.toml`、`bongocat-update::UpdateTargetTriple` 保持一致。
- 更新载荷组装与签名属于该 crate，`SIGNING_PRIVATE_KEY` 与 `SIGNING_PRIVATE_KEY_PASSWORD` 是唯一注入点；未设置则跳过签名，release workflow 断言发布构建已签名。签名必须是最后一步，签名后不得改名、重压缩或 strip；私钥不得进入源码、产物或日志，公钥写入 `bongocat-update::RELEASE_SIGNING_KEY`。
- 每次 `just build` 只写当前 target 的 fragment（`<os>-<arch>.json`），运行时只请求共享 `latest.json`，多 target 发布必须先执行 `just manifest`；不在 CI 或脚本里拼 manifest JSON。
- 细节与退出条件见 ADR-0033、ADR-0034 与 `docs/update-signing-guide.md`。

### 9.3 不变量与完成定义

- 压力测试 key/button edge 丢失计数为 0；任何 pressed state 最终由 release、reconcile 或 reset 清除。
- Renderer 不阻塞 runtime，鼠标移动不阻塞释放边沿。
- 模型加载/切换失败不破坏当前可用模型；配置写入或损坏恢复不丢失当前环境的可用配置或用户模型。
- 退出不依赖强杀，所有 worker 在超时内 join；8 小时 soak 无持续内存、GPU、handle、线程或日志增长。
- scaffold 不等于功能完成；编译通过不等于平台行为完成；单个模型通过不等于模型兼容完成；增加超时不等于修复 issue #47；未验证签名、权限和回滚不等于可发布。

### 9.4 文档与 TODO

- 架构决策、约束变化和 go/no-go 结果写入 `docs/adr/`；Benchmark 写入 `docs/benchmark/`；历史考古写入 `docs/migration/`。
- Technical Design 只描述当前目标架构，不写历史路线、候选方案比较或已放弃设计，也不把旧配置兼容重新纳入产品范围。
- 根级用户/贡献者文档同时维护英文与简体中文：`README.md` / `README.zh-CN.md`、`CONTRIBUTING.md` / `CONTRIBUTING.zh-CN.md`、`CHANGELOG.md` / `CHANGELOG.zh-CN.md`；英文文件不带语言后缀，修改一份必须同步另一份。
- 公开文档只说明产品行为、用户流程和必要操作，不展示内部代号、迁移叙事或不必要的源码文件名；开发背景由 `AGENTS.md`、Technical Design、TODO 与 ADR 承载，公开文档只保留受保护的 `pre-refactor-tauri` 分支链接。`CHANGELOG` 是面向用户的版本通知，必须保留替换、迁移、重写、默认行为变化和升级注意事项。
- 仓库自有文件名与用户文档不使用历史重写阶段的名称或同义内部限定词；普通文案中的 `native` 只用于操作系统原生能力或 Cubism Native 官方产品名；既有 Rust 类型与 API 边界不因文档清理顺带重命名。
- TODO checkbox 仅在完成定义完整满足后勾选；部分完成保持 `[ ]` 并写明状态与剩余工作；新任务放入正确 phase，标明依赖与退出条件。
- changelog 分段标题词表、版本标题和发版 commit 规则见 `docs/changelog-conventions.md`；没有对应的下一个版本标题时统一写入 `Unreleased`，不得自行创建版本标题。

## 10. Git 与交付

- 未经用户明确要求，不 commit、push、开 PR、release 或打 tag。
- 主分支是 `master`（以 `origin/HEAD` 为准）。除 §10.2 的发布版本外，`master` 上禁止直接提交，按实际改动创建 `<type>/<short-topic>` 语义分支（如 `feat/settings-window`、`fix/input-release`、`docs/phase-0-plan`）后再提交；已在其他分支时按用户指定执行，未指定则先询问。未经要求不创建、删除、重命名或切换分支，不 reset、checkout、覆盖或格式化无关文件。
- Commit message 必须依据实际 staged diff，遵循 Conventional Commits：`<type>: <summary>`，只有 scope 有明确区分价值时用 `<type>(<scope>): <summary>`。type 用 `feat`、`fix`、`docs`、`test`、`refactor`、`perf`、`build`、`ci`、`chore`，不用 `update`、`changes`、`misc`；summary 用简洁英文祈使句、不加句号、建议不超过 72 字符；不兼容变更加 `!` 并在正文写 `BREAKING CHANGE:`。提交前核对 staged diff，排除未暂存或无关改动。
- **推送顺序固定为「先拉上游 → 解决冲突 → 验证 → 提交 → push」。** 动手写代码前先 `git fetch`（必要时 `git pull --ff-only`），在最新上游之上工作；提交前若发现远端已有新 commit，必须先把上游改动合入并解决冲突再提交，不得留下 `Merge remote-tracking branch 'origin/master' into <branch>` 这类 merge commit。合并后必须在最终树上重跑完整验证，不能沿用合入前的结论。
- 冲突按实际语义逐个解决，不用 `checkout --ours/--theirs` 覆盖对方实质改动，解决后确认本任务改动仍在。合并后若出现失败测试，先判断是否由本次改动引入（在纯上游 commit 上单独复跑）；上游自身失败不代为修复，如实报告并交由用户决定。
- push 前先完成 CHANGELOG 检查。判断范围是待推送 commit（`git log @{u}..HEAD`）及其 diff：存在用户可见的新增、移除、默认值/行为/性能/UI/平台/本地化/配置格式变化或升级注意事项时，同步更新两份 changelog 并沿用现有 section 与文案风格；纯内部重构、测试、CI、格式化、注释、构建、依赖调整和未落地实现跳过，不新增临时 section。CHANGELOG 修改必须先提交再 push，判断结论写入最终报告。

### 10.1 推送代码与 PR

用户明确要求**“推送代码”**时，固定顺序是：fetch → 合入上游 → 完整验证 → 提交 → push → **创建 PR** → **切回主分支**。

- 本仓库是主项目（`ayangweb/BongoCat`）而不是 fork 时，push 到功能分支后**必须**继续创建对应的 Pull Request，不能只完成 push 就停止。
- 在 fork 上开发时，按正常的 fork/上游协作流程处理：推送到 fork 并向上游开 PR，不在本仓库创建 PR。
- 完成推送和 PR 创建后 `git switch` 回主分支（默认 `master`），工作树不留在本次开发分支。
- PR 标题沿用同一 Conventional Commits 标题，正文写清改动、验证结果、未运行测试和已知风险。
- 未经明确要求不打 tag、不 release。

### 10.2 发布版本

用户明确要求**“发布版本”**（如“发布 2.2.0 版本”）时，直接在 `master` 上完成，不建分支、不开 PR，固定顺序是：fetch 并确认 `master` 与上游一致 → 改 `[workspace.package].version` 并 `cargo update --workspace` 同步 `Cargo.lock` → 两份 changelog 的 `## Unreleased` 改为 `## <version> - <date>` → `just release-notes` 与 `python3 -m unittest tools.tests.test_release_changelog_contract` 本地校验 → 在 `master` 提交 `chore: release v<version>` → 打附注 tag `v<version>` → `git push origin master` 与 `git push origin v<version>` 触发 release 流水线。

- 发布 commit 只包含版本号、`Cargo.lock` 与 changelog 标题等版本相关改动，功能改动不得混入；标题与 tag 规则见 `docs/changelog-conventions.md` §3。
