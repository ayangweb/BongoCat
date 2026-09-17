# ADR-0042: 键位图存在才触发按键动作

状态：已接受（2026-09-17）
依赖：ADR-0041（完整键位词表与绑定覆盖）、ADR-0040（小键盘键位图的 `Kp*` 命名与主键盘键位图回退）、ADR-0004（可校正输入状态）

## 背景

ADR-0041 把标点键、`PrintScreen`、导航键和小键盘整块都绑到了左手，并在残余风险 1 里记下了代价：
这些键现在都会让左爪下压，而预置模型对其中大多数键并没有图。用户把它报成了缺陷：
**模型没有对应图片资源时，按下该键不应该产生任何动作。**

决定本 ADR 形态的事实：

1. **一次按键的"动作"是两件事，但只有一个开关。** 按键会驱动
   `ProductParameter::LeftHandDown`/`RightHandDown`（即 Cubism 的 `CatParamLeftHandDown`/
   `CatParamRightHandDown`，见 `bongocat-runtime::apply_model_input`），以及按键图层
   （`resolve_key_overlays`）。两者都由 `InputState::model_snapshot` 里同一个 `hand_for`
   归属决定：有归属的按键才置 hand 标志并成为该侧的候选 press，`None => {}` 的按键直接被丢弃。
   **所以"绑定"就是那个唯一开关**，不需要第二套门禁。
2. **图是模型包自带的，只有渲染侧知道。** `bongocat-live2d::load_key_assets` 扫描
   `resources/left-keys` / `resources/right-keys`，`resolve_key_overlays` 用
   `key_name_candidates` 的候选顺序在已加载的资产里找图；找不到就不画。
3. **预置 `standard` 的 55 张键位图里没有 `Dot.png`、`Minus.png`、`PrintScreen.png`、
   `NumLock.png`、小键盘 `*`/`+`/`-`/`.`**，也没有 `right-keys` 目录。按下 `.`、`,`、`-`
   或 `PrintScreen` 时，用户看到的是"爪子按下去但什么都没出现"——动作指向一个永远画不出来的东西。
4. **判断必须发生在模型与绑定相遇的地方。** runtime 是平台无关 crate，只在 Windows/macOS
   依赖 `bongocat-live2d`，且它的 `model_snapshot` 只接收 `InputBindings`；renderer 只消费不可变
   `RenderSnapshot`，不得决定动作（Technical Design §5.2/§5.3）。而 `bongocat-app` 在模型激活时
   本来就为每个模型计算绑定（`input_bindings_for_model`），手上正好有 `CommittedModel`。
5. **"可绘制"必须与实际绘制同源。** 如果判断用的目录扫描或候选顺序与渲染侧不一致，就会出现
   "以为画得出来却没画"或"画得出来却不响应"。两者必须由同一段代码回答。

## 决策

### 1. `bongocat-live2d` 提供键位图清单

新增 `KeyImageInventory`：

- `read(root)` 扫描 `resources/left-keys` 与 `resources/right-keys`，只取目录下的常规 `.png`，
  文件名主干即资源名，**不解码任何图片**。
- `provides(side, name)` 回答某一手是否提供该名字的图。
- `can_draw(side, hid_usage)` 用 `key_name_candidates` 的候选顺序回答"这个键在这一手能不能画出来"
  ——精确名、家族图（`Fn`、`Control`/`Shift`/`Alt`/`Meta`）、旧名别名（`AltGr`、`Return`）、
  小键盘回退（`Kp1..Kp9`→`Num1..Num9` 等）都算。
- `load_key_assets` 与 `KeyImageInventory::read` 共用同一个私有目录扫描（`key_image_files`），
  所以"清单说有"与"渲染加载到"不可能漂移；这也是唯一的文件规则来源（目录、扩展名大小写不敏感、
  只认目录下的常规文件）。

### 2. 绑定按模型实际资源收窄

`bongocat-app::input_bindings_for_model` 保留 ADR-0041 的静态 hand 表作为**候选集合**（命名与
绑定仍覆盖同一集合，词表仍不以美术存在为前提），但每个键只有在 `can_draw` 为真时才写进
`InputBindings`。也就是说：

```text
写入 runtime 的每模型绑定 = 静态 hand 表 ∩ 该模型的键位图
```

- 模型补上 `Dot.png` 后，`. ` 键在下次激活该模型时自动恢复绑定——仍然不需要改产品代码。
- 判断在 `bongocat-app` 完成，`prepare_model` / `select_model` 两条激活路径都传入
  `CommittedModel::root()` 的清单，因此启动恢复、设置页切换和 overlay 预览走的是同一条规则。
- 判断放在绑定层而不是渲染层：renderer 不得决定动作，runtime 也不应为了解图片资源而依赖
  Cubism/`image` 相关 crate。

### 3. 缺图即无动作

模型没有图的按键不写入绑定，于是 `InputState::model_snapshot` 在 `hand_for == None` 处丢弃它：
既不置 `left_hand_down`/`right_hand_down`（因此不驱动 `CatParamLeftHandDown`/
`CatParamRightHandDown`），也不产生按键图层。**规则对所有缺少图片资源的按键一致**，不区分
"词表新补的名字"和"出厂就有的名字"。

仍然生效的既有行为：

- `Delete.png`（ADR-0041 恢复的可达资源）继续绑定并绘制。
- 小键盘数字继续回退到 `Num1..Num0`，小键盘 Enter 继续回退到 `Enter`，`KpDivide` 继续回退到
  `Slash`。
