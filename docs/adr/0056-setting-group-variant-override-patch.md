# ADR-0056: SettingGroup variant 与上游 GPUI Kit 固定 revision

状态：已接受（2026-09-23；2026-09-24、2026-09-25 修订）

## 背景

模型库页面的内容是自绘卡片网格。设置窗口的组容器使用官方
`Settings::with_group_variant(GroupBoxVariant::Outline)`，因此该页在外层多出一个卡片容器：
边框、内边距与圆角包裹在自绘网格外面。`SettingGroup::variant(GroupBoxVariant::Normal)`
可以在不改变其它页面的情况下移除这一层容器。

`gpui-kit 0.6.6` 的文档已经承诺 group variant 可单独覆盖，但实现没有对应 API。维护者向
`longbridge/gpui-kit` 提交 issue #3202 与 PR #3203；PR 已于 2026-09-23 合并，merge commit
为 `500852f449c05dc01920ec82f3ae2656a61d0387`。同一上游 revision 还包含
`Popover::arrow(bool)`，可供项目内 `PopConfirm` 转发。对应能力尚未发布到 crates.io。

修订（2026-09-25）：ADR-0066 将模型对象拆为 Model library 与 Model behavior 两个独立页面。
`SettingGroup::variant(GroupBoxVariant::Normal)` 只用于前者——它承载自绘卡片网格；后者是
独立页面并使用窗口默认 Outline。这个 variant 边界不因模型行为独立成页而扩大。

## 决策

- 根 workspace 直接依赖上游 `longbridge/gpui-kit` 的固定 revision
  `500852f449c05dc01920ec82f3ae2656a61d0387`，删除 `[patch.crates-io]` 与维护者 fork
  依赖。该 commit 的 package 元数据为 `0.6.5`；revision 比版本号更重要，禁止改用未固定的
  `main` branch。
- lockfile 中 `gpui-kit`、`gpui-component`、`gpui-base`、`gpui-component-macros` 与
  `gpui-kit-assets` 全部从上述同一 git commit 解析；GPUI 本身仍来自 crates.io
  `gpui-pre 0.3.6` 同步包，不引入第二套 GPUI 类型。
- Model library 页面继续调用 `SettingGroup::variant(GroupBoxVariant::Normal)`；独立的
  Model behavior 页面不覆盖 variant，调用点与测试 harness 不因依赖来源或页面拆分而改变。
- `PopConfirm` 增加 `arrow(bool)` 转发，模型删除确认在按钮上方显示并启用 anchor-aligned arrow。上游 revision
  同时改为由 `Root` 自动挂载 dialog、sheet 与 notification layer，因此业务根视图删除旧的
  `Root::render_*_layer` 调用。
- `deny.toml` 只允许 `https://github.com/longbridge/gpui-kit`，其它 git source 仍拒绝。

## 影响

- 依赖来源从维护者 fork patch 切换为上游固定 commit；不再维护或信任 fork 分支。
- 上游发布同时包含 `SettingGroup::variant()` 与 `Popover::arrow()` 的 crates.io release 后，
  必须升级到该 release 的精确 registry pin，删除 git source，并恢复纯 registry 来源策略。
- `Root` layer API 迁移只删除重复挂载；窗口仍使用 `gpui_kit::component::Root`，dialog、
  sheet 与 notification 的 owner 仍为 GPUI Kit。
- `PopConfirm::arrow` 默认关闭以保持调用方显式选择；当前模型删除确认使用 `BottomRight` 向上显示并显式启用箭头。组件几何测试
  固定 arrow 会为 trigger 与 surface 预留额外间距。
