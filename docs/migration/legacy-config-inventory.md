# Legacy Config Inventory (Reference Only)

状态：历史考古记录；不属于 Native Rewrite 产品兼容范围

日期：2026-08-28

## Current Decision

Native Rewrite 不探测、读取、转换或导入旧 Tauri/Pinia 配置。首次启动在当前 Development 或 Production 数据根中生成全新配置，字段使用 `shared/config/native-config-contract.md` 定义的自有 `snake_case` 命名。

本文件只解释历史行为和已有合成 fixture 的来源。它不能作为生产配置 mapping，`tools/legacy-config-inspector/` 也不能进入应用依赖或发布产物。

## Observed Legacy Storage

macOS 旧版实机数据位于：

```text
~/Library/Application Support/com.ayangweb.BongoCat/
  custom-models/
  tauri-plugin-pinia/
    app.json
    general.json
    cat.json
    model.json
    shortcut.json
    meta.tauristore
```

开发构建还可能生成 `*.dev.json` 和 `meta.dev.tauristore`。Native Rewrite 不访问这些位置，也不需要推断 Windows 上对应旧目录。

历史实现使用 `@tauri-store/pinia 3.7.1` 和 `tauri-plugin-pinia = "3"`：

- 五个 store 独立写入，没有统一事务或 schema version。
- backend 使用 merge/patch；被新版本省略的 key 可能长期残留。
- `model` store 曾持久化或残留 pressed state、能力缓存、动作和表情缓存。
- 稀疏对象、deprecated 顶层字段与新嵌套字段可能同时存在。

这些特征说明旧数据不适合作为新 schema 的隐式输入，也支持了“不兼容旧配置”的当前决策。

## Historical Field Groups

| Store      | Observed historical content                                                                        | Native Rewrite treatment |
| ---------- | -------------------------------------------------------------------------------------------------- | ------------------------ |
| `app`      | app metadata and physical window coordinates                                                       | 不读取                   |
| `general`  | autostart, taskbar/tray visibility, theme, derived dark state, locale and update preference        | 不读取                   |
| `cat`      | model/window settings plus deprecated top-level aliases and migration markers                      | 不读取                   |
| `model`    | model paths/ids, selection, shortcuts, pressed state, supported-key cache, motions and expressions | 不读取                   |
| `shortcut` | serialized global shortcut strings                                                                 | 不读取                   |

用户模型只能通过 Native Rewrite 的显式导入流程进入当前环境。导入器重新校验模型目录和资源，不信任旧配置保存的路径、ID 或能力缓存。

## Retained Archaeology Assets

`shared/config/legacy-pinia/` 保存完全合成的 default、长期升级冲突、自定义模型和损坏 JSON 样本。它们不含复制的用户路径、模型 ID 或快捷键值。

`tools/legacy-config-inspector/` 可只读展示旧 store 的结构风险，测试确保不修改源文件且不回显敏感值。该工具的结果不生成 Native config，也不证明任何升级兼容性。

这些资产在 Phase 0 之后可继续用于理解历史行为，但必须与产品 workspace、启动路径和发布 dependency graph 隔离。
