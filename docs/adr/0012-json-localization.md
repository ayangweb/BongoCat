# ADR-0012: JSON Localization

状态：已接受（2026-09-10）

## 决策

BongoCat 使用 `rust-i18n = 4.2.2` 加载编译期嵌入的 JSON 语言资源。应用层资源由独立的 `bongocat-i18n` crate 管理，默认语言为 `en-US`，已落地的语言为 `zh-CN`、`zh-TW`、`ar-SA`、`vi-VN` 与 `pt-BR`。

**语言命名采用带地区子标签的提名式**，与 `en-US` 一致：一份语言只发一份 catalog，地区子标签提名这份文案所依据的主要变体，并不声称 catalog 未携带的地区特化。`en-US` 本身也是这种名字——它同样服务 `en-GB`。

**解析走 RFC 4647 的 language-subtag fallback**（浏览器 `Intl`、CLDR 与操作系统自身的做法）：
按 primary subtag 匹配已发布 catalog，`ar`/`ar-EG`/`ar_MA`/`ar-SA` 全部落到 `ar-SA`，`vi` 与
`vi-VN` 全部落到 `vi-VN`，`pt`/`pt-PT`/`pt-BR` 全部落到 `pt-BR`，`en`/`en-GB` 全部落到 `en-US`。
因此机器实际上报的 tag 几乎总不是 catalog 自身的名字，而 primary subtag 匹配让它们收敛到同一份
文案，而不是各自回退英文。`pt-BR` 尤其说明这件事的必要性：葡萄牙语没有单一写法，catalog 只发
一种变体，报 `pt-PT` 的机器读到的也是它。语言下拉里的名称按 endonym 规则写成本地人自称语言的名字，
不附地区后缀——`Português` 而不是 `Português (Brasil)`，`العربية` 而不是 `العربية (السعودية)`。

`zh` 是唯一不能只看 primary subtag 的情形：简繁是不同书写体系而非地区变体，因此两种写法各发
一份 catalog（`zh-CN` 简体、`zh-TW` 繁体），并在 `zh` 分支内按书写体系子标签直接分流，不进入
下面的 subtag 循环。`hant` 与 `TW`/`HK`/`MO` 判为繁体，`hans` 与其余 `zh` 标签判为简体——两种
拼写都要覆盖，因为平台只报地区子标签时 `TW`/`HK`/`MO` 是唯一可用的线索。
`bongocat-config` 的 `Language::from_system_locale` 与 `bongocat-i18n::locale_code` 各做一次
同样的判定：两个 crate 之间不存在依赖方向，不能共用一个 helper，行为必须由测试对齐。

语言文件放在 `crates/bongocat-i18n/locales/`，每种语言一个 JSON 文件，使用 `_version: 1`
和真正嵌套的领域结构。`rust-i18n` 在编译期将嵌套路径解析为查找 key；JSON 源文件本身不得使用
点号分隔的扁平 key。Rust UI 在文案实际使用处直接引用稳定的领域路径，不内嵌翻译文本或维护
enum 到 key 的集中映射。

新增一种语言是一次**数据加注册**的改动，不改变本 ADR 的任何结构决定：写一份与 `en-US` 同
key、同占位符的 JSON，然后在 `build.rs` 的 `CATALOGS`、`Language`/`SettingsLanguage` 两个
枚举及其 `code`/`catalog_locale`/`from_system_locale`/`resolve`、`settings_language_display_name`
和 `tools/validate-locales.py` 的 `EXPECTED_LOCALES` 中各加一项。语言下拉里每种语言用自身
文字书写（endonym），而不是当前窗口语言的名字。

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
  语言清单只在 `crates/bongocat-i18n/src/tests/mod.rs` 的 `LOCALES` 一处枚举，各条比较型测试
  都遍历它，因此新增语言不会让某条比较悄悄少覆盖一种语言。
- UI 文案的书写约定见 `docs/localization-copy-conventions.md`。当前已落地的一条：**省略号一律写
  单个 `…`（U+2026，视觉上是三个点）**，禁止中文排版习惯的 `……`（六个点）与拉丁写法的 ASCII
  `...`——同一个 key 由所有语言共用，写法必须与语言无关。该规则由 `tools/validate-locales.py`
  机械强制，不依赖 review。

## 取舍

`gpui-kit` 的底层 `gpui-component` 也使用 `rust-i18n`，因此依赖生态一致。JSON 比 YAML/TOML 更适合现有前端资源、翻译工具和跨语言校验；本方案不引入 Fluent 的复数/选择语法。若后续产品需要复杂 ICU/Fluent 消息，应另行提交 ADR，不在业务代码中混用第二套格式。
