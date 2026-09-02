# ADR-0019: gpui-component Design System

状态：已接受（2026-09-02）

## 背景

Native 设置窗口需要与固定的 `gpui 0.2.2` 兼容的主题、输入和桌面控件。维护者决定将现有
`guise-ui 1.5.3` 替换为 `gpui-component`，并指定其官方组件文档作为 API 事实来源。

## 决策

- Native workspace 精确固定 `gpui-component = 0.5.1`，关闭未使用的可选 feature；该版本是
  2026-09-02 审计时 crates.io 最新非 yanked 稳定版，许可证为 Apache-2.0，并依赖 `gpui 0.2.2`。
  crates.io 显示该版本发布于 2026-02-05；crate 自身 `src/` 没有显式 `unsafe`，平台 unsafe 仍由
  GPUI 及其传递依赖承担。
- 设置窗口在首次创建组件前调用 `gpui_component::init`，以官方 `Root` 作为窗口根视图，并在
  系统外观变化时同步 `Theme`。
- 图标资源使用官方 `gpui-component-assets = 0.5.1` 的 `Assets`（Lucide SVG），通过
  `Application::with_assets` 注册给 GPUI；应用不自维护重复的 `icons/*.svg` 文件。
- UI 使用组件库的 `Button`、`Switch`、`Tag`、`Divider`、`Input` 和 `NumberInput`。
  `InputState` 只持有编辑状态；`InputEvent`/`NumberInputEvent` 转换为现有 typed command，
  runtime snapshot 仍是持久状态事实来源。
- `gpui-component 0.5.1` 没有普通 `Card` primitive；`HoverCard` 是悬浮预览组件而非内容容器。
  设置内容容器统一使用官方 `GroupBox::outline()`，导航继续保留无状态视觉封装。
- 第三方类型不进入 runtime、config 或 model 公共 API。停止维护或 GPUI 版本不兼容时，替换
  边界保持在 `bongocat-ui` 和窗口初始化代码内。

## 影响

`gpui-component` 提供的控件范围更广，但传递依赖和编译成本高于 Guise。锁文件必须提交，后续
升级继续按仓库依赖规则审计版本、许可证、平台构建和完整输入/窗口 smoke。双平台辅助功能、缩放
及真实交互证据仍是 UI 完成门禁，本 ADR 不将这些 TODO 标记完成。
