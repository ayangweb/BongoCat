# 更新日志书写规范

> **文档性质：规范性约定。** 本文只约束 `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的分段标题
> 怎么写。目标架构仍以 `docs/technical-design.md` 和 `docs/adr/` 为准。
>
> UI 文案的书写规范另见 `docs/localization-copy-conventions.md`；该文 §1.2 明确把
> `CHANGELOG` 排除在自己的适用范围之外，所以两套规范互不覆盖。

适用范围：`CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的**版本条目内**的 `###` 分段标题。
版本标题（`## <version> - <date>`）不受本文约束。

## 1. 分段标题只能取自固定词表

**规则：一个分段标题由一个 emoji 加一个固定文案组成，英文侧和中文侧各自只能从词表里取。**

发布时 `just release-notes` 会把两份 changelog 的同一条目**拼成同一份文档**（GitHub
release 正文和 App 内更新窗口读的是同一个文件），读者会在同一段文字里连续看到中英文。
如果分段标题是临时想出来的，就会出现"中文条目有这一节、英文条目没有"或"同一节两种
叫法"，而读者无从判断这是漏译还是本来就不同。

因此**词表按 emoji 索引**：emoji 是这一节的标识，两侧文案是它的固定写法。

| Emoji | 英文 | 中文 |
| --- | --- | --- |
| ⚠️ | Upgrade Notice | 升级说明 |
| ✨ | Features | 新功能 |
| 🐛 | Bug Fixes | 问题修复 |
| ⚡️ | Performance | 性能优化 |
| 🔐 | Security | 安全 |
| ⬆️ | Dependencies | 依赖更新 |
| 🗑️ | Removals | 移除 |
| 🎨 | Interface | 界面 |
| 🌍 | Localization | 本地化 |
| ⚙️ | Configuration | 配置 |
| 🧪 | Testing | 测试 |
| ♻️ | Refactoring | 重构 |
| 🔧 | Maintenance | 维护 |
| 📝 | Documentation | 文档 |
| ⏪ | Reverts | 回退 |
| 💻 | Support Changes | 支持范围变化 |

不需要某一节就整节省略，不要保留空标题，也不要为了凑齐词表而新增空节。

### 1.1 emoji 的变体选择符不作要求

`⚠️`（U+26A0 U+FE0F）与 `⚠`（U+26A0）在不同编辑器里都会被写出来，U+FE0F 只决定字形
怎么画，不决定它是哪一节。校验比较前会去掉 U+FE0F，所以两种写法都算对；**不要**为了
"统一"去批量改写某一侧的 emoji。

## 1.2 新增一节

需要词表里没有的分段时，**在词表里加一行**，同时给出英文和中文，再在两份 changelog 里使用。
不要只在一侧新增后靠人工翻译——那正是本规范要消除的情况。

### 1.3 下载链接、模型库与赞助不进 changelog

发布说明不只有 changelog 条目。`bongocat-packaging` 给每个语言块**生成**一个
`## Changelog` / `## 更新日志` 标题，并在块末尾生成一节下载链接、一节模型库
（Awesome-BongoCat）和一节赞助（见 §1.4），再与两份 changelog 的条目合成同一份文件。
**不要把这些内容写进 `CHANGELOG.md` 或 `CHANGELOG.zh-CN.md`**：

- 它们的每一项事实都由打包工具已经持有的值生成——产物名来自 `ReleaseTarget::download_asset`，
  下载地址来自 `release_asset_url`，版本号来自同一个 `CARGO_PKG_VERSION`；手写一份就多出一处
  要在每次发版时人工核对的地方。
- 它们每个版本都一样，进 changelog 等于每次发版改两个文件重述一遍不变的文字。
- 词表管的是 changelog **条目内**的 `###` 分段。生成块用的是 `##`，因此**不需要**在词表里
  加行，加了反而会让门禁去校验一份根本不由人写的文案。

要改文案、换赞助或加赞助商，改 `crates/bongocat-packaging/src/main.rs` 里的
`APPENDIX_ENGLISH`、`APPENDIX_CHINESE` 和 `RELEASE_SPONSORS`，两份 changelog 不动。

### 1.4 生成块的形状

