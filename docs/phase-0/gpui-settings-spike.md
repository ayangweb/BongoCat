# GPUI Settings Spike Record

状态：macOS 默认 shader、`.app`、主题、基础文本交互、应用菜单、marked-text contract、runtime bridge、loading/error/retry、tooltip 与 modal dialog/AccessKit AX tree/action 通过；Windows 真实窗口/首帧/shutdown、dialog 与 loading/error/retry UI Automation 已通过，`busy=true` 投影、双平台真实 IME 与真实辅助技术待完成
日期：2026-08-30
原始重构基线 commit：`94af230`；后续验证源码与本记录保持同一提交

## 范围

本 spike 只验证以下最小闭环：

1. 独立于历史 Tauri workspace 解析并锁定 `gpui = 0.2.2`；
2. 创建 760x520 设置窗口并绘制基础布局和文本；
3. 使用 GPUI 默认预编译 shader 路径完成 debug/release 构建；
4. 生成具有固定 bundle id 的最小 macOS `.app` 并通过 LaunchServices 启动；
5. 验证应用菜单、关闭、重开和 GPUI `quit()` 生命周期；
6. 使用 GPUI 公共输入协议验证一个最小设置表单：System/Light/Dark 主题选择、焦点边框、Tab/Shift-Tab、Unicode 文本编辑、选择、剪切、复制、粘贴和 marked-text 接口；
7. 通过合成 runtime 验证 GPUI executor、强类型 command、revision snapshot 和 shutdown acknowledgement；不接入产品 runtime、输入、Live2D 或原生 overlay。
8. 通过 GPUI 公共 raw window handle 安装 AccessKit adapter，验证系统辅助功能 API 可读取设置语义并把 action 送回 GPUI 主线程。

源码位于 `spikes/gpui-settings/`。该目录包含独立 `Cargo.lock`，不会改变历史根 workspace。

## 环境

```text
Hardware: MacBook Pro 18,1 / Apple M1 Pro / 16 GB
GPU: Apple M1 Pro 16-core / Metal 4
Display: 3456x2234 Retina
OS: macOS 26.5.2 (25F84)
Xcode: 26.6 (17F113), macOS SDK 26.5
Metal Toolchain: 17F109, installed
Rust: rustc 1.97.1, aarch64-apple-darwin
GPUI: crates.io 0.2.2, Apache-2.0
```

## 命令与结果

```text
cargo fmt -- --check
cargo clippy --locked --all-targets -- --deny warnings
cargo test --locked
cargo check --locked
cargo build --release --locked
./scripts/package-macos.sh
codesign --verify --deep --strict --verbose=4 "target/package/BongoCat GPUI Spike.app"
open -W "target/package/BongoCat GPUI Spike.app" --args --auto-quit-ms 1500
```

结果：格式化、Clippy、10 项 contract test 和 debug/release 编译通过，`.app` 的 ad-hoc 签名通过 strict bundle integrity 校验；LaunchServices smoke 以 0 退出。直接运行 binary 时输出 `accessibility tree root_role=AXGroup nodes=9 controls=7`、`window opened`、`runtime snapshot revision=1`，随后通过 GPUI `quit()` 输出 `runtime stopped` 和 `stopped` 并以 0 退出。

2026-08-29 将同一个 settings executable 接入 `windows-latest` 真实生命周期 smoke。runner 启动窗口、等待最多 30 秒并要求进程以 0 退出，同时检查 `window opened`、首帧 elapsed/scale factor、runtime revision 1，以及 `runtime stopped` 先于 `stopped`。spike 自身也改为在初始或 reopen 窗口创建失败时触发有序 quit，并在 event loop 返回后以非零退出，避免“打印 failed 但 CI 仍成功”。push run `33250457705`、job `99095132076` 已通过全部 build/test/release 和真实窗口 smoke；这不代表字体、IME、DPI 切换或 UI Automation 已验证。

同日将文本、选择和 marked range 收敛为不依赖 GPUI context 的 `TextBuffer` 并增加 4 项 IME contract 回归。测试发现并修复了 `replace_and_mark_text_in_range` 将 IME 的相对 UTF-16 selection 错按完整输入内容换算的问题；已有中文前缀时，旧逻辑可能产生越界 selection。当前测试覆盖连续 `ni -> 你`、`hao -> 好` 组合更新、surrogate pair、反向选择提交清理和异常 range 归一化，并同时进入 macOS/Windows GPUI test job。该证据验证文本协议和纯状态实现，不替代系统输入法端到端 smoke。

## Runtime Bridge 结果

