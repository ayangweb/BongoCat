# BongoCat Native Rewrite Technical Design

状态：架构决策稿，Phase 0 证据补齐与 Phase 1 渐进实现并行
最后更新：2026-08-31
首发平台：Windows 10 1903+、macOS 12+
后续平台：Linux（首发后评估）

> BongoCat Native Rewrite 采用单一 Rust 应用。GPUI 负责设置界面，Rust 平台模块直接负责输入、窗口、系统集成和 GPU 渲染。

## 1. 设计结论

```text
Rust 2024 edition application
├── GPUI                         设置、模型管理、快捷键和诊断 UI
├── Product Runtime              状态、输入语义、动画和配置协调
├── Live2D Runtime               模型、动作、表情、物理和音效
├── Windows Backend              windows-rs、Raw Input、Win32、D3D11
├── macOS Backend                objc2、CGEventTap、AppKit、Metal
└── Shared Assets / Fixtures     模型、schema、本地化和测试数据
```

主要决策：

- BongoCat 自有应用代码统一使用 Rust。
- Windows 只发布 x64 与 ARM64，不构建或发布 x86；ARM64 在官方 desktop Cubism Core 可用并通过 ABI/模型验证前保持发布阻塞。
- GPUI 只负责常规设置 UI，不承担主猫 Live2D 渲染。
- 主猫窗口由 Rust 平台模块直接创建和管理，与 GPUI 设置窗口共享同一应用生命周期。
- Windows 使用 D3D11，macOS 使用 Metal；首发不为了未来 Linux 强行统一 GPU backend。
- Windows 键鼠输入自行实现 Raw Input + 系统状态校正，从架构上避免 issue #47 的永久卡键。
- macOS 输入自行封装 CGEventTap、TCC 权限、tap 恢复和按键状态校正。
- 官方 Cubism Core 是预编译厂商二进制，是“应用代码纯 Rust”的唯一 FFI 例外；BongoCat 业务逻辑不得进入 SDK bridge。
- Linux 不进入首发范围，但共享业务 crate 不得依赖 Win32/AppKit 类型，不得故意封死后续 backend。

## 2. “纯 Rust”的定义

本项目中的“纯 Rust”定义为：

- 应用入口、UI、状态、动画、输入、配置、模型管理、窗口、渲染和系统服务均由 Rust 源码实现。
- UI、运行时和平台服务均由同一个 Rust workspace 构建和管理。
- 允许通过窄 FFI 调用官方 Cubism Core 平台二进制，因为 `.moc3` 运行依赖厂商 SDK。
- 操作系统 API、GPU driver 和系统 framework 不属于应用语言范围。

因此对外准确表述应为“BongoCat 应用代码使用 Rust 重写”，而不是“最终二进制完全不含任何非 Rust 代码”。

## 3. 目标与非目标

### 3.1 目标

- 使用 Rust 实现完整的桌面应用、设置 UI 和实时运行链路。
- 使用一套 Rust 业务实现覆盖 Windows 和 macOS。
- 保持现有模型、动作、表情和用户资源格式尽可能兼容；配置使用全新的 Native Rewrite schema 和命名。
- 修复输入事件丢失导致的永久卡键，包括 issue #47 的截图快捷键场景。
- 设置界面具备一致、清晰、可主题化的桌面体验。
- 主猫窗口具备低延迟、透明、置顶、穿透、多显示器和高 DPI/Retina 支持。
- 所有后台服务具有明确的 start、stop、restart 和 shutdown 生命周期。
- 使用 fixture 验证跨平台行为，而不是只依靠人工观察。

### 3.2 非目标

- 首发不支持 Linux，不承诺 Wayland 全局输入能力。
- 不提供进程内插件 ABI。
- 不要求 Windows/macOS 像素完全一致。
- 不把 GPUI fork、Zed 私有 UI crate 或第三方输入库变成业务 API。
- 历史版本只作为行为与模型资源参考；Native Rewrite 不读取或导入旧 Tauri/Pinia 配置。
- 不在技术 spike 通过前完整迁移所有产品功能。

## 4. 设计原则

| 原则           | 设计约束                                                   |
| -------------- | ---------------------------------------------------------- |
| 单一状态所有者 | runtime 独占输入、动画、配置和模型可变状态                 |
| 实时链路内聚   | 输入直接进入 runtime，模型求值后生成不可变渲染快照         |
| 输入最终一致   | 按键边沿、系统状态校正和生命周期复位共同维护 pressed state |
| UI 与渲染分离  | GPUI 负责设置，独立 overlay renderer 负责 Live2D           |
| 平台能力显式   | 系统 API 封装在平台模块，业务 crate 不接触平台 handle      |
| 配置可恢复     | schema、环境隔离、验证、备份、版本演进和原子提交均可测试   |
| 行为可重复     | fixture、规范化状态快照和平台 smoke test 共同验收          |

