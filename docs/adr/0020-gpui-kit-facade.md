# ADR-0020: GPUI Kit 统一依赖入口

状态：已接受（2026-09-04）

## 背景

ADR-0019 直接组合 Zed git source 的 `gpui`、`gpui_platform` 与 GPUI Component 开发版及
assets。应用必须手工保证四个 source 的类型一致，manifest、import 和升级边界分散。GPUI Kit
随后在 crates.io 发布 `0.6.0`，提供 GPUI、platform、base、component 和 assets 的统一入口。

## 决策

- Native workspace 只直接依赖精确固定的 crates.io `gpui-kit = "=0.6.0"`，提交完整
  `cargo update` 后的 `Cargo.lock`。不再直接声明 `gpui`、`gpui_platform`、
  `gpui-component` 或独立 assets crate，也不使用 git source 覆写 GPUI。
- 代码从 `gpui_kit` 根使用 GPUI 类型，从 `gpui_kit::platform`、`gpui_kit::component` 和
  `gpui_kit::assets` 使用对应层；组件初始化统一调用 `gpui_kit::init`。
- `gpui-kit 0.6.0` 使用 Apache-2.0 许可证，发布时是 crates.io 最新稳定版。它的 GPUI 依赖
  通过 crates.io `gpui-pre` 同步包交付；当前 lockfile 解析到 `0.3.3`，包元数据声明对应
  Zed `gpui 0.2.2` revision `5b055fa789a8b8d38ac951a6e0cde272f66b4495`。因此项目不再直接
  覆写 GPUI，但也不把该传递包误记为 crates.io 包名 `gpui = 0.2.2`。
- GPUI Kit 默认 component/assets feature 正好覆盖当前设置窗口。tree-sitter、decimal、
  inspector 和 test-support 等可选 feature 不启用。其 native facade 还会引入配套 HTTP client
  与 TLS 传递依赖；这些依赖不得进入 BongoCat 的业务 API。
- 现有项目 AccessKit bridge、typed command/snapshot 和独立 overlay 边界保持不变。
  `Application::new_inaccessible` 继续通过 `gpui_kit::platform` 构造，直到项目桥接由验证过的
  GPUI 原生 element 语义整体替代。
- 替换边界限定在 `bongocat-ui` 与 `bongocat-app` 的窗口入口。上游停止维护、许可证变化或
  GPUI 版本不兼容时，只替换这一 UI 边界，不向 runtime、config、model 或 renderer 扩散
  GPUI Kit 类型。

## 影响

manifest 和 Rust import 只有一个版本入口，避免应用与组件解析到两套 GPUI 类型。crates.io
发布物与 checksum 进入 lockfile，移除了未固定 revision 的 Zed git 依赖。代价是 GPUI Kit
统一管理整套传递版本，升级必须作为单独变更重跑双平台构建、设置窗口、IME、辅助功能、缩放、
窗口重建和 shutdown smoke；本 ADR 不把仍缺少实机证据的 UI TODO 标记完成。