spike 使用容量为 8 的 `async-channel 2.5.0` 传递强类型 `ReadSnapshot` 和 `Shutdown` command。每个 command 携带容量为 1 的 reply channel；UI 通过 GPUI `Context::spawn` 等待结果，再使用 weak entity 更新视图状态。snapshot 带单调递增 revision，UI 忽略不比当前 revision 更新的结果，runtime worker 在 GPUI background executor 上运行。

退出时，runtime 先关闭 command receiver，再发送 shutdown acknowledgement，避免 acknowledgement 返回后仍有请求成功入队并永远等待 reply。contract test 覆盖两次 snapshot 的 revision、health、shutdown acknowledgement 和停止后请求失败。macOS release `.app` smoke 同时证明 GPUI executor 能完成首次 snapshot 请求，并在 auto-quit 的 100 ms GPUI shutdown 窗口内收到 acknowledgement；该结果不等于产品 runtime、持久化或高负载 channel 已完成。

`--runtime-error-probe` 使用同一 typed command/reply 边界，并通过 GPUI background executor 的非阻塞 timer 将每次 read 延迟 3000 ms；第二次 read 返回稳定 `BridgeError::ProbeFailure`，第三次重试恢复且 revision 从 1 递增到 2。该 probe 不在 UI executor sleep，也不让 UI 直接控制 runtime 内部状态。macOS 打包 `.app` 的外部 AX smoke 已读取 `Runtime Ready · revision 1 -> runtime probe failed -> Runtime Ready · revision 2`，随后 Cmd+Q 仍先收到 runtime shutdown acknowledgement。AccessKit contract 另验证 loading status 的 `busy=true` 与 error status 的 `Invalid::True`。commit `21ee8aa` 的 push run `33291750411`、job `99204478369` 与 pull request run `33291751558`、job `99204481348` 已从 Windows UI Automation 依次读到 `Refreshing...`、`runtime probe failed` 和重试后的 `Runtime Ready · revision 2`，三者保持硬门槛。runner 托管 UIA client 的 `AutomationElement.AriaPropertiesProperty` 标识为 `null`，因此该 client 不能判定 AccessKit Windows 0.35.0 源码声明的 `busy=true` 投影；workflow 只在标识可用时探测该属性，这一证据缺口仍保留。

760x520 Retina 人工 smoke 确认 `Runtime Ready · revision 1` 和 Refresh 控件完整显示，无文字裁剪或卡片溢出。此次检查没有提交全屏截图，避免把用户桌面内容纳入仓库证据；可重复证据以 contract test、release binary 日志、bundle 校验和本文环境记录为准。

## 性能基线

运行 `spikes/gpui-settings/scripts/benchmark-macos.sh` 取得 commit `248a770375291f2467f7ffae7d5cc1172da601b3` 的 macOS 基线。脚本在同一设备上执行 10 次 warm-start，记录 Rust `main` 到首个 GPUI frame callback 的时间；另一次运行预热 5 秒后每秒采集 10 个 `%CPU`/RSS 样本。二进制增量是 release 可执行文件与同 toolchain、`opt-level=2`、thin LTO 的空 Rust 可执行文件之差。

| 指标                       |                                     结果 |
| -------------------------- | ---------------------------------------: |
| 首帧 min / p50 / p95 / max | 202.136 / 230.745 / 308.403 / 308.403 ms |
| idle CPU mean / max        |                           3.160% / 10.0% |
| idle RSS mean / max        |                      95,614 / 95,920 KiB |
| release executable         |                          6,095,008 bytes |
| empty Rust executable      |                            439,608 bytes |
| executable increment       |                          5,655,400 bytes |

环境为 MacBook Pro 18,1、Apple M1 Pro、16 GB、macOS 26.5.2 (25F84)、Rust 1.97.1 `aarch64-apple-darwin`、GPUI 0.2.2、760x520 Retina 设置窗口。`ps %CPU` 是进程生命周期平均值，可能受窗口系统或桌面活动影响；数据没有模型、输入采集或 Live2D renderer，因此不能用来判断最终应用预算。原始数据保存在 `docs/benchmark/data/gpui-settings-macos-248a770-startup.csv` 和 `docs/benchmark/data/gpui-settings-macos-248a770-idle.csv`。

### 交互和视觉验证（macOS 人工 smoke）

已验证：

- 主题选项可切换，浅色/深色表面、文本和输入框样式同步更新；System 模式能跟随系统 appearance；
- 文本框可输入普通 Unicode 和中文粘贴内容；字符计数与内容更新；
- 鼠标选择、Shift-方向键扩展选择、Cmd/Ctrl-A、剪切、复制和粘贴可用；换行粘贴会被归一化为空格；
- Tab 和 Shift-Tab 在主题选项与文本框之间移动焦点；焦点控件显示 accent border；
- 关闭设置窗口不会退出进程，重新激活应用可重建窗口；
- 截图证据：`docs/phase-0/evidence/gpui-settings-light-760x520.jpg`（SHA-256 `e9343ae1cfeed487dbe368121c35a4ec4146eab2fd72da38d3f7eb9755fa2401`）和 `docs/phase-0/evidence/gpui-settings-dark-760x520.jpg`（SHA-256 `6c84b9e2473586e556a647e44e3584c2c6b5ec334564b7b423c1428cb5d3d158`）。

