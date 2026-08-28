# GPUI Settings Spike Record

状态：macOS 默认 shader、`.app` 与窗口生命周期通过；内容辅助功能与完整交互待完成
日期：2026-08-28
原始重构基线 commit：`94af230`；后续验证源码与本记录保持同一提交

## 范围

本 spike 只验证以下最小闭环：

1. 独立于历史 Tauri workspace 解析并锁定 `gpui = 0.2.2`；
2. 创建 760x520 设置窗口并绘制基础布局和文本；
3. 使用 GPUI 默认预编译 shader 路径完成 debug/release 构建；
4. 生成具有固定 bundle id 的最小 macOS `.app` 并通过 LaunchServices 启动；
5. 验证应用菜单、关闭、重开和 GPUI `quit()` 生命周期；
6. 不接入 runtime、输入、Live2D 或原生 overlay。

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
cargo check --locked
cargo build --release --locked
./scripts/package-macos.sh
codesign --verify --deep --strict --verbose=4 "target/package/BongoCat GPUI Spike.app"
open -W "target/package/BongoCat GPUI Spike.app" --args --auto-quit-ms 1500
```

结果：debug/release 编译通过，`.app` 的 ad-hoc 签名通过 strict bundle integrity 校验；LaunchServices smoke 以 0 退出。直接运行 release binary 时输出 `window opened`，随后通过 GPUI `quit()` 输出 `stopped` 并以 0 退出。

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

## 未完成

- 中文输入法、文本编辑、复制粘贴、键盘焦点顺序、tooltip、dialog 和主题尚未实现或验证。
- GPUI 内容辅助功能树未通过；当前只有窗口 chrome 和菜单可识别。
- 未保存带 commit/窗口尺寸/主题标注的截图证据；本次只完成人工视觉 smoke。
- 菜单栏常驻策略、隐藏行为和 native overlay 共存尚未验证。
- Windows 构建、DPI、字体、IME 和退出路径尚未验证。

因此只勾选默认 shader 工具链这一完整子项；`.app` 综合验收、完整 GPUI spike 和 Phase 0 退出门槛保持未完成。
