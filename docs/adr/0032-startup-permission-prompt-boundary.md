# ADR-0032: Startup Permission Prompt Boundary

状态：已接受（2026-09-14）；同日实机验收发现 macOS 侧必须使用不触碰 AppKit 的 `rfd` 路径，见「macOS 弹框实现修正」；2026-09-15 修正检查执行方式为专用 worker 线程上的非阻塞检查，见「非阻塞执行修正」；同日启用 Windows 自定义按钮文案，见「Windows 按钮文案修正」；2026-09-18 为修复主题跟随把 macOS 提示改为主线程 `NSAlert`，见「主题外观修正」；2026-09-24 收口 Windows 按钮文案，使其准确描述“退出并打开程序文件夹”和“继续运行”

## 背景

全局输入需要一项平台能力，而缺省状态下两平台都不会主动告知用户：macOS 需要 Input Monitoring
TCC 授权，Windows 需要提权令牌才能在完整性级别更高的前台窗口存在时继续收到 Raw Input。旧版
Tauri 实现把这两件事做成产品自有弹窗，并记录用户是否已经处理过；Native Rewrite 此前只在
Settings 和 Diagnostics 里投影状态，用户必须自己发现问题。

项目需要一套启动时的检查与引导：使用平台原生弹框，复用既有平台权限检测，不给用户新增需要
维护的提示状态。同时 ADR-0024 已经限定 macOS 的 TCC 请求 API 只能在用户发起的设置操作中调用，
本决策必须与之相容。

## 调研结论

已核实（2026-09-14，源码阅读 `rfd 0.17.2` 与 `windows 0.62.2`）：

- `rfd 0.17.2`（MIT，双平台 target-only，已按 ADR-0030 精确 pin 且 `default-features = false`）
  的消息对话框在没有父窗口时的实现是平台原生的，并与有父窗口时不同：macOS 的**同步**路径走
  `sync_pop_dialog` → `CFUserNotificationDisplayAlert`
  （`src/backend/macos/utils/user_alert.rs`），Windows 走 `MessageBoxW`
  （`src/backend/win_cid/message_dialog.rs`，`not(feature = "common-controls-v6")` 分支）。
  两条路径都不需要产品自建窗口，也不进入 GPUI renderer。
- macOS **同步**路径虽然避开 `NSAlert::runModal`（因此不重入 GPUI 事件循环），但它为了设置激活策略
  与焦点会先构造 `PolicyManager`/`FocusManager`，两者都调用 `NSApplication::sharedApplication`
  （`src/backend/macos/utils/{policy_manager,focus_manager}.rs`）。这会创建共享应用实例，而该实例
  在启动阶段属于谁是有约束的，见下方修正章节；**无父窗口的异步路径**（`async_pop_dialog` →
  `UserAlert::new(opt, None)`）不创建这两个 manager，因此完全不触碰 AppKit，只构建
  `CFUserNotification`。
- macOS 自定义按钮文案会被保留（`OkCancelCustom` 映射为前两个按钮标题）；Windows **不会**保留，
  因为自定义标题来自 `TaskDialogIndirect`，而它只由 ComCtl32 v6 导出。`rfd` 自身文档要求同时
  启用 `common-controls-v6` feature **并**在应用 manifest 中声明 ComCtl32 v6 依赖。原始调研
  认为本产品两者都不具备（`crates/bongocat-app/windows/bongocat-app.rc` 不含该 dependency，
  依赖声明也没有该 feature），所以当时 Windows 只显示系统标准 OK/Cancel 两个按钮；2026-09-15
  复查发现 manifest 前提实际由 `gpui-pre` 静态库满足，只补了 feature，见「Windows 按钮文案
  修正」。
- Windows 提权状态以 `TokenElevation` 为准：`OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY)`
  + `GetTokenInformation(_, TokenElevation, ..)`。`windows 0.62.2` 的 `HANDLE` 未实现 `Drop`
  （只实现 `windows_core::Free`），必须显式 `CloseHandle`。用户按引导勾选「以管理员身份运行此
  程序」后新进程的令牌就是 elevated，与提示承诺的状态一致，同时也能正确识别在提权 shell 中启动
  的情况。
