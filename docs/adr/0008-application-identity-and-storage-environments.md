# ADR-0008: Application Identity and Isolated Storage Environments

状态：Accepted
日期：2026-08-28

## Context

Native Rewrite 需要稳定的应用身份，同时开发构建不能读取、覆盖或锁住用户的生产数据。旧版配置字段和存储布局不再是新应用的兼容目标，因此新配置可以直接采用清晰、一致的 Rust 领域命名。

仅依赖 `debug_assertions` 或可变命令行参数选择数据目录会让错误构建访问生产数据。只隔离 `config.json` 也不充分：模型、备份、日志、锁和其他可变文件仍可能互相污染。

## Decision

Native Rewrite 的 Bundle ID 固定为：

```text
com.ayangweb.bongo-cat
```

构建产物携带不可变的 `BuildEnvironment`：

```text
Development
Production
```

环境由受控构建入口显式写入构建元数据。应用启动后不能通过 CLI、环境变量或设置项切换环境；测试必须显式注入隔离临时根目录。

持久数据根目录为：

| 平台    | Development                                                         | Production                                                         |
| ------- | ------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Windows | `%APPDATA%\BongoCat\development\`                                   | `%APPDATA%\BongoCat\production\`                                   |
| macOS   | `~/Library/Application Support/com.ayangweb.bongo-cat/development/` | `~/Library/Application Support/com.ayangweb.bongo-cat/production/` |

两个环境使用相同的目录结构和 schema：

```text
<data-root>/
  config.json
  state.json
  models/
  backups/
  logs/
```

锁、单实例命名、诊断和更新 channel 也必须包含环境身份。任何环境都不得探测、读取或回退到另一个环境的目录。

Native Rewrite 不读取或导入旧 Tauri/Pinia 配置。配置 JSON 键统一使用 `snake_case`，名称从当前产品领域语义定义，不保留旧字段 alias。`schema_version` 只用于 Native Rewrite 自身未来版本的顺序演进。

## Consequences

- 首次启动 Native Rewrite 时生成当前环境的全新配置。
- 开发构建可安全使用合成数据和自定义模型，不影响生产安装。
- 旧配置目录和字段只保留为历史行为参考，不进入生产 dependency graph 或发布产物。
- 用户模型仍可通过受验证的显式导入流程加入；不会根据旧配置路径自动发现或搬运。
- 构建和打包任务必须拒绝缺失或未知的环境值。

## Verification

- 对 Windows/macOS path resolver 分别测试 Development 和 Production，断言根目录不同且内部相对结构一致。
- 在两个环境写入不同 sentinel，重启后只读取各自数据。
- 验证锁、日志、备份、模型目录和更新 channel 均无跨环境访问。
- 发布产物验证 Bundle ID 精确等于 `com.ayangweb.bongo-cat`。
- 扫描发布依赖和运行日志，确认没有旧 Tauri/Pinia 配置探测。
