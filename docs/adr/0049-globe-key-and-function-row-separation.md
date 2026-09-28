# ADR-0049: 地球键与 `Fn` 的语义分离

状态：已接受（2026-09-19）
依赖：ADR-0041（完整键位词表与绑定覆盖）、ADR-0042（键位图存在才触发按键动作）、ADR-0038（左右 Alt 的键位图命名与旧名兼容）、ADR-0037（应用内导入 BongoCatMver 模型）

## 背景

需求是"F1–F24 与 macOS 左下角地球键都要完整支持，且不能共用一张图"。考古后有一处前提需要修正：
**今天并不存在共用，`Fn` 也从来不是地球键的名字。**

决定本 ADR 形态的事实：

1. **`Fn` 是"F 编号通配符"，不是 Fn 键。** 唯一产生点是旧版
   `pre-refactor:src/composables/useDevice.ts:105-110`：

   ```js
   if (key.startsWith('F') && unsupportedKey) {
     nextKey = key.replace(/F(\d+)/, 'Fn')
   }
   ```

   只对"模型没逐键画图"的 `F<数字>` 生效。两个预置键盘模型出厂的
   `resources/models/{standard,keyboard}/resources/left-keys/Fn.png`（各自 55 张键位图之一）
   就是这张共享图。物理 Fn 键在 `rdev` 里叫 `Function`，`F` 后没有数字，命不中该正则。

2. **旧版地球键的名字是 `Function`，而且它是可达的。** `rdev` 的 macOS 表
   `const FUNCTION: CGKeyCode = 63;` → `Key::Function`（上游 `rdev/src/macos/keycodes.rs:36,274`），
   `pre-refactor:src-tauri/src/core/device.rs` 用 `format!("{:?}", key)` 取名；旧版 model store 的键集
   就是模型包自己的文件名主干（`pre-refactor:src/pages/main/index.vue:82`），所以一个提供
   `Function.png` 的模型在旧版里确实能画出地球键——只是已知的预置与社区模型都没有这张图。

3. **地球键在 BongoCat 里完全不可达。** `bongocat-platform/src/macos.rs` 的 `map_key_code`
   没有 keycode `63` 的 arm，而 `FlagsChanged` 分支先查表再解码（同文件 `1636-1641`），查不到就计入
   `unmapped_keys` 丢弃。缺的只是这一个 arm：`ModifierDecoder` 早就认识 63（`modifier_family_bit`
   里有 `63 => MaskSecondaryFn`）。

4. **`Fn` 不能同时是两个键的名字。** 若地球键取名 `Fn`，旧包里的 `Fn.png`（功能键共享图）与新地球键
   美术就同名，导入归一化无法区分二者；而 `next` 不做迁移（AGENTS.md §4.1），用户数据根里已安装的
   模型不会被重新归一化。结果是按 F3 画出地球图、按地球键画出功能键图——双向回归，且没有能同时满足
   两边的归一化规则。这不是取舍，是方案本身不成立。

5. **地球键不在 HID Keyboard/Keypad 页。** 它是 Apple 厂商页 `0xFF` 的 usage `0x03`（`KeyboardFn`）。
   任何 `0x04..=0xe7` 式区间都覆盖不到它，所以命名、绑定、两处契约并集都必须显式写出。

## 决策

### 1. 地球键 = `Globe`

`bongocat-render` 新增 `GLOBE_KEY_USAGE: u16 = 0xff03`（`0xff00 | 0x03` 的折叠值）作为六处共用的
唯一来源；`bongocat-runtime::PhysicalKey::GLOBE` 就是它，`bongocat-platform` 的 keycode 表也引用它
而不是重写数值。

不叫 `Fn`：见事实 4。不叫 `GlobeKey`：词表里没有 `*Key` 后缀，而 `KeyA`…`KeyZ` 是 W3C
`KeyboardEvent.code` 的既有拼写，加后缀会被读成同一族。`Function` 是旧名，但它与功能键回退名 `Fn`
只差两个字符而意思不同——这个仓库已经被 `Backslash`/`BackSlash` 这类近似名坑过一次（ADR-0050），
词表里不该再放一对。

### 2. `Fn` 保持原义，`Fn.png` 不改名、不迁移

`Fn` 是 F1–F24 的共享回退图名，语义本来就正确。改名会把每个现存模型的功能键美术挪到地球键上，
是对全部现存模型的行为回归，也与"导入要兼容旧图片资源"直接冲突。

地球键与功能键的候选列表**完全不相交**：

```text
F1 … F24 (0x3a..=0x45, 0x68..=0x73) → F1 … F24, Fn
0xff03   (Globe)                    → Globe, Function
```

模型只提供 `Fn.png` 时地球键画不出来；只提供 `Globe.png` 时功能键画不出来。

### 3. 地球键的候选与旧名兼容

`Function` 是旧版对同一物理键的可达名，排在最具体名之后。导入归一化把
`resources/{left-,right-}keys/Function.png` 改名为 `Globe.png`；运行时的 `Function` 候选保证
**已经安装过**的包（不会被重新归一化）仍然能画。这与 ADR-0038 对 `AltGr` 的处理同构。