- 现有可复用能力：`bongocat-platform::input_monitoring_permission`
  （`CGPreflightListenEventAccess`，只读）、`request_input_monitoring_permission`
  （`CGRequestListenEventAccess`）、`rfd`（目录选择已使用的既有依赖）、
  `opener 0.8.5` 的 `reveal`（既有依赖且已启用 feature，用于在资源管理器中定位可执行文件）。
  不需要任何新依赖。

## 决策

- 检查与提示全部放在 `bongocat-platform` 的私有 `startup_permission` adapter：
  只暴露项目自有的 `StartupPermissionPrompt`、`StartupPermissionStatus`、
  `STARTUP_PERMISSION_CAPABILITY`、`startup_permission_available()` 和
  `check_startup_permission()`。`rfd`、`windows` token API 和 AppKit 类型都不离开该 adapter。
- 文案由 `bongocat-app` 从翻译目录构造后传入 adapter，沿用 `SystemMenuPresentation` 的既有模式；
  adapter 不读取配置、不持有产品文案。
- 两平台都先做只读查询，再决定是否提示：

  | 平台 | 只读查询 | 提示实现 | 「授权/去设置」动作 |
  | --- | --- | --- | --- |
  | macOS | `CGPreflightListenEventAccess` | 主线程 `NSAlert`（经 `dispatch2::run_on_main` 投递，2026-09-18 起，见「主题外观修正」） | `CGRequestListenEventAccess` + `NSWorkspace` 打开 `x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent` |
  | Windows | `TokenElevation` | `rfd::MessageDialog`（`TaskDialogIndirect`，调用线程，见「Windows 按钮文案修正」） | `opener::reveal` 定位当前可执行文件，正文给出「属性 → 兼容性 → 勾选以管理员身份运行」路径 |

- macOS 曾要求使用 `rfd` 的无父窗口异步实现（见「macOS 弹框实现修正」），2026-09-18 起改为
  主线程 `NSAlert`；同 crate 的同步实现仍被 contract 测试禁止，原因不变（见「主题外观修正」）。

- macOS 先调用 TCC 请求 API 再打开面板：请求是产品出现在「输入监控」列表里的唯一途径，否则用户
  还要手动添加应用。该调用仍严格位于用户点击之后，满足 ADR-0024 「启动、轮询和服务恢复不得弹出
  请求」的约束；启动路径只调用只读 preflight。
- Windows 不自动提权：不调用 `ShellExecuteEx` + `runas`，不写入 HKCU/HKLM，不注册 service 或
  scheduled task。产品继续按 ADR-0023 以 per-user、无提权方式安装和运行，提示只解释兼容性开关
  这一系统标准路径。
- 提示不做任何持久化：不新增配置字段、不写入 `window-state.json`、不新增应用日志事件、不缓存「用户选过
  稍后」。每次启动都重新读取平台当前状态；已授权则完全不提示，未授权则本次启动继续提示。判定依据
  只有平台状态。
- 检查点（2026-09-15 修正）：检查不再阻塞启动流程。产品在 GPUI run loop 内完成 overlay、设置
  服务、系统菜单和 update worker 的启动之后，由主线程 spawn 一个专用 worker 线程执行检查与提示；
  提示未应答、被关闭或检查失败都不影响任何产品窗口的显示与使用。ADR-0054 前由 `--configuration-recovery-mode` 进入的恢复-only 路径不提示；当前配置 fallback 直接走普通设置窗口，自动化 harness 运行仍不提示。原始决策「检查点在 GPUI run loop 之前、提示
  先于猫窗口出现」已被本修正取代，见「非阻塞执行修正」。
