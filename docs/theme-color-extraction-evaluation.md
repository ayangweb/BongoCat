# 主题色代码是否独立成 crate —— 可行性、收益与成本评估

评估日期：2026-09-18（`next` @ b6d9a67 + 主题原生表面改动）
性质：架构评估。**结论是不拆分**，并给出替代方案。本文不含代码改动。

---

## 1. 结论

**不建议新建独立 crate。** 建议改为在 `bongocat-ui` 内部收口成一个 `theme` 模块。

理由可以压缩成三条：

1. **"主题色"在本仓库不是一件事，而是四件事**，它们各自被现有的架构规则钉在不同的 crate 上，
   任何新 crate 都只能承接其中一小块，剩下的仍然散着。
2. **调色板不归我们所有。** 全仓库自有代码里**没有任何颜色字面量**，`Tokens` 的 6 个字段
   全部是对 `gpui_component::Theme` 的字段读取。可拆出去的"颜色代码"实际只有约 25 行。
3. **拆出去会违反 ADR-0020。** 该 ADR 明确限定 GPUI Kit 类型"不向 runtime、config、model 或
   renderer 扩散"，只允许出现在 `bongocat-ui` 与 `bongocat-app` 的窗口入口。一个承载颜色令牌的
   独立 crate 必然依赖 `gpui-kit`，正好是这条边界要阻止的扩散。

真正该修的是**内聚问题**（同一件事在三个地方各写一遍），不是**crate 边界问题**。内聚在
`bongocat-ui` 内就能修完，成本远低于新建 crate，而且将来真要提级成 crate 时路径依然通畅。

---

## 2. 先拆问题：主题色其实是四件事

| # | 关注点 | 现状归属 | 能否独立 | 为什么 |
| --- | --- | --- | --- | --- |
| A | 持久化的偏好（`System/Light/Dark`） | `bongocat-config` | ✗ | 是 `schema_version: 1` 配置契约的一部分（AGENTS §10）。挪走会让 `bongocat-config` 反过来依赖新 crate 才能定义自己的 schema 类型 |
| B | 调色板（亮/暗两套颜色值） | `gpui-component`（第三方） | ✗ | 不在本仓库。项目只消费，不定义 |
| C | 颜色令牌投影（`Tokens`） | `bongocat-ui` | △ | 只有 6 个字段、约 25 行，且类型是 GPUI 的 `Hsla` |
| D | 原生外观桥（标题栏/弹框/菜单/文件框） | `bongocat-platform` | ✗ | AGENTS §5.4 要求平台 API 封装在 `bongocat-platform`；挪出去等于要么重复平台层，要么让新 crate 依赖 `bongocat-platform`，纯属多一层间接 |

四个关注点没有任何一个"应该"搬到新 crate，而把它们塞进同一个 crate 只会造出一个同时依赖
`bongocat-config`、`gpui-kit` 和 `bongocat-platform` 的交叉点。

---

## 3. 现状盘点

### 3.1 代码分布

`theme` / `appearance` 命中行数（`rg` 统计，含测试与注释）：

| crate | 命中 | 位置 |
| --- | --- | --- |
| `bongocat-platform` | 98 | 新增的 `src/theme.rs`（原生外观桥） |
| `bongocat-ui` | 191 | `window.rs` 67、`lib.rs` 26、`update_window.rs`/`tests.rs`/`smoke.rs` 各 21、`view_state.rs` 15、`render.rs` 14、`accessibility.rs` 14、`lifecycle.rs` 8、`window/settings.rs` 7 |
| `bongocat-app` | 52 | `settings.rs` 31（服务与命令）、`lib.rs` 13、`main.rs` 8 |
| `bongocat-config` | 7 | `AppearanceConfig` + `Theme` 枚举 + 默认值 |
| `bongocat-i18n` | 7 | 两个 locale 里的 `settings.appearance.theme.*` 文案 |

整个 Native workspace 的 crate 源码约 33k 行，主题相关约 350 行，占比约 1%。

### 3.2 关键事实