## 5. GPUI 决策

### 5.1 为什么选择 GPUI

GPUI 用于设置窗口、模型管理、快捷键编辑、权限状态、更新和诊断：

- Rust 原生、GPU 加速，适合视觉统一的桌面工具界面。
- Windows 后端使用 Win32、DirectWrite、D3D11/DirectComposition，macOS 使用 AppKit/Metal。
- retained/immediate 混合模型适合设置表单、列表、实时预览和自定义控件。
- 已有目标桌面平台 backend，并为后续 Linux 留出路径。
- Zed 的实际产品规模证明其能够支撑复杂桌面 UI。

### 5.2 GPUI 边界

GPUI 仍是 pre-1.0，公共渲染 API 也没有稳定的 Windows/macOS 外部 Live2D 纹理合成路径。因此：

- 首个 spike 使用审计时 crates.io 最新稳定版并固定为 `gpui = "=0.2.2"`，提交 `Cargo.lock`，禁止 `version = "*"`。
- 不自动跟随 Zed main，不直接依赖 Zed 应用内部 UI crate。
- 项目内建立小型 design system：颜色、排版、间距、焦点、按钮、表单、列表、弹窗和通知。
- GPUI `Entity` 只保存视图状态；真实输入、动画、配置和模型状态由 runtime 管理。
- 正式 `bongocat-ui` 只定义设置 command/snapshot 协议和 GPUI 视图；`bongocat-app`
  的有界 service worker 独占配置写入和 runtime command，UI executor 不执行阻塞 I/O。
- 设置控件的辅助功能语义由 UI crate 维护项目自有 AccessKit tree；平台 adapter 只通过
  GPUI 公开的 raw window handle 安装，辅助技术 action 经有界强类型通道回到 GPUI 主线程。
- 辅助功能实现不得使用 GPUI 私有 renderer、隐藏原生控件或独立业务状态副本；可见控件、
  语义节点、焦点、loading/error 和 value 必须由同一份 UI snapshot 更新。
- GPUI 不加载 Cubism、不持有主猫 GPU 资源、不驱动 Live2D 帧循环。
- 关闭设置窗口不影响 runtime、输入、音频、frame source 和 overlay，它们继续由 app
  coordinator 持有。macOS 销毁对应 GPUI `Entity`，reopen 创建一个新窗口；GPUI 0.2.2
  的 Windows `WM_CLOSE` 销毁回调存在同步重入缺陷，因此 Windows platform adapter 拦截 close、
  使用 `ShowWindow(SW_HIDE)` 保留唯一 Entity，并在 reopen 时重显。两条路径都主动读取最新
  revisioned snapshot。Windows 显式退出先停止 frame source 与输入生产者，再 shutdown/join
  runtime、配置和音频 owner，最后释放 renderer/GPU 与 overlay；由于 GPUI 0.2.2 及当前上游
  `main` 都会在最终 `WM_DESTROY` 同步重入 `AsyncApp` 并触发进程 fast-fail，平台 adapter 只在
  这些产品 owner 全部有序关闭后使用进程退出跳过有缺陷的 GPUI 窗口析构。该兼容措施不得提前
  终止业务 shutdown；升级到修复此回调的固定 GPUI 版本后必须移除，并恢复正常 GPUI 析构门禁。
- 平台服务不返回 GPUI 类型，避免框架扩散到业务模块。
- GPUI 升级必须单独提交，附变更说明、双平台构建和 UI smoke test。

所有新的 crates.io 直接依赖同样先选择引入时最新的非 yanked 稳定版，再精确 pin 并提交 lockfile。只有 Rust toolchain、目标平台、许可证或已验证的安全边界不兼容时才允许暂缓，且必须留下可复核的版本差异和解除条件；不能用旧版本回避正常的 API 迁移。传递依赖在上游约束允许的范围内保持最新，不 fork 上游只为修改版本号。

Phase 0 必须验证输入法、文本编辑、缩放、辅助功能、窗口重开、托盘应用生命周期，以及 GPUI 设置窗口与独立 overlay 共存。ADR-0011 允许已通过自动化契约的模块进入正式 workspace；未解决的问题继续阻塞对应完整功能或 stable 发布，并必须在进入完整 UI 实现前解决并记录。

## 6. 总体架构

```text
                       GPUI settings window
                              |
                       Commands / Snapshots
                              |
                     App coordinator/service
                              |
Platform input ---> Runtime thread ---> Model/Animation state
     |                    |                    |
     |                    +--> Config service |
     |                    +--> Audio service  |
     |                                         v
     +--> reconciliation                 Render snapshot
                                                   |
                                      Native overlay window
                                      D3D11 / Metal renderer
```