这些结果是 macOS 当前环境下的视觉和交互 smoke，不等价于跨平台验收。GPUI 输入实现已接入 UTF-16 selection、grapheme 边界和 marked-text 公共协议，纯状态 contract 已覆盖已有多字节内容上的连续中文组合更新，但尚未用系统中文输入法完成真实组合态验证；Windows 字体、IME、DPI 和辅助技术也尚未验证。

2026-08-28 按 ADR-0008 将 Bundle ID 更新为 `com.ayangweb.bongo-cat` 后重新执行打包、strict codesign 和 LaunchServices auto-quit，三项均通过。打包脚本会在签名前读取 Info.plist 并拒绝任何非预期 Bundle ID。

上游 `block 0.1.6` 和 `proc-macro-error2 2.0.1` 被 Cargo 标记为 future-incompatible。`docs/phase-0/future-incompatibility.md` 已形成结论：当前图只允许用于 Phase 0 spike，进入产品 workspace 前必须通过 GPUI 上游升级或单独审计的可复现 patch 消除两条 warning。

## Metal Toolchain 结果

初次探针发现可选 Metal Toolchain 未安装，GPUI 默认预编译 shader 路径提示：

```text
xcodebuild -downloadComponent MetalToolchain
```

安装后 `xcodebuild -showComponent MetalToolchain -json` 报告 build `17F109`、status `installed`。探针已移除 `runtime_shaders` feature，并在 GPUI 默认 feature 下通过 debug/release 构建和运行。

该结果只固定当前 spike 的可用组合，不是 macOS 最低 Xcode/Rust 版本结论。干净构建机仍需通过显式 bootstrap 安装同一组件。

## Application Bundle and Lifecycle

`scripts/package-macos.sh` 生成 `target/package/BongoCat GPUI Spike.app`，复制固定 `Info.plist` 与 release executable，并执行 ad-hoc signing。系统检查得到：

- bundle id：`com.ayangweb.bongo-cat`；
- executable：arm64 Mach-O；
- 最低系统声明：macOS 12.0；
- `codesign --verify --deep --strict`：通过；
- 窗口标题：`BongoCat Settings`，760x520 Retina 内容可见且无明显裁剪；
- 应用菜单：原生 `NSMenu` 可识别 Application、Edit 和 Window 结构及 Services、Hide、Quit、Cut/Copy/Paste/Select All、Minimize 和 Zoom；一次性 AppKit run-loop probe 通过真实菜单项依次触发 Select All、Cut、Paste，并验证 focused GPUI text input 和剪贴板结果；
- 关闭设置窗口后进程保持运行，再次激活应用可重建窗口；
- `Cmd+Q` 触发 GPUI action，进程正常退出。

ad-hoc signing 只验证本地 bundle 完整性，不代表 Developer ID、Hardened Runtime、notarization 或发布 Gatekeeper 已通过。

## Accessibility Result

GPUI 0.2.2 没有普通 element 的公共语义 API，但 `Window` 实现标准
`raw-window-handle 0.6.2`。spike 使用 `accesskit 0.25.0`、macOS adapter `0.27.0` 与
Windows adapter `0.35.0`，在窗口首次显示/聚焦前分别动态 subclass GPUI `NSView` 和
subclass `HWND`。方案不依赖 Zed 私有 crate、GPUI renderer、隐藏控件或 fork。

项目自有 tree 当前包含顶层 window、Appearance group、System/Light/Dark radio、模型名称
text input、live runtime status、Reset 和 Refresh button。节点公开 role、title、description、value、
focus、selected、busy、invalid 与 click/focus/set-value action。AccessKit 回调只向容量 32 的强类型 channel
发送 `AccessibilityAction`；GPUI application thread 再更新可见控件和语义 snapshot，平台
callback 不直接访问 Entity。队列拒绝会输出诊断，不会静默丢 action。

macOS 本机通过真实 `accessibilityChildren`、`accessibilityRole` 和 `accessibilityTitle`
读取 `AXGroup` root 与 7 个控件，并对 Dark `AXRadioButton` 调用
`accessibilityPerformPress`；日志确认 action 在 GPUI 线程应用。AccessKit 按 VoiceOver 约定
隐藏顶层 `Role::Window` 的重复标题，因此测试验证 native window title 和内部 role/title，
不错误要求 semantic root 再暴露同名 label。commit `fd9ad85` 的 push run `33255204781`、
job `99107586036` 已由外部 .NET UIA client 读取 Appearance group、三个 radio、text input、
status bar 和 Refresh button，再调用 Dark radio 的 `SelectionItem.Select` 并验证
`SelectionItem.IsSelected` 与 GPUI typed action marker。该证据覆盖 Windows 原生 UIA
role/name/action/selected，不替代 Narrator、错误/loading 宣读、IME、DPI 或窗口重建实测。

