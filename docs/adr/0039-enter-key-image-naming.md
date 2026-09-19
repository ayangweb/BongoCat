# ADR-0039: 主 Enter 与小键盘 Enter 的键位图命名与旧名兼容

状态：已接受（2026-09-17）
依赖：ADR-0038（左右 Alt 的键位图命名与旧名兼容）、ADR-0037（应用内导入 BongoCatMver 模型）、ADR-0036（模型导入来源识别与 staging 边界）、ADR-0004（可校正输入状态）

## 背景

ADR-0038 处理了左右 Alt 的 `rdev` 旧名，但同一套 `rdev` 词汇表还有第二个问题：主 Enter 键
（HID usage `0x28`）的键位图叫 `Return`。仓库预置模型
（`resources/models/{standard,keyboard}/resources/left-keys/Return.png`）与真实社区模型
（`送葬人 · 标准模式`、`Bongo Cat v0.16/BongoCat - 标准模式`）都带 `Return.png`，而运行时
`key_name_candidates(0x28)` 也查这个名字——两者自洽，但 `Return` 不是 HID 语义的名字。

更实际的缺口在小键盘：小键盘 Enter 是另一个物理键（HID `0x58`，Windows Raw Input 的
make code `0x1c` + E0 扩展位，macOS keycode `76`），`bongocat-platform` 在两个平台上都把它
正确映射为 `0x58`，但 `key_name_candidates` 对 `0x58` 没有任何候选——按下小键盘 Enter 什么都不
画。旧词汇表里也没有这个名字：翻遍本机全部真实样本（送葬人、经典小键盘、Bongo Cat v0.16），
没有任何模型为小键盘 Enter 配图，旧产品对它是完全无感的。

两个决定本 ADR 形态的事实：

1. **`Num` 前缀在产品词汇表里已被数字行占用。** `key_name_candidates(0x1e..=0x27)` 输出
   `Num1..Num0`（主键区数字行），预置模型与社区模型按这个约定画图。小键盘 Enter 不能叫
   `NumEnter`，否则和既有语义冲突。
2. **BongoCatMver 的键位表不区分两个 Enter。** legacy 表用 Windows 虚拟键码 + `GetKeyState`
   查询，`VK_RETURN`（`0x13` 之前的 `0x0D`，教程编号 `13`）对主 Enter 与小键盘 Enter 都为真，
   与 `VK_MENU` 之于左右 Alt 完全同类（ADR-0038 决策 4 的先例）。真实样本
   （`bongo_cat_mver_0.1.6_64`）的 keyboard/standard 绑定不含 `13`，gamepad 的 `13` 是十字键下，
   所以这次改动不影响真实样本的转换产物。

## 决策

### 1. 命名方案

| 物理键 | HID usage | canonical 名 | 说明 |
| --- | --- | --- | --- |
| 主 Enter | `0x28` | `Enter` | 取代 `Return`，与 HID 语义一致 |
| 小键盘 Enter | `0x58` | `KpEnter` | 沿用 Mver 转换词汇表已有的 `Kp` 前缀（`Kp0..Kp9`、`KpMultiply`、`KpPlus`、`KpMinus`、`KpDecimal`、`KpDivide`） |

两个名字分属不同 HID usage，候选列表互不重叠，不存在解析冲突。

### 2. 运行时：精确名优先，`Return` 是第二个旧名遗留

`key_name_candidates` 的候选顺序：

```text
0x28 (主 Enter)   → Enter, Return
0x58 (小键盘 Enter) → KpEnter, Enter
```

- `Enter` 排在 `Return` 之前：新导入与预置模型命中 canonical 名；未经归一化的已安装模型
  落到 `Return.png`，主 Enter 照常画图（与 ADR-0038 保留 `AltGr` 的理由相同——`next` 没有
  就地迁移路径，旧安装只能靠运行时 alias 保持可用）。
- `KpEnter` 排在 `Enter` 之前：模型画了专用图就用专用图；没画就回退主 Enter 的图。回退在
  `key_name_candidates`（资源加载层）实现，不要求模型额外提供图片；模型补了 `KpEnter.png`
  后自动优先。`Return` 不出现在 `0x58` 的候选里——它从来不是小键盘 Enter 的名字。

### 3. 预置模型重命名，冻结快照逐字段同步

`resources/models/{standard,keyboard}/resources/left-keys/` 下 `Return.png → Enter.png`
（`git mv`，字节不变）。`shared/fixtures/model-fixtures/preset-model3-index.json` 同步更新
`id` 与 `file` 两处；条目按 id 字母序移到 `Delete` 与 `Escape` 之间。`spikes/model-package`
解析器的 `preset_model_indices_match_frozen_snapshot` 重跑通过即逐字段相等（顺带清掉了
工作树里未跟踪的 `resources/models/standard/resources/.DS_Store`，与 ADR-0038 记录的是
同一个干扰项）。预置模型不新增 `KpEnter.png`：没有专属 artwork，回退到 `Enter.png` 就是
期望行为。

### 4. 包导入归一化扩展到 `Return`

`key_names::LEGACY_KEY_IMAGE_NAMES` 增加第三项 `("Return", "Enter")`，沿用既有规则：只处理
`resources/{left-,right-}keys` 一层常规文件，canonical 文件已存在时保留它、旧名原样留下，
目录与 `.zip` 两种来源在 store staging 上得到同一结果，用户选中的源永远是只读输入。