- **配置里没有颜色。** `AppearanceConfig` 只有两个字段：
  ```rust
  pub struct AppearanceConfig {   // crates/bongocat-config/src/lib.rs:177
      pub theme: Theme,           // System | Light | Dark
      pub language: Language,
  }
  ```
  用户能选的是"模式"，不是"颜色"。
- **调色板是第三方的。** `Tokens`（`crates/bongocat-ui/src/window.rs:222-243`）6 个字段
  `canvas/border/text/muted/accent/danger` 全部来自 `cx.theme()`，即
  `gpui_component::Theme`。
- **自有代码零颜色字面量。** 全仓库 `Hsla` / `rgb(` / `rgba(` / 6 位十六进制色值检索结果里，
  与主题相关的只有 `Tokens` 的类型标注；其余命中是 tray PNG 的 `Icon::from_rgba` 和
  `HRESULT(0x80004005)` 之类的错误码。
- **UI 侧偏好类型是刻意的镜像。** `SettingsTheme`（`crates/bongocat-ui/src/lib.rs:376`）与
  `bongocat_config::Theme` 结构完全相同，转换函数写在 `bongocat-app/src/settings.rs:1370`
  与 `:1378`。这不是重复失误：`SettingsLanguage`（`:384`）对 `bongocat_config::Language`
  用了同一套镜像写法，说明 command/snapshot 边界故意携带 UI 自有类型而不是 config 类型。
  因此**不建议**把这两个枚举合并。

### 3.3 依赖关系（现状）

```
bongocat-config ──┐
bongocat-i18n  ──┼──> bongocat-ui ──> bongocat-app
pulldown-cmark ──┤         │
gpui-kit       ──┤         └──(macos/windows)──> bongocat-platform
bongocat-platform┘
```

`bongocat-ui` 的下游只有 `bongocat-app` 一个；`gpui-kit` 只被 `bongocat-ui` 和
`bongocat-app` 依赖。

---

## 4. 逐项评估候选边界

### 4.1 方案一：`bongocat-theme`，收 A + C + D（"一个主题 crate"）

收益：调用方"import 一个 crate 就够"。

成本：

- 依赖方向被打乱。`bongocat-config` 要定义 `Theme` 就得依赖它 → config 依赖 theme，
  而 theme 又需要 config 的 schema 概念 → 要么循环，要么把 schema 所有权搬走，
  违反 AGENTS §10。
- 依赖面变大。它要同时依赖 `gpui-kit`（为了 `Hsla` 和 `Theme`）与 `bongocat-platform`
  （为了 D），而 ADR-0020 明确禁止 GPUI Kit 类型越过窗口入口扩散。
- 与 ADR-0030 §7 冲突：该条要求自研只保留"产品特有语义、可靠性和平台所有权边界所需的适配
  代码"，并且"不得形成第二套业务 runtime"。一个横跨 config/UI/platform 的主题 crate 就是
  第二套中心。

**判定：不可取。**

### 4.2 方案二：`bongocat-theme-color`，只收 C（令牌投影）

收益：几乎没有。25 行、6 个字段、唯一消费者是 `bongocat-ui`。

成本：新增 manifest、`Cargo.lock` 条目、一个编译单元；每次改一个令牌字段要动两个 manifest
加一层 re-export；CI 多一个检查目标。

AGENTS §4 直接写着"不为了目录美观提前创建大量空 crate"。

**判定：不可取。**

### 4.3 方案三：把 D 从 `bongocat-platform` 挪出

`bongocat-platform/src/theme.rs`（351 行）是原生外观桥。它的每个分支都是平台 API 调用
（`NSApplication.setAppearance`、`DwmSetWindowAttribute`、`uxtheme` 序号 135），
AGENTS §5.4 要求这类封装只能待在 `bongocat-platform`。

**判定：不可取。**

### 4.4 方案四：在 `bongocat-ui` 内新增 `theme.rs` 模块（推荐）

把散在 UI 里的决策逻辑收成一个模块：

