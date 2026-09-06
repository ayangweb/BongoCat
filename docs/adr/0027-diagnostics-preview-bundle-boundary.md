# ADR-0027: Diagnostics Preview Bundle Boundary

状态：已接受（2026-09-06）

## 背景

ADR-0016 定义的 `diagnostics.json` 是当前环境内原子写入的匿名摘要。它足以显示
runtime、输入、配置和日志 owner 的聚合状态，但不能让维护者查看有界的应用生命周期 code
历史。直接复制 `logs/` 的所有内容不可接受：Cubism Core 的 message 不能被当作稳定、无用户内容
的记录，文件名、路径和未来日志 owner 也不能隐式进入导出。

## 决策

- 预览包是当前不可变环境 `logs/` 中的固定 `diagnostics-preview.zip`。它是 format version 1 的
  ZIP container，内容路径固定为 `manifest.json`、`diagnostics.json` 和
  `application-events.jsonl`；archive entry 不携带宿主绝对路径、环境根、时间戳、模型 ID 或任意
  user-provided name。
- package writer 只接受已生成的匿名 `diagnostics.json` bytes，以及由 app-owned application log
  parser 严格重新解析、再按固定 `component`、`level`、`code` 字段重新序列化的事件记录。读取失败、
  过大、未知字段、无效 JSON 或超出固定文件/总字节上限的历史日志一律跳过并增加匿名计数；不得把
  原始 bytes、错误文本或来源路径写入 archive。
- Cubism Core 原始 `.jsonl` 文件和 Core message 永远不进入预览包。Core 的 retained-file/byte 与
  written/dropped/rotated/pruned 聚合值继续只由 ADR-0016 的 `diagnostics.json` 表示。未来若要导出
  Core 历史内容，必须先定义可证明的固定结构和新的 ADR，不能复用路径关键词清洗作为授权。
- ZIP 在当前环境 `logs/` 下以私有同目录 staging file 创建，完成所有 entry 的大小、数量和结构验证
  后 `flush`/`sync_all` 并原子替换目标。取消、解析、写入、同步或替换失败必须删除本次 staging，
  保留上一份有效 preview；Unix 目录/文件分别恢复 `0700`/`0600`，Windows 使用当前 profile ACL。
- package format 继续是当前 `next` 首版 v1；在首版发布前字段可直接同步调整 manifest、writer、
  fixture 和测试，不添加任何兼容 reader 或 migration。首版发布后才为不兼容演进建立新的 versioned
  format 与 ADR。
- UI executor 只调用 typed export command。UI 显示 stable result code、format version、entry count
  和 bytes，不直接枚举日志或解压 archive；失败不得显示 OS text、路径、archive entry 或日志正文。

## 验证

- unit tests 覆盖固定 entry 名称、严格 application record reserialization、unknown/oversized/corrupt
  source 跳过、Core message 缺席、路径/model/key/clipboard-like strings 缺席，以及 entry/total size
  上限。
- transaction tests 覆盖成功原子替换、cancel/parse/write/sync/replace failure 清理、已有 preview 保留、
  symlink/non-regular staging 拒绝和 Development/Production 目录隔离。
- settings/UI contract tests 覆盖 typed result、pending/error/retry、AccessKit 语义以及不显示动态
  path 或 archive 内容。Windows/macOS release smoke 验证 owner-only storage 和预览包可由系统工具
  打开，但不需要把原始日志上传或读回产品 runtime。

## 后续边界

本 ADR 不实现 ZIP writer、UI preview、更新 diagnostics、Core history export、remote upload 或 support
endpoint。具体 crate dependency、license/maintenance audit 和 archive writer lifetime 必须在实现提交中
记录；在验证完成前，Phase 7 日志导出门禁保持未勾选。
