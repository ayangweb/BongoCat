# ADR-0012: Native JSON Localization

状态：已接受（2026-09-10）

## 决策

Native Rewrite 使用 `rust-i18n = 4.2.2` 加载编译期嵌入的 JSON 语言资源。应用层资源由独立的 `bongocat-i18n` crate 管理，默认语言为 `en-US`，当前首批迁移语言为 `zh-CN`。

语言文件放在 `native/crates/bongocat-i18n/locales/`，每种语言一个 JSON 文件，使用 `_version: 1`
和真正嵌套的领域结构。`rust-i18n` 在编译期将嵌套路径解析为查找 key；JSON 源文件本身不得使用
点号分隔的扁平 key。Rust UI 在文案实际使用处直接引用稳定的领域路径，不内嵌翻译文本或维护
enum 到 key 的集中映射。

## 约束

- 翻译 key 使用小写 `snake_case`，按 `navigation`、`settings`、`models`、`shortcuts`、
  `diagnostics`、`about`、`actions`、`status` 和 `errors` 等领域分层；字段名必须表达具体上下文。
- 插值使用 `rust-i18n` 的 `%{name}` 语法；所有语言必须保持相同的占位符集合。
- 找不到语言或 key 时回退到 `en-US`；`system` 在配置/平台层先解析为受支持语言。
- GPUI 语言切换通过已有带 revision 的设置 snapshot 触发重绘；本地化查询不进入 overlay frame loop。
- 新文案必须先加入 JSON，再由 Rust 使用 key；禁止在 `.rs` 中新增自然语言翻译文本。
- `bongocat-i18n` 是唯一调用 `rust_i18n::i18n!` 的 catalog owner。UI 使用其 `text(locale, key)`
  facade，并在使用点写出 key；不得为 UI crate 再初始化同一份 catalog 或依赖全局 locale。
- 测试/CI 必须检查 JSON 可解析、语言 key 集合相同、值为非空字符串且占位符集合一致。

## 取舍

`gpui-kit` 的底层 `gpui-component` 也使用 `rust-i18n`，因此依赖生态一致。JSON 比 YAML/TOML 更适合现有前端资源、翻译工具和跨语言校验；本方案不引入 Fluent 的复数/选择语法。若后续产品需要复杂 ICU/Fluent 消息，应另行提交 ADR，不在业务代码中混用第二套格式。
