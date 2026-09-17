# ADR-0038: 左右 Alt 的键位图命名与旧名兼容

状态：已接受（2026-09-17）
依赖：ADR-0037（应用内导入 BongoCatMver 模型）、ADR-0036（模型导入来源识别与 staging 边界）、ADR-0004（可校正输入状态）

## 背景

旧版 BongoCat 用 `rdev` 做输入。`rdev` 把两个 Alt 键分别命名为 `Alt`（左）和 `AltGr`（右），
因此按旧命名导出的模型把左右 Alt 画成了 `resources/left-keys/Alt.png` 与
`resources/left-keys/AltGr.png`，仓库内三个预置模型也是这个名字（`git log` 可见 141 张键位图
的既有布局）。

Native Rewrite 的输入层从 HID usage 出发，左右修饰键是可区分的 canonical 名
（`shared/behavior/input-semantics.md`：`AltLeft`/`AltRight`）。`bongocat-live2d` 因此按
`AltLeft`、`AltRight` 去查资源，而预置模型里既没有 `AltLeft.png`，`AltGr.png` 也不在任何候选
列表里，于是：

| 按键 | 修复前的候选 | 实际命中 | 结果 |
| --- | --- | --- | --- |
| 左 Alt（HID `0xe2`） | `AltLeft`、`Alt` | `Alt.png` | 正确 |
| 右 Alt（HID `0xe6`） | `AltRight`、`Alt` | `Alt.png` | **画成左 Alt 的图**，模型自己的 `AltGr.png` 不可达 |

两个真实事实决定了本 ADR 的形态：

1. **`Alt.png` 与 `AltGr.png` 是两张不同的图**（预置模型中两者 SHA-256 不同，尺寸同为
   612×354），所以右 Alt 命中 `Alt.png` 是可见错误，不是"反正一样"。
2. **BongoCatMver 的键位表不区分左右 Alt。** 上游固定 commit 的教程原图
   （`BongoCatMverUI/Resources/tutorial/img_tutorial_8.jpg`）把左右两个 Alt 都编号为 `18`，而
   C++ 侧用 `GetKeyState(pkeynotecurrent->key)` 查询该码——`VK_MENU` 对任一 Alt 都为真。旧格式
   本身没有"哪一侧"这一位信息。

## 决策

### 1. 预置模型按 canonical 名重命名

`resources/models/{standard,keyboard}/resources/left-keys/` 下：

```text
Alt.png   → AltLeft.png
AltGr.png → AltRight.png
```

改名用 `git mv`，字节不变；`shared/fixtures/model-fixtures/preset-model3-index.json` 是
`spikes/model-package` 解析器产出的冻结快照，同步更新对应 `id` 与 `file`（重跑该解析器逐字段
比对通过）。文件名顺序在 ASCII 下不变（`.` < `L`），因此 `left_keys` 数组次序无需调整。

不新增通用的 `Alt.png`：重命名后两侧各有专属图，家族图对预置模型已无意义。

### 2. 运行时优先精确名，`AltGr` 是唯一的旧名遗留

`bongocat-live2d::key_name_candidates` 的候选顺序：

```text
0xe2 (左 Alt) → AltLeft, Alt
0xe6 (右 Alt) → AltRight, AltGr, Alt
```

`AltGr` 只出现在右 Alt 的候选里，且排在 `AltRight` 之后、家族名 `Alt` 之前，所以：

- 已按新名导出的模型命中自己的图；
- 未经归一化的旧模型右 Alt 命中 `AltGr.png`（正确的右侧图），左 Alt 仍走 `Alt.png`；
- 只有一张共用 `Alt.png` 的模型两侧都还能画出来。

保留 `AltGr` 而不是让未归一化模型"降级为共用图"，是为了让**已经安装过**的模型不出现回退：
导入归一化只作用于新导入，不会回头改写用户数据根里已存在的模型目录，而这些目录在 `next`
上没有任何迁移路径可用（AGENTS.md §4.1）。

`Alt`/`Control`/`Shift`/`Meta` 家族回退保持原样；本 ADR 不改变 `ControlLeft`/`ShiftLeft` 等
既有行为。

### 3. 包导入时归一化，且在 store 自己的 staging 上做

`bongocat-model` 新增 `key_names` 模块：把 staging 树里 `resources/{left-,right-}keys/` 下的
`Alt.png`、`AltGr.png` 改名为 canonical 名。调用点在目录复制与归档解压**之后**、共用的
`commit_installed_staging` **之前**，因此：

