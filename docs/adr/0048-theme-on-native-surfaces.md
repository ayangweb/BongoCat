# ADR-0048: 主题跟随到原生表面

状态：已接受（2026-09-18）
依赖：ADR-0030（优先复用现有方案）、ADR-0031（托盘图标边界）、ADR-0020（GPUI Kit 统一依赖入口）

## 背景

1. **产品要求**：用户选择的浅色/深色要作用于**产品没有自己绘制的**那些表面——窗口标题栏、
   系统弹框、右键菜单、托盘菜单、文件选择框。这些表面的 owner 是操作系统，不是 GPUI。
2. **哪些表面能跟随应用主题，由平台决定，不由产品决定**：macOS 通过进程级
   `NSApplication.appearance` 可以让整棵继承链（窗口 → 弹框 → 菜单 → 面板）一起变；Windows
   只有窗口框有官方 API，ComCtl32 弹框、Win32 菜单和 shell 文件框**没有任何**官方接口可以跟随
   应用主题，最多只能跟随**系统**主题。
3. **调研结论：不存在可以直接用的现成 crate。** 逐个核查的候选见
   `docs/theme-mode-native-surface-research.md` §2，摘要如下：

   | 候选 | 为什么不能直接用 |
   | --- | --- |
   | `dark-light` | 只**检测**系统外观，不设置任何东西 |
   | `tao` / `winit` | 完整窗口库；引入等于替换 GPUI 的窗口层 |
   | `muda::MenuTheme` | 上游明确只作用于菜单栏，**不含 popup**；Windows 上等于没有 |
   | `tray-icon` / `rfd` | 没有主题 API |
   | `windows` / `objc2-app-kit` | 是 binding，不是方案——但它就是我们需要的那一层 |
   | `winsafe` | 为一个窗口属性引入第二个 Win32 抽象层，不成比例 |
   | 任何 `SetPreferredAppMode` 封装 crate | crates.io 上不存在 |

   所需能力全部已在依赖树内（`windows 0.62.2`、`objc2-app-kit 0.3.2`），因此按 §3.1 阶梯
   停在"已安装依赖 + OS 原生能力"这一级，**不新增第三方依赖**。
4. **根因是结构性的，不是零散 bug。** `bongocat-platform` 此前没有主题入口
   （`crates/bongocat-platform/src/` 下不存在 `theme.rs`），`SystemMenu`、picker、prompt 的构造
   签名里没有任何主题参数。于是主题知识只存在于 UI 层，而原生表面的 owner 在平台层——这是
   "主题 bug 很多、代码越写越乱"的结构性原因。
5. **现状缺陷（带证据，完整清单见调研文档 §4.2）**：

   - **B1（P0 / Windows）** `cx.set_window_appearance` 在 Windows 上是 trait 默认空实现
     （`gpui-pre-0.3.5/src/platform.rs:235`；macOS 有覆写，Windows 后端 0 处覆写）。用户在
     Windows 上选暗色，**任何原生表面都不会变**。
   - **B2（P0 / macOS）** gpui 的 macOS 覆写写的是 `NSApplication.setAppearance:`（进程级），而
     `update_window.rs` 硬编码 `SettingsTheme::System`，其 `System` 分支会清掉覆盖 →
     **更新窗口打开就把设置窗口刚设好的暗色清掉**，下一次 render 再设回来（闪烁）。
   - **B3（P1）** 三套并行同步，幂等缓存各不相同（`window/view_state.rs:4` 无缓存、
     `update_window.rs` 用 `applied_theme`、`window.rs` 的 `apply_optimistic_component_theme`
     比对 `cx.theme().mode` 且 `Theme::change(mode, None, cx)` 不刷新任何窗口）。
   - **B4（P1）** 切回 `System` 时先清覆盖、紧接着读 `window.appearance()`，而它返回的是只在
     `appearance_changed()` 里更新的**缓存字段**，同一帧内读到的仍是覆盖前的值。
   - **B5（P1）** 原生弹框与 6 处 picker 全部没有 `set_parent`（全仓库 `set_parent` 命中 0 处）。
   - **B6（P2）** `bongocat-platform` 没有主题入口，原生表面若要跟随主题**无处可接**。

