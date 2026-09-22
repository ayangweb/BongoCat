# 本地化文案书写规范

> **文档性质：规范性约定。** 本文只约束 `crates/bongocat-i18n/locales/*.json` 里的 UI 文案
> 怎么写。目标架构、工作顺序与验收门槛仍以
> `docs/BongoCat Native Rewrite Technical Design.md`、
> `docs/BongoCat Native Rewrite Implementation TODO.md` 和 `docs/adr/` 为准；
> catalog 的结构、key 命名与占位符规则见
> `docs/adr/0012-native-json-localization.md`。

适用范围：`crates/bongocat-i18n/locales/` 下的全部语言资源。**新增文案一律遵守**，
不因语言而异。

## 1. 省略号：一律三个点

**规则：省略号写成单个 `…`（U+2026），一个字符，视觉上是三个点。**

| 写法                  | 点数 | 是否允许           |
| --------------------- | ---- | ------------------ |
| `…`（U+2026 ×1）      | 3    | ✅ 唯一允许的写法   |
| `……`（U+2026 ×2）     | 6    | ❌ 中文排版习惯，不采用 |
| `...`（ASCII 句点 ×3） | 3    | ❌ 拉丁写法，不采用 |

理由：

- **同一份 catalog 的同一个 key 由中英文共用**，写法必须与语言无关。`……` 只在中文排版里
  成立、`...` 只在拉丁文里成立，任何一种都会让另一语言看起来是错的。
- `…` 是单个字符、占一个码位，中英文字体都能正确渲染，宽度也稳定；`……` 是两个字符，
  在非中文字体下还可能被拆成两个独立字形。
- 文案里没有需要区分"三个点"和"六个点"的语义，长度差异纯属排版习惯。

示例（`crates/bongocat-i18n/locales/zh-CN.json`）：

```json
{
  "models": {
    "import": {
      "step": {
        "choosing": "正在打开文件选择器…",
        "importing": "正在导入模型中…",
        "capturing": "正在截取封面中…"
      }
    }
  }
}
```

英文侧同样是一个 `…`，不写 `...`：

```json
{
  "models": {
    "import": {
      "step": {
        "choosing": "Opening the file picker…",
        "importing": "Importing model…",
        "capturing": "Capturing cover…"
      }
    }
  }
}
```

### 1.1 怎么强制

`tools/validate-locales.py` 会遍历每个 locale 的每个值：一旦出现**连续两个及以上的 `…`**
或**连续两个及以上的 ASCII `.`**，校验直接失败并报出 key 与实际写法，例如：

```text
error: zh-CN: models.import.step.importing spells an ellipsis as '……'; use a single '…' (U+2026) in every locale
```

因此这条规则**不依赖 review 记忆**：写错就会在门禁上变红。新增/修改文案后本地跑一次
`python3 tools/validate-locales.py` 即可。

### 1.2 例外

- 文档、源码注释、`CHANGELOG`、memory 记录属于自然语言，不受本规范约束。但**引用 UI 文案时
  照抄 catalog 里的写法**，否则文档与界面会不一致。
- 代码里的 `..`、`...`（Rust 语法、路径、范围表达式）不属于文案。