`--extract-release-notes` 产出的文件是「`## Changelog` → 条目 → 生成块 → `---` → `## 更新日志`
→ 条目 → 生成块」。每个语言块内，changelog 条目在前、生成块在后：条目通常以「⚠️ 升级说明」
开头，而 2.0.0 这类版本要求先卸载旧版本再装新版本，读者应当先读到这句再点到下载链接。

条目自己只写 `###` 分段，而生成块的三节都是 `##`。所以**条目前面必须由工具补一个
`## Changelog` / `## 更新日志`**：没有它，条目会顶着文档开头，它的 `###` 看上去像是属于
前一个块，而读者分不清哪一半是人写的、哪一半是发版时生成的。这个 `##` 同样**不要**写进
changelog 文件——门禁会把粘进去的写法拦下来。

生成块只用 App 内更新窗口能安全渲染的 Markdown：普通链接和列表项，不含图片和内联 HTML
（`crates/bongocat-ui/src/update_markdown` 会拒绝解释两者）。链接目标一律 `https`，
因为窗口只把 `https` 渲染成可点击控件，其余 scheme 会被降级为纯文本。

### 1.5 怎么强制

`tools/tests/test_release_changelog_contract.py` 的 `ChangelogSectionVocabularyTests`
逐条断言：

- 两侧每个 `###` 标题的 emoji 在词表内，且文案与词表逐字相同；
- 两侧的 emoji 序列**完全一致且顺序相同**（漏译一侧会失败）；
- 词表自身没有重复项。

`ReleaseNotesContractTests` 另外断言两份 changelog 都没有写 §1.3 的生成块标题——有人把它
粘进 changelog 就会失败，因为发布后那几行会各出现两次。

```sh
python3 -m unittest tools.tests.test_release_changelog_contract
```

因此这条规则**不依赖 review 记忆**：写错、漏译或在词表外自造一节都会在门禁上变红。
`python3 -m unittest discover -s tools/tests` 会一并跑到它。

## 2. 版本标题

版本标题是 `## <version> - <date>`。`just release-notes` 按
`[workspace.package].version`（即 `just version` 打印的值）去 changelog 里找对应条目，
找不到就让发布失败。因此发版时必须把 `## Unreleased` 改成 `## <version> - <date>`。
详见 `docs/adr/0033-build-packaging-and-release-toolchain.md`。

## 3. 发布提交：标题固定为 `chore: release v<version>`

**规则：打 tag 的那个 commit，标题必须逐字为 `chore: release v<version>`，其中
`${version}` 就是 tag 去掉 `v` 后的部分。**

```
chore: release v2.0.0
```

理由：

- **历史读起来和发布页一致。** `git log` 里一眼能看出哪个 commit 是发版点，不必先知道
  版本号再去找 tag。
- **版本升级是可检索的。** `git log --grep='^chore: release'` 就能列出所有发版点。
- **它把「发版」和「改代码」分成两类。** 发版 commit 只装版本相关的改动（版本号、
  changelog 标题），功能改动留在各自的 `feat`/`fix` 里。Conventional Commits 的
  `chore` 正是这个用途。

### 3.1 怎么强制

`.github/workflows/release.yml` 的「Check the tag against the single product version
source」步骤在**构建之前**就断言这件事，所以标题写错只需几秒就失败，而不是等三个平台
白跑二十分钟：

```text
::error::the commit tagged v2.0.0 is titled 'fix: 某个功能'; a release commit must be titled 'chore: release v2.0.0'
```

这个断言和同一步里既有的 tag／版本号一致性断言是同一个守卫：只有
`GITHUB_REF_TYPE == 'tag'` 时才生效，因此用 `workflow_dispatch` 手动跑不会误伤。

### 3.2 发版时的动作顺序

```sh
# 1. 版本号只有一个来源，改它：[workspace.package].version
# 2. 两份 changelog 的 `## Unreleased` 改成 `## 2.0.0 - <date>`
# 3. 提交，标题逐字为 `chore: release v2.0.0`
git commit -m "chore: release v2.0.0"
# 4. 打 tag 并推送，流水线按 tag 触发
git tag -a v2.0.0 -m "Release 2.0.0"
git push origin master && git push origin v2.0.0
```

改完第 2 步先本地验一次，避免构建跑完才发现发布说明取不到：

```sh
just release-notes release-notes.md
```

