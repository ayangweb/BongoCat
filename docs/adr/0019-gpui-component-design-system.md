# ADR-0019: gpui-component Design System

状态：已接受（2026-09-02）

## 背景

Native 设置窗口需要与固定的 `gpui 0.2.2` 兼容的主题、输入和桌面控件。维护者决定将现有
`guise-ui 1.5.3` 替换为 `gpui-component`，并指定其官方组件文档作为 API 事实来源。

## 决策

- Native workspace 使用 GPUI Kit 官方开发版 `gpui-component 0.5.2` revision
  `c0946e6acdc9e2f984f317ef7f998ee2c79f1a87`，关闭未使用的可选 feature；其许可证为
  Apache-2.0，依赖 GPUI `0.2.2`。GPUI Kit 的 manifest 使用官方 Zed git source，提交的
  `Cargo.lock` 固定当前解析到的 GPUI revision `55c0cc36d18cf06c6c54f640a87ce83d09413ae4`，
  避免组件与应用加载两份 GPUI 类型；`gpui_platform` 使用同一 Zed source。
- 设置窗口在首次创建组件前调用 `gpui_component::init`，以官方 `Root` 作为窗口根视图，并在
  系统外观变化时同步 `Theme`。
- 该 GPUI revision 默认会为窗口安装自己的 AccessKit adapter，而现有项目桥接仍承载已验证的
  macOS AX/Windows UIA 语义和 action contract。过渡期所有 GPUI 应用入口使用
  `Application::new_inaccessible(gpui_platform::current_platform(false))`，仅禁用重复的 GPUI
  adapter，避免两套 `accesskit_macos` 向同一 NSView 注册同名 Objective-C 类并触发 abort；
  项目桥接继续提供辅助功能。迁移到 GPUI 原生 element-level 语义后必须同时删除项目桥接、
  直接 AccessKit 依赖和此兼容构造，并重跑双平台辅助功能门禁。
- 图标资源使用官方 `gpui-component-assets = 0.5.1` 的 `Assets`（Lucide SVG），通过
  `Application::with_assets` 注册给 GPUI；应用不自维护重复的 `icons/*.svg` 文件。
- UI 使用组件库的 `Settings`、`SettingPage`、`SettingGroup`、`SettingItem`、`SettingField`，以及
  `Button`、`Switch`、`Tag`、`Input` 和 `NumberInput`。偏好设置页面由官方 Settings
  sidebar 管理，页面内按职责使用 SettingGroup；Settings 自带的搜索会按 SettingItem 标题和
  描述过滤，业务关键词放入描述以保持搜索索引与显示文案同源。
  `InputState` 只持有编辑状态；`InputEvent`/`NumberInputEvent` 转换为现有 typed command，
  runtime snapshot 仍是持久状态事实来源。
- `gpui-component 0.5.2` 没有普通 `Card` primitive；`HoverCard` 是悬浮预览组件而非内容容器。
  Settings 分组使用官方 `GroupBoxVariant::Outline`，模型管理和诊断等复杂内容通过官方
  `SettingField::element` 嵌入，不另建设置框架。
- 第三方类型不进入 runtime、config 或 model 公共 API。停止维护或 GPUI 版本不兼容时，替换
  边界保持在 `bongocat-ui` 和窗口初始化代码内。

## 影响

`gpui-component` 提供的控件范围更广，但传递依赖和编译成本高于 Guise。锁文件必须提交，后续
升级继续按仓库依赖规则审计版本、许可证、平台构建和完整输入/窗口 smoke。双平台辅助功能、缩放
及真实交互证据仍是 UI 完成门禁，本 ADR 不将这些 TODO 标记完成。