6. **一个影响范围的事实**：设置窗口在 Windows 上**没有原生标题栏**——gpui 对
   `WindowKind::Normal` 只设 `WS_SYSMENU | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX`，
   从不设 `WS_CAPTION`。所以"标题栏跟随主题"在 macOS 是真实需求，在 Windows 只剩 DWM 的
   边框/圆角/阴影这一项。

## 决策

### 1. `bongocat-platform` 新增 `theme` 模块，作为原生主题的唯一入口

`crates/bongocat-platform/src/theme.rs` 公开四个类型和三个函数：

```rust
pub enum AppTheme { Light, Dark }          // 已解析的取值，System 不往下传
pub enum SystemAppearance { Light, Dark }  // 系统当前外观
pub enum NativeThemeError { .. }           // 稳定 error code，不进 config
pub fn apply_process_theme(theme: Option<AppTheme>) -> Result<(), _>
pub fn apply_theme(window: &impl HasWindowHandle, theme: Option<AppTheme>) -> Result<(), _>
pub fn init_native_theme() -> Result<(), _>
#[cfg(target_os = "macos")] pub fn system_appearance() -> SystemAppearance
```

`apply_process_theme` 是启动入口：`bongocat-app` 在把 `Application` 移入 settings service
之前读取持久化的 `appearance.theme`，并在 `gpui_application.run` 的主线程回调中、创建 overlay
窗口之前调用它。这样 macOS 的 `NSApplication.appearance` 先于模型窗口和其右键菜单生效；
主题不依赖设置窗口是否曾经打开。窗口级 `apply_theme` 仍用于 Windows DWM 窗口框以及设置/更新
窗口创建后的幂等重应用。Windows 的 `apply_process_theme` 是 no-op，系统菜单/弹框/文件框由
`init_native_theme` 预置为跟随系统主题。


`AppTheme` 是**已解析**的取值：调用方决定 `System` 是什么意思，平台层永远收不到 `System`，
所以两处调用不可能各自推导出不同答案。

### 2. `System` 的解析口径收归平台层，并且全 crate 只有一处解析

`SettingsTheme::System` 不再读 `window.appearance()`。macOS 走
`bongocat_platform::system_appearance()`（`NSApplication.effectiveAppearance`），其余平台沿用
gpui 的值（它们从不安装应用级覆盖，gpui 的答案就是系统值）。这直接修掉 B4：平台层查的是
**当前生效**的外观，不是需要等回调刷新的缓存。

`bongocat-ui` 内只有 `resolved_theme_mode(theme, window, cx)` 一个入口做这个解析，render 路径与
smoke 断言都调它。**smoke 此前用 `component_theme_mode(theme, cx.window_appearance())` 独立推导
期望值**，与产品实际走的路径不是同一条；smoke 的职责是证明产品做了什么，用自己的第二套公式去
推导期望值只会让两者悄悄漂移，所以改成调用产品自己的入口。

**为什么 macOS 不复用 gpui 已有的查询（§3.1 评估记录）**：gpui 的 `App::window_appearance()` 与
`Platform::window_appearance()`（`gpui-pre-macos-0.3.5/src/platform.rs:718`）确实已经实时读
`NSApplication.effectiveAppearance`，形式上够用。但它的名字映射
（`gpui-pre-macos-0.3.5/src/window_appearance.rs`）只识别 `Aqua`、`DarkAqua`、`VibrantLight`、
`VibrantDark`，其余一律**打印到 stdout 并回退成 `Light`**。macOS 开启「提高对比度」后 AppKit 报的
是 `AccessibilityHighContrastDarkAqua`，gpui 会把暗色系统判成浅色，产品于是会在暗色系统里画出
浅色 UI。这是 gpui 的既存缺陷，产品不能继承，所以这一处自研（约 15 行）而不是复用。

### 3. macOS：进程级 `NSApplication.setAppearance:`

这是唯一能覆盖全部四行表面的做法。按窗口设置只会让菜单和面板继续留在系统主题上，因为
`NSWindow.appearance` 不会被窗口自己弹出的菜单继承。

`NSAppearanceName*` 常量以字符串字面值写入（`appearanceNamed:` 与 `-isEqualToString:` 都按内容
比较），避免为一个常量引入 `unsafe`。深色判定接受 `DarkAqua`、`VibrantDark` 和
`AccessibilityHighContrastDarkAqua` 三个名字。

### 4. Windows：窗口框用官方 API，其余表面接受"跟随系统主题"

