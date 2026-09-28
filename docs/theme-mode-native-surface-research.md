# 主题模式与原生控件适配 —— 库调研与修复方案

调研日期：2026-09-18（`next` @ b6d9a67）
范围：窗口标题栏、系统弹框、右键菜单、托盘菜单、文件选择框在 macOS / Windows 上跟随应用主题
性质：调研与设计输入。**§1–§4 是事实与证据，仍然有效；§5–§6 是决策前的候选方案，已被 ADR-0048
取代，阅读实现时以 ADR-0048 为准。**

> **§5–§6 与最终实现的四处差异（2026-09-18 落地后补记）：**
>
> 1. **macOS 用进程级 `NSApplication.setAppearance:`，不是 §5.1 提议的窗口级
>    `NSWindow.setAppearance:`。** 窗口级不会被子窗口弹出的菜单和面板继承，会让"右键菜单 /
>    托盘菜单 / 文件面板"这几行留在系统主题上。§5.3 的 B2 行写的"窗口级外观替代进程级"是错的。
> 2. **`SetPreferredAppMode` 被采用，而不是 §6 决策 2 建议的"不采用"。** 用户明确选择采用以改善
>    弹框观感；取 `AllowDark` 而非 `ForceDark`，以维持"这些表面跟随系统主题"的边界。相应地
>    `apply_process_theme` 从 no-op 变成了 `init_native_theme`，且必须从 `main` 调用一次。
> 3. **公开 API 收敛为一个函数。** 实现是 `apply_theme(window, Option<AppTheme>)` +
>    `init_native_theme()`，而不是 §5.1 的 `apply_window_theme` + `apply_process_theme` 两个
>    函数：`None` 已经表达了"恢复系统外观"，再拆一个进程级入口只会让调用方多一次选择。
> 4. **B4 在 macOS 上不是读 `NSWindow.effectiveAppearance`，也没有在 Windows 上引入 `UISettings`。**
>    实现只查 `NSApplication.effectiveAppearance`（与 §5.3 的 B4 行写的 `NSWindow` 不同），
>    Windows 继续用 gpui 已经给的值——它从不安装应用级覆盖，gpui 的答案就是系统值。
>    另外 gpui 的**外观名映射**（`gpui-pre-macos-0.3.5/src/window_appearance.rs`）不识别
>    `AccessibilityHighContrastDarkAqua`，会打印并回退成 `Light`，所以这一处必须自研而不能复用
>    gpui 的查询（§3.1 评估记录见 ADR-0048 决策 2）。
>
> §5.3 的 B5（picker / prompt 的 `set_parent`）**本次未修**，理由见 ADR-0048「明确不做」。

---

## 1. 结论先行

1. **没有可以直接用的现成 Rust 库。** crates.io 上不存在"把应用主题应用到标题栏 / 弹框 / 菜单 /
   文件框"的跨平台 crate。逐个候选的核查结论见 §2。
2. **需要的能力已经在依赖树里**，不需要新增第三方依赖：
   - Windows：`windows 0.62.2` 已提供 `DwmSetWindowAttribute` +
     `DWMWA_USE_IMMERSIVE_DARK_MODE`（`Graphics/Dwm/mod.rs:241`）与 `SetWindowTheme`
     （`UI/Controls/mod.rs:1222`），只是当前 feature 没开。
   - macOS：`objc2-app-kit 0.3.2` 已有 `NSAppearance` feature（`Cargo.toml:140`），
     提供 `appearanceNamed`、`NSAppearanceNameAqua`、`NSAppearanceNameDarkAqua`，当前没开。
3. **真正的根因不是"缺库"**，而是三件事叠加：
   - 主题只应用在 `bongocat-ui`，而 `bongocat-platform`（弹框、菜单、托盘、文件框的 owner）
     **完全没有主题输入**，没有 `theme` 模块；
   - UI 用的 `cx.set_window_appearance(...)` 在 **Windows 上是空实现**（gpui-pre 的
     `Platform` trait 默认方法，Windows 后端没覆写），在 **macOS 上是进程级**
     （写 `NSApp.appearance`），两个平台的语义根本不一致；
   - 有 **三处**独立的主题同步路径，各自维护自己的"是否已应用"判断，其中更新窗口硬编码为
     `System`，会把另一个窗口刚设好的应用主题冲掉。

