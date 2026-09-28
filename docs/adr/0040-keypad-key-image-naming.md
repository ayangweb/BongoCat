# ADR-0040: 小键盘键位图的 `Kp*` 命名与主键盘键位图回退

状态：已接受（2026-09-17）
依赖：ADR-0039（主 Enter 与小键盘 Enter 的键位图命名与旧名兼容）、ADR-0038（左右 Alt 的键位图命名与旧名兼容）、ADR-0037（应用内导入 BongoCatMver 模型）、ADR-0004（可校正输入状态）

## 背景

ADR-0039 把主 Enter（HID `0x28`）与小键盘 Enter（HID `0x58`）分开命名，并把"把 `Kp0..Kp9`、
`KpMultiply` 等接入运行时候选"明确记为后续项。本 ADR 就是那一步。

决定本 ADR 形态的事实：

1. **小键盘是 HID `0x53`…`0x63` 一整块，两个平台 adapter 早已把整块映射正确。**
   Windows 的 `map_scan_code`（非扩展 `0x37/0x47..0x53`、扩展 `(0x1c,E0)`、`(0x35,E0)`）与 macOS 的
   keycode 表（`65/67/69/71/75/76/78/82..92`）都产出这些 usage，各自有 contract 测试。
2. **没有任何模型为小键盘画过专属图。** 预置 `standard`/`keyboard` 的 `left-keys` 不含任何
   `Kp*.png`；ADR-0037/0038/0039 收集的真实样本（送葬人、经典小键盘、Bongo Cat v0.16）同样没有。
   小键盘 Enter 在 ADR-0039 之后靠回退到 `Enter.png` 才画得出来，其余小键盘键什么都不画。
3. **`key_name_candidates` 对小键盘几乎无候选。** 改动前只有 `0x58 → KpEnter, Enter` 一条，
   `0x53..=0x57` 与 `0x59..=0x63` 全部落到 `_ => function_key`（`None`），候选列表为空。
4. **`InputState::model_snapshot` 会丢弃没有 hand 归属的按键。**
   `bongocat-app::input_bindings_for_model` 的手表只有 `0x04..=0x27`、功能键两段、`0x28..0x2c`、
   `0x35/0x38/0x39/0x4c`、`0x4f..=0x52`（右手）和修饰键——小键盘整块一个都没有。因此
   **ADR-0039 给 `0x58` 加的回退在实机上是不可达的**：按下小键盘 Enter 的 `KeyPress` 在
   `hand_for == None` 的 `None => {}` 分支就被丢掉，`resolve_key_overlays` 从未被调用。
   这与功能键逐键图不可达是同一个缺口（已在功能键那一项里记录并闭合），也是本 ADR 必须
   同时改绑定的原因。
5. **`Num` 前缀属于主键区数字行，`Kp` 前缀属于小键盘。** `key_name_candidates(0x1e..=0x27)` 输出
   `Num1..Num0`；Mver 转换的 legacy 表（VK `0x60..0x6F`）输出 `Kp0..Kp9`、`KpMultiply`、`KpPlus`、
   `KpMinus`、`KpDecimal`、`KpDivide`，ADR-0039 决策 1 已把 `KpEnter` 并入这套词汇。

## 决策

### 1. 小键盘与主键盘的对应清单

小键盘中凡是与主键盘产生**同一个字符**的键，都回退到主键盘那一张键位图；其余键没有对应关系。
（ADR-0041 补齐主键盘词表后，`KpMinus`/`KpDecimal` 也拿到了对应键，见下表。）

