# ADR-0056: SettingGroup 按组 variant 覆盖的过渡 patch

状态：已接受（2026-09-23）

## 背景

模型管理页的内容是一个自绘卡片的模型网格。设置窗口的组容器走官方
`Settings::with_group_variant(GroupBoxVariant::Outline)`（ADR-0019/0020），该页因此在外层
多出一个卡片容器：边框、内边距与圆角包裹在自绘网格外面，读作双重容器。

`gpui-kit 0.6.6`（crates.io 最新）的 `Settings::with_group_variant` 文档写明该值
"unless overridden individually"，但 `SettingGroup` 没有任何覆盖 API；`SettingGroup` 的
`Styled` refinement 只落到 `GroupBox` 外层 wrapper，而 Outline 的边框与内边距画在内层
元素上，样式上无法去除。全局换 variant 不可行：快捷键页两个带标题组的标题在 Outline 下
画在边框盒外，且带标题组是二级侧边栏入口，其他页面无法保持像素不变。上游 main 在 0.6.6
之后对 setting 模块无改动，无可摘修复。

## 决策

- 向上游提交 issue（longbridge/gpui-kit#3202）与 PR（#3203）：为 `SettingGroup` 增加
  `variant(GroupBoxVariant)` 覆盖，`render` 时 `self.variant.unwrap_or(options.group_variant())`，
  兑现 `with_group_variant` 的既有文档承诺。
- 在上游合并发版前，通过 `[patch.crates-io]` 将 `gpui-kit = "=0.6.6"` 指向维护者 fork 的
  过渡分支 `patch/0.6.6-setting-group-variant`（rev `720aeef7`）。该分支从 0.6.6 发布
  commit（`9765ae2c`）切出，仅重放上述覆盖与其测试。patch 的是 facade `gpui-kit` 而非
  `gpui-component`，使整棵 gpui 系依赖树在单一 commit 内解析，避免 git/registry 双份
  同名包的 `links` 冲突与类型分裂。
- 模型管理页的组（`crates/bongocat-ui/src/window/render.rs`）使用
  `SettingGroup::variant(GroupBoxVariant::Normal)`，页面内容直接呈现；其余页面的
  Outline 卡片保持不变。`WrappedModelsPageHarness`（`window/tests.rs`）同步该覆盖，
  以继续忠实复现产品的真实包装链。
- `Cargo.lock` 中 gpui-kit/gpui-component/gpui-base/gpui-component-macros/gpui-kit-assets
  五个包换源到该 git rev，其余依赖与 `gpui-pre 0.3.6` 解析不变。

## 影响

- 其他页面布局与样式不变；设置窗口的搜索、二级侧边栏与重置路径不受影响（组结构未变）。
- 该 patch 是过渡措施：上游 PR 合并发版后，升级 `gpui-kit` 并删除 `[patch.crates-io]`
  段，锁定版本规则（ADR-0020）恢复为 crates.io 精确 pin。升级前 CI 需要访问
  `github.com/ayangweb/gpui-kit`。
- 上游 PR 被要求改名或改形态时，本仓库同步更新 patch 分支与引用 rev。