- `--startup-permission-smoke` 是只读诊断开关：只输出当前能力名与
  `available`/`missing`，用于双平台可重复验收，不弹框、不写状态。
- 自动化 harness 运行不提示：除 `--run-seconds` 与帮助外的任何参数都表示这是一次 smoke/诊断运行
  （脚本与 CI 上无人应答原生弹框）。产品路径（`--run-seconds 0`，含登录启动项）与 Development
  `cargo run` 始终检查，与打包后的 Production 行为一致，不因构建环境而不同。
- 与旧 Tauri 行为的已知差异：旧版在 Windows 非管理员时提示后不继续运行，本决策改为「继续运行」
  仍可进入应用；2026-09-15 起收窄为「用户选择设置路径且流程成功时退出」，见「Windows 按钮文案
  修正」。macOS 的「请求 + 再次引导」意图保留，但请求时机收紧到用户点击之后（ADR-0024）。旧版
  行为对照仍见 `docs/phase-0/behavior-inventory.md`，不在本 ADR 重复维护。

## macOS 弹框实现修正（2026-09-14 实机验收）

首次实机验收在真实打包产物上复现了「启动即 abort」的崩溃，根因是下面这条 GPUI 与 AppKit 的
交互约束。原先「无父窗口同步弹框在 GPUI run loop 之前弹出是安全的」这一条结论是错的：它只检查了
`NSAlert::runModal`，漏看了 `PolicyManager`/`FocusManager` 对共享 `NSApplication` 的创建。

### 现象与证据

- 真实场景：双击 `/Applications/BongoCat.app`（Production 打包产物）→ 权限提示出现 → 用户应答后
  进程 `EXC_CRASH (SIGABRT)`、`abort() called`；应用日志只有 `started` 紧跟 `panicked`
  （release profile 的 `panic = "abort"` 把 panic 变成 abort）。
- 崩溃报告的帧偏移以「同源码重新链接的二进制」反查：本地用同一源码做
  `cargo rustc --release -p bongocat-app --bin bongocat-app -- -C strip=none`（`__text` 大小
  `0xd901e4` 与已安装产物逐字节一致）后用 `atos` 得到：
  `bongocat_app::main` → `gpui::Application::run` → `gpui_macos::platform::MacPlatform::run` →
  `objc::runtime::Object::get_mut_ivar` → panic → abort。
- 用同一份代码重建并触发同一条件，panic 消息为：
  `panicked at objc-0.2.7/src/runtime.rs:503:25: Ivar platform not found on class NSApplication`。

### 约束

- `gpui_macos` 在 `#[ctor]` 中注册 `GPUIApplication`（`NSApplication` 子类）并在其上声明 `platform`
  ivar；`MacPlatform::run` 先 `[GPUIApplication sharedApplication]`，再对该实例
  `set_ivar("platform", ..)`，而 `objc 0.2` 在 ivar 不存在时 panic。
- 进程共享的 `NSApplication` 实例由**第一个** `sharedApplication` 调用决定。若它先在基类
  `NSApplication` 上被调用（`rfd` 的同步 macOS 消息框正是如此），之后
  `[GPUIApplication sharedApplication]` 返回的就是基类实例，GPUI 的 `set_ivar` 随即 panic。
- 因此**在产品建立 GPUI 平台之前运行的任何代码都不得创建共享 `NSApplication`**。
  `NSWorkspace`、`CGPreflightListenEventAccess` 和 `CGRequestListenEventAccess` 已实机验证不创建它。

### 决策修正

- macOS 启动提示改用 `rfd::AsyncMessageDialog`（无父窗口 → `async_pop_dialog` →
  `UserAlert::new(opt, None)` → 不构造 `PolicyManager`/`FocusManager` → 不触碰 AppKit），主线程用
  `async_io::block_on` 等待结果。这与 `directory_picker` 已在用的 `rfd` + `async_io` 用法一致，
  不引入新依赖、不改变提示出现时机（仍在任何产品 UI 之前）。