| 小键盘键 | HID | 主键盘对应键 | 主键盘 HID | 可复用键位图 | 回退候选 |
| --- | --- | --- | --- | --- | --- |
| NumLock | `0x53` | — | — | — | 无 |
| KpDivide `/` | `0x54` | `/` Slash | `0x38` | `Slash.png` | `Slash` |
| KpMultiply `*` | `0x55` | `Shift`+`8`，无独立键 | — | — | 无 |
| KpMinus `-` | `0x56` | `-` Minus | `0x2d` | 尚无模型提供 `Minus.png` | `Minus` |
| KpPlus `+` | `0x57` | `Shift`+`=`，无独立键 | — | — | 无 |
| KpEnter | `0x58` | Enter | `0x28` | `Enter.png` | `Enter` |
| Kp1 … Kp9 | `0x59..=0x61` | 1 … 9 | `0x1e..=0x26` | `Num1.png` … `Num9.png` | `Num1` … `Num9` |
| Kp0 | `0x62` | 0 | `0x27` | `Num0.png` | `Num0` |
| KpDecimal `.` | `0x63` | `.` Dot | `0x37` | 尚无模型提供 `Dot.png` | `Dot` |

HID `0x64`（`Keyboard Non-US \ and |`）不是小键盘键，而是 ISO/ABNT2 的额外键，本 ADR 不处理。

### 2. 候选顺序：精确名在前，主键盘对应键在后

`key_name_candidates` 的候选顺序：

```text
0x53         → NumLock
0x54         → KpDivide, Slash
0x55         → KpMultiply
0x56         → KpMinus, Minus
0x57         → KpPlus
0x58         → KpEnter, Enter
0x59..=0x62  → Kp1 … Kp9, Kp0，再补 Num1 … Num9, Num0
0x63         → KpDecimal, Dot
```

- 精确名在前：模型画了专用 `Kp*.png` 就用专用图，与 ADR-0039 对 `KpEnter` 的规则完全一致。
- 主键盘对应键在后：没有任何已知模型画过小键盘图，所以这条回退就是用户实际看到的东西。
  回退仍在 `key_name_candidates`（资源加载层）实现，不要求模型额外提供图片。模型补了 `Kp*.png`
  后自动优先，但**必须放在 `left-keys`**——`resolve_key_overlays` 只按按下的 side 查同目录资源
  （见决策 3）。
- 没有对应键的三个键只保留精确名。`*` 和 `+` 在主键盘上只能通过 `Shift` 得到（`NumLock` 则完全
  没有对应键），主键区本身没有这两个键的图，因此它们缺图时照旧不绘制。不引入"借用 `Num8` 当
  `*`"这类会误导用户的映射。
- 数字表复用：新增的 `KEYPAD_DIGIT_NAMES`（`Kp1…Kp9, Kp0`）与既有 `KEY_NUMBERS`（`Num1…Num9,
  Num0`）逐下标对齐，一个 `hid_usage - 0x59` 同时索引两张表，两张表不可能漂移。

### 3. 小键盘整块绑到左手

`bongocat-app::input_bindings_for_model` 为键盘模型（`Installed` 或预置 `standard`/`keyboard`）
新增 `0x53..=0x63` → `HandSide::Left`。

- **这一步是必要前提，不是可选优化**：没有 hand 归属的按键在 `InputState::model_snapshot` 就被
  丢弃，回退永远不可达（事实 4）。只加候选不改绑定会得到一段在实机上跑不到的死代码。
- **为什么是左手**：`resolve_key_overlays` 只按按下的 side 查同目录资源，而可复用的
  `Num*.png`/`Enter.png`/`Slash.png` 只存在于 `left-keys`；`standard` 预置甚至没有 `right-keys`
  目录，`keyboard` 的 `right-keys` 只有四张方向键图。绑到右手会让本 ADR 的回退对每一个已知模型
  都不可达。代价是按下小键盘时动的是左爪——这是"显示降级"可接受的近似。
  同一条 side 严格性也意味着**模型为小键盘补图时必须放进 `left-keys`**：放在 `right-keys` 的
  `Kp*.png` 在左手绑定下不可达（见残余风险 2）。
- 不碰 `0x4f..=0x52`：方向键保持右手，`standard` 照旧不绑方向键（与既有契约测试一致）。
- `gamepad` 预置保持按钮专用映射，不绑键盘小键盘。

## 明确不做