---

## 2. 候选库核查

核查方式：`cargo search` / `cargo info` / crates.io API / 阅读 registry 内源码。

| 候选 | 版本 | 许可证 | 覆盖什么 | 能否用 | 结论 |
| --- | --- | --- | --- | --- | --- |
| `dark-light` | 3.0.0 | MIT OR Apache-2.0 | 只**检测**系统亮/暗 | ✗ | 只读不写。且检测能力已由 gpui 提供（`Window::appearance()`），引入即重复 |
| `tao` | 0.37.0 | Apache-2.0 | Windows 暗色标题栏 + macOS，但绑定在 tao 自己的 `Window` 上 | ✗ | 我们的窗口由 `gpui-pre` 创建，tao 无法驱动既有 HWND；引入等于并存第二套窗口栈，违反 AGENTS §3.1/§9 |
| `winit` | — | — | 无任何主题 API | ✗ | 不相关 |
| `muda` | 0.20.0（已在用） | Apache-2.0 OR MIT | 有 `MenuTheme{Dark,Light,Auto}` + `set_theme_for_hwnd`（`items/menu.rs:667,379`） | △ | 上游注释明确写着 *"the theme only affects the menu bar itself and not submenus or context menu"*。托盘菜单和 overlay 右键菜单都是 popup，**用不上**；且是 Windows-only 的 `unsafe fn` |
| `tray-icon` | 0.25.0（已在用） | MIT OR Apache-2.0 | 无主题 API | ✗ | 托盘菜单主题只能由 `muda` 侧决定 |
| `rfd` | 0.17.2（已在用） | MIT OR Apache-2.0 | 无主题 API；但有 `set_parent`（`file_dialog.rs:96,241`、`message_dialog.rs:72,139`） | △ | 不能直接设主题，但 `set_parent` 决定了 macOS 上是 sheet 还是游离模态窗，进而决定外观继承来源。**当前项目一处都没调用** |
| `windows` | 0.62.2（已在用） | MIT OR Apache-2.0 | `DwmSetWindowAttribute` / `DWMWA_USE_IMMERSIVE_DARK_MODE` / `SetWindowTheme` | ✓ | 正解，需补 feature |
| `objc2-app-kit` | 0.3.2（已在用） | MIT | `NSAppearance`、`NSAppearanceNameAqua/DarkAqua` | ✓ | 正解，需补 feature |
| `winsafe` | 0.0.29 | MIT | 含 uxtheme 绑定 | ✗ | 为一个 `SetPreferredAppMode` 引入一个巨型 GUI 框架不成比例（§3.1 第 7 条） |
| `SetPreferredAppMode`（uxtheme 序号 135）封装 crate | — | — | 无 | ✗ | crates.io 上**不存在**可用封装；只能自行 `GetProcAddress` |

**结论**：走 ADR-0030 阶梯的第 4/5 级（操作系统原生能力 + workspace 已装依赖），
在 `bongocat-platform` 内写一个薄平台适配层。不新增第三方依赖。

---

## 3. 平台能力矩阵：哪些控件**能**跟随应用主题

Microsoft 官方文档（*Support Dark and Light themes in Win32 apps*）只承诺一件事：**暗色标题栏**
（`DwmSetWindowAttribute` + `DWMWA_USE_IMMERSIVE_DARK_MODE`）。其余全部属于未文档化领域。

| 控件 | macOS | Windows | 说明 |
| --- | --- | --- | --- |
| 窗口标题栏 | ✅ 应用主题 | ⚠️ 见下 | macOS：`NSWindow.appearance`（原生 titled 窗口）。Windows：**我们的窗口没有 WS_CAPTION**（见 §4.2），DWM 只画边框/圆角/阴影 |
| 系统弹框（MessageBox / TaskDialog / NSAlert） | ✅ 应用主题 | ⚠️ 仅系统主题 | macOS：`NSAlert` 继承所在窗口或 `NSApp` 的 appearance。Windows：只有未文档化的 `SetPreferredAppMode(AllowDark)` 能在**系统本身处于暗色时**让控件变暗，无法强制成应用主题 |
| 右键菜单 / 托盘菜单 | ⚠️ 待实机确认 | ❌ 仅系统主题 | macOS：`NSMenu` 走 `muda`，外观按视图/应用继承，需实机确认取的是哪个层级。Windows：Win32 popup menu 无任何文档化暗色途径；`muda::MenuTheme` 上游已声明不覆盖 popup |
| 文件选择框 | ✅ 应用主题 | ❌ 仅系统主题 | macOS：`NSOpenPanel`/`NSSavePanel` 继承窗口或 `NSApp` appearance。Windows：`IFileDialog` 是 shell 对话框，只跟随系统，无 API 可覆盖 |