### 6.1 模块责任

- `app`：应用入口、服务装配、生命周期、单实例和 shutdown coordinator。
- `runtime`：唯一业务状态所有者，处理输入、快捷键、动画选择和模型命令。
- `ui`：显示 runtime snapshot，发送显式 command，不直接修改业务字段。
- `platform`：窗口、输入、托盘、权限、显示器、启动项、文件和更新。
- `model`：模型包解析、路径安全、资源索引和显式导入。
- `live2d`：Cubism Core 生命周期、motion/expression/physics/pose 求值。
- `audio`：motion 音效的有序 command、FLAC 解码、唯一 voice、输出设备和 shutdown。
- `render`：不可变 render snapshot 和 renderer contract。
- `config`：环境隔离、版本化 schema、验证、备份和原子提交。

### 6.2 依赖方向

```text
ui protocol <------- app -------> runtime <------- platform adapters
                                  |
                                  v
                             model / live2d
                                  |
                                  v
                            render contract
                                  ^
                                  |
                     D3D11 renderer / Metal renderer
```

业务 crate 不得导入 Win32、Objective-C、GPUI 或 GPU handle。平台实现可以依赖业务定义的 command/event 类型。

## 7. 仓库布局

```text
BongoCat/
  native/                    正式 Native Rewrite workspace；发布切换时成为根构建入口
    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    crates/
      bongocat-app/           入口、装配和 shutdown
      bongocat-runtime/       状态、输入语义、动画和命令
      bongocat-config/        schema、环境隔离和原子存储
      bongocat-model/         模型包、导入和资源索引
      bongocat-live2d/        Cubism Core 边界与模型求值
      bongocat-audio/         motion 音效队列、解码与设备 owner
      bongocat-render/        render snapshot/contract
      bongocat-ui/            GPUI 设置界面和 design system
      bongocat-platform/      Windows/macOS 平台服务
  shared/
    config/                   Native JSON schema、命名与存储契约
    behavior/                 输入、动画和快捷键规范
    fixtures/                 输入序列、预期状态和模型样本
    resources/                模型、图标和本地化
  tools/                      不随应用发布的验证与考古工具
  docs/
    adr/
    benchmark/
    migration/                历史参考，不进入生产配置路径
```

crate 是编译和责任边界，不是动态库。首期不为目录美观建立空 crate；只有依赖方向或测试隔离确实需要时才拆分。

迁移期将正式 workspace 放在 `native/`，使历史 Tauri workspace 和构建入口继续
作为行为对照且不进入新依赖图。发布切换阶段再把 Native workspace 提升为仓库根
构建入口；该路径差异不改变 crate 边界或产品架构。

## 8. Runtime 与并发

### 8.1 状态所有权

单一 runtime 线程拥有 `AppState`、`InputState`、`AnimationState` 和当前模型控制状态。其他线程不能通过共享可变引用修改它们。

```text
Input producers ---- reliable edge queue -----+
UI commands -------- reliable command queue --+--> runtime tick
Cursor motion ------- latest-value slot -------+        |
Gamepad axes -------- latest-value slot -------+        +--> UI snapshot
                                                        +--> Render snapshot
```

- Key/button down/up、设备连接和 command 必须可靠、有序；溢出是可观测错误，不能静默丢弃。
- 可靠队列溢出时必须清空无法证明顺序的缓存，并在队首注入 `Reset`；原始失败 item 返回 producer，溢出、恢复和被清理 item 数量进入诊断 snapshot。
- 鼠标移动和摇杆轴可以合并为最新值，不能阻塞边沿事件。
- 手柄 axis latest-value 以 `{device_id, connection_generation, axis}` 为 key 并限制总 key 数；每次连接分配新 generation，断开后旧 generation 的迟到样本不得作用于重连设备。
- 动画和延迟使用单调时钟 `Instant`，持久化时间才使用墙上时钟。
- `motion_stop` 只作用于匹配的当前动作。非零 `FadeOutTime` 在 runtime snapshot 中保留
  active identity 和首次 stop command sequence，renderer 以正弦权重淡出并在结束帧后
  清理；重复 stop 不重启计时，零时长立即清理，旧动作的 stop 不影响后启动动作。
- render snapshot 不含锁和平台对象，通过双缓冲或 latest-value channel 交给渲染线程。
- GPUI 通过 command/snapshot 边界交互，不直接持有 runtime mutex。
- GPUI 拥有平台主事件循环；应用 coordinator 在该主线程调度 overlay `tick`，GPUI
  `Entity` 不持有 renderer、render snapshot 或 frame-loop 状态。
