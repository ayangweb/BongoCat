# ADR-0082: 手柄摇杆显示参数与多手柄轴投影

状态：已接受（2026-10-04）

## 背景

维护者报告手柄模式仍有多处行为不对，要求对照重构前 Tauri 版本与 BongoCat-Mver 复核
按键图片、摇杆、扳机和整体行为。复核结果是三处产品侧缺口，加一处第三方 backend 缺口。

**1. `CatParamStickShowLeftHand` / `CatParamStickShowRightHand` 从未被写入。**
`bongocat-live2d::ProductParameter` 声明了这两个参数，预置 `gamepad` 模型和 Mver 转换出的
手柄模型都带它们（前者 `demomodel3.cdi3.json` 里就叫「显示摇杆左手」/「显示摇杆右手」），
但 `apply_model_input` 的参数表里没有它们。重构前的 `useGamepad.ts` 明确写这两个参数：

```ts
watch(sticks.left, ({ x, y, moved, pressed }) => {
  sticks.left.moved = x !== 0 || y !== 0
  live2d.setParameterValue('CatParamStickShowLeftHand', moved || pressed)
}, { deep: true })
```

结果：手柄模式下两个模拟摇杆的美术永远不出现。`CatParamStickLX/LY/RX/RY` 有写，所以摇杆
的**位置**参数是对的，只是可见性参数没人写——这正是「参数在、效果不在」的典型形态。

**2. 摇杆在动时爪子不落。** 同一段旧代码还通过 `stickActive` 参与爪子判定：

```ts
handleKeyChange(true, stickActive.left || hasLeft)
handleKeyChange(false, stickActive.right || hasRight)
```

`stickActive` 就是 `moved || pressed`。也就是说旧版里推动左摇杆或按下 L3 会让左爪落下。
当前实现只有按键/按钮的 hand 归属能让爪子落下，摇杆完全不参与。

**3. 多手柄时第二个手柄不动。** `GamepadAxisValues::project` 先用
`.keys().filter(connected).map(connection).min()` 选**一个** connection，再只投影它的六个
轴值。重构前没有 device 概念，所有手柄的同名轴写进同一份状态，最新一次写入获胜。两个手柄
同时连接时，先连接的那个即使完全不动也一直占着全部六个轴，用户后来插上的手柄推动摇杆、
扣扳机都不产生任何变化。

**4. Windows WGI 的默认 mapping 把扳机和右摇杆 Y 轴对调（第三方 backend，见 ADR-0066）。**
这条不在本仓库修复，单独记录在 ADR-0066 的「已知阻塞」里。

## 决策

- **`CatParamStickShowLeftHand` / `CatParamStickShowRightHand` 由 runtime 写入，条件是
  `displaced || pressed`。** 两个值由 `ModelInputSnapshot` 已经带的 `stick_*_down` 与
  `stick_*_x`/`stick_*_y` 现算，不新增快照字段：多一份副本就多一处可能与轴值不一致的
  地方。条件取的是**死区之后**的轴值，和重构前一致（旧版读的是 `gilrs` 自带 jitter/deadzone
  过滤之后的值，当前版把死区放在 runtime 投影阶段，语义位置相同），所以静止的摇杆不会因为
  硬件噪声而闪现。
- **爪子跟随摇杆，但只在模型自己声明了摇杆时跟随。** 门禁是模型参数表里查
  `StickShowLeftHand`/`StickShowRightHand` 是否存在，即 `set_normalized_parameter` 报
  `Unsupported` 的同一个答案。这把 ADR-0042「缺图不动爪」从按键图片推广到摇杆美术：一个
  没有摇杆的模型上，推动摇杆不会让爪子为一件永远不出现的东西落下。预置 `standard` /
  `keyboard` 都没有这两个参数，因此和重构前「只在 gamepad 模式下监听手柄」的结果一致。
- **摇杆的三个事实仍然互相独立。** `StickLeftDown`/`StickRightDown` 只表示摇杆键被按下，
  `Stick*L*`/`Stick*R*` 只表示位置，可见性参数只表示「这一侧的手柄正在被使用」。旧版
  `CatParamStickLeftDown = value !== 0` 也只跟着摇杆键走。
- **每个轴由最新的样本获胜，而不是由某一个 connection。** 投影遍历所有仍被 runtime 接受
  的 connection 的样本，逐轴取 `at` 最大的那个，再对它应用死区。这与重构前「同名轴最新
  一次写入获胜」逐字一致，也让「摇杆回到中心」读到 0 而不是停在最后一个非零值。先选
  connection 再取值的旧写法在单手柄下不变（只有一个 connection 能赢）。
- **不在本仓库为 WGI 默认 mapping 兜底。** 那个映射表属于 `ayangweb/gilrs`，本仓库补偿
  它就要在 adapter 里复制一份第三方位置表，并在上游修好之后静默变成双重修正。ADR-0066
  记录了证据与需要的修法。

## 后果

- 手柄模式下推动任一摇杆或按下 L3/R3，该侧的摇杆美术出现，并且该侧的爪子落下；松开或回中
  后同一帧消失并抬起。预置 `gamepad` 模型与 Mver 转换出的手柄模型立刻可见地正确了。
