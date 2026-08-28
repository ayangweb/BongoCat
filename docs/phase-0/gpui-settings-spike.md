# GPUI Settings Spike Record

状态：macOS 最小生命周期通过，交互与双平台验证待完成
日期：2026-08-28
基线 commit：`44f44bcf2b17b8e16463ad479a477a949d01cc9a`

## 范围

本 spike 只验证以下最小闭环：

1. 独立于历史 Tauri workspace 解析并锁定 `gpui = 0.2.2`；
2. 创建 760x520 设置窗口并绘制基础布局和文本；
3. 由 GPUI 自身的 `quit()` 路径自动退出；
4. 不接入 runtime、输入、Live2D 或原生 overlay。

源码位于 `spikes/gpui-settings/`。该目录包含独立 `Cargo.lock`，不会改变历史根 workspace。

## 环境

```text
Hardware: MacBook Pro 18,1 / Apple M1 Pro / 16 GB
GPU: Apple M1 Pro 16-core / Metal 4
Display: 3456x2234 Retina
OS: macOS 26.5.2 (25F84)
Xcode: 26.6 (17F113), macOS SDK 26.5
Rust: rustc 1.97.1, aarch64-apple-darwin
GPUI: crates.io 0.2.2, Apache-2.0
```

## 命令与结果

```text
cargo fmt -- --check
cargo check --locked
BONGOCAT_SPIKE_AUTO_QUIT_MS=1500 cargo run --locked
```

结果：编译通过，进程输出 `window opened`，随后通过 GPUI `quit()` 输出 `stopped` 并以 0 退出。

上游 `block 0.1.6` 和 `proc-macro-error2 2.0.1` 被 Cargo 标记为 future-incompatible；进入产品 workspace 前需要评估 GPUI 更新或上游修复时间。

## Metal Toolchain 约束

本机 Xcode 可以定位 `xcrun metal`，但未安装可执行的可选 Metal Toolchain。GPUI 默认的预编译 shader 路径因此失败，并提示：

```text
xcodebuild -downloadComponent MetalToolchain
```

为避免在 spike 中修改系统环境，当前探针启用 GPUI 的公开 `runtime_shaders` feature，由 Metal API 在运行时编译 shader。这只用于开发验证，不是发布决策。正式构建必须固定 Xcode/Metal Toolchain，并重新验证 GPUI 默认预编译 shader 路径。

## 未完成

- 由于命令行应用尚未打包为 `.app`，当前 UI 检查器无法按 bundle id 读取窗口，未形成截图和像素级视觉证据。
- 中文输入法、文本编辑、复制粘贴、焦点、键盘导航、辅助功能树和主题尚未实现或验证。
- 设置窗口关闭/重开、菜单栏生命周期和 native overlay 共存尚未验证。
- Windows 构建、DPI、字体、IME 和退出路径尚未验证。

因此本记录不满足完整 GPUI spike 或 Phase 0 退出门槛，相关 TODO 不勾选。