- shutdown 顺序：停止输入 -> runtime drain/停止 -> 保存配置 -> 停止音频并 join -> 停止渲染 -> 销毁 overlay/GPU -> 关闭 GPUI。

### 8.2 输入事件

```rust
enum InputEvent {
    KeyDown { key: PhysicalKey, at: Instant, repeat: bool },
    KeyUp { key: PhysicalKey, at: Instant },
    MouseDown { button: MouseButton, at: Instant },
    MouseUp { button: MouseButton, at: Instant },
    CursorMoved { position: PhysicalPoint, at: Instant },
    DeviceConnected { id: DeviceId, at: Instant },
    DeviceDisconnected { id: DeviceId, at: Instant },
    Reset { reason: InputResetReason, at: Instant },
}
```

应用优先保存物理键身份；字符、布局和显示名称属于映射层。左右 Ctrl/Alt/Shift/Meta 必须可区分。

## 9. 输入可靠性

### 9.1 issue #47

PixPin、Win+L 或其他系统级快捷键可能让应用收到按下边沿，却收不到对应的释放边沿。输入系统不能把事件流视为永远完整，必须通过系统状态查询和生命周期复位保证 pressed state 最终一致。系统查询集中在输入服务中，renderer 不直接读取键盘状态。

### 9.2 Windows

1. 使用 `windows-rs` 调用 `RegisterRawInputDevices`，后台窗口接收 `WM_INPUT`。
2. 从 scan code、extended flag 和 `RI_KEY_BREAK` 建立物理键边沿。
3. runtime 维护 pressed set，但事件流不是唯一事实来源。
4. 校正使用单调时钟周期调度；默认每 `250 ms` 查询一次，并要求同一个 key 连续 `2` 次快照缺失才确认释放，避免单次系统查询异常误清除。时钟回退不得推进调度游标。
5. 对 pressed set 中确认释放的键生成内部 `KeyUp`。
6. 会话锁定、桌面切换、睡眠、设备移除、服务重启和队列异常时发送 `Reset`；这些生命周期复位不等待确认阈值。
7. 必要时用 `WH_KEYBOARD_LL` 补充合成事件，但 hook 不得覆盖 Raw Input 物理状态。
8. `RegisterHotKey` 只处理应用快捷键；冲突必须反馈 UI 并保留旧绑定。
9. XInput 固定轮询 0–3 号 slot；连接/断开和按钮使用可靠序列，摇杆/trigger 使用带 connection generation 的 latest-values。平台层只归一化原始范围，产品 dead-zone 由 runtime 配置统一决定。

该方案不承诺安全桌面交付每个释放事件，而是保证丢事件不会产生永久卡键。自动释放超时只是最后保险，不是正常语义。

Windows 验收覆盖 PixPin `Ctrl+Alt+A`、Win+L、PrintScreen、UAC、管理员/非管理员进程、睡眠唤醒、多键连按和队列压力。

### 9.3 macOS

- 使用 listen-only `CGEventTap`，不经过 GPUI 响应链。
- 区分 Input Monitoring/Accessibility 的 unknown、denied、granted、restart-required 状态。
- 监听 tap 被系统禁用、超时和 session 变化，并自动重建。
- `FlagsChanged` 的 down/up 方向必须在 callback 中从事件自身 flags、左右修饰键 keycode 和 callback decoder 的前一边沿状态冻结；这样左右同类修饰键同时按下时仍能识别单侧 release。decoder 状态不属于 runtime pressed state，并随任何 `Reset` 清空；不得等到 consumer drain 时用较新的全局状态反推旧事件，无法识别的修饰键必须触发可观测 `Reset`。
- 对键盘和鼠标 pressed set 分别使用 `CGEventSourceKeyState`、`CGEventSourceButtonState` 校正；保留 0–31 号 mouse button 身份，按统一的 `250 ms`/连续 `2` 次缺失策略确认释放，睡眠、锁屏、权限变化和 tap 重启时直接复位。
- GameController owner 在服务期启用后台事件，连接分配新的 generation；按钮/连接边沿进入可靠队列，axis 进入固定容量 keyed latest-values，断开后旧 generation 的 callback 和待消费样本不得作用于重连设备。
- callback 只做映射和入队，不执行模型、文件或 UI 工作。

## 10. 平台实现

### 10.1 Windows

- 应用：GPUI/Win32 主事件循环，单实例使用 named mutex + 唤醒消息。
- Overlay：Win32 透明无边框 popup；设置变化时切换 topmost 和 click-through。
- Renderer：D3D11 + DXGI + DirectComposition/DWM，预乘 alpha。
- DPI：Per-Monitor-V2，处理 `WM_DPICHANGED`、显示器热插拔和负坐标。
- 输入：Raw Input、状态校正、可选低级 hook、XInput 手柄。
- 托盘：`Shell_NotifyIcon` + `HMENU`。
- 启动项：当前用户范围，默认不要求管理员权限。