- 窗口框：`DwmSetWindowAttribute` + `DWMWA_USE_IMMERSIVE_DARK_MODE`（唯一有文档的做法）。
  `theme = None` 时**什么都不做**——系统偏好由 Windows 自己读，没有属性需要清。
- 弹框 / 菜单 / 文件框：**产品不自绘**，接受跟随系统主题。为了让它们在系统为暗色时真的变暗，
  进程在创建任何窗口之前调用一次未文档化的 `SetPreferredAppMode`（`uxtheme.dll` 序号 135），
  取 `PreferredAppMode::AllowDark`（= 1）。

  选 `AllowDark` 而不是 `ForceDark` 是刻意的：这些表面的契约是"跟随**系统**主题"，不是"跟随
  应用主题"。用 `ForceDark` 会让系统为浅色的用户在这些表面上看到暗色，与第 4 条的边界矛盾。

  调用点在 `main` 里、`RunOptions::parse` 之后、任何窗口之前，**一次，且不撤销**。

### 5. 启动时先应用进程级主题

主题的全局事实来源是持久化的 `appearance.theme`，不是设置窗口的生命周期。`bongocat-app`
在创建 overlay 之前解析该字段并调用 `bongocat_platform::apply_process_theme`：macOS 直接
设置 `NSApplication.appearance`，因此模型窗口及其右键菜单从第一帧起就继承正确外观；Windows
该入口不做应用级强制，继续由 `init_native_theme` 让系统-owned 表面跟随系统。设置/更新窗口
打开后仍会调用 `apply_theme`，但那是幂等补偿，不是首次生效的前置条件。

### 6. 原生主题失败不致命

`apply_theme` / `init_native_theme` 的失败一律降级为"该表面保持系统外观"，也就是每个平台的
文档化兜底。UI 层因此 `let _ =` 丢弃结果，不向用户报错、不改配置、不阻止启动。理由：能拒绝的
表面恰好就是**本来就无法跟随应用主题**的那些，为一个外观开关拒绝启动或弹出错误更糟。

### 7. 更新窗口改为携带应用外观，而不是硬编码 `System`

`UpdateView` 增加 `appearance_theme` 字段，`open_update_window` 增加同名参数，`ProductCoordinator`
缓存它并在系统菜单轮询里同步。更新窗口的 `observe_window_appearance` 回调改为读
`appearance_theme`：只有当前偏好是 `System` 时才响应系统变化，固定的偏好已经交给平台，不会因为
系统变了而改变。这修掉 B2。

## 明确不做

- **不新增任何第三方依赖**：能力已在 `windows` / `objc2-app-kit` 内，见背景第 3 条。
- **不自绘菜单或弹框**：那等于重写系统控件，且会失去平台的无障碍与输入法集成。
- **不使用 `muda::MenuTheme`**：上游明确它不覆盖 popup，用它只会让人误以为已经解决。
- **不做 Linux**：首发范围外（ADR-0006）；Linux 分支的 `apply` / `init` 是**有文档的 no-op**，
  不是待办。
- **不在平台层做颜色**：这个模块只回答"哪个外观"，不回答"什么颜色"。
- **不给原生表面加配置字段**：`schema_version: 1` 不变，`appearance.theme` 仍是唯一来源。
- **不在本次修 B5（picker/prompt 的 `set_parent`）**：它是模态归属与 UX 缺陷，不是主题缺陷。
  macOS 上无 parent 的弹框继承 `NSApp` 外观，恰好已经能跟随应用主题；Windows 上跟随系统主题。
  修它属于独立改动。

## 残余风险与待验证项（不得当作已确认）

1. **`SetPreferredAppMode` 是未文档化的序号导出。** Microsoft 从未发布头文件或函数名；序号在
   未来的 Windows 版本或补丁里可能被移除或改变语义。已做的处置：用
   `LOAD_LIBRARY_SEARCH_SYSTEM32` 解析（防止被同名 DLL 劫持）、解析结果缓存、缺失时返回
   `NativeCallFailed` 并静默退化。**这不能证明它在所有受支持 Windows 版本上行为一致。**
