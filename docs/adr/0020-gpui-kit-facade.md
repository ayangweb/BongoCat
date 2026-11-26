# ADR-0020: GPUI Kit 统一依赖入口

状态：已接受（2026-09-04）

> 后续修订（2026-09-24）：ADR-0054 退役项目自有设置语义桥。上游已合并 `SettingGroup::variant()`，但包含该能力与 `Popover::arrow()` 的 crates.io release 尚未发布；当前直接依赖上游固定 revision `500852f449c05dc01920ec82f3ae2656a61d0387`，不再使用 `[patch.crates-io]` 或维护者 fork。对应 release 发布后恢复纯 registry 精确 pin。

> 后续修订（2026-09-28）：上游已发布 `gpui-kit v0.7.0`，该 release 同时包含
> `SettingGroup::variant()` 与 `Popover::arrow()`。按上一条与 ADR-0056 的既定条件，直接依赖
> 切回 crates.io 精确 pin `=0.7.0`，`longbridge/gpui-kit` git source 与 `deny.toml` 的对应
> 放行项一并删除。GPUI 依赖随之上移到 `gpui-pre 0.3.7`。
>
> 后续修订（2026-10-06）：升级到 crates.io 精确 pin `=0.7.1`，GPUI 依赖随之上移到
> `gpui-pre 0.3.8`。仍然只依赖同一个 registry 版本，不改变本 ADR 的替换边界。
>
> 后续修订（2026-10-12）：升级到 crates.io 精确 pin `=0.7.3`，GPUI 依赖随之上移到
> `gpui-pre 0.3.9`（Zed 快照 `93d745668805029476e6d7fbb19652116fed51de`）。仍然只依赖同一个
> registry 版本，不改变本 ADR 的替换边界。

## 背景

ADR-0019 直接组合 Zed git source 的 `gpui`、`gpui_platform` 与 GPUI Component 开发版及
assets。应用必须手工保证四个 source 的类型一致，manifest、import 和升级边界分散。GPUI Kit
随后在 crates.io 发布 `0.6.0`，提供 GPUI、platform、base、component 和 assets 的统一入口。

## 决策

- workspace 只直接依赖 `gpui-kit`，当前为 crates.io 精确 pin `=0.7.3`，并提交完整
  `cargo update` 后的 `Cargo.lock`。不再直接声明 `gpui`、`gpui_platform`、`gpui-component`
  或独立 assets crate，也不使用其它 git source 混装 GPUI。
- 代码从 `gpui_kit` 根使用 GPUI 类型，从 `gpui_kit::platform`、`gpui_kit::component` 和
  `gpui_kit::assets` 使用对应层；组件初始化统一调用 `gpui_kit::init`。
- 当前固定的 `gpui-kit` 使用 Apache-2.0 许可证。它的 GPUI 依赖
  通过 crates.io `gpui-pre` 同步包交付；当前 lockfile 解析到 `0.3.9`（v0.7.3 起
  `gpui-kit` 以 `=0.3.9` 精确 pin 该家族，`gpui-pre-reqwest` 仍为 `0.12.15`），包元数据声明
  Zed `gpui 0.2.2` revision `93d745668805029476e6d7fbb19652116fed51de`。因此项目不再直接
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
- 2026-09-28：`gpui-kit v0.7.0` 发布，包含上述两项 API 与 `Root` 的自动挂载行为，因此
  切回 crates.io 精确 pin `=0.7.0`，删除 `longbridge/gpui-kit` git source 与 `deny.toml`
  的对应放行项。lockfile 中五个 suite package 统一解析为 `0.7.0`，GPUI 同步包升到
  `gpui-pre 0.3.7`（v0.7.0 以 `=0.3.7` 精确 pin 该家族）。项目未使用 release notes 中
  breaking change 涉及的 Chart/Plot、DatePicker/TimeField、Table、TextView heading 与
  表单/表格/弹层 primitive 迁移面，源码零改动；`Root` 与 `WindowExt` 仍由业务窗口入口
  直接使用。