### 10.2 macOS

- 应用：GPUI/AppKit 主事件循环，平台 UI 操作固定在 main thread。
- Overlay：通过 `objc2` 创建透明 nonactivating `NSPanel`。
- Renderer：Metal + `CAMetalLayer`，drawable size 跟随 backing scale。
- Spaces：按配置设置 collection behavior 和 full-screen auxiliary。
- 输入：CGEventTap、状态校正、GameController，必要时 IOHIDManager。
- 菜单栏：`NSStatusItem` + `NSMenu`；登录启动：`SMAppService`。
- 发布：Hardened Runtime、签名、notarization 和 TCC 权限说明。

平台 `unsafe` 必须集中在小型 wrapper，写明安全不变量并有 smoke test。业务和 UI crate 默认禁止 `unsafe_code`。

## 11. Live2D 与渲染

### 11.1 Cubism 边界

```text
Official Cubism Core binary
          |
   raw Rust sys bindings
          |
  Moc / Model safe wrappers
          |
model evaluation + render snapshot
```

- Core 二进制按平台分发，版本、hash、来源和许可证记录在构建清单中。
- 当前验证基线固定为 Cubism Native `5-r.5` / Core `06.00.0001`；升级必须重跑
  header/binding provenance、目标 ABI、三个预置 Moc、offscreen/enhanced rendering
  fixture 和双 renderer 门禁。
- raw binding 只由精确锁定的离线生成工具从 hash 固定的官方 header 生成；生成配置、target ABI、libclang 版本和输出 hash 必须进入 provenance，禁止手改生成代码。
- 维护者提供并批准用于开发的固定基线已存入 `native/vendor/cubism/5-r.5`，包括
  Core、官方 header 和由该 header 生成的 target binding。公开发布前另行核对
  attribution 与再分发清单；该发布工作不阻塞本地功能实现和 `next` 开发提交。
- 原始指针不离开 safe wrapper；Moc 必须比 Model 活得更久。
- 不把未经验证的新纯 Rust Cubism 兼容 crate 作为生产基础。
- `.model3.json`、motion、expression、physics 和 pose 兼容性由 fixture 验证。
- 每帧从 Core 默认 parameter 开始，依次应用 motion、expression、physics/pose（实现后）、
  类型化产品输入，最后调用 Core update。motion 的自然结束与显式停止均使用 model3/
  curve fade，显式停止的外层正弦权重与 curve 权重相乘。`PartOpacity` motion curve
  遵循 R5 Framework 语义，按 curve ID 写入 Core parameter sink 且不继承普通 parameter
  curve 的 fade weight。model3 `Groups` 由模型索引保留并校验；motion `Model` target 中
  `EyeBlink` 对匹配的 Parameter curve 做乘法、`LipSync` 做加法，对未被 Parameter curve
  覆盖的首个同名 Parameter group（最多 64 个 ID）使用 motion fade 插值。`Opacity` 作为
  独立 model opacity 进入 `RenderSnapshot`，只在 renderer 最终颜色 pass 与 drawable/
  窗口透明度相乘，不参与 mask 生成，并保持到后续 motion opacity curve 更新或模型切换。
  expression 使用 `.exp3.json` 的
  Add/Multiply/Overwrite 和正弦淡入淡出；替换期间最多保留上一层与当前层，稳定后只保留
  最新 expression，使内存和每帧成本保持有界。真实输入最后覆盖对应产品 parameter，
  避免表情让按下状态失真。
- 模型加载采用 prepare/commit/rollback，失败时保留当前可用模型。
- 模型切换是 CPU/GPU 两阶段提交：runtime 先保留旧 active model/bindings，准备新的
  Cubism generation，并随候选 `RenderSnapshot` 发布一次性强类型 commit token；平台
  renderer 只有在纹理、mesh、mask 和 pipeline 资源全部准备成功后才能确认。runtime
  收到匹配 token 的确认后才替换产品事实状态；renderer 拒绝时双方继续使用旧模型。
- 普通 render frame 使用独立单调 transport sequence，可以按 latest-value 合并；模型
  commit feedback 使用不可覆盖的可靠单槽并且同时只允许一个候选 generation，不能被
  普通帧、cursor 或输入事件合并。等待 GPU 确认期间可靠输入仍由旧 bindings 消费。

官方 Cubism Framework 的动作、物理等逻辑必须在 Phase 0 验证 Rust 实现的兼容性。未达到退出门槛时必须形成 go/no-go ADR，不能绕过该门槛扩大实现范围。

