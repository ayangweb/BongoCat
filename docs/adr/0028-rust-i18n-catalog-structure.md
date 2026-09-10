# ADR-0028: Rust i18n catalog structure

状态：已接受（2026-09-10）

## 背景

Native Rewrite 最初把所有文案放在 `ui.*` 扁平 key 下。随着设置、模型、快捷键和诊断页面
增加，这种命名无法表达页面上下文，也让相同词语在不同语义下被误复用。`rust-i18n` 会在
编译期把嵌套 JSON 展平为查找路径，因此可以同时获得可读的源文件层级和稳定的运行时 key。

## 调研结论

- [Zed](https://github.com/zed-industries/zed) 的桌面 UI 资源按产品域和页面区域组织，状态与
  操作不是一个无意义的 `common` 大对象。
- [Joplin](https://github.com/laurent22/joplin) 的 locale 资源强调稳定语义标识和参数契约，
  复数/参数消息与页面结构分离，便于翻译工具和校验脚本处理。
- [Nextcloud](https://github.com/nextcloud/server/tree/master/core/l10n) 的大型桌面/服务界面
  以功能上下文维护消息，避免把视觉组件名称作为 key；同一文案只有在语义确实相同才复用。
- [KeePassXC](https://github.com/keepassx/keepassx/tree/master/share/translations) 的桌面对话框
  和菜单动作按交互上下文组织，说明按钮、菜单项和错误提示需要稳定的领域归属，而不是泛化
  的 `label`/`text`。

这些项目没有被直接照搬；BongoCat 根据自己的页面和 runtime 状态建立独立目录。

## 决策

locale 文件保留根级 `_version: 1`，所有叶子 key 使用 `snake_case`，并按以下顶层领域组织：

| 领域          | 内容                                            |
| ------------- | ----------------------------------------------- |
| `navigation`  | 设置窗口导航项及页面简介                        |
| `settings`    | 外观、overlay、模型交互、输入、应用和运行时设置 |
| `models`      | catalog、身份、导入流程、行为和校验             |
| `shortcuts`   | 快捷键范围、录入、动作和冲突                    |
| `diagnostics` | runtime、输入指标、配置恢复和导出               |
| `about`       | 产品信息、许可证、Cubism attribution 和隐私     |
| `actions`     | 跨页面且语义稳定的确认、取消、刷新等动作        |
| `status`      | 跨页面生命周期状态                              |
| `errors`      | 按 settings/models/runtime 归属的错误消息       |

页面区域使用 `title`、`description` 等有限且有上下文的字段；实体或状态继续使用具体名称，
例如 `models.identity.source.preset` 和 `settings.overlay.maximum_fps.description`，不使用
无上下文的 `common.name`。需要参数的消息保留 `%{name}` 占位符，所有 locale 的 key、层级和
占位符集合必须完全一致。

Rust 只保存 `UiText` 到稳定路径的映射。动态摘要、错误、快捷键冲突和诊断指标也通过
`bongocat-i18n` 查询，不在 `localization.rs` 中按语言分支写自然语言。

## 验证

`bongocat-i18n` 测试递归展开 JSON 后比较语言 key 集合和占位符集合；`rust-i18n` 的 fallback
继续使用 `en-US`。新增语言必须先复制完整结构，再提交翻译，不得增删字段。
