# ADR-0041: 完整键位词表与绑定覆盖

状态：已接受（2026-09-17）
依赖：ADR-0040（小键盘键位图的 `Kp*` 命名与主键盘键位图回退）、ADR-0039（主 Enter 与小键盘 Enter 的键位图命名与旧名兼容）、ADR-0038（左右 Alt 的键位图命名与旧名兼容）、ADR-0004（可校正输入状态）

## 背景

ADR-0040 把 `Kp0..Kp9`、`KpMultiply` 等接入了运行时候选，但当时的范围只覆盖小键盘，并且判断
依据里混进了"当前没有模型画这些键"这一事实。本 ADR 纠正这个思路：**键位词表是与模型作者的契约，
不是"预置模型当前画了哪些图"的清单。**

决定本 ADR 形态的事实：

1. **词表原本是不完整的。** 改动前 `key_name_candidates` 只命名了字母、数字行、`Enter`、`Escape`、
   `Backspace`、`Tab`、`Space`、`BackQuote`、`Slash`、`CapsLock`、四个方向键、小键盘、修饰键和
   `F1..F24`。以下 usage 没有 arm，落到 `_ => function_key`（`None`）：
   `0x2d` `Minus`、`0x2e` `Equal`、`0x2f` `LeftBracket`、`0x30` `RightBracket`、`0x31` `BackSlash`、
   `0x32`、`0x33` `SemiColon`、`0x34` `Quote`、`0x36` `Comma`、`0x37` `Dot`、`0x46` `PrintScreen`、
   `0x47` `ScrollLock`、`0x48` `Pause`、`0x49` `Insert`、`0x4a` `Home`、`0x4b` `PageUp`、
   `0x4c` `Delete`、`0x4d` `End`、`0x4e` `PageDown`、`0x64`、`0x65` `Apps`、`0x67`。
2. **缺名字的代价不是理论上的。** `0x4c`（`Delete`）在 `input_bindings_for_model` 的左手表里，
   并且 `resources/models/{standard,keyboard}/resources/left-keys/Delete.png` **已随预置模型出厂**——
   但因为没有名字 arm，`key_name_candidates(0x4c)` 返回空候选，**`Delete.png` 从出厂那天起就
   永远画不出来**。逐张核对两个预置模型的 55 张键位图，`Delete.png` 是唯一不可达的一张。
   这与功能键逐键图曾经的缺口（已在功能键那一项闭合）是同一类错误。
3. **名字和绑定必须覆盖同一个集合。** `InputState::model_snapshot` 对 `hand_for == None` 的按键走
   `None => {}` 直接丢弃，`resolve_key_overlays` 从未被调用。所以"有名字、没绑定"和
   "没名字"一样不可达——只补名字不补绑定，等于把词表做成摆设。ADR-0040 已经踩过这个坑
   （`KpEnter` 有了名字却没有 hand 归属）。
4. **范围由平台 adapter 的实际产出界定。** Windows 的 `map_scan_code` 产出
   `0x04..=0x31`、`0x33..=0x45`、`0x46..=0x4e`、`0x4f..=0x64` 与八个修饰键 usage
   `0xe0..=0xe7`（E0/E1 前缀与非扩展码）；macOS 的 keycode 表产出
   `0x04..=0x31`、`0x33..=0x45`、`0x4a..=0x63`、`0x67`、`0x68..=0x6f`、`0xe0..=0xe7`。
   两者并集 = `0x04..=0x65` ∪ `{0x67}` ∪ `0x68..=0x73` ∪ `0xe0..=0xe7`。`0x66`（`Power`）
   两边都不产出。

   > **修订（2026-09-17，同日）：** 本事实原先写的并集漏掉了修饰键 `0xe0..=0xe7`（它们不在
   > `0x04..=0x65`、`0x67`、`0x68..=0x73` 任何一段里），决策 2 的绑定循环照着这个不完整的并集
   > 重写，于是八个修饰键 usage 失去了 hand 归属：`Shift`/`Control`/`Alt`/`Meta` 的按键被
   > `InputState::model_snapshot` 丢弃，模型再也画不出 `ShiftLeft.png`、`AltLeft.png`、
   > `AltRight.png`、`Meta.png` 等修饰键美术，爪子也不再为它们下压——即 ADR-0038 专门为 Alt
   > 图片做过的事被同时作废。修正：绑定循环补回 `0xe0..=0xe7` → 左手，并把并集作为**规范**写进
   > 绑定契约测试（遍历 `0x04..=0x65` ∪ `{0x67}` ∪ `0x68..=0x73` ∪ `0xe0..=0xe7`，断言每个
   > usage 的绑定等于"该模型能画则绑"），不再只遍历实现恰好用到的那几段。测试盲区是本次回归能
   > 溜过 `just check` 的直接原因：命名测试与绑定测试当时都只遍历前三段。