- 功能键继续优先专属 `F1.png`…`F24.png`，缺失时回退 `Fn.png`。
- 左右修饰键继续使用各自精确名，回退家族图。

### 4. 作用范围是键位图

- **鼠标**不属于本规则：`mouse_left_down`/`mouse_right_down` 驱动的是
  `ParamMouseLeftDown`/`ParamMouseRightDown`，是指针状态而不是键位图，产品词表里也没有它们的
  图片名。
- **手柄按钮**不属于本规则：它们经由 `hand_for_gamepad` 驱动爪子，但键位图解析器没有手柄按钮
  的词表，手柄美术今天本来就不进入按键图层（预置 `gamepad` 的 `South.png` 等只被加载、不被绘制）。

## 明确不做

- **不给预置模型补任何键位图**：缺图就是缺图，本 ADR 只让缺图时不再假装有反馈。
- **不改 `key_name_candidates` 的词表与 `resolve_key_overlays` 的 side 严格性**：命名仍覆盖
  平台 adapter 能产出的全部按键，同侧资源缺失仍不绘制。
- **不把判断搬进 runtime 或 renderer**：runtime 需要图片资源信息就得依赖 Cubism/图片解码；
  renderer 决定动作会破坏"renderer 只消费 `RenderSnapshot`"的边界。
- **不改鼠标与手柄输入**。

## 残余风险与待验证项（不得当作已确认）

1. **只有另一手有图时该键无动作**：side 严格性是既有契约，现在"绑定"与"绘制"在这一点上也一致
   ——模型把小键盘美术放进 `right-keys` 时左手绑定下不可达（同 ADR-0040 决策 3）。
2. **预置 `gamepad` 不再响应任何键盘按键**：它没有键盘键位图（此前也不绘制任何键盘图层），
   键盘输入现在完全不驱动它；手柄按钮映射不受影响。
3. **完全没有键位图的导入模型不再响应键盘**：这类模型此前会让爪子下压但画不出图；现在键盘输入
   对它完全无动作，模型仍由指针、呼吸和眨眼驱动。这是本 ADR 的直接意图，但改变了观感。
4. **`bongocat-overlay` 的预览工具仍持有自己的静态绑定表**（`preview_input_bindings`，停留在
   ADR-0041 之前的形态），不感知键位图：`just preview gamepad` 仍会绑定 `A` 并让左爪下压，
   与产品行为不同。它是人工诊断工具，本次未改；后续如要收敛，应与产品共用同一套绑定构造。
5. **实机观感未验证**：本次只跑自动化契约，未在 Windows/macOS 实机上逐键确认。

## 验证

已完成（2026-09-17，本机 Windows / x86_64-pc-windows-msvc）：

- `bongocat-live2d` 52 测试（新增 2）：
  `key_image_inventory_lists_exactly_the_assets_the_renderer_loads` 对三个预置模型断言清单与
  `load_key_assets` 加载到的 (side, name) 集合逐侧相等（"报告"与"绘制"同源）；
  `a_shipped_model_can_draw_only_the_keys_it_ships_artwork_for` 断言 `standard` 能画 `KeyA`、
  `Delete`、功能键（`Fn`）、小键盘数字（回退 `Num*`）、小键盘 Enter（回退 `Enter`），不能画
  `.`、`-`、`PrintScreen`、`NumLock`、小键盘 `.`、小键盘 `=` 与方向键（含 `right-keys` 不存在），
  并断言 `keyboard` 的右手方向键可画、`gamepad` 连 `KeyA` 都不可画。
- `bongocat-app` `--lib` 125 测试（新增 1，改写 3）：
  `a_key_the_active_model_cannot_draw_never_moves_the_paw` 启动真实渲染应用、激活预置
  `standard`，断言按下 `A`（有 `KeyA.png`）、左 `Shift`（`ShiftLeft.png`）与右 `Meta`
  （无 `MetaRight.png`，回退共享 `Meta.png`）都置 `left_hand_down` 并进入 `key_presses`，而按下
  `.`（无 `Dot.png`）既不置爪也不产生按键 press；
  `keyboard_models_bind_every_drawable_key_of_the_standard_layout` 断言绑定等于"静态表 ∩ 模型
  键位图"，**遍历两个平台 adapter 的产出并集**（`0x04..=0x65` ∪ `{0x67}` ∪ `0x68..=0x73` ∪
  `0xe0..=0xe7`，见 ADR-0041 事实 4 修订），并对 `KeyA`/`Num1`/`Delete`/小键盘 1/小键盘 Enter/
  左右 `Shift`/`Alt`/`Control`/`Meta` 为真、`.`/`PrintScreen`/`NumLock`/小键盘 `.` 为假做逐条
  抽查；功能键用例改为断言 `PrintScreen` 不再绑定（无图），
  `installed_models_get_default_keyboard_bindings` 断言 `gamepad` 预置不绑任何键盘键、手柄按钮
  映射不变。
- `just check` 的六道门全过：`cargo fmt --all -- --check`；
  `cargo clippy --locked --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings`；
  `cargo clippy --locked -p bongocat-app --all-targets --features storage-test-injection -- -D warnings`；
  `cargo clippy --locked -p bongocat-app --all-targets --features production -- -D warnings`；
  `cargo test --locked --workspace`；`cargo check --locked --workspace --release`。

**未运行**：Windows/macOS 实机按键与观感确认、UI 实机点击、真实社区模型回归。