### 11.2 Renderer

GPUI renderer 与 Live2D renderer 完全分离：

```text
RenderSnapshot
├── model opacity / drawables / offscreens / order / opacity / masks
├── color + alpha blend / multiply + screen color
├── vertex / uv / index buffers
├── texture ids
└── transform / viewport
        ├── Windows D3D11 renderer
        └── macOS Metal renderer
```

renderer 负责遮罩、混合、裁剪、纹理上传、dirty flag、present 和 GPU 生命周期；不读取配置、不决定动作、不访问 GPUI entity。

renderer 的模型资源准备结果通过稳定项目 error code 回到 runtime，不返回 GPU handle、
平台对象或任意字符串协议。候选准备失败只结束对应模型 command，不得终止 frame loop、
清空当前 GPU model 或让 runtime 提前宣布候选模型 active。

Linux 阶段再决定增加 Vulkan/OpenGL backend，或基于数据迁移到 wgpu。首发优先保证 Windows/macOS 透明窗口的确定性。

### 11.3 Motion 音效

- `bongocat-audio` 使用精确锁定的 `rodio 0.22.2`，只启用 output playback 与 FLAC；
  rodio/CPAL 类型不进入 runtime、model、UI 或 renderer 公共接口。
- runtime 只在 motion priority 与资源解析均成功后，通过固定容量有序队列非阻塞发布
  强类型 `Play`/`Stop`。解码、文件 I/O 和设备创建全部由独立 worker 执行。
- 同时最多一个 motion voice。新动作（包括没有 sound 的动作）、显式停止、禁用音效、
  成功模型 commit 和 shutdown 都停止旧 voice；被 priority 拒绝或加载失败的动作不改变
  当前 voice。
- model3 sound 相对路径必须先通过模型包规范化与包根约束，再由 runtime 组合为本地路径；
  audio backend 不解析 model3，也不接受 URL 或未验证的模型引用。
- 文件、解码、输出设备和队列故障只更新匿名诊断，不改变 motion、input 或 render 状态。
  满载恢复丢弃不可信 backlog 并停止 voice，禁止阻塞输入边沿。
- motion3 UserData 由 Live2D evaluator 保留并按单调 elapsed 的 `(previous, current]` 区间
  产生强类型 occurrence；循环跨越不重复，时间回退不重放，单 tick 最多发布 256 项并
  对跳过数量计数，避免睡眠恢复后的无界分配。

## 12. 配置、模型与安全

应用身份：

```text
com.ayangweb.bongo-cat
```

构建产物携带不可变的 `Development` 或 `Production` 环境。环境由构建入口显式选择，运行时参数、环境变量和设置项均不能切换。两个环境使用相同 schema 和相对目录结构，只改变数据根目录：

