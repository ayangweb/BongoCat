# ADR-0016: Anonymous Diagnostics Export Contract

状态：已接受（2026-09-01）

## 背景

Diagnostics 页面已经聚合 runtime、输入、配置和模型目录状态，但用户和维护者需要一个可预览
的文件来复现问题。直接打包原始日志、模型目录或输入事件会泄漏用户路径、模型身份和按键内容，
也会让导出格式依赖 Rust `Debug` 文本而无法稳定解析。

## 决策

- 通过 settings service 的 `ExportDiagnostics` 强类型 command 生成 format version 2 JSON。
- 导出目标固定为当前不可变构建环境的 `logs/diagnostics.json`，使用同目录原子写入；UI executor
  不执行文件 I/O。
- 文档只包含 runtime/input/configuration 的稳定 code、匿名计数、模型来源计数和
  settings/config revision。模型 ID、路径、原始按键或事件流、原始配置、时间戳和动态 I/O 文本
  永远不进入导出。
- 导出成功状态只回传格式版本和字节数，作为 UI snapshot 的非配置 revision；失败返回固定的
  `diagnostics_export_failed`，不暴露操作系统文本。
- Diagnostics 页面提供键盘和 AccessKit 可访问的导出操作；“无报告”与成功字节数属于显示状态，
  不影响配置 revision。
- 导出额外包含 app-owned 日志 writer 的匿名聚合统计（written、dropped、rotated、pruned、bytes
  和 retained_files），但不读取或复制任何日志正文。
- format version 2 仅以追加字段扩展版本 1 的匿名结构；消费者必须拒绝未知版本，不能猜测或
  忽略不兼容的导出格式。

## 后续边界

应用级日志 writer 的实现与有界清理已记录在 ADR-0017；跨域历史日志合并、预览器和更新系统的
诊断 manifest 仍需单独设计。
实现不得为了导出而放宽日志隐私、环境隔离或原子写入约束。
