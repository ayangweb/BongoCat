# ADR-0027: Diagnostics Preview Bundle Boundary

状态：已接受（2026-09-06）；2026-09-24 由 ADR-0064 修订 application event 输入与输出格式

> 后续修订（2026-09-23）：ADR-0054 后设置 UI 不再以 AccessKit 语义作为当前契约；本文的 “AccessKit 语义”验收应理解为当时的历史证据，当前改以可见文案和 typed result 为准。

## 背景

ADR-0016 定义的 `diagnostics.json` 是当前环境内原子写入的匿名摘要。它足以显示
runtime、输入、配置和日志 owner 的聚合状态，但不能让维护者查看有界的应用生命周期 code
历史。直接复制 `logs/` 的所有内容不可接受：Cubism Core 的 message 不能被当作稳定、无用户内容
的记录，文件名、路径和未来日志 owner 也不能隐式进入导出。

## 决策

- 预览包是当前不可变环境 `logs/` 中的固定 `diagnostics-preview.zip`。它是 format version 1 的
  ZIP container，内容路径固定为 `manifest.json`、`diagnostics.json` 和
  `application-events.log`；archive entry 不携带宿主绝对路径、环境根、时间戳、模型 ID 或任意
  user-provided name。
- package writer 只枚举严格的 `application-YYYY-MM-DD.log` 与
  `application-YYYY-MM-DD.<generation>.log` 普通文件，并使用 `bongocat-log` 的共享严格文本 parser
  验证固定宽度 UTC timestamp、level、module、stable code、message 与有界 context 语法。历史 JSONL
  与 `cubism-core-*.log` 不是 application event 来源，也不会触发兼容 reader。
- 每个 code 必须来自 app-owned 闭合 catalog，module 必须与 code 的固定 component 相等，level
  必须与 code 的固定 severity 相等。任一记录未知、损坏、UTF-8/大小/普通文件检查失败时，整份来源
  文件跳过并增加匿名 `skipped_source_files`；不从部分文件复制记录。目录本身无法枚举则整个导出
  失败关闭，不伪装成空 preview。
- 重写后的每条 event 固定为 `<LEVEL padded> [<module>] <code>\n`。输出不包含源 timestamp、固定或
  动态 message、context、原始行、来源路径或模型身份；因此 `application-events.log` 既保留旧版
  component/level/code 的可读性，又不会把日志正文或用户邻近数据复制进 support bundle。
- Cubism Core 原始 `.log` 文件和 Core message 永远不进入预览包。Core 的 retained-file/byte 与
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

- unit tests 覆盖 active/rotated 固定文件名、固定 entry 名称、严格 text record reserialization、
  全部 catalog code、unknown/mismatched module/level/code、malformed timestamp/delimiter、整文件跳过、
  旧 JSONL/Core 排除，以及路径/context/message/时间戳不进入输出。
- transaction tests 覆盖成功原子替换、parse/write/sync/replace failure 清理、已有 preview 保留、
  symlink/non-regular source 拒绝、目录枚举失败和 Development/Production 目录隔离。
- settings/UI contract tests 覆盖 typed result、pending/error/retry，不显示动态 path 或 archive 内容。
  Windows/macOS release smoke 验证 owner-only storage、真实 application event 重写和预览包可由系统
  工具打开，但不需要把原始日志上传或读回产品 runtime。

## 后续边界

本 ADR 不实现 remote upload、Core history export 或 support endpoint。具体 archive writer lifetime
必须在实现提交中记录；在完整验证前，Phase 7 日志导出门禁保持未勾选。

首个 writer 使用 `zip = 8.6.0`（MIT，`zip-rs/zip2`，Rust 1.88+），精确 pin 且关闭默认 feature，
只写标准 `Stored` entries；它不提供 encryption、compression 或 archive extraction。替换边界是同样
能在双平台写出并验证此固定 v1 ZIP entry 集合的维护中 Rust crate，不能改变本 ADR 的隐私/原子性约束。