- Windows 保持同步 `MessageDialog`：`MessageBoxW` 没有该约束，且在调用线程上语义最直观；改用
  `TaskDialogIndirect` 后该路径也保持同步并在 worker 线程上运行，见「Windows 按钮文案修正」。
- 该路径用无点击方式验证过：请求弹框但不等待答复时，产品正常启动并干净退出（exit 0），同时采样确认
  对话框线程真实阻塞在 `CFUserNotificationReceiveResponse`（即弹框确实已提交显示）；同一位置若换回
  同步实现，则必然复现上述 panic。

## 非阻塞执行修正（2026-09-15）

启动实机使用中发现原始检查点（GPUI run loop 之前同步执行）让权限提示成为启动的第一个交互：
用户不应答，模型窗口、菜单栏/托盘和设置 UI 都不会创建。产品要求权限提示只作为辅助提示存在，
于是把「检查的执行方式」从启动路径中拆出来，检查与提示逻辑本身不变。

### 变更

- 移除 `main` 在 `gpui_application.run` 之前的同步调用；语言在主线程解析一次，检查改由 GPUI
  run loop 内（overlay、设置服务、系统菜单、update worker 均已启动、`ProductCoordinator`
  已注册之后）spawn 的专用 worker 线程执行，线程名 `bongocat-startup-permission`。
- 只读平台查询（`CGPreflightListenEventAccess` / `TokenElevation`）与提示文案构造随 worker
  一起移入后台；`CGRequestListenEventAccess` 仍严格位于用户点击引导按钮之后，满足 ADR-0024。
- Worker 线程刻意 detached：原生对话框由 OS 持有、没有可用的远程取消通道，若在退出时 join，
  会把「未应答的弹框阻塞退出」这一原始问题原样搬到 shutdown 路径。该线程只拥有语言与文案
  字符串，不与产品共享任何锁或句柄，用户应答后自行终止；进程退出时 OS 回收线程并随之关闭
  对话框，不构成任务泄漏。
- spawn 失败仅记入启动失败收集，应用照常启动（与「提示失败不阻止启动」同语义）。

### 线程安全证据（2026-09-15 核实）

- macOS 的异步 `rfd` 路径只构建 `CFUserNotification`、不触碰 `NSApplication`，在 worker 线程
  上与 GPUI run loop 并行是安全的；同步路径会在调用线程上运行 `NSAlert` 模态机制，禁止使用，
  原 contract 测试继续固定这一约束。
- `objc2-app-kit 0.3.2` 绑定中 `NSWorkspace::sharedWorkspace()` 与 `openURL` 均未标记
  main-thread-only（无 `MainThreadMarker` 参数），可在 worker 线程执行。
- Windows 的 `MessageBoxW` 是调用线程模态，不阻塞其他线程的消息循环；`OpenProcessToken` 与
  `opener::reveal` 均无线程亲和性要求。
- worker 与产品无共享可变状态，不存在与窗口创建、输入服务或 shutdown 的锁竞争。

## 安全与生命周期不变量

- 提示运行在专用 worker 线程上（2026-09-15 修正，原始的「主线程阻塞等待用户应答」已被取代）：
  主线程继续 GPUI run loop，overlay、输入服务和 runtime 的启动不等待用户应答；worker 只拥有
  语言与文案字符串，不与渲染或输入生命周期共享任何锁。macOS 的对话框在 2026-09-18 起经
  `dispatch2::run_on_main` 在主线程上运行，worker 阻塞等待其结果（见「主题外观修正」）；
  `rfd` 的同步 macOS 路径在该线程上仍被禁止（见上方修正与 contract 测试）。
- 平台对话框的 owner 是 `rfd`，产品不持有任何 dialog handle；不注册回调，不在回调中做阻塞工作。
- macOS 侧只使用 `CGPreflightListenEventAccess`（只读）与用户点击后的
  `CGRequestListenEventAccess`；不调用任何 Accessibility trust/prompt API，不扩大 TCC 面
  （ADR-0024）。
