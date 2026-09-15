# ADR-0032: Startup Permission Prompt Boundary

状态：已接受（2026-09-14）；同日实机验收发现 macOS 侧必须使用不触碰 AppKit 的 `rfd` 路径，见「macOS 弹框实现修正」；2026-09-15 修正检查执行方式为专用 worker 线程上的非阻塞检查，见「非阻塞执行修正」

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
  启用 `common-controls-v6` feature **并**在应用 manifest 中声明 ComCtl32 v6 依赖。本产品两者
  都不具备（`crates/bongocat-app/windows/bongocat-app.rc` 不含该 dependency，依赖声明也没有该
  feature），所以 Windows 只显示系统标准 OK/Cancel 两个按钮。
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
  | macOS | `CGPreflightListenEventAccess` | `rfd::AsyncMessageDialog`（无父窗口，仅 `CFUserNotification`），调用线程用 `async_io::block_on` 等待 | `CGRequestListenEventAccess` + `NSWorkspace` 打开 `x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent` |
  | Windows | `TokenElevation` | `rfd::MessageDialog`（`MessageBoxW`，调用线程） | `opener::reveal` 定位当前可执行文件，正文给出「属性 → 兼容性 → 勾选以管理员身份运行」路径 |

- macOS 必须使用上表中的异步实现，不能用同一 crate 的同步实现：原因与证据见「macOS 弹框实现修正」。
  这是启动阶段的硬约束，不是风格选择；`bongocat-platform` 用一条 contract 测试固定它。

- macOS 先调用 TCC 请求 API 再打开面板：请求是产品出现在「输入监控」列表里的唯一途径，否则用户
  还要手动添加应用。该调用仍严格位于用户点击之后，满足 ADR-0024 「启动、轮询和服务恢复不得弹出
  请求」的约束；启动路径只调用只读 preflight。
- Windows 不自动提权：不调用 `ShellExecuteEx` + `runas`，不写入 HKCU/HKLM，不注册 service 或
  scheduled task。产品继续按 ADR-0023 以 per-user、无提权方式安装和运行，提示只解释兼容性开关
  这一系统标准路径。
- 提示不做任何持久化：不新增配置字段、不写入 `state.json`、不新增应用日志事件、不缓存「用户选过
  稍后」。每次启动都重新读取平台当前状态；已授权则完全不提示，未授权则本次启动继续提示。判定依据
  只有平台状态。
- 检查点（2026-09-15 修正）：检查不再阻塞启动流程。产品在 GPUI run loop 内完成 overlay、设置
  服务、系统菜单和 update worker 的启动之后，由主线程 spawn 一个专用 worker 线程执行检查与提示；
  提示未应答、被关闭或检查失败都不影响任何产品窗口的显示与使用。`--configuration-recovery-mode`
  恢复路径仍不提示，自动化 harness 运行仍不提示。原始决策「检查点在 GPUI run loop 之前、提示
  先于猫窗口出现」已被本修正取代，见「非阻塞执行修正」。
- `--startup-permission-smoke` 是只读诊断开关：只输出当前能力名与
  `available`/`missing`，用于双平台可重复验收，不弹框、不写状态。
- 自动化 harness 运行不提示：除 `--run-seconds` 与帮助外的任何参数都表示这是一次 smoke/诊断运行
  （脚本与 CI 上无人应答原生弹框）。产品路径（`--run-seconds 0`，含登录启动项）与 Development
  `cargo run` 始终检查，与打包后的 Production 行为一致，不因构建环境而不同。
- 与旧 Tauri 行为的已知差异：旧版在 Windows 非管理员时提示后不继续运行，本决策按产品要求改为
  「继续运行」仍可进入应用，只有用户选择设置路径时才离开启动流程；macOS 的「请求 + 再次引导」意图
  保留，但请求时机收紧到用户点击之后（ADR-0024）。旧版行为对照仍见
  `docs/phase-0/behavior-inventory.md`，不在本 ADR 重复维护。

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
- Windows 保持同步 `MessageDialog`：`MessageBoxW` 没有该约束，且在调用线程上语义最直观。
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
  语言与文案字符串，不与渲染或输入生命周期共享任何锁。macOS 的对话框本身运行在 `rfd` 的工作
  线程上，worker 用 `async_io::block_on` 等待其结果；`rfd` 的同步 macOS 路径在该线程上被禁止
  （见上方修正与 contract 测试）。
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
  - Windows 10 1903+：普通权限启动出现原生提示且按钮为系统标准 OK/Cancel；「打开所在文件夹」
    能定位可执行文件；勾选兼容性开关并重启后不再提示；提权启动时完全不提示。
- Windows 实机矩阵与 macOS TCC 矩阵仍是发布门禁，cross-check 只能证明编译与边界。

## 替换边界

替换点只有 `bongocat-platform` 的私有 `startup_permission` adapter 和 `bongocat-app` 的文案
构造。升级 `rfd` 时必须复验：无父窗口消息框在两平台仍为原生实现、macOS 自定义按钮标题仍保留、
macOS **异步**路径仍不创建共享 `NSApplication`（本约束的唯一已知可复现崩溃来源）、Windows 无
`common-controls-v6` 时仍回退到 `MB_OKCANCEL`、以及无父窗口路径仍不需要应用已运行。
若未来选择启用 `common-controls-v6`，必须同时提供声明 ComCtl32 v6 的应用 manifest，并同步修改
Windows 提示正文（现在正文按标准 OK/Cancel 描述），不得只改 feature 让对话框静默失败。
升级 `opener` 时必须复验 `reveal` feature 与失败语义。adapter 之外不得依赖 `rfd` 类型。
若升级 `gpui-kit`/`gpui-pre-macos`，必须复验 `GPUIApplication` 子类与 `platform` ivar 的约束是否
仍然成立：一旦 GPUI 不再依赖该 ivar，本 ADR 的 macOS 同步/异步选择可以重新评估。
