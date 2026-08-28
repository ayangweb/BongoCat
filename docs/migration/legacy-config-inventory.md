# Historical Configuration Inventory

状态：macOS 实机与源码完成第一轮；Windows 实机待确认
基线 commit：`44f44bc`
记录日期：2026-08-28

## 已确认位置

macOS 实机存在以下结构：

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

开发构建还可能生成 `*.dev.json` 和 `meta.dev.tauristore`。Windows 的真实落盘路径与大小写必须在 Windows 实机确认，不能只根据库默认值推断。

仓库不保存用户当前配置、绝对路径、模型 id 或快捷键值。迁移 fixture 必须人工匿名化或合成。

`shared/config/legacy-pinia/` 已建立完全合成的 default、长期升级+自定义模型和截断损坏三类 fixture。fixture 中的路径 token 只能在隔离临时目录解析，不得访问真实用户目录。

## 存储机制

- 前端使用 `@tauri-store/pinia 3.7.1`，Rust backend 声明 `tauri-plugin-pinia = "3"`。
- `app`、`general`、`cat`、`model`、`shortcut` 分别写入独立 JSON，没有统一事务或 `schemaVersion`。
- 生产文件位于 `tauri-plugin-pinia/<store>.json`；开发构建使用 `<store>.dev.json`。
- 应用启动时依次执行各 store 的 `$tauri.start()`；插件默认在退出时保存。
- backend 使用 merge/patch 维护对象。被新版本省略的 key 不一定从旧 JSON 删除，因此磁盘文件可能长期保留 deprecated 字段和派生缓存。
- `model` store 当前以默认 `omit` 策略过滤 `supportKeys`、`pressedKeys`。该过滤只影响后续同步；实机升级样本仍包含历史 `supportKeys`，证明迁移器必须主动忽略，而不能以“当前代码已过滤”为依据。

macOS 实机的生产 JSON 都能解析，但部分 store 是稀疏对象，只包含曾写入或历史遗留字段。迁移器必须逐字段读取并应用默认值，不能要求五个文件都具有当前源码的完整形状。

## Store 字段

### app.json

| 字段                                        | 分类          | 迁移建议                               |
| ------------------------------------------- | ------------- | -------------------------------------- |
| `name`, `version`                           | 派生 metadata | 不迁移，从新应用构建信息读取           |
| `windowState.<label>.x/y/width/height/type` | 用户状态      | 转换为版本化窗口布局；验证显示器与 DPI |

### general.json

| 字段                  | 分类          | 迁移建议                         |
| --------------------- | ------------- | -------------------------------- |
| `app.autostart`       | 用户配置      | 迁移，并与系统实际状态 reconcile |
| `app.taskbarVisible`  | 用户配置      | 迁移                             |
| `app.trayVisible`     | 用户配置      | 迁移，但禁止形成无入口状态       |
| `appearance.theme`    | 用户配置      | 迁移 `auto/light/dark`           |
| `appearance.isDark`   | 派生/历史字段 | 不作为权威值                     |
| `appearance.language` | 用户配置      | 迁移支持的 locale                |
| `update.autoCheck`    | 用户配置      | 迁移                             |
| `migrated`            | 历史标记      | 不迁移                           |

### cat.json

| 字段                      | 分类         | 迁移建议                           |
| ------------------------- | ------------ | ---------------------------------- |
| `model.mirror`            | 用户配置     | 迁移                               |
| `model.mouseMirror`       | 用户配置     | 迁移                               |
| `model.motionSound`       | 用户配置     | 迁移                               |
| `model.behavior`          | 用户配置     | 迁移                               |
| `model.autoReleaseDelay`  | 兼容配置     | 迁移为最后保险，不作为正常释放语义 |
| `model.maxFPS`            | 用户配置     | 范围验证后迁移                     |
| `model.ignoreMouse`       | 用户配置     | 迁移                               |
| `model.single`            | 历史运行字段 | 需要确认版本来源和产品语义         |
| `window.visible`          | 用户状态     | 迁移                               |
| `window.passThrough`      | 用户配置     | 迁移                               |
| `window.alwaysOnTop`      | 用户配置     | 迁移                               |
| `window.scale`            | 用户配置     | clamp 后迁移                       |
| `window.opacity`          | 用户配置     | clamp 后迁移                       |
| `window.radius`           | 用户配置     | clamp 后迁移                       |
| `window.hideOnHover`      | 用户配置     | 迁移                               |
| `window.hideOnHoverDelay` | 用户配置     | clamp 后迁移                       |
| `window.keepInScreen`     | 用户配置     | 迁移                               |
| `window.position`         | 历史字段     | 需确认与 windowState 的优先级      |

顶层 `mirrorMode`、`mouseMirror`、`penetrable`、`alwaysOnTop`、`scale`、`opacity`、`singleMode` 和 `visible` 是历史字段。新嵌套字段存在时优先使用新字段；只有新字段缺失时才读取历史字段。`migrated` 不进入新配置。

### shortcut.json

| 字段                | 分类     | 迁移建议                                      |
| ------------------- | -------- | --------------------------------------------- |
| `visibleCat`        | 用户配置 | 解析并重新注册；冲突时保留配置但标记 inactive |
| `visiblePreference` | 用户配置 | 同上                                          |
| `mirrorMode`        | 用户配置 | 同上                                          |
| `penetrable`        | 用户配置 | 同上                                          |
| `alwaysOnTop`       | 用户配置 | 同上                                          |

### model.json

| 字段                                 | 分类          | 迁移建议                                    |
| ------------------------------------ | ------------- | ------------------------------------------- |
| `models[]`                           | 用户配置/索引 | 只迁移用户模型；预置模型由新资源索引生成    |
| `currentModel`                       | 用户选择      | 通过稳定模型 id/hash 重新匹配               |
| `shortcuts`                          | 用户配置      | 迁移 motion/expression 绑定，验证资源仍存在 |
| `pressedKeys`                        | 瞬时状态      | 不迁移                                      |
| `modelReady`                         | 瞬时状态      | 不迁移                                      |
| `supportKeys`                        | 派生缓存      | 不迁移，重新扫描模型资源                    |
| `motions` / `currentMotions`         | 派生缓存      | 不迁移，重新解析 model3                     |
| `expressions` / `currentExpressions` | 派生缓存      | 不迁移，重新解析 model3                     |

## 迁移优先级

```text
new nested field
  -> historical top-level field
  -> validated default
```

迁移不得信任保存的资源 URL、安装目录或能力缓存。所有模型路径必须重新 canonicalize，并验证位于允许目录内。

## 待办

- 已建立 `tools/legacy-config-inspector/` 只读 dry-run spike。它只消费合成 fixture，输出不含路径、模型 id 或快捷键值的稳定诊断；它不是生产迁移器，也不写入用户文件。
- Windows 实机确认 store 目录、文件名和历史安装版本差异。
- 从更早发布 tag 收集不同 schema 样本，补充当前合成 fixture 未覆盖的字段演化。
- 明确 `model.single`、`singleMode`、`window.position` 的历史版本语义。
- 定义新 `schemaVersion: 1` 的完整 JSON schema。
- 在已有截断样本上继续添加越界、未知字段和重复迁移 fixture。