- Windows 侧只读取自身进程令牌，不打开其他进程、不改写系统设置、不请求 UAC。令牌句柄在读取后
  立即关闭；失败路径返回「未提权」而不是 panic。
- 提示内容只包含产品名、权限用途和系统设置路径，不含用户路径、按键、剪贴板或任何诊断载荷。
- 提示失败（对话框后端不可用、`opener` 失败）不阻止应用启动，也不改变产品行为；此时仅相当于用户
  选择了「稍后」。

## 验证

- 单元测试覆盖 `rfd` 结果到选择的映射：自定义标签、标准 OK/Cancel 对以及关闭窗口/未知标签都不能
  被误判为「已授权」，并固定能力名的稳定取值。
- `bongocat-i18n` 目录测试保证中英两份目录键集与占位符一致，`bongocat-app` 侧测试保证本平台用到
  的四个键在两种语言下都有非空、非键名回退的文案。
- `RunOptions` 测试固定：`--startup-permission-smoke` 不打开设置窗口且标记为自动化运行，
  `--run-seconds 0` 保持交互式，其余被接受的参数都判定为 harness 运行。
- macOS contract 测试禁止 `startup_permission` 的 macOS 模块出现 `rfd::MessageDialog::new`；
  该测试做过反向自检（注入该字符串后确实失败），避免变成空断言。
- 崩溃回归的验证方式（自动测试无法覆盖「用户点击原生弹框」）：用同一源码重新链接出带符号的
  release 二进制并用 `atos` 反查崩溃报告帧偏移；以及「请求弹框但不等待答复」的运行确认产品可正常
  启动并干净退出。
- 可重复验收命令：`cargo run -p bongocat-app -- --startup-permission-smoke`，在授权前应输出
  `missing`，授权后应输出 `available`；该命令不弹框、不写入任何产品状态。
- 发布前必须在真实设备补足下列人工验收（当前环境无法代替）：
  - macOS 12+：未授权启动出现原生提示；「打开系统设置」落点为「隐私与安全性 → 输入监控」且产品
    已在列表中；「稍后再说」后猫窗口正常出现且输入服务进入匿名 `PermissionDenied`；授权并重启后
    不再提示。
  - Windows 10 1903+：普通权限启动出现原生提示且按钮为「退出并打开程序文件夹」/「继续运行」两个
    自定义文案（Task Dialog）；「退出并打开程序文件夹」能定位可执行文件且产品随后走常规 shutdown
    退出；勾选兼容性开关并重启后不再提示；提权启动时完全不提示。
- Windows 实机矩阵与 macOS TCC 矩阵仍是发布门禁，cross-check 只能证明编译与边界。

## Windows 按钮文案修正（2026-09-15）

> 本节保留 2026-09-15 的实现背景；按钮名称已按 2026-09-24 的 catalog 审计同步为当前文案。

产品要求 Windows 提示的两个按钮显示自有文案（「退出并打开程序文件夹」/「继续运行」），而原始决策下
Windows 走 `MessageBoxW`、只显示系统标准 OK/Cancel。「替换边界」曾推断 manifest 前提不满足，实查
后发现只差一半：

- **manifest 前提本已满足**：`gpui-pre` 的静态库（`gpui.lib`）内嵌了一份完整应用 manifest
  （`asInvoker`、Windows 10 `supportedOS`、PerMonitorV2 DPI、SegmentHeap、ComCtl32 v6 的
  Common-Controls 依赖），链接期已进入 executable。原始调研只检查了产品自有
  `bongocat-app.rc`，漏看了这条来源。
- **feature 是唯一缺口**：为 `rfd` 启用 `common-controls-v6`（仅为 `windows-sys` 增加
  `Win32_UI_Controls` gateway，不新增 crate）。

曾尝试在 `bongocat-app.rc` 追加自有 manifest（`1 24`），与 gpui 的 manifest 资源
（同 ID 1）冲突，链接报 `CVT1100 duplicate resource: MANIFEST, name 1`；已回退。不得再为
manifest 另加来源。