2. **Windows 分支只经过隔离 crate 编译验证，不是真实目标构建。** workspace 无法交叉编译到
   `x86_64-pc-windows-msvc`（`libdeflate-sys`，来自 `oxipng`，需要 Windows C 工具链）。已用
   `/tmp/theme-win-check` 隔离 crate（`raw-window-handle 0.6.2` + `windows 0.62.2`，同 features）
   抽取 Windows 分支单独编译，`cargo check` 与 `cargo clippy -- -D warnings` 均通过。**类型正确
   不等于运行时正确。**
3. **双平台实机均未验证。** 未在 Windows 上确认 DWM 暗色边框、暗色弹框/菜单/文件框实际生效；
   未在 macOS 上确认标题栏、弹框、托盘菜单、文件面板随应用主题切换。
4. **`NSApplication.appearance` 的作用面比需求更宽。** 它是进程级的，会作用于产品没有列出的
   表面（例如 AppKit 自己画的一切）。当前没有观察到副作用，但这是一个"比要求做得更多"的选择。
5. **更新窗口的乐观路径有滞后。** 设置页下拉切换主题时
   `apply_optimistic_component_theme` 只改组件主题、不碰原生表面（该函数拿不到 `Window`），
   原生表面要等配置往返 + 下一次 snapshot 轮询才更新。滞后一个轮询周期，最终一致。
6. **`AccessibilityHighContrastDarkAqua` 只在提高对比度时出现。** 把它算作暗色是有意为之
   （见决策 2：gpui 会把它判成浅色），但它没有在提高对比度 + 暗色的实机上验证过。
7. **`apply_theme` 要求主线程（macOS），但 UI 层丢掉了结果。** 非主线程调用返回 `WrongThread`
   而不是 panic，`system_appearance()` 在非主线程返回 `Light`（中性兜底，不是真实答案）。
   两者都是刻意的降级，但 `apply_native_theme` 用 `let _ =` 丢弃返回值，所以**一次线程模型的
   回归会表现为"主题悄悄不生效"而不是任何信号**。当前 `apply_component_theme` 只从 render
   调用，不存在这个路径；若要让它可诊断，需要按 ADR-0017 的有界日志契约新增一个固定 code，
   本次未做。
8. **本 ADR 只覆盖"哪个外观"，不覆盖颜色。** 主题色的架构评估见
   `docs/theme-color-extraction-evaluation.md`（结论：不拆 crate，在 `bongocat-ui` 内收口）。
9. **`apply_optimistic_component_theme` 与 `resolved_theme_mode` 仍是两个函数。** 前者只服务
   "用户刚点了下拉、配置还没往返" 这一个场景，且拿不到 `Window`，因此没有并入统一入口。B3 的
   三套并行同步已收敛为「一处解析 + 一处乐观」两条职责清晰的路径，但没有收敛成一条。
10. **macOS 的 AppKit 退出路径不会回到 `main`，所以失败必须在可达边界处理。** `App::quit()`
   走 `msg_send![NSApplication, terminate:]`（`gpui-pre-macos-0.3.5/src/platform.rs:557-575`），
   `NSApplication::run()` 不返回；末尾的 `Arc::try_unwrap(failures)` 汇总对 macOS 永远不可达。
   原先 smoke 记录失败却 exit 0，已由 TODO 第 87 项修复：automated verification 在
   `on_app_quit` 调 `begin_product_shutdown` 后、等待异步 `finish()` 前检查已有失败，写入固定的
   `product run failed: ...` 并 `std::process::exit(1)`。实测故意注入 `forced smoke failure` 得到
   exit 1；干净的 `--settings-window-smoke` 得到 exit 0。
   **剩余边界**：如果失败只在 `finish().await` 内新产生，而 AppKit 在 future 完成前终止，仍可能
   无法反映到退出码；当前 smoke 断言和绝大多数 runtime failure 都在 quit 之前已记录。
11. **收敛成单一解析入口后，smoke 的断言只能验证"一致"，不能验证"正确"。** 产品设的值与 smoke
   期望的值来自同一个 `resolved_theme_mode`，所以把一个错误的解析同时喂给两边时断言仍然通过
   （已用变异确认）。解析本身的正确性依赖 `NSApplication.effectiveAppearance` 与
   `appearanceNamed:` 这两个文档化 API，以及两个不变量单测；没有端到端的正确性测试。

## 后记（2026-09-18）：启动权限弹框的例外已修复