- 两种来源（目录、`.zip`）得到同一个结果，不存在"压缩包是第二个解析器"的问题；
- 用户选中的源目录/归档始终只读，与 ADR-0036 的边界一致；
- 改名不改变文件数与字节数，`ModelImportProgress` 的计数仍然真实。

冲突不猜：canonical 文件已存在时保留它，旧名文件原样留下——运行时优先 canonical，模型照常
可用。只处理 `resources/*-keys` 下的一层常规文件；目录同名或其它位置出现 `Alt.png` 交给既有的
包校验按它自己的规则拒绝。

Mver 转换路径**不做**这层归一化：转换输出已经是产品词汇表（ADR-0037 §7），它写出的每个名字都是
从 HID usage 反推出来的，没有"旧名"需要翻译。

### 4. Mver 的 Alt 映射：有侧信息就用侧，没有就两侧都写

`mver::legacy_key_names` 取代原来的单值 `legacy_key_name`：

| 控制码 | 输出 | 依据 |
| --- | --- | --- |
| `0x12` `VK_MENU`（教程图的 `18`） | `AltLeft` + `AltRight` | 旧表把两侧都记为该码，转换把同一张合成图同时装到两侧，既不丢行为也不再用共用的 `Alt` 名 |
| `0xA4` `VK_LMENU` | `AltLeft` | 手写键位表可以点名一侧；与该码在产品 Windows 输入层（`bongocat-platform`）的含义一致 |
| `0xA5` `VK_RMENU` | `AltRight` | 同上 |

一个绑定因此可以产出多于一个目标名；重复目标仍按"先到者胜"去重，且同一次绑定的合成图只做一次
编码（`MverSlotImage` 复用）。

`0x10`（Shift）、`0x11`（Control）与 `0x12` 属于同一类歧义，但**本次不改**：它们保持
`Shift`/`Control` 家族名，运行时会为两侧解析该家族图，且真实样本（`bongo_cat_mver_0.1.6_64`）
的转换结果 `left-keys{Control, KeyR, Shift}` 已被 ADR-0037 记录为验证证据，改动它会同时作废那份
证据。若将来要统一，应作为一次单独改动连同该证据一起更新。

## 明确不做

- **不新增 `Alt` 家族图的兼容 alias 以外的键名**：不引入 `LeftAlt`/`RightAlt` 之类第三套拼写。
- **不就地升级已安装模型**：`next` 不做迁移；既有安装靠运行时的 `AltGr` alias 保持正确。
- **不改 `Control`/`Shift`/`Meta` 的候选顺序与家族回退**（理由见决策 4）。
- **不把归一化做成模型加载期的动态映射**：磁盘上的包应当是产品自己的词汇表，否则每个消费方都要
  再实现一遍旧名识别。

## 残余风险与待验证项（不得当作已确认）

1. **没有真实"绑定 Alt 的 Mver 模型"样本**：上游教程图标明两侧同为 `18`，但仓库内唯一真实样本
   （`bongo_cat_mver_0.1.6_64`）的键位表只有 `17`/`16`（Control/Shift）与字母、方向键，不含 Alt，
   所以 `0xA4`/`0xA5` 与 `0x12` 的展开都只有合成测试覆盖。
2. **只提供一张 `Alt.png` 的模型**：归一化后它变成 `AltLeft.png`，右 Alt 再无专属图，只能靠家族
   回退画同一张。这与旧版 `rdev` 行为不完全相同——旧版右 Alt 报 `AltGr`，模型没有 `AltGr.png`
   时**什么都不画**。选择"两侧都画同一张"而不是"右侧留空"，是因为后者在用户侧看起来是"某个键
   没反应"的新缺口，而前者只影响本来就无法区分左右的老模型。真实样本里存在这类模型
   （`Bongo Cat v0.16/BongoCat - 标准模式` 只有 `Alt.png`）。
3. **`0x12` 展开成两张图**在极端情况下会掩盖旧数据的分歧：若某个 Mver 模型真的为左右 Alt 各配了
   一个绑定（两条 `18`），转换结果只保留先出现的那张图覆盖两侧——旧实现同样只保留一张，但那时
   是共用名，现在两侧共用同一份字节。
4. **Windows 未实机验证**：本次改动全部在 macOS/aarch64 上验证；`0xe2`/`0xe6` 的实际事件来源
   （Raw Input 的 E0 扩展码路径）不在本次改动范围内，也未重新实机复核。