- **不给预置模型新增任何 `Kp*.png`**：没有专属 artwork 时回退就是期望行为（同 ADR-0039 决策 3）。
- **不引入文字/字形渲染管线**：渲染器（`bongocat-render`、`bongocat-overlay`）目前只有带纹理的
  四边形，没有字体资源、字形图集或文字 shader。"显示其可读名称"因此落地为"按可读名解析到已有
  键位图"，与 ADR-0039 的 `KpEnter → Enter` 完全同构；真正的屏幕文字需要字体资源加 D3D11/Metal
  两套文字管线，属于独立改动。
- **不改 `resolve_key_overlays` 的 side 严格性**：缺同侧资源时不绘制是既有契约
  （Technical Design §11），不为小键盘放宽成跨侧查找。
- **不处理 HID `0x64`**：ISO/ABNT2 额外键，不是小键盘键。

## 残余风险与待验证项（不得当作已确认）

1. **小键盘的实机事件仍未验证**：两个平台 adapter 的映射是既有代码且有 contract 测试，本次未新增
   实机按键验证；Windows 侧同样受既有的交叉编译限制。
2. **左手绑定是产品近似，并且决定了小键盘美术只能放 `left-keys`**：按下小键盘会让左爪下压；
   放在 `right-keys` 的 `Kp*.png` 在左手绑定下不可达。若后续为小键盘出右手美术，需要同时把绑定
   改到右手（届时回退会失去 `left-keys` 里的 `Num*`/`Enter`/`Slash`），决策 3 需要重写。
   注意 `bongocat-model-store` 的 Mver 转换是按 legacy 的 `lefthand`/`righthand` 列表决定输出目录的
   （`OUTPUT_LEFT_KEYS`/`OUTPUT_RIGHT_KEYS`），因此手写的 legacy 表若把小键盘码放进右手列表，
   产物会落到 `right-keys`；已知真实样本不含小键盘码，所以当前不存在这种产物。
3. **`NumLock`、`KpMultiply`、`KpPlus` 仍然不画任何东西**：主键盘上没有这三个键（`*`、`+` 只能靠
   `Shift` 得到），因此没有可借的图。这不是本 ADR 引入的缺陷，但用户可能期望它们也有反馈。
4. **`0x64` 仍然无候选**：Windows 的 `map_scan_code` 会把扫描码 `0x56` 映射为 `0x64`（ISO/ABNT2
   额外键），该键在本 ADR 之后依旧不绘制，行为与改动前一致。

## 验证

已完成（2026-09-17，本机 Windows / x86_64-pc-windows-msvc）：

- `bongocat-live2d` 48 测试（新增 2）：
  `keypad_keys_name_themselves_and_fall_back_to_their_main_keyboard_twin` 固定整块
  `0x53..=0x63` 的候选列表（含五个无回退键只列精确名）；
  `shipped_keyboard_models_draw_the_keypad_from_the_main_keyboard_artwork` 用真实预置
  `standard`/`keyboard` 断言十二个小键盘键命中 `resources/left-keys/` 的 `Num1..Num9`、`Num0`、
  `Enter`、`Slash`，并断言预置模型确实不含任何 `Kp*` 图、五个无对应键解析为空（不绘制）。
- `bongocat-app` `--lib` 124 测试（新增 1）：
  `keyboard_models_assign_the_whole_keypad_block_to_the_left_hand` 断言三种键盘模型的整块小键盘
  左手绑定，且方向键 `0x4f..=0x52` 在 `Installed`/`keyboard` 上仍是右手，`gamepad` 预置不绑小键盘。
- `just check` 的六道门全过：`cargo fmt --all -- --check`；
  `cargo clippy --locked --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings`；
  `cargo clippy --locked -p bongocat-app --all-targets --features storage-test-injection -- -D warnings`；
  `cargo clippy --locked -p bongocat-app --all-targets --features production -- -D warnings`；
  `cargo test --locked --workspace`；`cargo check --locked --workspace --release`。
  （注意 `cargo clippy --workspace --all-features` 不带 `--exclude bongocat-app` 会因为
  `storage-test-injection` 与 `production` 互斥而失败，这是既有约束，不是本 ADR 引入的。）

**未运行**：Windows 实机按键、macOS 构建与实机按键、UI 实机点击、真实社区模型上的小键盘回归
（无样本画过小键盘图，因此没有可对照的产物证据）。