- `SettingsTheme` → `ThemeMode`（组件库配色）的映射；
- `SettingsTheme` → `Option<bongocat_platform::AppTheme>`（原生外观）的映射；
- `System` → 实际外观的解析；
- 一个幂等的 `apply(theme, window, cx)` 入口，设置窗口与更新窗口共用。

它不新增 crate、不改依赖方向、不越过 ADR-0020 的边界，直接消掉当前"三处各写一遍"的
重复（`window/view_state.rs`、`update_window.rs`、`window.rs` 各有一套同步与幂等判断）。

**判定：推荐。** 这正是本次主题改动本来就要做的事。

---

## 5. 收益对比

| 维度 | 新建 crate | 在 bongocat-ui 内收口 |
| --- | --- | --- |
| 消掉三处重复 | ✅ | ✅ |
| 可独立复用 | ❌（无第二消费者） | ➖（同 crate 内可见） |
| 依赖方向 | 变差（config↔theme） | 不变 |
| ADR-0020 边界 | 违反 | 保持 |
| 新增 manifest / lock 条目 | 2 | 0 |
| 编译单元 | +1 | 0 |
| 改动行数 | 新 crate + 改 3 个 manifest + 全部调用点 | 移动约 120 行，调用点签名不变 |

---

## 6. 对调用方与构建的影响

- **调用方**：`bongocat-app` 只经 `bongocat_ui::SettingsTheme` 与 settings service 交互，
  两种方案下它的代码都不需要改。这也是"拆不拆对收益没差别"的直接证据——没有第二个消费者。
- **构建**：新 crate 会多一个编译单元和一组 CI 检查目标（当前门禁按 crate 分别跑 clippy）。
  收益是 25 行代码的归属更"整齐"，不成比例。
- **测试**：主题逻辑是纯映射，在 `bongocat-ui` 内单测即可，不需要 crate 隔离。

---

## 7. 推荐粒度与迁移步骤

**粒度：`bongocat-ui` 内的一个模块，不是 crate。**

1. 新增 `crates/bongocat-ui/src/theme.rs`，只放三件事：两个映射函数、`System` 解析、
   幂等的 `apply` 入口。
2. `window.rs` 的 `apply_component_theme` / `apply_optimistic_component_theme` /
   `component_theme_mode` 迁入并合并成一个入口。
3. `window/view_state.rs` 与 `update_window.rs` 的两个 `sync_component_theme` 改成调用同一入口，
   幂等判断收敛为一份（由调用方持有"已应用值"，模块只负责应用）。
4. 保持 `SettingsTheme` 作为 UI 边界的公开类型不变。
5. 平台侧的 `bongocat-platform/src/theme.rs` 位置不变。

---

## 8. 什么情况下重新评估

满足任一条时，把 `bongocat-ui/src/theme.rs` 提级为独立 crate 就是合理的：

1. **产品新增用户可见的主题色定制**（自定义主色/强调色、导入主题包）。届时会出现真正属于
   我们的调色板与 schema，`bongocat-config` 需要一个独立的颜色模型，B 和 C 才成为可拆的实体。
2. **出现第二个消费者**：例如 CLI、诊断工具或 overlay 也需要同一套主题解析，而不是只有一个
   `bongocat-app`。
3. **GPUI Kit 被替换或剥离**（ADR-0020 的替换边界被触发）。届时颜色令牌不再绑定 `gpui-kit`
   的类型，crate 化的最大障碍消失。

在此之前，拆分的成本是确定的，收益是想象的。

---

## 9. 参考

- ADR-0020 `gpui-kit-facade.md`：GPUI Kit 类型"不向 runtime、config、model 或 renderer 扩散"
- ADR-0030 `prefer-existing-solutions.md`：复用阶梯；不得形成第二套业务 runtime
- AGENTS.md §4「不为了目录美观提前创建大量空 crate」、§5.4「平台模块」、§10「配置、文件与安全」
- `crates/bongocat-config/src/lib.rs:177,186`
- `crates/bongocat-ui/src/lib.rs:376,384`
- `crates/bongocat-ui/src/window.rs:222`
- `crates/bongocat-app/src/settings.rs:1370,1378`
- `crates/bongocat-platform/src/theme.rs`