| 平台    | Development                                                         | Production                                                         |
| ------- | ------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Windows | `%APPDATA%\BongoCat\development\`                                   | `%APPDATA%\BongoCat\production\`                                   |
| macOS   | `~/Library/Application Support/com.ayangweb.bongo-cat/development/` | `~/Library/Application Support/com.ayangweb.bongo-cat/production/` |

每个根目录包含 `config.json`、`state.json`、`models/`、`backups/` 和 `logs/`。锁、单实例命名、更新 channel 和诊断同样按环境隔离；任何环境不得读取、写入或 fallback 到另一个环境。

要求：

- Native Rewrite 配置从全新 schema 开始，不读取、不探测、不导入旧 Tauri/Pinia store。
- JSON key 使用 `snake_case`，字段按当前领域语义命名，不提供旧字段 alias。
- 配置包含显式 `schema_version`，只对 Native Rewrite 自身后续 schema 执行顺序、幂等升级。
- 当前 v2 将模型选择持久化为成对的 `selected_model_origin` 与 `selected_model_id`；v1
  非空 ID 按当时产品语义迁移为 preset，迁移在当前环境 writer lock 内备份并原子写回。
- 写入使用同目录临时文件、flush、原子替换和提交后验证；失败保留当前文件和受限数量的备份。
- `config.json` 只包含用户设置；窗口布局写入 `state.json`，pressed state、权限结果和模型解析缓存不持久化。
- 模型导入防止路径穿越、符号链接逃逸、压缩炸弹和覆盖现有用户数据。
- 任意外部目录只能产生待导入的 `PreparedModel`；只有当前环境 `ModelStore` 完成复制、
  复验和原子提交后签发的 `InstalledModel` 才能进入 runtime 激活 command。
- 应用装配时打开并持有只读 `PresetModelCatalog`；设置与模型管理只消费预置目录和当前
  环境 `ModelStore` 的合并目录，不在 UI executor 临时扫描或解析模型文件。
- 模型目录身份是 `(origin, model_id)`。同一 `model_id` 的 preset 与 installed 条目都保留，
  排序固定为 `model_id` 升序、同 ID 时 preset 在前；后续选择 command 必须携带 origin，
  不得以静默覆盖解决冲突。
- 模型选择 command 携带完整复合身份。应用先校验并加载候选，再以 expected revision
  持久化选择并启动 runtime/renderer 两阶段提交；候选被拒绝时 runtime 保留旧模型，应用
  使用刚取得的 config revision 原子恢复旧选择。配置写入失败时不得发送激活 command。
- 合并目录通过强类型 settings snapshot 投影来源、可用状态、资源计数和稳定诊断码；无效
  模型继续可见，但用户路径、底层 I/O 文本和模型内容不得进入 UI snapshot 或日志。
- 模型导入 command 携带用户确认的 model ID 与文件选择目录，只由 settings service worker
  执行复制和复验；成功后刷新合并目录，但不隐式激活模型或修改选择配置。失败只返回稳定、
  可操作且不含用户路径的导入错误码。settings snapshot revision 同时观察 runtime 变化并为
  catalog-only 变化递增，禁止返回内容已变但 revision 未变的快照。settings client 为每次导入
  分配跨 clone 单调递增的强类型 operation ID；operation 只公开 prepare/copy/validate/commit
  stage、已复制文件数和字节数，不携带用户路径，并通过共享原子取消令牌在 service worker
  阻塞于分块复制时仍可取消。cancel 在原子 rename 提交前生效并清理 staging，final result
  携带同一 operation ID；后续 Models 页面只消费该契约，不自行执行文件 I/O。
- 模型删除 command 同样携带 `(origin, model_id)`；preset 永不可删。installed 模型只有在既
  不是当前 runtime active、也不是配置所选来源时才能以 rename 后删除事务退休；同 ID preset
  不得阻止删除 installed 副本。成功只刷新 catalog，不隐式切模或改写配置。
- 更新只允许 HTTPS，安装包和更新包必须签名并支持失败回滚。
- 日志不记录真实按键序列、剪贴板内容或用户文件内容。

初始字段命名和数据分类见 `shared/config/native-config-contract.md`，环境和 Bundle ID 决策见 ADR-0008。

继续兼容：

```text
.model3.json  .moc3  .motion3.json  .exp3.json
.physics3.json  .pose3.json  .cdi3.json
texture_*.png  audio files
resources/left-keys  resources/right-keys
resources/background.png  resources/cover.png
```

新增 manifest 只能是可选元数据，不能破坏现有模型。

## 13. 测试、指标与可观测性

### 13.1 测试层级

- Rust 单元测试：reducer、输入映射、动画、配置、路径验证和模型语义。
- fixture：相同输入序列产生规范化状态快照。
- Cubism fixture：三个预置模型和异常/自定义样本的加载、动作、表情、物理和销毁。
- GPUI 测试：设置表单、命令、错误状态、键盘导航和窗口重建。
- 平台集成：窗口、输入、权限、显示器、托盘、启动项和单实例。
- renderer smoke/golden：非空帧、alpha、遮罩、blend 和资源语义。
- 性能/soak：启动、首帧、frame time、输入延迟、CPU/RSS/GPU 和 8 小时运行。

### 13.2 输入不变量

- 每个 pressed key 最终由 KeyUp、状态校正或 Reset 释放。
- 队列压力不能静默丢 key/button edge。
- 重复 KeyDown 不破坏按压状态或边沿动画。
- 设备断开、锁屏和睡眠后 pressed set 为空。
- 鼠标移动合并不能阻塞键盘释放。

### 13.3 验收指标

- 60 FPS 时 p95 frame time <= 16.7 ms。
- input callback 到 runtime 接收 p95 目标 <= 2 ms。
- 正常压力测试 key/button edge 丢失计数为 0。
- 8 小时固定模型无持续内存增长或 GPU 资源泄漏。
- 所有线程在退出超时内完成 join，不依赖进程强杀。

Windows 使用 ETW/WPA、PresentMon、GPUView；macOS 使用 Instruments、Metal System Trace 和 os_signpost。基准记录硬件、系统、模型、DPI、FPS、样本和构建 commit。

## 14. Linux 策略

Linux 是后续能力，不是隐藏的首发任务：

- runtime、配置、模型和 UI 不使用 Windows/macOS 类型。
- GPUI 保持 X11/Wayland 构建路径，但首期 CI 不发布 Linux 安装包。
- X11 评估 XInput2；Wayland 全局输入受 compositor/portal 限制，不能承诺功能等价。
- Linux renderer、托盘、启动项和打包单独立项并建立能力矩阵。
- 不用轮询兼容层掩盖 Wayland 权限或协议缺失。

## 15. 风险与 Phase 0 退出条件

| 风险                         | 控制措施                         | 退出条件                          |
| ---------------------------- | -------------------------------- | --------------------------------- |
| GPUI pre-1.0                 | 精确 pin、UI 封装、升级隔离      | 双平台 UI/IME/辅助功能 smoke 通过 |
| GPUI 与 overlay 生命周期冲突 | 最小平台原型                     | 两窗口反复开关并正常退出          |
| Rust Live2D 工作量过大       | Core/动作/物理/renderer spike    | 三个预置模型完成输入到绘制闭环    |
| 透明合成不稳定               | D3D11/Metal 截图和压力测试       | alpha、置顶、穿透双平台通过       |
| 输入仍卡键                   | Raw Input/CGEventTap + reconcile | #47 和生命周期矩阵无残留键        |
| Cubism 授权不明确            | 二进制/许可证清单                | 发布方式有书面结论                |
| 后续 Linux 不等价            | 单独能力矩阵                     | 不影响 Windows/macOS 首发         |

任一核心退出条件失败，先记录 ADR 并调整受影响的实现或发布目标，不能用产品代码、合成测试或编译结果掩盖 spike 失败。按 ADR-0011，未完成的外部证据不阻止无关模块的渐进实现，但持续阻塞对应功能声明和 stable 发布。

## 16. ADR 摘要

### ADR-001：单一 Rust 应用

Rust 同时承担 UI、业务、平台调用和渲染实现，所有模块在同一 workspace 内以强类型接口协作。

### ADR-002：GPUI 只用于设置 UI

GPUI 提供设置体验，但不成为 Live2D renderer 或实时状态所有者，以隔离 pre-1.0 风险。

### ADR-003：平台原生 Overlay Renderer

Windows 使用 D3D11，macOS 使用 Metal。主猫窗口不嵌入 GPUI renderer。

### ADR-004：输入状态可校正

输入事件提供低延迟边沿，系统状态查询与生命周期 Reset 保证最终一致。任何 hook 库都不是唯一事实来源。

### ADR-005：Cubism 是唯一厂商 FFI 边界

允许调用官方 Cubism Core；BongoCat 业务不得进入 SDK bridge。

### ADR-006：首发不支持 Linux，但不封死 Linux

共享模块保持平台无关；Linux 的输入和窗口限制在后续能力矩阵中诚实处理。

### ADR-007：单一 Rust 运行环境

生产版本的 UI、运行时、平台服务和渲染器均属于同一 Rust 应用。历史版本只用于行为与资源对照。

### ADR-008：应用身份与存储环境隔离

Bundle ID 固定为 `com.ayangweb.bongo-cat`。Development 与 Production 使用相同数据结构和不同存储根，Native Rewrite 不读取旧配置。

### ADR-011：渐进实现与发布门禁分离

允许正式 Rust workspace 在外部证据补齐期间持续开发；Cubism 授权与分发、实机矩阵、签名和稳定性证据保持 stable 发布门禁。

### ADR-012：有序 Motion 音频服务

runtime 非阻塞发布强类型音效命令，独立 Rust worker 使用最小 rodio/FLAC feature 管理唯一 voice、错误恢复和 shutdown；音频失败不影响动作或渲染。

## 17. 实施阶段

### Phase 0：风险验证和行为冻结

冻结参考行为和模型样本，定义全新配置命名与环境隔离契约；完成 GPUI + 独立 overlay、Cubism、输入可靠性、透明合成和许可证 spike。

### Phase 1：Rust 工程骨架

在 Phase 0 外部证据补齐期间渐进建立 Cargo workspace、CI、日志、配置最小实现、runtime 生命周期、GPUI 空设置窗口和双平台空 overlay；只提升已通过对应 contract 的模块。

### Phase 2：输入到 Live2D 最小闭环

加载标准模型，键鼠输入驱动参数/动作并绘制；Windows 完成 #47 校正，macOS 完成权限和 tap 恢复。

### Phase 3：产品 Runtime 与模型兼容

实现状态、动画、手柄、动作、表情、物理、音效和模型管理，并由 fixture 验证。

### Phase 4：GPUI 设置和配置存储

完成 design system、设置页、模型管理、快捷键、权限、诊断、环境隔离与 Native schema 演进。

### Phase 5：系统集成

完成托盘/菜单栏、启动项、单实例、更新、日志导出、签名和权限流程。

### Phase 6：稳定性与发布

完成性能基线、异常恢复、8 小时 soak、安装/升级/回滚和发布清单。达到门槛后完成发布切换。

详细任务和完成定义见 `BongoCat Native Rewrite Implementation TODO.md`。