**这意味着用户"支持就跟随应用主题、不支持就跟随系统主题"的诉求，在 Windows 上对菜单和文件框
无法满足"跟随应用主题"那一半**，只能跟随系统主题。这是一个需要产品决策的点，见 §6。

---

## 4. 现状实现与缺陷

### 4.1 现在的主题链路

```
config.appearance.theme {System|Light|Dark}          crates/bongocat-config/src/lib.rs:180,186,941
  └─ SettingsCommand::SetAppearanceTheme              crates/bongocat-app/src/settings.rs:572
      └─ snapshot.appearance_theme                    crates/bongocat-ui/src/lib.rs:450
          ├─ SettingsView::render                      window/render.rs:112
          │   └─ apply_component_theme                 window.rs:1906
          │       ├─ cx.set_window_appearance(..)      ← GPUI API，两平台语义不同
          │       └─ Theme::change(mode, window, cx)   ← 组件库配色
          └─ window.observe_window_appearance          window/lifecycle.rs:73-85
              └─ sync_system_component_theme           window.rs:1891
```

### 4.2 缺陷清单（均带证据）

**B1（P0 / Windows）`cx.set_window_appearance` 在 Windows 上是空操作。**
`gpui-pre-0.3.5/src/platform.rs:235` 是 trait 默认实现 `fn set_window_appearance(&self, _: Option<WindowAppearance>) {}`；
`gpui-pre-macos-0.3.5/src/platform.rs:726` 有覆写，`gpui-pre-windows-0.3.5` 递归检索 **0 处**覆写。
Windows 侧只会在建窗时（`gpui-pre-windows-0.3.5/src/window.rs:569`）和收到 `ImmersiveColorSet`
时（`events.rs:1248`）用**系统**外观调用 `configure_dwm_dark_mode`（`util.rs:134`）。
→ 用户在 Windows 上选暗色，任何原生表面都不会变。

**B2（P0 / macOS）应用主题是进程级的，且被更新窗口冲掉。**
`gpui-pre-macos-0.3.5/src/platform.rs:745` 写的是 `NSApplication.setAppearance:`，不是
`NSWindow.setAppearance:` → 覆盖作用于整个进程，两个窗口无法各自持有不同主题。
而 `crates/bongocat-ui/src/update_window.rs:245` 硬编码
`self.sync_component_theme(SettingsTheme::System, window, cx)`，`apply_component_theme` 在
`System` 分支会执行 `cx.set_window_appearance(None)`（`window.rs:1909`）——**把设置窗口刚设好的
暗色覆盖清掉**。设置窗口下次 render 又设回暗色 → 闪烁 + "更新窗口不跟随我的主题选择"。

**B3（P1）三套并行的主题同步，幂等判断各不相同。**

| 位置 | 幂等缓存 | 备注 |
| --- | --- | --- |
| `window/view_state.rs:4` | 无 | 设置窗口每次 render 都跑 |
| `update_window.rs:222` | `applied_theme` | 与上面语义不同 |
| `window.rs:1895` `apply_optimistic_component_theme` | 比对 `cx.theme().mode` | 调 `Theme::change(mode, None, cx)` —— **window 传 `None`，不刷新任何窗口** |

**B4（P1）切回 `System` 时读到的是过期外观。**
`apply_component_theme` 的 `System` 分支先 `cx.set_window_appearance(None)`，紧接着读
`window.appearance()`。但 `Window::appearance()` 返回的是缓存字段
（`gpui-pre-0.3.5/src/window.rs:2735`），只在 `appearance_changed()`（同文件 `2720`，由平台的
`on_appearance_changed` 延迟触发）里更新。同一帧内读到的仍是覆盖前的值。
macOS 上"暗色 → 系统"可能不生效。