- 2026-10-06：`gpui-kit` 由 `=0.7.0` 升级到 `=0.7.1`（crates.io 最新非 yanked 稳定版）。
  suite 五个 package 统一为 `0.7.1`，GPUI 同步包升到 `gpui-pre 0.3.8`（v0.7.1 以 `=0.3.8`
  精确 pin 该家族；同版本内含 continuation-indent API 适配，保持原有换行行为）。release notes
  里的两处行为变化——chart 首次绘制动画默认开启、Questionnaire 单选即确认——只涉及项目未使用的
  组件；新增的 speech/ColorSelect 与 `RenderedText::source` 也是增量 API。IME 候选框定位、输入框
  单行裁剪、disabled Radio、菜单键盘高亮、CJK 标点换行等修复落在设置界面实际使用的组件上，
  源码零改动。workspace 范围的 `cargo update` 同时把 `rust-i18n-macro`/`rust-i18n-support` 带到
  4.2.4，与 `gpui-component` 展开 `t!` 所需的 `rust-i18n` 运行时不再匹配，因此直接 pin 由 `=4.2.2`
  上调到 `=4.2.4`（ADR-0012）。
- 2026-10-12：`gpui-kit` 由 `=0.7.1` 升级到 `=0.7.3`（crates.io 最新稳定版；上游发布说明
  声明 v0.7.3 取代 v0.7.2 并包含其全部改动）。suite 五个 package 统一为 `0.7.3`，GPUI 同步包
  家族统一为 `gpui-pre 0.3.9`（v0.7.3 以 `=0.3.9` 精确 pin 该家族）。lockfile 随之移除
  `gpui-pre` 不再需要的传递依赖——`tracing-subscriber`/`sval`/`value-bag` 系列、
  `bindgen`/`clang-sys`/`cexpr`/`prettyplease`、`bzip2` 与 `libbz2-rs-sys`、`cgl`、
  `core-graphics2`、`core-video`、`io-surface`、`objc2-core-media`、
  `objc2-screen-capture-kit`、`async-compression`/`compression-*`——并新增 v0.7.3 引入的
  可选后端 `gpui-fast 0.1.7` 家族与 `Diff` 组件所需的 `similar`。`libbz2-rs-sys` 已不在依赖图
  中，`deny.toml` 为它保留的 `bzip2-1.0.6` 例外一并删除（`cargo deny` 会把它报成
  unmatched license exception）。
- v0.7.3 的两处行为变化落在项目实际使用的组件上：`TextView` 默认排版改为按继承字号计算标题
  字号（0.7.1 是相对硬编码 `px(14.)` 的绝对 rem 阶梯，更新窗口以 `.text_xs()` 渲染更新日志时
  二级标题会到 21px），段落间距由 `1rem` 收到 `0.75rem`；设置列表的横向内边距移进列表行，
  焦点环不再被列表裁剪。两者都是外观变化，不涉及项目代码的 API 迁移面——Dialog 入场动画、
  `TooltipDefaults`、DataTable `header_row_height`、Editor 补全、`InputState::add_selection`
  等均为项目未使用或纯增量能力，源码零改动。
- `gpui-fast` 是 v0.7.3 新增的可选 feature，启用后整套 GPUI 实现由 `gpui-pre` 换成 wgpu 后端的
  `gpui-fast`。本 ADR 不启用它：后端替换需要单独的 ADR、双平台实机证据与设置窗口、IME、缩放、
  窗口重建回归，不能作为依赖升级的附带结果。
- GPUI Kit 默认 component/assets feature 正好覆盖当前设置窗口。tree-sitter、decimal、
  inspector 和 test-support 等可选 feature 不启用。其 native facade 还会引入配套 HTTP client
  与 TLS 传递依赖；这些依赖不得进入 BongoCat 的业务 API。
- 当时保留项目 AccessKit bridge、typed command/snapshot 和独立 overlay 边界；`Application::new_inaccessible` 通过 `gpui_kit::platform` 构造。ADR-0054 后项目桥接已退役，`new_inaccessible` 只作为禁用 GPUI adapter 的兼容选择保留，不再形成 BongoCat 的辅助功能 contract。
- 替换边界限定在 `bongocat-ui` 与 `bongocat-app` 的窗口入口。上游停止维护、许可证变化或
  GPUI 版本不兼容时，只替换这一 UI 边界，不向 runtime、config、model 或 renderer 扩散
  GPUI Kit 类型。

## 影响

manifest 和 Rust import 只有一个版本入口，避免应用与组件解析到两套 GPUI 类型。来源在
2026-09-28 回到 crates.io 精确 pin，lockfile 记录 registry checksum，`deny.toml` 不再放行
GPUI Kit 的 git 仓库。代价是 GPUI Kit
统一管理整套传递版本，升级必须作为单独变更重跑双平台构建、设置窗口、IME、辅助功能、缩放、
窗口重建和 shutdown smoke；ADR-0054 后“辅助功能 smoke”不再是项目自有 UI 完成条件。本 ADR 不把仍缺少实机证据的 UI TODO 标记完成。