### 行为与证据

- 启用后 Windows 提示走 `TaskDialogIndirect`，`OkCancelCustom` 的两个自定义文案成为真实按钮，
  结果以 `MessageDialogResult::Custom(label)` 返回，既有映射逻辑无需修改。
- 官方文档的 Remarks 未要求调用线程初始化 COM；`rfd` 自身的异步 Windows 实现也是在裸
  `std::thread` 上调用 `TaskDialogIndirect`，与本项目专用 worker 线程同构，该路径经 Tauri
  生态长期使用。
- 失败语义保持保守：`TaskDialogIndirect` 绑定失败（激活上下文不可用）时 `rfd` 返回 `Cancel`，
  映射逻辑把它视为「继续运行」，绝不会被误判为同意；文案正文（已精简）不引用具体按钮名，
  两种按钮形态下都成立。
- 按钮顺序与默认焦点由 Task Dialog 决定：主按钮「退出并打开程序文件夹」在前且为默认按钮（与原
  `MessageBoxW` 默认「确定」一致），Esc/关闭窗口等价于「继续运行」。
- Windows 的主按钮现在名副其实：权限流程成功（`reveal` 已定位可执行文件）后，worker 通过托盘
  退出使用的同一个 `shutdown_requested` 标志请求退出，产品走常规 shutdown coordinator
  （flush 配置、停 runtime、join 服务）后进程退出，不绕过任何清理步骤；兼容性开关需要重启
  才生效，退出与按钮承诺一致。`reveal` 失败时保持运行（同「提示失败仅相当于选择稍后」的
  既有语义），用户可以再次尝试。macOS 主按钮只打开系统设置，没有退出承诺，行为不变。
- 验证：workspace 构建链接通过；链接产物 `bongocat_app.exe` 内含且仅含一份 RT_MANIFEST，
  内容即 gpui 的 ComCtl32 v6 manifest。

## 主题外观修正（2026-09-18）

ADR-0048 把用户选择的浅色/深色接到原生表面后，实机发现启动权限提示在应用深色下仍是浅色。根因：
macOS 的 `rfd` 无父窗口消息框（同步与异步都是）最终调用 `CFUserNotificationDisplayAlert`
（`rfd 0.17.2` `src/backend/macos/utils/user_alert.rs`），这条路径完全不经过 AppKit，不继承
`NSApplication.appearance`，只会跟随系统外观。ADR-0048 修复的正是 `NSApp.appearance` 继承链，
而本提示当时根本不在链上。

### 变更

- macOS 提示改用 `objc2-app-kit` 的 `NSAlert`（`NSAlertStyle::Warning`，两个自定义按钮文案不变，
  返回值经既有 `requested_permission_flow` 映射，Windows 侧零改动）。`NSAlert` 的窗口继承应用
  appearance，从第一帧起跟随启动时设置的进程级主题（ADR-0048 的 `apply_process_theme`）。
- 呈现通过 `dispatch2::run_on_main` 投递主线程并同步等待结果（`dispatch2 =0.3.1` 因此从
  dev-dependencies 转为正式依赖）：worker 仍负责只读查询与阻塞等待，AppKit 窗口只允许在主线程
  构建。worker 在 GPUI run loop 内 spawn（2026-09-15 修正），`GPUIApplication` 已存在，因此
  09-14「不得触碰共享 `NSApplication`」约束的场景（GPUI 平台建立之前）不适用于呈现阶段；
  `rfd` 同步消息框仍被 contract 测试禁止，原因不变（它会在调用线程上构造共享 `NSApplication`
  的 plumbing）。
- `runModal` 从主队列 block 执行，位于 GPUI 事件之间而非某个事件处理器栈内。模型选择器注释记录的
  `RefCell already borrowed` 重入崩溃发生在「GPUI 事件处理器内同步 `runModal`」，与此处不同；
  `rfd` 自身的文件对话框同样经 `run_on_main` 在主线程上运行模态，是该模式的既有先例。