5. **旧版键位映射图给出的名字是词表依据。** 它覆盖标准 104/105 布局 + 小键盘，包含
   `Minus`/`Equal`/`LeftBracket`/`RightBracket`/`BackSlash`/`SemiColon`/`Quote`/`Comma`/`Dot`/
   `PrintScreen`/`ScrollLock`/`Pause`/`Insert`/`Home`/`PageUp`/`Delete`/`End`/`PageDown`/`Apps`
   这些当前代码没有 arm 的名字。图中的空格只是排版（`Back Quote` → `BackQuote`、
   `Kp Divide` → `KpDivide`），实际资源名无空格。

## 决策

### 1. 命名标准布局 + 小键盘的全部按键

`key_name_candidates` 补齐上述所有缺失 usage 的精确名：

```text
0x2d Minus   0x2e Equal   0x2f LeftBracket   0x30 RightBracket   0x31 BackSlash
0x32 IntlHash   0x33 SemiColon   0x34 Quote   0x36 Comma   0x37 Dot
0x46 PrintScreen   0x47 ScrollLock   0x48 Pause
0x49 Insert   0x4a Home   0x4b PageUp   0x4c Delete   0x4d End   0x4e PageDown
0x64 IntlBackslash   0x65 Apps   0x67 KpEqual
```

- `0x32`/`0x64` 是 ISO/ABNT2 的额外键，旧图未命名。沿用 W3C UI Events `KeyboardEvent.code`
  的既有拼写 `IntlHash`/`IntlBackslash`，避免自创词汇；`0x64` 是 Windows `map_scan_code`
  真会产出的 usage，不命名就等于又留一个不可达的键。
- `0x67`（小键盘 `=`）只有 macOS 产出（keycode `81`），按既有 `Kp*` 方案命名 `KpEqual`。
- **命名不以美术是否存在为前提。** 这些键今天都没有图，这是资源事实，不是命名依据。

### 2. 绑定覆盖同一集合

`bongocat-app::input_bindings_for_model` 的键盘模型分支收敛为一条循环：

```text
0x04..=0x65（方向键 0x4f..=0x52 除外）→ 左手
F13..F24（0x68..=0x73）              → 左手
0x67                                 → 左手
0xe0..=0xe7（八个修饰键）             → 左手   ← 2026-09-17 修订补回，见事实 4
```

方向键仍归右手，且仍只对 `Installed`/`keyboard`/`gamepad` 生效（`standard` 预置照旧不绑方向键，
与既有契约测试一致）；`gamepad` 预置仍是按钮专用映射。左爪画键盘区、右爪画方向键，这条分工不变。