本文档的「alerts | application theme」一行只覆盖经 `NSApplication.appearance` 继承链的 AppKit
弹框。落地当天的实机截图显示启动权限提示在应用深色下仍是浅色：该提示当时走 `rfd` 的无父窗口
消息框 = `CFUserNotification`，完全不经过 AppKit，不在继承链上——ADR-0048 的实现没有错，是
这个提示本身不在被修复的路径上。已按 ADR-0032「主题外观修正（2026-09-18）」把它改为主线程
`NSAlert`，本表格自此对产品全部系统弹框成立。

## 验证

已完成（2026-09-18，本机 macOS arm64，固定工具链 1.97.1）：

- `just check` 六道门全绿：`cargo fmt --all -- --check`；三组严格 Clippy（workspace 排除 app、
  app `storage-test-injection`、app `production`）；`cargo test --locked --workspace`
  **749 passed / 0 failed**；`cargo check --locked --workspace --release`。
- `cargo check --locked --workspace --all-targets` 通过，`bongocat-platform` / `bongocat-ui` /
  `bongocat-app` 无告警。
- Windows 分支交叉编译：`/tmp/theme-win-check` 的 `cargo check --target x86_64-pc-windows-msvc`
  与 `cargo clippy --target x86_64-pc-windows-msvc -- -D warnings` 均通过（以临时注入
  `compile_error!` 的方式确认该文件确实被编译，随后还原）。
- `crates/bongocat-platform/src/theme.rs` 单测：`error_codes_are_stable_and_unique`（4 个 error
  code 唯一且带 `native_theme_` 前缀）、`only_dark_reports_itself_as_dark`。
- `crates/bongocat-ui` 新增 2 个不变量测试（共 117）：
  `only_the_system_choice_lets_the_system_decide` 断言固定偏好对
  `Light`/`Dark`/`VibrantLight`/`VibrantDark` 四种系统外观都解析到同一个模式、且只有 `System`
  允许跟随系统；`the_native_and_component_halves_pin_together` 断言 `pinned_native_theme` 与
  `pinned_theme_mode` 同时固定或同时不固定。两者都用**变异测试**验证过有牙齿：把
  `pinned_theme_mode(SettingsTheme::Light)` 改成返回 `Dark` 后两个测试都失败，随后还原。
- 依赖 feature 变更已记录用途：`objc2-app-kit` 增加 `NSAppearance`，`windows` 增加
  `Win32_Graphics_Dwm`。
- **主题全局启动路径实测**：`--run-seconds 4 --hidden-model-switch-smoke`（不打开设置窗口）
  exit 0，模型 overlay 正常启动并完成 hidden model switch；启动时已在 overlay 创建前调用
  `apply_process_theme`，因此模型窗口及其右键菜单不再依赖设置窗口。
- **macOS 运行时冒烟（`just dev-smoke` = `--run-seconds 4 --settings-window-smoke`）跑通，
  exit 0**：设置窗口打开、辅助功能桥挂上、`show_general_page_for_smoke` 在主线程上执行完
  `apply_component_theme` → `apply_native_theme` → `NSApplication.setAppearance` /
  `effectiveAppearance`，无 panic、无 `WrongThread`、正常 shutdown。现在 exit 0 有基本的
  失败传播保证：automated smoke 若在 quit 前记录失败会 exit 1。
- **退出码变异验证**：在 smoke 分支故意记录 `forced smoke failure`，同一命令得到
  `exit 1` 且 stderr 输出 `product run failed: forced smoke failure`；还原后干净 smoke 得到
  `exit 0`。这证明第 87 项修复点确实可达且有牙齿。
- **主题断言的强度仍有限**：`resolved_theme_mode` 同时被产品路径和 smoke 期望值调用，因此
  smoke 验证的是一致性；解析本身由 macOS `effectiveAppearance`/`appearanceNamed:` 文档化 API
  与不变量单测约束，不是端到端的肉眼主题验证。

**未运行**：双平台实机主题切换（肉眼确认标题栏/弹框/托盘菜单/文件面板的深浅色）；Windows DWM
暗色边框与暗色弹框/菜单/文件框实测；`SetPreferredAppMode` 在 Windows 10 1903 / 11 各版本上的
行为；运行中切换系统主题后的跟随行为；提高对比度 + 暗色组合；只在
`ProductShutdown::finish()` 内新产生失败时的 macOS 退出码传播。