- 提示出现时调用 `activateIgnoringOtherApps(true)` 把产品带到前台（`-activate` 为 macOS 14+，
  产品支持 12+，故用旧选择器并 `#[allow(deprecated)]`）。这是相对 `CFUserNotification` 的行为
  变化：提示现在是模态的，应答前主线程处于 modal panel run loop（GPUI 的渲染走 display link，
  不受影响），与文件对话框先例一致。
- 响应常量不用 `objc2-app-kit` 的 `NSModalResponseOK`/`NSModalResponseCancel`：该绑定把弃用的
  `NSOKButton`(1)/`NSCancelButton`(0) 值挂在了这两个名字上，而 `-runModal` 对自定义按钮返回
  `NSAlertFirstButtonReturn`(1000)/`NSAlertSecondButtonReturn`(1001)。已对照 macOS SDK 的
  `NSAlert.h` 核实（2026-09-18），crate 内以自有常量 `ALERT_FIRST_BUTTON_RESPONSE` 固定。

### 证据与验证

- 根因证据：`rfd 0.17.2` 源码调用链 + 实机截图（应用深色、提示浅色）与该路径「只跟随系统外观」
  的行为一致；SDK 头文件确认 `NSAlert` 返回值与绑定常量的出入。
- `cargo check` / `cargo clippy --all-targets -p bongocat-platform` 通过；platform 全部单测通过，
  含更新后的 contract 测试：仍禁止 `rfd::MessageDialog::new`、要求 macOS 模块经 `run_on_main`
  呈现、禁止回退 `AsyncMessageDialog`。
- 深浅两色下弹框外观的实机肉眼验收未完成，归入 ADR-0048 既有的「双平台实机主题验收」门禁。

## 替换边界

替换点只有 `bongocat-platform` 的私有 `startup_permission` adapter 和 `bongocat-app` 的文案
构造。升级 `rfd` 时只需复验 **Windows** 侧：无父窗口消息框仍为原生实现、在
`common-controls-v6` + ComCtl32 v6 manifest 齐备时仍通过 `TaskDialogIndirect` 显示自定义按钮、
manifest 缺失时仍回退到 `MB_OKCANCEL` 且结果被当作「继续运行」、以及无父窗口路径仍不需要应用
已运行（macOS 提示已不再使用 `rfd`，见「主题外观修正」）。`common-controls-v6` feature 与
executable 内嵌的 ComCtl32 v6 manifest 必须同时存在，缺一会让
Windows 对话框静默失败或按钮回退为系统标准文案。manifest 的唯一来源是 `gpui-pre` 静态库内嵌
的 `gpui.lib` manifest：升级/更换 `gpui-pre` 时必须重新核实该 manifest 仍声明 Common-Controls
v6（可用字节扫描 `Common-Controls` 复核），且不得在产品自有 `.rc` 里再嵌入 manifest
（RT_MANIFEST ID 1 冲突，`CVT1100`）。升级 `opener` 时必须复验 `reveal` feature 与失败语义。
adapter 之外不得依赖 `rfd` 类型。升级 `objc2-app-kit` 时必须复验两件事：`NSAlert` 绑定的方法面
（本提示依赖 `runModal`/`addButtonWithTitle`/`setMessageText`/`setInformativeText`/
`setAlertStyle`）与 `NSModalResponseOK`/`NSModalResponseCancel` 常量值是否仍与
`-runModal` 实际返回值不符（当前不符，crate 内用 `NSAlert.h` 的 1000/1001 自有常量）。
若升级 `gpui-kit`/`gpui-pre-macos`，必须复验 `GPUIApplication` 子类与 `platform` ivar 的约束是否
仍然成立：一旦 GPUI 不再依赖该 ivar，本 ADR 的 macOS 同步/异步选择可以重新评估。