2026-08-30 增加只通过 GPUI 公共 `tooltip` API 构建的 Reset 说明，以及项目自有的 modal
确认框。Reset command 在 760x520 下使用固定 command group 宽度，状态文本占用剩余空间；可见
smoke 曾发现未固定 flex shrink 时按钮语义存在但视觉宽度为 0，当前布局与外部 AX tree 都能看到
`Reset settings...`。确认框打开后 AccessKit 只把 `AlertDialog`、说明、Cancel 和默认 Reset
暴露为 Appearance 的子树，背景表单不再进入辅助技术遍历；初始焦点落在 Cancel。Tab 与
Shift-Tab 在两个 dialog command 之间循环，Enter/Space 只在 `SettingsButton` key context
激活，不会截获文本框空格，Escape 关闭后焦点返回 Reset。macOS `.app` 的真实窗口 smoke 已
执行打开、Shift-Tab/Tab、Escape 和 Cmd+Q，并确认 dialog 子树撤销及 runtime-first shutdown。
commit `45b8dba` 的 push job `99156013603` 已通过进程外 UI Automation 的
open -> role/focus -> cancel -> subtree removed 门禁；tooltip 的真实 hover 延迟和
VoiceOver/Narrator 宣读仍待完成。

同日将最小菜单扩展为 Application、Edit 和 Window 三组。自绘 `TextInput` 的编辑命令必须使用
GPUI action；若错误映射为 Cocoa `cut:`/`paste:` responder selector，菜单会因输入框不是
`NSTextField` 而禁用。`--menu-probe` 先等待 AX radio action 完成并把焦点恢复到文本框，再通过
一次性 Core Foundation main-run-loop timer 调用 `NSMenu.performActionForItemAtIndex`，避免在
`AsyncApp::update` 内同步回调造成可重入借用。Select All -> Cut -> Paste 的文本/剪贴板结果和
runtime-first shutdown 已在本机通过；该证据不包含后续 `NSStatusItem` 常驻菜单。

`accesskit_macos 0.27.0` 的公开 adapter 类型基于 `objc2 0.5.x`，因此仅用于 AX 诊断消息的
直接 `objc2` 精确固定为 `0.5.2`，避免通过 `objc2 0.6` Rust 类型访问另一 generation 的
Objective-C 对象。该版本例外的解除条件是 AccessKit macOS adapter 升级到 0.6 generation；
业务和语义类型不依赖 `objc2`。

## 本地化资源边界

当前五种 locale（`en-US`、`pt-BR`、`vi-VN`、`zh-CN`、`zh-TW`）仍位于历史前端的 `src/locales/`，仅作为 Native Rewrite 的迁移输入。`tools/validate-locales.py` 在迁移前检查每个语言包的递归 key、叶子类型、非空文本和占位符集合，并由 Phase 0 CI 执行。迁移到 `shared/resources/localization/` 时必须保留相同 key 契约，再由 GPUI UI crate 显式加载；该检查不代表 Native 本地化迁移或 GPUI 辅助功能已经完成。

## 未完成

- marked-text 纯状态 contract 已通过；真实中文输入法组合态尚未在 macOS 上完成端到端验证，Windows IME、字体、DPI 和辅助技术尚未验证。
- tooltip builder、modal dialog、焦点陷阱、Escape 和 macOS 原生 Application/Edit/Window 菜单交互已通过；真实 tooltip hover 延迟与双平台辅助技术朗读尚未验证。
- macOS 内容 AX tree/action、错误/retry value 与 Windows UI Automation dialog/loading/error/retry/revision 2 runner 已通过；Windows `busy=true` AriaProperties 投影、真实 VoiceOver/Narrator 操作和宣读顺序仍待完成。
- 菜单栏常驻策略、隐藏行为和 native overlay 共存尚未验证。
- Windows 已通过编译和真实窗口/首帧/退出 runner smoke；字体、IME、DPI 切换、辅助技术和系统集成仍未验证。

因此默认 shader 工具链、`.app` bundle/lifecycle、主题、基础编辑交互与双平台最小 AX/UIA tree/action 子项可以单独记录为通过；真实辅助技术/IME、overlay 共存和完整 GPUI spike 仍保持未完成。GPUI go/no-go 决策必须等 ADR-0009 的全部 gate 有证据后再做。