### 4. macOS 映射 keycode 63

`map_key_code` 加 `63 => PhysicalKey::GLOBE.hid_usage()`。Windows 不加：Fn 键由键盘固件处理，
Raw Input 从不报告它，没有可映射的码。

### 5. Mver 转换补齐 F13–F24

`legacy_virtual_key_name` 加 `0x7C..=0x87` → `F13`…`F24`。`VK_F13`…`VK_F24` = `124`…`135` 是
`windows` crate `Win32/UI/Input/KeyboardAndMouse` 里的连续常量（可离线交叉验证），不是猜的硬件表。
地球键在旧版码空间里不存在，转换永不产出 `Globe.png`。

## 明确不做

- **不重命名 `Fn.png`**，理由见决策 2。
- **不为地球键新增任何预置美术**：两个预置键盘模型与 `gamepad` 都保持现状，按下地球键不产生任何动作
  （ADR-0042 的缺图规则）。
- **不加 Windows 映射**：Fn 由固件处理，Raw Input 不报告。
- **不猜 Windows 的 F13–F24 扫描码**：仓库此前已在 TODO `P4-MODEL-LEGACY-SOURCE` 判定"缺可依据的
  扫描码，不猜值"，本次沿用；macOS 侧 Carbon 没有 `kVK_F21`…`kVK_F24`。
- **不改 `FUNCTION_KEY_USAGES` 的边界**：`Fn` 回退仍只覆盖 `0x3a..=0x45` 与 `0x68..=0x73`。

## 残余风险与待验证项（不得当作已确认）

1. **F21–F24 是"命名到位但两个平台都不可达"**：Windows 扫描码不补（见上），macOS 没有对应 keycode。
   词表保留它们是为了让模型可以先画图，不是声称平台能报。
2. **macOS 实机地球键未验证**：`CGEventSourceKeyState(63)` 的周期校正是否会误释放地球键，只有合成事件
   证据（见验证），真实 HID 状态没测过。
3. **`Function.png` 兼容路径只有单元测试覆盖**：没有真实用户数据根里的旧模型样本，`Function.png` 在
   野外是否存在也不确定（它从未随任何已知模型出厂）。
4. **`Fn` 这个名字仍然容易误读**：它读起来像 Fn 键。本 ADR 选择不动它（改名会回归现存模型），代价是
   文档必须反复强调；`bongocat-live2d::key_name_candidates` 的文档注释已写明这一点。
5. **地球键归左手**：与其余键盘区一致，但爪部落点没有实机观感确认。
6. **`bongocat-overlay` 的预览工具不认地球键**（`preview_input_bindings`，
   `macos/switch_preview.rs` / `windows/switch_preview.rs`）。该表停留在 ADR-0041 之前的形态，
   连 F1–F12 都没有，也不感知键位图，
   ADR-0042 残余风险 4 已记录它是人工诊断工具、本次未收敛。`just preview` 因此不覆盖地球键。

## 验证

已完成（2026-09-19，本机 macOS / aarch64）：

- `bongocat-live2d` 55 测试（新增 2）：
  `the_globe_key_never_shares_an_image_with_the_function_row` 断言 `0xff03` 的候选恰为
  `["Globe", "Function"]`、F1–F24 的候选不含 `Globe`/`Function`，并用合成资源断言 `Fn.png` 画不出
  地球键、`Globe.png` 与 `Function.png` 都画不出功能键；
  `every_key_image_name_the_conversion_can_install_resolves_to_a_key`（见 ADR-0050）。
- `bongocat-platform` 62 测试（新增 1）：
  `this_adapter_reports_f1_through_f20_and_the_globe_key_only` 遍历全部 `u16` keycode，断言可达功能键
  恰为 `0x3a..=0x45` ∪ `0x68..=0x6f`（F1–F20），且 `0xff03` 是唯一不在 `0x07` 页的产出；
  `mac_key_codes_map_to_usb_hid_usages` 补 `map_key_code(63) == Some(PhysicalKey::GLOBE)`。
- `bongocat-runtime` 74 测试（新增 1）：
  `the_globe_key_projects_through_the_model_snapshot_like_any_other_key` 走完
  input → binding → `model_snapshot`，证明这条路径不按下标索引 usage、不窄化成 `u8`，且未绑定时完全惰性。
- `bongocat-app` `--lib`（新增 1）：`the_globe_key_binds_only_for_a_model_that_ships_its_image` 断言
  四个预置/已安装模型都不绑地球键，而提供 `Globe.png` 或 `Function.png` 的模型绑到左手。
- `bongocat-model`（新增 2）：`the_globe_key_image_is_renamed_from_its_pre_rename_stem` 断言
  `Function.png` → `Globe.png` 且 `Fn.png` 原样保留。
- **变异验证**：6 处改动逐个还原（删命名 arm、删 keycode arm、删归一化项、把 `BackSlash` 改回旧拼写、
  删 F13 arm、删绑定语句），对应测试全部变红（exit 101），随后逐个还原并复核文件内容一致。

**未运行**：macOS 实机地球键与爪部观感、Windows 实机、真实社区模型回归、UI 实机点击。
