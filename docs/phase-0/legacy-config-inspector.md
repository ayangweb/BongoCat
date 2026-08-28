# Phase 0 Legacy Config Inspector Spike

状态：完成只读历史考古子项；不属于产品配置兼容范围

日期：2026-08-28

## 假设与范围

旧版 Pinia/Tauri 配置由五个独立 JSON store 组成。本 spike 用于确认这些历史输入的结构风险、字段优先级和隐私边界，只读取合成 fixture，不写入用户目录，也不生成 Native Rewrite 配置。

实现位置：`tools/legacy-config-inspector/`

## 可重复运行

在工具目录运行：

```sh
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo check --release --locked
cargo run --locked -- --input ../../shared/config/legacy-pinia/default
```

依赖版本固定在工具自己的 `Cargo.lock`。工具使用 Rust 2024、`serde 1.0.228` 和 `serde_json 1.0.149`，并声明 `#![forbid(unsafe_code)]`。

## 输出契约

CLI 将稳定 JSON 写到 stdout：

- `status` 为 `ready` 或 `blocked`；缺失、不可读、非法 JSON 或非对象 store 会阻塞报告并以退出码 `2` 结束。
- `stores` 按 `app`、`general`、`cat`、`model`、`shortcut` 的固定顺序报告状态。
- `settings` 只包含安全的归一化设置；数值使用 spike 的临时范围检查并以 `value_clamped` 诊断标出。
- `inventory` 只包含窗口、模型和快捷键数量，以及当前模型的 `preset/custom/unknown/none` 类别。
- `diagnostics` 使用稳定 code 和字段名，记录旧字段 fallback/shadow、忽略的派生/瞬时状态、非法输入和自定义模型待验证项。

报告禁止包含输入目录、绝对路径、模型 ID/路径、快捷键值、具体按键名或原始 JSON 片段。源 store 按字节保持不变。

## Fixture 结果

```text
default                         ready
upgraded-with-custom-model     ready; nested fields win; custom model requires validation
damaged (cat.json)             blocked; store_invalid_json
missing store                  blocked; store_missing
```

自动化测试覆盖：默认值、deprecated/新字段优先级、非法枚举 fallback、模型与快捷键计数、损坏 JSON、缺失 store、越界 clamp、10 次重复序列化一致性、隐私不泄漏、源文件不变，以及 CLI 退出码。当前验证结果为 9 个库测试和 2 个 CLI 测试全部通过。

## 非目标与后续

本工具不定义 Native `schema_version`，不处理真实 Windows 路径、不校验模型目录安全性，也不转换或迁移文件。Native Rewrite 不兼容旧配置，因此本工具只保留为隔离的 Phase 0 考古资产，不能接入应用启动、设置或发布依赖。