**B5（P1）原生弹框全部无 parent。**
`startup_permission.rs:102-111`（macOS `AsyncMessageDialog`）、`158-166`（Windows
`MessageDialog`）、`model_source_picker.rs:117,139,162,179,201,218` 六处 picker
均未调用 `rfd` 的 `set_parent`。全仓库 `set_parent` 命中 0 处。
→ macOS 上是游离模态窗（外观只能继承 `NSApp`，恰好"能跟随"是巧合，不是设计）；Windows 上
主题完全不传播。

**B6（P2）`bongocat-platform` 没有主题入口。**
`crates/bongocat-platform/src/` 下不存在 `theme.rs`，`SystemMenu`、picker、prompt 的构造签名
里没有任何主题参数。菜单/弹框/文件框若要跟随主题，现在**无处可接**。这是"代码越写越乱"的结构性
原因：主题的知识只存在于 UI 层，而原生表面的 owner 在平台层。

**补充事实（影响方案，不是缺陷）**：设置窗口在 Windows 上**没有原生标题栏**。
`gpui-pre-windows-0.3.5/src/window.rs:487-507` 对 `WindowKind::Normal` 只设
`WS_SYSMENU | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX`，**从不设 `WS_CAPTION`**；
`events.rs:786` 在 `hide_title_bar == false` 时让 `WM_NCCALCSIZE` 走默认处理，所以非客户区只是
一圈 resize 边框。项目用的 `TitlebarOptions { title: .., ..Default::default() }`
（`window/lifecycle.rs:28`）里 `appears_transparent` 默认 `false`。macOS 侧相反：
`gpui-pre-macos-0.3.5/src/window.rs:995` 设了 `NSTitledWindowMask | NSClosableWindowMask`，
有原生标题栏（项目为此在 `window/lifecycle.rs:272` 计算 `settings_window_content_top_inset()`）。
→ **"标题栏跟随主题"在 macOS 是真实需求，在 Windows 只剩 DWM 边框/圆角/阴影这一项。**

---

## 5. 修复方案

### 5.1 新增平台主题适配层

新增 `crates/bongocat-platform/src/theme.rs`：

```rust
/// 已解析的主题：不含 System，调用方负责先把 System 解析成当前实际外观。
pub enum AppTheme { Light, Dark }

pub enum ThemeError { WindowHandleUnavailable, UnsupportedWindowHandle, NativeCallFailed }

/// 把主题应用到单个窗口。窗口内的 sheet / 子控件继承该窗口外观。
pub fn apply_window_theme(window: &impl HasWindowHandle, theme: AppTheme)
    -> Result<(), ThemeError>;

/// 进程级兜底：无 parent 的原生弹框只能继承这一层。
pub fn apply_process_theme(theme: AppTheme) -> Result<(), ThemeError>;
```

实现要点：

- **macOS**：`apply_window_theme` 走 `NSWindow.setAppearance:`（按窗口，而不是现在 gpui 的按进程），
  用 `objc2_app_kit::NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua/Aqua)`。
  `apply_process_theme` 才写 `NSApp.appearance`。需要给 `objc2-app-kit` 打开 `NSAppearance` feature。
- **Windows**：`apply_window_theme` 调 `DwmSetWindowAttribute(hwnd,
  DWMWA_USE_IMMERSIVE_DARK_MODE, &BOOL(theme.is_dark()))`。需要给 `windows` 打开
  `Win32_Graphics_Dwm` feature。`SetWindowTheme`（需 `Win32_UI_Controls`）**暂不建议加**：
  GPUI 窗口没有主题化的子控件，只有 `WS_THICKFRAME` 边框，加了对标题栏无收益。
  `apply_process_theme` 在 Windows 上是 no-op（见 §6 决策 2）。
- 两个函数都是平台分支内的最小 `unsafe`，按 AGENTS §8 在每个 block 前写明安全不变量
  （HWND 必须属于本进程且存活 / 必须在主线程）。

### 5.2 主题归属收口

- 在 `bongocat-runtime` 或 `bongocat-app` 里保留"当前生效主题"的唯一解析点：
  `System → 由平台查询到的系统外观`，`Light/Dark → 直接采用`。UI 和平台层都读它。