5. **`AltGr` alias 只在单元测试里验证**：没有在真实用户数据根里的旧模型上跑过整条链路。

## 验证

已完成（2026-09-17，本机 macOS / aarch64）：

- `bongocat-live2d` 45 测试（新增 2）。`alt_keys_resolve_their_own_image_and_keep_the_legacy_alias`
  固定三组候选与命中结果（新名、旧名、共用图）；
  `shipped_keyboard_models_draw_both_alt_keys_with_their_own_artwork` 用真实预置模型断言左 Alt 命中
  `resources/left-keys/AltLeft.png`、右 Alt 命中 `.../AltRight.png`、两者字节不同，且预置模型里
  不再存在 `Alt.png`/`AltGr.png`。
- `bongocat-model` 92 测试（新增 6）：`key_names` 3 个（两侧目录改名、冲突保留 canonical、缺目录/
  非 PNG 名不动）、`store` 3 个（目录来源不修改源、归档来源同样生效、两侧同存时 canonical 优先）、
  `mver` 2 个（`0x12`/`0xA4`/`0xA5` 的展开；一次绑定产出两侧同名同图且都落盘）。
- **真实社区模型验证**（新增 env 门禁用例 `imports_the_bongo_cat_sample_named_by_the_environment`，
  由 `BONGOCAT_PACKAGE_SAMPLE` 指定，未设置时直接返回）：逐个跑通本机 `~/Downloads` 与
  `~/Documents/BongoCat` 下的真实模型，目录与 `.zip` 两种来源各跑一遍：

  | 样本 | left-keys | 旧名 | 结果 |
  | --- | ---: | ---: | --- |
  | `送葬人 · 标准模式`（目录 / zip） | 56 | 2 | `Alt.png`→`AltLeft.png`、`AltGr.png`→`AltRight.png`，字节相同 |
  | `经典小键盘 · 标准模式`（目录 / zip） | 15 | 0 | 无改名，15 张图逐字节相同 |
  | `Bongo Cat v0.16/BongoCat - 标准模式` | 50 | 1 | `Alt.png`→`AltLeft.png` |

  用例同时断言：导入后 `resources/{left-,right}-keys` 的**完整文件集合与逐文件字节**等于"源集合
  按规则改名后的期望"，且**重新读取源得到的快照与导入前完全一致**（目录不被改写、归档条目不变）。
- **真实 Mver 样本回归**：`cargo run -p bongocat-model --example model_conversion_smoke -- --source
  /Users/ayang/Downloads/bongo_cat_mver_0.1.6_64` 逐模式输出与 ADR-0037 记录的完全一致
  （`standard` 15 张键位图 / 31 文件 / 1 081 672 字节；`keyboard` 23 文件 / 1 015 987 字节且
  `left-keys{Control, KeyR, Shift}`；`gamepad` 28 文件 / 1 082 753 字节），说明展开改动没有改变
  真实样本的产物。
- 冻结快照：用 `spikes/model-package` 的解析器对仓库预置模型重新生成索引，与更新后的
  `preset-model3-index.json` **逐字段相等**（工作树里未跟踪的 `resources/models/standard/resources/.DS_Store`
  需排除，否则会多出一个 `package_file_count` 与一个 unreferenced 文件）。
- `cargo fmt --all --check`、三组 clippy（workspace `--all-targets --all-features --exclude
  bongocat-app`、`bongocat-app` 的 `storage-test-injection` 与 `production`，均 `-D warnings`）、
  `cargo test --locked --workspace`（全部测试二进制全绿：`bongocat-app` 126、`bongocat-ui` 115、
  `bongocat-model` 92、`bongocat-runtime` 73+2、`bongocat-platform` 50、`bongocat-live2d` 45、
  `bongocat-update` 36+10、`bongocat-render` 15、`bongocat-overlay` 28、`bongocat-packaging` 23、
  `bongocat-config` 51、`bongocat-audio` 10、`bongocat-i18n` 4、`bongocat-log` 3）、
  `cargo check --locked --workspace --release` 均以退出码 0 通过（仅剩与本次无关的
  `block v0.1.6` future-incompatibility 警告）。

**未运行**：Windows 构建与实机按键、真实旧版安装数据根上已安装模型的 `AltGr` 兼容路径、任何 UI
实机点击。
