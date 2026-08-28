# GPUI Settings Spike Record

状态：macOS 默认 shader、`.app`、主题、基础文本交互和 runtime bridge 通过；内容辅助功能、真实 IME 和跨平台验证待完成
日期：2026-08-28
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

结果：格式化、Clippy、1 项 runtime bridge contract test 和 debug/release 编译通过，`.app` 的 ad-hoc 签名通过 strict bundle integrity 校验；LaunchServices smoke 以 0 退出。直接运行 release binary 时输出 `window opened`、`runtime snapshot revision=1`，随后通过 GPUI `quit()` 输出 `runtime stopped` 和 `stopped` 并以 0 退出。

## Runtime Bridge 结果

spike 使用容量为 8 的 `async-channel 1.9.0` 传递强类型 `ReadSnapshot` 和 `Shutdown` command。每个 command 携带容量为 1 的 reply channel；UI 通过 GPUI `Context::spawn` 等待结果，再使用 weak entity 更新视图状态。snapshot 带单调递增 revision，UI 忽略不比当前 revision 更新的结果，runtime worker 在 GPUI background executor 上运行。

退出时，runtime 先关闭 command receiver，再发送 shutdown acknowledgement，避免 acknowledgement 返回后仍有请求成功入队并永远等待 reply。contract test 覆盖两次 snapshot 的 revision、health、shutdown acknowledgement 和停止后请求失败。macOS release `.app` smoke 同时证明 GPUI executor 能完成首次 snapshot 请求，并在 auto-quit 的 100 ms GPUI shutdown 窗口内收到 acknowledgement；该结果不等于产品 runtime、持久化或高负载 channel 已完成。

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

这些结果是 macOS 当前环境下的视觉和交互 smoke，不等价于跨平台验收。GPUI 输入实现已接入 UTF-16 selection、grapheme 边界和 marked-text 公共协议，但尚未用中文输入法完成真实组合态验证；Windows 字体、IME、DPI 和辅助技术也尚未验证。

2026-08-28 按 ADR-0008 将 Bundle ID 更新为 `com.ayangweb.bongo-cat` 后重新执行打包、strict codesign 和 LaunchServices auto-quit，三项均通过。打包脚本会在签名前读取 Info.plist 并拒绝任何非预期 Bundle ID。

上游 `block 0.1.6` 和 `proc-macro-error2 2.0.1` 被 Cargo 标记为 future-incompatible；进入产品 workspace 前需要评估 GPUI 更新或上游修复时间。

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
- 应用菜单：系统可识别 `Services` 和 `Quit BongoCat GPUI Spike`；
- 关闭设置窗口后进程保持运行，再次激活应用可重建窗口；
- `Cmd+Q` 触发 GPUI action，进程正常退出。

ad-hoc signing 只验证本地 bundle 完整性，不代表 Developer ID、Hardened Runtime、notarization 或发布 Gatekeeper 已通过。

## Accessibility Finding

macOS 辅助功能 API 能识别应用、标准窗口、标题、traffic-light buttons、菜单栏和菜单项，但不能识别 GPUI 绘制的 `Appearance`、`Theme`、`Models` 等内容节点。截图可见不等于辅助功能可用。

GPUI 0.2.2 公共源码中没有找到可为普通绘制 element 设置 role、label、value 的通用 accessibility API。本问题在进入产品 UI 前必须得到以下之一：

1. GPUI 的受支持公共 accessibility API/版本升级；
2. 可维护且不依赖 Zed 私有 UI crate 的项目内方案；
3. 若仍无法满足设置表单基础要求，提交 GPUI go/no-go ADR 并评估 Iced。

## 本地化资源边界

当前五种 locale（`en-US`、`pt-BR`、`vi-VN`、`zh-CN`、`zh-TW`）仍位于历史前端的 `src/locales/`，仅作为 Native Rewrite 的迁移输入。`tools/validate-locales.py` 在迁移前检查每个语言包的递归 key、叶子类型、非空文本和占位符集合，并由 Phase 0 CI 执行。迁移到 `shared/resources/localization/` 时必须保留相同 key 契约，再由 GPUI UI crate 显式加载；该检查不代表 Native 本地化迁移或 GPUI 辅助功能已经完成。

## 未完成

- 真实中文输入法组合态/marked text 尚未在 macOS 上完成端到端验证；Windows IME、字体、DPI 和辅助技术尚未验证。
- tooltip、dialog 和完整菜单交互尚未验证。
- GPUI 内容辅助功能树未通过；当前只有窗口 chrome 和菜单可识别。
- 菜单栏常驻策略、隐藏行为和 native overlay 共存尚未验证。
- Windows 构建、退出路径和系统集成尚未验证。

因此默认 shader 工具链、`.app` bundle/lifecycle、主题和基础编辑交互子项可以单独记录为通过；内容辅助功能、真实 IME、Windows 验证、overlay 共存和完整 GPUI spike 仍保持未完成。GPUI go/no-go 决策必须等辅助功能策略明确后再做。
