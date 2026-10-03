# ADR-0078: macOS 输入监控引导采用 permission-flow（第二个厂商 FFI 例外）

状态：已接受（2026-09-30）；上游 0.1.40 已接入产品，macOS 12 适配落在 fork 的 rev 上（上游 PR #2 未合并），locale 参数与实机验收未完成，见「未完成项」

补充：ADR-0005（Cubism Core FFI 边界）、ADR-0024（macOS TCC 能力边界）、ADR-0032（启动权限
提示边界）、ADR-0008（应用身份与存储环境）、ADR-0030（实现决策阶梯）

## 背景

ADR-0032 定下 macOS 的启动权限提示：主线程 `NSAlert` 承担提示壳，用户点主按钮后调用
`CGRequestListenEventAccess` 并打开「隐私与安全性 → 输入监控」面板。这条路径只把用户送到系统
设置，之后如何在列表里勾选、如何把应用拖进授权列表，全靠用户自己摸索。

[`veecore/permission-flow`](https://github.com/veecore/permission-flow) 提供带拖拽引导的浮动
面板：打开正确的设置面板、显示目标应用 bundle、给出「把此应用拖入授权列表」的引导文案。本
ADR 记录把它用于 macOS 输入监控引导的决策、实测得到的能力边界，以及已经落地的实现。

### 上游事实（crates.io `permission-flow` 0.1.40，2026-05-03 发布）

- MIT，edition 2024，`rust-version = "1.85"`；许可证在 `deny.toml` 的 allow 列表内。
- **没有任何 normal 依赖**。只有 build-dependency `swift-rs`（feature `build`）和一个
  dev-dependency `iced`（不参与下游构建）。Rust 侧 499 行，Swift 侧 2885 行 / 34 文件，
  其中 `PermissionFlow/` 是 `jaywcjlove/PermissionFlow` 的 vendored 副本。
- 非 macOS 上编译为 no-op 垫片，`build.rs` 在 `CARGO_CFG_TARGET_OS != "macos"` 或 `DOCS_RS`
  时直接 return，因此 Windows 构建不受影响。
- 公开 API：`PermissionFlowController::{new, start_flow, stop_current_flow}`、
  `StartFlowOptions::{new, use_click_source_frame, without_click_source_frame}`、
  `Permission::INPUT_MONITORING` 等 8 项、`Permission::authorization_state()`、
  `AppPath::suggested_host_app()`。
- Swift 侧另有 `PermissionFlowConfiguration.localeIdentifier`，但 shim 把它硬编码为
  `.init(promptForAccessibilityTrust: false)`，`authorize(pane:suggestedAppURLs:sourceFrameInScreen:)`
  也没有 locale 参数，Rust API 更没有暴露——**上游的 Rust 接口无法指定语言**。

### 实测结论（2026-09-30，macOS arm64，Swift 6.4 / Xcode 27，Rust 1.97.1）

1. **Swift 运行时 rpath 必须由下游 binary crate 自己加。** 库依赖的 `rustc-link-arg` 不传递到
   最终链接的二进制。不加时 `otool -l` 里没有 `LC_RPATH`，程序在 `main` 之前就 abort：

   ```
   dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
     Reason: no LC_RPATH's found
   ```

2. **SwiftPM 资源包不会被 swift-rs 发布。** `PermissionFlow_PermissionFlow.bundle` 只留在
   `target/<profile>/build/permission-flow-<hash>/out/swift-rs/…/Products/<Config>/`，可执行文件
   旁没有。SwiftPM 生成的访问器（`resource_bundle_accessor.swift`）依次查找
   `Bundle.main.resourceURL`、`Bundle(for: BundleFinder.self).resourceURL`、`Bundle.main.bundleURL`，
   都不含该 bundle 时执行 `fatalError("unable to find bundle named PermissionFlow_PermissionFlow")`。

3. **嵌套 bundle 的语言由主 bundle 决定。** 实测：系统语言为 `zh-Hans-CN`、应用未声明任何
   `CFBundleLocalizations` 时，嵌套 bundle 的 `preferredLocalizations` 仍是 `["en"]`（development
   region），面板显示英文；在主 bundle 的 `Info.plist` 声明 `CFBundleLocalizations` 之后，主
   bundle 与嵌套 bundle 同时变成 `["zh-Hans"]`；再把 `AppleLanguages` 写进**本应用自己的
   CFPreferences 域**，两者同时变成覆盖值（`zh-Hant` → 「輸入監控」、`ar` → 「مراقبة الإدخال」）。
   `vi` 没有对应 `.lproj`，回落到 `["en"]`。

4. **`AppPath::suggested_host_app()` 在开发二进制上指向错误的应用。** 实测返回启动它的应用
   （`cargo run` 产物不在 `.app` 内，该方法回退到父进程链）。

5. **`authorization_state()` 只回答宿主进程的状态**，不能用作「是否需要引导」的判据；产品现有的
   进程内 `CGPreflightListenEventAccess` 才是。

6. **controller 是 `!Send`/`!Sync`，且 `new()` 必须运行在 macOS 主线程**（shim 用
   `Thread.isMainThread` 判定，非主线程返回 status 3 → `NewControllerError`）。

7. **最低版本是 macOS 13。** crate 的 `build.rs` 写 `MINIMUM_MACOS_VERSION = "13.0"`，两处
   `Package.swift` 都声明 `.macOS(.v13)`，实测 swiftc 收到 `-target arm64-apple-macosx13.0`。
   产品 `LSMinimumSystemVersion` 是 `12.0`。

8. **在 arm64 宿主上无法交叉编译到 `x86_64-apple-darwin`。** 实测
   `cargo build --target x86_64-apple-darwin` 链接失败（Swift 对象是 arm64，Rust 目标要 x86_64）。
   根因在 `swift-rs 1.0.8` 的 `src-rs/build.rs`：Swift 的 `--arch` 取自
   `std::env::consts::ARCH`（宿主架构），且 `cross_compiling` 把 `x86_64-apple-darwin` 判为非交叉。

9. **能力面比「输入监控」这一项大。** shim 导入 `Carbon`、`CoreBluetooth`、`StoreKit`、
   `MusicKit`，`fullDiskAccessAuthorizationState()` 会读 `/private/etc/sudoers`、
   `/Library/Application Support/com.apple.TCC/TCC.db` 等路径，Rust 侧也暴露 8 类权限。

10. **CI 构建的 arm64 包在 macOS 27 上点「打开系统设置」会 SIGTRAP。** 复现（v2.1.0 CI 包，
    arm64，macOS 27.0）：点击引导按钮进入 `request_permission_flow` → `start_flow()`，SwiftUI
    面板的 `NSHostingView` 加入窗口时断言失败，`bongocat-app` 崩溃。崩溃栈均在 SwiftUI 框架
    内（`NSHostingView.didChangeRequiredBridges` → `GraphHost.preferenceValues()` →
    `AGGraphGetValue` → `ViewBodyAccessor.updateBody` 内的 `_assertionFailure`）。同一源码、
    同一 `Cargo.lock` 的本地产物不崩。唯一可复现的差异是构建 SDK：v2.1.0 CI 包
    （`build-provenance.json` 为 arm64、`vtool` 显示 `sdk 26.5`、LD 1267.0）在 macOS 27 上崩，
    本地产物（`sdk 27.0`、LD 27037.1、Xcode 27.0）不崩。也就是说 permission-flow 的 SwiftUI
    面板是「Swift 静态库版本 ↔ 系统 SwiftUI」的运行时不变量敏感代码：用 macOS 26 SDK 编译的
    SwiftUI 客户端跑在 macOS 27 的 SwiftUI 上会触发 AG 图断言，反向（macOS 27 SDK）不会。

## 决策

### 1. 接受 Swift/AppKit 作为第二个厂商 FFI 例外

AGENTS.md §1 目前只允许「官方 Cubism Core 平台二进制」一个厂商 FFI 例外。本 ADR 明确追加第二个
例外，范围严格限定为 **macOS 输入监控权限引导的呈现**，与 ADR-0005 的 Cubism 边界并列：

- `permission_flow` 只出现在 `bongocat-platform` 的私有 `startup_permission` adapter 之后，
  它的类型不进入项目公共 API，也不进入 runtime、render、overlay、input、config、UI；
- 该例外不放宽「不引入 Tauri、WebView、Node.js、JavaScript 或第二套 UI framework」：Swift 面板
  是权限引导的专用表面，设置界面仍然只有 `gpui-kit`。

依赖只声明在 `bongocat-platform` 的 macOS target 下（`permission-flow = "=0.1.40"`），因此
Windows 构建不构建 `swift-rs`，也不受 Swift 工具链影响。

### 2. 保留 ADR-0032 的提示壳，只替换 macOS 的引导动作

`NSAlert` 提示、文案 key、worker 生命周期都不动。`startup_permission` adapter 的 macOS
`request_permission_flow` 换成 guided flow；Windows 侧只多接受一个被忽略的 locale 参数。

### 3. 每次进入授权流程前执行 `tccutil reset ListenEvent com.ayangweb.bongo-cat`

2026-09-30 明确选择，实现为 `reset_input_monitoring_grant()`，结果被忽略（系统没有该 bundle 的
条目时 `tccutil` 报错，而那正是要到达的状态）。**这一项偏离既有决策，必须同步修订 ADR-0024 与
ADR-0032：**

- ADR-0032 的「已授权则完全不提示」语义改变：流程入口会主动把「已授权」打回「未授权」，因此
  **每次点引导按钮都要求重新授权**，即使此前已经授权过。
- ADR-0008 固定 Bundle ID 为 `com.ayangweb.bongo-cat`、Development 与 Production 共用，因此该
  reset 会**同时清除已安装 Production 应用的授权**。这与 ADR-0008「开发构建不得读取、写入或锁住
  生产数据」的精神冲突，本 ADR 记录该冲突并接受它。
- 正在运行的事件 tap 会因此失去授权。既有恢复路径（tap 被禁用 → 重启 → `CGPreflightListenEventAccess`
  复查 → `PermissionDenied`）已覆盖这一状态，但**用户重新授权之后没有自动重启输入服务的触发点**，
  即重新授权后输入要等应用重启才恢复。这是本决策的已知后果，见「未完成项」。
- 重置让提示文案里那段「如果列表中已有 BongoCat，请先选中并点击「−」移除，再点击「＋」重新添加」
  变成描述产品自己已经做完的事：条目在面板打开前就被清掉了。因此
  `startup_permission.input_monitoring.description` 只保留用途那一句，第二段从全部 6 个 locale
  删除（2026-09-30）。Windows 的 `administrator.description` 保留——它的第二段是那条路径上唯一
  说明兼容性开关怎么设的地方。

### 4. 不使用 `AppPath::suggested_host_app()`

拖拽目标取自当前可执行文件所在的 `.app`（`enclosing_bundle`），没有 bundle 时取可执行文件本身。
Swift 侧会把非 `.app` 项过滤掉，因此开发二进制得到一个空的拖拽目标，面板仍显示面板引导。
契约测试禁止 macOS 模块出现 `suggested_host_app`。

### 5. 语言对齐靠 `CFBundleLocalizations` + 本应用域的 `AppleLanguages`

上游 Rust 接口没有 locale 参数，所以按实测结论第 3 条走 macOS 自己的机制：

- `macos/Info.plist` 声明 `CFBundleLocalizations`，列出 Swift 面板实际提供 catalog 的语言
  （`ar`、`en`、`pt`、`zh-Hans`、`zh-Hant`）。缺了它，嵌套 bundle 永远回落到 development region。
- 进入流程前把产品语言写进本应用 CFPreferences 域的 `AppleLanguages`（`apply_flow_language`）。
  locale 由 `bongocat-app` 解析后经 `StartupPermissionPrompt::locale` 传入 adapter，与提示文案
  同源，不会各自漂移。
- 该偏好是持久的，写的是本应用自己的域，等价于「本应用的界面语言」，因此应用的其他 AppKit 表面
  也会跟随——这是语言设置本来就该有的行为。
- `vi-VN` 没有上游 catalog，回落英文，记录为已知缺口。

### 6. 链接与打包的必做项（已实现）

- `crates/bongocat-app/build.rs` 的 `link_swift_runtime()` 在 macOS 目标上输出
  `-Wl,-rpath,/usr/lib/swift`；契约测试固定这一行。
- `bongocat-packaging` 在打包前定位 `permission-flow` 的 `OUT_DIR` 里的 Swift 资源包，映射进
  `Contents/Resources`，并在 `verify_app_bundle` 里断言它存在——丢了这个包会在用户手上崩溃，必须
  在打包阶段失败。
- 开发二进制没有打包步骤，adapter 在资源包不可解析时把它从 Cargo profile 目录复制到可执行文件
  旁（`ensure_resource_bundle_available`），并且**不写入 `.app`**（会破坏签名）。复制失败时不启动
  引导面板，退回 ADR-0032 的原有动作——Swift 面板在找不到资源包时会 abort 整个进程，崩溃不是可
  接受的降级方式。

### 7. 依赖来源：临时固定在 fork 的 rev

macOS 12 支持只能先落在 fork（上游 PR <https://github.com/veecore/permission-flow/pull/2>）：

- fork `ayangweb/permission-flow` 的 `feat/macos-12-support`，rev `fad0354d960cf775ac332e6375916f7cc6424c7b`。
- 根 `Cargo.toml` 的 `permission-flow` 暂时写成 `{ git = …, rev = … }`；`deny.toml` 的 `allow-git`
  相应增加该来源。`required-git-spec = "rev"` 保证不会漂到分支上。
- fork 里的改动只有 macOS 12 相关这几类：`MINIMUM_MACOS_VERSION` 与两处 `Package.swift` 的
  `.macOS(.v12)`、全部 `@available(macOS 12.0, *)`、`PermissionFlowLocalizer` 在 12 上按 region
  推断 script（`Locale.Language` 是 13+）、`SettingsNavigator` 兼容 System Preferences 的旧路径，
  以及隐私面板 deeplink 的面板标识按系统版本切换（macOS 12 只认
  `com.apple.preference.security`，13+ 才是 `com.apple.settings.PrivacySecurity.extension`；
  这一点是实机验证 macOS 12 时发现的）。`PermissionFlowButton` 仍是 13+，因为它用
  `LocalizedStringResource`。
- 上游合并并发布后必须改回 crates.io 精确 pin，并从 `allow-git` 删掉这一行；在此之前
  `Cargo.lock` 记录的是 git source，`cargo deny check sources` 是这一项的守门。

### 8. CI 产物的 SDK 必须匹配产物运行面向的 macOS 大版本（2026-10-03）

v2.1.0 CI 的 arm64 包在 macOS 27 上点权限引导的「打开系统设置」会 SIGTRAP（实测结论第 10 条），
根因是同一 Swift 面板在「macOS 26 SDK 编译、macOS 27 运行」的组合下触发 SwiftUI AG 断言。修复：
release workflow 的 Apple Silicon leg 从 `macos-latest`（当时是 macOS 26 / Xcode 26.x）换成
`xcode-27`（macOS 27 / Xcode 27.0 / SDK 27.0），让 CI 产物与实测不崩的本地产物用同一代 SDK。
Intel leg 保留 `macos-26-intel`：macOS 27 是 Apple silicon 专用（2026-09 公布的兼容列表不含 Intel
机型），Intel 机最高只到 macOS 26，SDK 26.x 产物与运行面匹配。两个 leg 的 Xcode 大版本随平台
能力各自取最新，不再人为对齐；「配置完全一致」的原始意图降级为「同为原生 runner」，见 ADR-0033
修订节。后续 macOS 大版本迭代时，两条 leg 的 Xcode/SDK 必须同步复核。

## 未完成项

- **上游 Rust 接口仍没有 locale 参数**：语言对齐继续依赖决策第 5 条的两半机制，等上游加参数后
  移除。
- **macOS 12 只验证了一部分**：实机（macOS 12）确认面板、拖拽卡片与资源包都正常，并因此发现
  deeplink 的面板标识问题（已在上游 PR #2 修掉）。仍未实机确认：修正后的 deeplink 是否落到
  「安全性与隐私 → 输入监控」、以及浮动面板的拖拽引导在 macOS 12 的复选框列表上是否可用——
  macOS 12 的隐私面板不接受拖拽，只提供「+」按钮，面板文案目前没有为该系统区分。
- **上游 PR 合并前依赖来源是 fork**：见决策第 7 条，合并发布后必须切回上游版本。
- **`x86_64-apple-darwin` 无法交叉编译**：`swift-rs` 让 swiftc 按**宿主架构**编译，所以在
  arm64 机器上构建该目标会链接失败（`_permission_flow_*` 未定义符号）。release workflow 已改为
  两条 macOS leg 各用一台同架构 runner（ADR-0033 修订节），发布不受影响；但本机
  `just build --target x86_64-apple-darwin` 仍打不出来。治本要 `swift-rs` 的 `--arch` 跟随
  Rust target（上游 `Brendonovich/swift-rs`，1.0.8 是当前最新版，尚无此改动）。
- **重新授权后输入服务不会自动重启**：见决策第 3 条。
- **实机验收未做**：面板外观、拖拽引导是否可用、以及它与 `GPUIApplication`（ADR-0032「macOS 弹框
  实现修正」记录的 `Ivar platform not found on class NSApplication` 崩溃类别）共存时是否安全，都
  需要人工在真机观察。自动化测试无法覆盖。
- **语言切换的缓存边界未验证**：`Bundle.preferredLocalizations` 按进程缓存。`AppleLanguages` 在
  流程开始前写入，但若进程内已有更早的读取，面板可能仍用旧值；这一条只能实机确认。

## 验证

- 契约测试：macOS 模块禁止出现 `suggested_host_app`；`bongocat-app` 的 build script 必须含 Swift
  运行时 rpath；macOS 模块必须含 `ListenEvent`；原有的 NSAlert / `run_on_main` / 禁止
  `rfd::MessageDialog::new` 与 `AsyncMessageDialog` 断言不变。
- 打包验证：`verify_app_bundle` 断言 `Contents/Resources/PermissionFlow_PermissionFlow.bundle`
  存在。
- fork 侧的 macOS 12 验证：`swift build -Xswiftc -target -Xswiftc arm64-apple-macos12.0`（含
  `--build-tests`）无 error 无 warning，`swift test` 5 passed，`cargo fmt --all --check` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace --lib` 通过，
  `otool -l libPermissionFlowShimFFI.a` 显示 `minos 12.0`。
- 可重复的人工验收命令：`cargo run -p bongocat-app -- --run-seconds 0`，在未授权时应出现提示，
  点主按钮后应依次看到 TCC 授权被清除、系统设置打开到「输入监控」、以及浮动的拖拽引导面板。
  `--startup-permission-smoke` 仍然只输出能力状态，不弹框。

## 替换边界

替换点是 `bongocat-platform` 的私有 adapter（macOS 的 `request_permission_flow`、主线程
controller 槽、资源包与语言处理）、`crates/bongocat-app/build.rs` 的 rpath、`macos/Info.plist`
的 `CFBundleLocalizations`，以及 `bongocat-packaging` 的资源映射与校验。升级 `permission-flow`
时必须复验：`MINIMUM_MACOS_VERSION`、`SwiftLinker` 的 `--arch` 是否改为跟随 Rust target、
`Bundle.module` 的查找路径、`PermissionFlowConfiguration` 的 locale 字段、上游 catalog 的语言
集合，以及 Swift 工具链最低版本（上游 `swift-tools-version` 为 6.0 / 6.1，不在
`rust-toolchain.toml` 覆盖范围内，CI 需显式提供）。上游合并发布后，替换点还包括依赖来源本身：
`Cargo.toml` 的 git rev 与 `deny.toml` 的 `allow-git` 条目。
