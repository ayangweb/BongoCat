# ADR-0050: 转换输出的键位图名必须用产品拼写

状态：已接受（2026-09-19）
依赖：ADR-0037（应用内导入 BongoCatMver 模型）、ADR-0038（左右 Alt 的键位图命名与旧名兼容）、ADR-0039（主 Enter 与小键盘 Enter 的键位图命名与旧名兼容）、ADR-0049（地球键与 `Fn` 的语义分离）

## 背景

ADR-0049 要求"键位事件、键名、资源名称、图片回退、BongoCat 模型导入与 Mver 转换之间的映射完全一致"。
把这句话做成可检验的检查时，发现两张表已经各自演化出了一处拼写漂移。

决定本 ADR 形态的事实：

1. **漂移是程序化 diff 找出来的，不是肉眼看到的。** 正则抽出 `mver::legacy_virtual_key_name` 与
   `live2d::key_name_candidates`（含 `KEY_LETTERS`、`KEY_NUMBERS`、`KEYPAD_DIGIT_NAMES`、
   `bongocat-render::FUNCTION_KEY_NAMES`）里所有 `"名字"` 字面量求差集：**114 个转换输出名里恰好一个
   漏网——`Backslash`。**
2. **漏网的那个是死键。** `mver.rs` 的 `0xDC => Some("Backslash")` 照抄参考工具
   `BongoCat-Converter/src/utils/keyMap.ts` 的 `220: "Backslash"`，而产品词表是
   `0x31 => Some("BackSlash")`。转换安装的 `Backslash.png` 从功能上线起就永远选不中；按 ADR-0042，
   该键**完全不产生动作**——不画图，爪子也不下压。这与 ADR-0038 记录的 `AltGr` 是同一类错误。
3. **只差大小写的改名在大小写不敏感的文件系统上是空操作。** macOS 与 Windows 都不区分路径大小写，
   所以 `Backslash.png` → `BackSlash.png` 的"改名"里 `destination.exists()` 恒为真（目标其实就是同一个
   文件），归一化直接跳过，包保留旧拼写。真正让这类包能画的是**运行时候选别名**，归一化只在大小写
   敏感的文件系统（非首发平台）上才真正改名。
4. **只断言"能解析"的契约测试挡不住这类漂移。** 别名一旦存在，旧拼写也能解析，于是"转换输出能解析"
   这条断言在漂移回退时仍然通过。变异验证证实了这一点：先把 `BackSlash` 改回 `Backslash`，只查
   "能解析"的版本**没有变红**。

## 决策

### 1. 转换输出改产品拼写

`legacy_virtual_key_name` 的 `0xDC` 输出 `BackSlash`。这与 ADR-0038 的 `Alt`/`AltGr` → 产品名、
ADR-0039 的 `Return` → `Enter` 是同一套做法：转换写产品词汇表，旧拼写由运行时候选兜底。

### 2. 运行时候选把旧拼写加成末位别名

```text
0x31 (BackSlash) → BackSlash, Backslash
```

canonical 在前。已经安装过、不会被重新归一化的包（`next` 无迁移路径）因此仍然能画。

### 3. 导入归一化登记该拼写

`LEGACY_KEY_IMAGE_NAMES` 加 `("Backslash", "BackSlash")`。按事实 3，它在两个首发平台上通常是空操作；
登记它的价值是让"旧拼写 → canonical"这件事只有一个来源，而不是散落在运行时别名里。

### 4. 把"转换能安装的名字"做成可遍历契约，并同时断言两件事

- `bongocat_model::legacy_keyboard_key_image_names()`：遍历两种键盘模式的全部虚拟键码，返回去重排序后的
  输出名集合。手柄按钮名**不在**其中（它们是 gamepad 模型的美术名，不进入键位图，ADR-0037）。
- `bongocat-live2d::every_key_image_name_the_conversion_can_install_resolves_to_a_key`：对每个输出名断言
  **既能解析、又是 canonical 名**（即某个可达 usage 的候选列表第一项）。只查前者不够（事实 4）。
  `Shift` 与 `Control` 是**有意**的非 canonical 名（旧表给两侧同一个码，ADR-0038 决策 4），用显式例外
  列表记录，并断言这两个例外仍然必要，避免列表变成死代码。

这样下次漂移会被测试发现，而不是被用户发现。

## 明确不做

- **不把游戏按钮名纳入该契约**：它们不进入键位图解析器（ADR-0037）。
- **不新增第三套拼写**（如 `BackSlashKey`）：ADR-0038 已经明确不做同类事。
- **不把 `Shift`/`Control` 展开成两侧**：那会同时作废 ADR-0037 记录的真实样本转换证据。

## 残余风险与待验证项（不得当作已确认）

1. **已安装的旧包靠别名工作，磁盘上没有被重写**：`Backslash` 是运行时候选表的一部分，属于永久词汇。
2. **例外列表是硬编码的两个名字**：如果将来 `Shift`/`Control` 有了 canonical 拼写，例外断言会先变红
   提醒删除，但删除本身是人工动作。
3. **该漂移没有真实样本覆盖**：仓库里唯一真实 Mver 样本（`bongo_cat_mver_0.1.6_64`）的键位表不含
   `0xDC`，所以这条路径只有合成测试。
4. **大小写不敏感文件系统上的行为只在 macOS 上验证过**：Windows 侧同样是大小写不敏感，但没有实机跑过
   这条归一化。

## 验证

已完成（2026-09-19，本机 macOS / aarch64）：

- **先红后绿**：修复前 `every_key_image_name_the_conversion_can_install_resolves_to_a_key` 变红并精确
  报出 `no key resolves these conversion outputs: ["Backslash"]`；修复后绿。
- **变异验证（关键）**：把 `0xDC` 改回 `Backslash` 后该测试变红（exit 101），证明断言有牙齿；同时确认
  了"只断言能解析"的初版**不会**变红，因此把断言加强为"同时是 canonical 名"。
- `bongocat-model-store` 新增 `the_conversion_emits_the_product_spelling_for_every_key_image`：对
  `BackSlash`/`Backspace`/`Enter`/`AltLeft`/`AltRight` 成对断言"新拼写出现、旧拼写不出现"，并断言输出名
  都是合法资源主干。
- `bongocat-model-store` 新增 `a_case_only_legacy_stem_keeps_its_artwork_under_the_canonical_name`：断言
  canonical 名可读到该美术（**不断言旧名消失**——在大小写不敏感的文件系统上它本来就不会消失）。
- `cargo test --locked -p bongocat-model-store -p bongocat-live2d` 全绿。

**未运行**：真实 Mver 样本回归（样本键位表不含 `0xDC`）、Windows 实机归一化路径。