### 5. Mver 转换：`VK_RETURN` 展开成两个名字

`legacy_key_names` 对键盘模式的 `0x0D` 返回 `["Enter", "KpEnter"]`（`legacy_key_name` 单值
表从 `Return` 改为 `Enter`，供测试与文档固定词形）；同一张合成图装到两个名字下，运行时按
实际按下的键各自解析。这与 `VK_MENU → AltLeft + AltRight` 的展开完全同类，gamepad 模式
不受影响（`13` 仍是 `DPadDown`）。

`0x10`（Shift）、`0x11`（Control）仍保持家族名不变——理由与 ADR-0038 决策 4 相同：改动会
移动真实样本的转换产物证据，留给单独的改动。

## 明确不做

- **不引入 `NumEnter` 之类第三套拼写**：`Num` 已被数字行占用。
- **不就地升级已安装模型**：既有安装靠运行时的 `Return` alias 保持可用（同 ADR-0038）。
- **不在本次把 `Kp0..Kp9`、`KpMultiply` 等接入运行时候选**：Mver 转换早已输出这些名字，但
  `key_name_candidates` 至今不解析它们（按下的 `0x54..0x63` 无候选、不画图）。这是 ADR-0037
  记录过的同类既有缺口（功能键 `F1..F12` 逐键图不可达同款），仓库内没有模型画过这些键，
  改动无真实数据可验证，记录为后续项。本次只接入小键盘 Enter，因为它有明确的产品需求。
- **不做模型加载期的动态旧名映射**：磁盘上的包应当是产品自己的词汇表（同 ADR-0038）。

## 残余风险与待验证项（不得当作已确认）

1. **`Return` alias 只在单元测试里验证**：没有在真实用户数据根里已安装的 `Return.png` 模型上
   跑过整条链路。
2. **小键盘 Enter 的实机事件未验证**：`0x58` 在两个平台 adapter 上的映射是既有代码（Windows
   `(0x1c, E0)`、macOS keycode `76`，各自有 contract 测试），本次未新增实机按键验证；Windows
   侧同样受既有的交叉编译限制。
3. **社区模型里如果存在名为 `Enter.png` 的图**（理论上不可能，旧词汇表从未用过这个名字），
   归一化会保留它并留下 `Return.png`——运行时优先 `Enter`，行为仍然正确。
4. **Mver 展开依赖真实样本不含 `13`**：一旦将来出现键盘模式绑定了 `13` 的真实 Mver 模型，
   其转换产物会多出一张 `KpEnter.png`（与 `Enter.png` 同字节）。这是行为保持的设计结果，
   不是缺陷，但会改变该样本的产物证据。

## 验证

已完成（2026-09-17，本机 macOS / aarch64）：

- `bongocat-live2d` 47 测试（新增 2）。`enter_keys_resolve_distinct_names_with_a_keypad_fallback`
  固定两组候选（`0x28 → [Enter, Return]`、`0x58 → [KpEnter, Enter]`）与四种模型的命中结果
  （canonical、双图专用、纯旧名 `Return`、空资源）；
  `shipped_keyboard_models_draw_both_enter_keys_from_the_renamed_artwork` 用真实预置模型断言
  `0x28` 与 `0x58` 都命中 `resources/left-keys/Enter.png` 且不再存在 `Return.png`。
- `bongocat-model` 93 测试（新增 1，改写 2）：`key_names` 既有 3 个用例扩展到 `Return →
  Enter`（两侧目录改名、同存时 canonical 优先）；`mver` 的
  `alt_control_codes_never_collapse_left_and_right` 扩展到 `0x0D → [Enter, KpEnter]`（gamepad
  的 `13` 仍是 `DPadDown`）、新增
  `one_shared_enter_binding_installs_the_same_overlay_for_both_keys`（一次 `13` 绑定产出
  `Enter.png` + `KpEnter.png` 且字节相同）。
- `spikes/model-package` 15 测试全绿：冻结快照与重解析产物逐字段相等。
- **真实社区模型验证**（`BONGOCAT_PACKAGE_SAMPLE` 门禁用例，目录与 `.zip` 各跑一遍）：

  | 样本 | 旧名 | 结果 |
  | --- | ---: | --- |
  | `送葬人 · 标准模式`（目录 / zip） | 3 | `Alt.png`、`AltGr.png`、`Return.png` 改名，字节相同，源不被改写 |
  | `Bongo Cat v0.16/BongoCat - 标准模式` | 3 | 同上 |
  | `经典小键盘 · 标准模式.zip` | 0 | 无改名，15 张图逐字节相同 |

- **真实 Mver 样本回归**：`cargo run -p bongocat-model --example model_conversion_smoke --
  --source /Users/ayang/Downloads/bongo_cat_mver_0.1.6_64` 逐模式输出与 ADR-0037 记录完全一致
  （standard 31 文件 / 1 081 672 字节、keyboard 23 文件 / 1 015 987 字节、gamepad 28 文件 /
  1 082 753 字节），确认 `0x0D` 展开与 `Return → Enter` 没有改变真实样本的产物。

**未运行**：Windows 构建与实机按键、真实旧版安装数据根上已安装模型的 `Return` 兼容路径、
任何 UI 实机点击（详见残余风险）。
