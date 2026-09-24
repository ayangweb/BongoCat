# ADR-0020: GPUI Kit 统一依赖入口

状态：已接受（2026-09-04）

> 后续修订（2026-09-24）：ADR-0054 退役项目自有设置语义桥。上游已合并 `SettingGroup::variant()`，但包含该能力与 `Popover::arrow()` 的 crates.io release 尚未发布；当前直接依赖上游固定 revision `500852f449c05dc01920ec82f3ae2656a61d0387`，不再使用 `[patch.crates-io]` 或维护者 fork。对应 release 发布后恢复纯 registry 精确 pin。

## 背景

ADR-0019 直接组合 Zed git source 的 `gpui`、`gpui_platform` 与 GPUI Component 开发版及
assets。应用必须手工保证四个 source 的类型一致，manifest、import 和升级边界分散。GPUI Kit
随后在 crates.io 发布 `0.6.0`，提供 GPUI、platform、base、component 和 assets 的统一入口。

## 决策

- Native workspace 只直接依赖上游 `longbridge/gpui-kit` 的固定 revision
  `500852f449c05dc01920ec82f3ae2656a61d0387`，提交完整 `cargo update` 后的
  `Cargo.lock`。不再直接声明 `gpui`、`gpui_platform`、`gpui-component` 或独立 assets
  crate，也不使用其它 git source 混装 GPUI。
- 代码从 `gpui_kit` 根使用 GPUI 类型，从 `gpui_kit::platform`、`gpui_kit::component` 和
  `gpui_kit::assets` 使用对应层；组件初始化统一调用 `gpui_kit::init`。
- 当前固定的 `gpui-kit` revision 使用 Apache-2.0 许可证。它的 GPUI 依赖
  通过 crates.io `gpui-pre` 同步包交付；当前 lockfile 解析到 `0.3.6`（v0.6.6 起
  `gpui-kit` 以 `=0.3.6` 精确 pin 该同步包），包元数据声明对应
  Zed `gpui 0.2.2` revision `bcf6582ce3500df93a8a39366640173e6786cea6`。因此项目不再直接
  覆写 GPUI，但也不把该传递包误记为 crates.io 包名 `gpui = 0.2.2`。
- 2026-09-19：`gpui-kit` 由 `=0.6.1` 升级到 `=0.6.4`（v0.6.2 功能版与 v0.6.4 补丁的
  最新稳定版；无 breaking API 变化），升级范围仍在 `bongocat-ui` 与 `bongocat-app`，
  不改变本 ADR 的替换边界。
- 2026-09-22：`gpui-kit` 由 `=0.6.4` 升级到 `=0.6.6`（crates.io 最新非 yanked 稳定版；
  v0.6.5/v0.6.6 为补丁：masked label 跳过高亮，以及把 `gpui-pre` 改为 `=0.3.6` 精确
  pin，无 breaking API 变化）。lockfile 中 `gpui-kit` 本体之外，gpui-base/
  gpui-component/gpui-kit-assets `0.6.6` 与 `gpui-pre 0.3.6`（Zed 快照
  `bcf6582ce3500df93a8a39366640173e6786cea6`，`zed-version` 仍为 `0.2.2`）在本次
  `cargo update` 前已由此前一次完整 `cargo update` 解析到位，本次无 diff；v0.6.6 的
  `=0.3.6` 约束恰好与既有解析一致。升级范围仍在 `bongocat-ui` 与 `bongocat-app`，
  不改变本 ADR 的替换边界。
- 2026-09-24：`cargo search gpui-kit --limit 1` 与 `cargo info gpui-kit` 仍显示 crates.io
  最新稳定版为 `0.6.6`，但该 release 不含上游后来合并的 `SettingGroup::variant()` 与
  `Popover::arrow()`。上游 merge commit `500852f449c05dc01920ec82f3ae2656a61d0387`
  的 package 元数据仍为 `0.6.5`，因此本仓库直接固定该完整 commit；lockfile 中
  `gpui-kit`、`gpui-component`、`gpui-base`、`gpui-component-macros` 与
  `gpui-kit-assets` 统一为同一 git source，`gpui-pre` 仍为 crates.io `0.3.6`。
- 同一 revision 由 `Root` 自动挂载 dialog、sheet 与 notification layer；业务根视图删除旧的
  `Root::render_*_layer` 调用。`PopConfirm` 转发 `Popover::arrow(bool)`，模型删除确认启用
  anchor-aligned arrow。对应上游 release 发布后，本条临时 git source 决策自动失效并恢复
  crates.io 精确 pin。
- GPUI Kit 默认 component/assets feature 正好覆盖当前设置窗口。tree-sitter、decimal、
  inspector 和 test-support 等可选 feature 不启用。其 native facade 还会引入配套 HTTP client
  与 TLS 传递依赖；这些依赖不得进入 BongoCat 的业务 API。
- 当时保留项目 AccessKit bridge、typed command/snapshot 和独立 overlay 边界；`Application::new_inaccessible` 通过 `gpui_kit::platform` 构造。ADR-0054 后项目桥接已退役，`new_inaccessible` 只作为禁用 GPUI adapter 的兼容选择保留，不再形成 BongoCat 的辅助功能 contract。
- 替换边界限定在 `bongocat-ui` 与 `bongocat-app` 的窗口入口。上游停止维护、许可证变化或
  GPUI 版本不兼容时，只替换这一 UI 边界，不向 runtime、config、model 或 renderer 扩散
  GPUI Kit 类型。

## 影响

manifest 和 Rust import 只有一个版本入口，避免应用与组件解析到两套 GPUI 类型。固定 git
revision 进入 lockfile，`deny.toml` 只允许该上游仓库；对应 crates.io release 发布后改回
registry checksum。代价是 GPUI Kit
统一管理整套传递版本，升级必须作为单独变更重跑双平台构建、设置窗口、IME、辅助功能、缩放、
窗口重建和 shutdown smoke；ADR-0054 后“辅助功能 smoke”不再是项目自有 UI 完成条件。本 ADR 不把仍缺少实机证据的 UI TODO 标记完成。