- 键盘/鼠标模型不受影响：它们没有摇杆参数，摇杆既不画也不动爪。
- 两个手柄交替使用时，后用的那个手柄的摇杆和扳机立刻生效；同一轴上最后一次移动的那只手
  柄获胜，这与重构前合并所有手柄的行为一致。多手柄仍然只有一副爪子，这是模型的限制。
- `ModelInputSnapshot` 的形状不变，`schema_version` 仍是 1，没有配置或持久化格式变化。
- 未变的行为：缺图的按钮保持惰性；`model.ignore_gamepad` 把六个轴投影为零并清掉摇杆与
  手柄的爪子贡献；扳机仍然在 `>= 0.5` 产生按钮边沿，其连续值仍然只作为轴运输。
- **仍然存在的空白，且不由本 ADR 关闭**：预置 `gamepad` 模型没有 `Select.png`、`Start.png`
  和两个摇杆键的图，因此这四个按钮不画图也不落爪（L3/R3 仍驱动 `*Stick*Down`）。这是美术
  缺失而不是映射缺失——ADR-0042 规定缺图按钮保持惰性，凭空生成美术会越过模型作者的
  contract。Mver 转换出的手柄模型不受此限：legacy config 的 XInput 序号表覆盖 0–15，
  转换会为全部十六个按钮生成图并按 `lefthand`/`righthand` 落盘。预置模型的美术补齐是独立的
  资源任务。

## 验证

- `bongocat-runtime`：`every_gamepad_axis_reaches_its_own_model_input_field` 走真实 transport
  （`GamepadAxisProducer` 样本 + `InputProducer` 边沿），逐轴断言六个 `ModelInputSnapshot` 字段
  各自承载自己的样本、且**该次发布没有改动其余五个**——latest-value 槽位会保留上一次的值，
  所以断言的是「只动了自己」而不是「其余为零」。
- `bongocat-runtime`：`every_stick_control_reaches_its_own_parameter_and_nothing_else` 在真实预置
  `gamepad` 模型上逐个控制（六轴 + 两个摇杆键 + 双摇杆同时 + 双扳机）断言**全部十个**摇杆与爪子
  参数，每次只允许一个（该侧的 XY / `*Down` / `StickShow*` / 爪子）变化。双扳机那一行断言它不
  触碰任何摇杆参数。
- `bongocat-runtime`：`a_displaced_stick_shows_and_presses_its_paw_only_where_the_model_has_a_stick`
  在真实预置 `gamepad` 与 `keyboard` 模型上逐项断言可见性参数、爪子参数与 `StickLeftDown`，
  并断言回中后同一帧复原；
  `the_stick_axes_and_the_stick_button_are_projected_independently` 断言三者不会互相串。
- `bongocat-runtime`：`worker::axes` 的三条测试固定多手柄语义——最新样本获胜、回中读零、
  已释放的 connection 即使样本最新也不贡献，以及死区只作用于获胜样本。
- `bongocat-app`：`every_gamepad_button_matches_the_live_model_artwork` 遍历全部十六个产品按钮，
  **期望值由模型自己的 `KeyImageInventory` 推导**而不是写死：对每个按钮同时断言 hand 归属、
  按键层数量与所在手、渲染器实际解析到的文件名、以及哪一侧爪子落下；没有美术的按钮断言完全
  惰性（不绑定、不画图、不落爪）。预置 `gamepad` 今天有十二个按钮有美术，缺美术的四个是
  `Select`、`Start`、`LeftStick`、`RightStick`——这是唯一允许随美术变化的数字，模型掉了图会在
  这里变红而不是静默失效。
- `bongocat-platform`：`the_shoulder_and_the_analog_trigger_keep_their_own_buttons_and_artwork`
  与 `only_the_analog_trigger_reports_a_continuous_axis` 逐个名字钉住肩键与模拟扳机的
  按钮、美术名和轴，避免 ADR-0070 修过的那类互换无声回归。
- 变异验证：把 `CatParamStickLX` 接到 `stick_right_x` 并把 `CatParamStickRY` 接到 `stick_left_y`，
  `every_stick_control_reaches_its_own_parameter_and_nothing_else` 报
  `left stick X must move CatParamStickLX`；把 `gamepad_hands_for_model` 的手固定成左手，
  `every_gamepad_button_matches_the_live_model_artwork` 报
  `South hand must come from the directory its image lives in`；去掉
  `CatParamStickShowLeftHand` 的写入、以及去掉爪子的模型参数门禁，各自让
  `a_displaced_stick_shows_and_presses_its_paw_only_where_the_model_has_a_stick` 变红；
  恢复旧的「先挑一个 connection」投影让
  `the_newest_sample_of_an_axis_wins_across_every_connected_pad` 变红。
- 既有门禁不变：`cargo fmt --all -- --check`、workspace clippy（`bongocat-app` 按 feature
  分组）、`cargo test --locked --workspace`、`cargo check --locked --workspace --release`。
- **未做**：真实手柄实机。摇杆美术与爪子落下的最终画面仍需 Windows/macOS 各一台物理
  手柄确认，ADR-0066 的物理设备矩阵继续作为发布门禁。