- `bongocat-ui` 停止直接调 `cx.set_window_appearance`，改为
  `bongocat_platform::apply_window_theme(window, resolved)`，再调
  `Theme::change(mode, Some(window), cx)` 只负责组件库配色。**职责分离**：
  平台层管原生表面，UI 层管自己画的像素。

### 5.3 逐项修复

| 缺陷 | 修法 |
| --- | --- |
| B1 | 走 5.1 的 Windows 分支，不再依赖 gpui 的空实现 |
| B2 | `update_window.rs:245` 改为读设置快照里的 `appearance_theme`（该窗口已在轮询 `SettingsClient`）；窗口级外观替代进程级 |
| B3 | 删掉 `apply_optimistic_component_theme` 的 `None` 窗口路径，或让它与 `apply_component_theme` 合并成单一入口；三处幂等判断收敛成一个 |
| B4 | 解析 `System` 时不再读 `Window::appearance()` 缓存，改由平台层直接查询（macOS 读 `NSWindow.effectiveAppearance`，Windows 读 `UISettings`，后者 gpui 已有实现可参照 `gpui-pre-windows-0.3.5/src/util.rs:160`） |
| B5 | 六处 picker + 两处 prompt 补 `set_parent`，把窗口句柄从调用方传下来；同时消掉 `model_source_picker.rs:110` 的 `asynchronous_sheet_is_available` 兜底逻辑（有 parent 后 sheet 必然可用） |
| B6 | 由 5.1 提供入口，`SystemMenu` / picker / prompt 的构造签名增加 `AppTheme` 参数 |

### 5.4 验证方式

- 单元测试：`AppTheme` 解析、`System` → 实际外观的映射（可注入）。
- macOS 实机：设置窗口切 Dark/Light/System，检查标题栏、托盘菜单、右键菜单、文件框、
  权限弹框；再开更新窗口确认两个窗口互不覆盖。
- Windows 实机：检查 DWM 边框颜色、托盘菜单、右键菜单、文件框、权限弹框。
- 系统主题在运行中切换（macOS 外观切换 / Windows `ImmersiveColorSet`）后仍要正确。
- `just check` 全门禁。

---

## 6. 需要决策的三点

1. **Windows 菜单与文件框只能跟随系统主题**，无法跟随应用主题。是接受"跟随系统"（符合用户
   原话的兜底分支），还是要为托盘/右键菜单自绘一套 GPUI 菜单？后者会触碰 ADR-0031 的托盘边界。
2. **是否采用未文档化的 `SetPreferredAppMode`（uxtheme 序号 135）。** 它能让 Windows 弹框和
   标准控件在**系统处于暗色时**变暗，但：未文档化、靠硬编码序号、进程级、无法强制成应用主题、
   且必须常驻不能 `FreeLibrary`。建议**不采用**，理由是与需求（跟随应用主题）不匹配且引入
   不可验证的依赖面。
3. **本次改动的文档落点。** 按 AGENTS §3.4.5，实现前需要新增 ADR（主题与原生表面边界）并在
   Implementation TODO 里建立对应条目与退出门槛。是否本次一并完成？

---

## 7. 参考

- Microsoft, *Support Dark and Light themes in Win32 apps* —— 官方只承诺暗色标题栏
- `gpui-pre 0.3.5`：`src/platform.rs:235,1444`、`src/window.rs:2186,2720,2735`
- `gpui-pre-macos 0.3.5`：`src/platform.rs:718,726-746`、`src/window.rs:995,1628,3436`
- `gpui-pre-windows 0.3.5`：`src/window.rs:487-507,569`、`src/events.rs:786,1248`、`src/util.rs:134,160`
- `muda 0.20.0`：`src/items/menu.rs:355,379,667`
- `rfd 0.17.2`：`src/message_dialog.rs:72,139`、`src/file_dialog.rs:96,241`
- `windows 0.62.2`：`Win32/Graphics/Dwm/mod.rs:241`、`Win32/UI/Controls/mod.rs:1222`
- `objc2-app-kit 0.3.2`：`Cargo.toml:140`、`src/generated/NSAppearance.rs:65,120,125`