> **修订（2026-09-17，同日，见 ADR-0042）：** 本节描述的是**候选** hand 表，它仍然覆盖整块布局。
> 真正写入 runtime 的每模型绑定是该表与"该模型确实有的键位图"的交集：模型没有图的按键不再进入
> `InputState::model_snapshot`，因此既不驱动 `CatParamLeftHandDown`/`CatParamRightHandDown`，
> 也不产生按键图层。残余风险 1 记录的"缺图也有爪部反馈"由 ADR-0042 消除；命名不以美术存在为
> 前提这一条不变——模型补图后绑定自动恢复。

### 3. `PrintScreen` 绑定，但明确不是功能键

`FUNCTION_KEY_USAGES` 的边界（`0x3a..=0x45` 与 `0x68..=0x73`）**不变**，`PrintScreen` 不享受
`Fn` 回退。它只是作为一张普通按键获得名字和 hand——这与"不享受功能键语义"并不矛盾，
原来的 `must not bind PrintScreen` 断言混淆了这两件事，本次改成断言
`function_key_name(0x46) == None`（真正的边界保证）+ `hand_for(0x46) == Some(Left)`（新的绑定事实）。

### 4. 已出厂的 `Delete.png` 恢复可达

`0x4c => Some("Delete")` 让两个预置模型已经出厂的 `Delete.png` 立即生效。这是本次唯一的
"修复既有不可达资源"，不是新增能力。

## 明确不做

- **不为这些键新增任何预置美术**：名字先就位，图由模型决定；预置模型保持现状。
- **不命名平台 adapter 产不出的 usage**：`0x66`（`Power`）两边都不映射，保持无名（有测试断言）。
- **不改 `resolve_key_overlays` 的 side 严格性**：缺同侧资源时不绘制是既有契约。
- **不改 `FUNCTION_KEY_USAGES` 的边界**：`PrintScreen` 依旧不是功能键。

## 残余风险与待验证项（不得当作已确认）

1. **绑定集合扩大是行为变化**：标点键、`PrintScreen`、导航键现在都会让左爪下压（此前完全无反应）。
   这是"名字可用"的必要代价，也让这些键在没有美术时仍有爪部反馈；但它确实改变了既有行为，
   尚未在实机上确认观感。
2. **这些键位图仍然全部不存在**：除 `Delete.png` 外，本次补的名字都不会画出任何东西，
   需要模型补图才有视觉效果。
3. **`IntlHash`/`IntlBackslash` 是本次新增词汇**：旧图未命名，两个平台 adapter 也只有 Windows
   产出 `0x64`、`0x32` 两边都不产出。若将来确认了更合适的旧名，需要按旧名兼容流程处理。
4. **实机按键仍未验证**：两个平台 adapter 的映射是既有代码且有 contract 测试，本次未新增实机验证。

## 验证

已完成（2026-09-17，本机 Windows / x86_64-pc-windows-msvc）：

- `bongocat-live2d` 50 测试（新增 2）：
  `every_key_the_platform_adapters_can_report_has_a_name` 断言 `0x04..=0x65`、`0x67`、
  `0x68..=0x73` 每个 usage 的候选列表非空，并断言 `0x66`（`Power`）保持无名；
  `a_model_providing_a_named_key_image_draws_it` 用合成资源断言 27 个新命名键在模型提供
  对应 PNG 时全部命中该图，并断言只提供出厂词汇的模型对这些键仍然解析为空。
- `bongocat-app` `--lib` 124 测试：
  `keyboard_models_bind_every_named_key_of_the_standard_layout`（改写自原小键盘用例）
  断言三种键盘模型绑定 `0x04..=0x65`（方向键除外）、`0x68..=0x73`、`0x67` 到左手，方向键
  对 `Installed`/`keyboard` 仍是右手，`gamepad` 仍不绑键盘小键盘；功能键那条用例改为断言
  `function_key_name(0x46) == None`（真正的"不是功能键"边界）与 `hand_for(0x46) == Some(Left)`
  （新的绑定事实）。
- `just check` 六道门（fmt、三组 clippy、workspace test、release check）。

**未运行**：Windows/macOS 实机按键、UI 实机点击。
