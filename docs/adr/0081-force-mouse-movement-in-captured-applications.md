# ADR-0081: 指针被捕获时按相对位移累计光标位置

状态：已接受（2026-10-04）
补充：ADR-0011（模块契约）、ADR-0054（配置读取失败恢复）
实现：Issue #785

## 背景

Issue #785 报告：打开桌宠后进入全屏游戏，键盘和鼠标**按键**都能跟随，但鼠标**移动**不再跟随。报告者
在附加信息里点名要「鼠标强制移动」功能，并提到参考实现 Bongo-Cat-Mver 里的「强制移动鼠标」开关。

根因不在采集，而在位置来源。当前 Windows 平台 adapter 通过 Raw Input（`RIDEV_INPUTSINK`）接收
`WM_INPUT`，因此前台应用独占输入时仍能拿到键鼠事件——这就是按键仍然工作的原因。光标位置则由
`forward_cursor` 在收到鼠标移动包后调用 `GetCursorPos` 读取。多数全屏游戏会捕获指针：调用
`ClipCursor` 把光标锁在窗口内、隐藏它、只消费设备报告的相对位移。此时 `GetCursorPos` 返回的是一个
不再变化的坐标，于是「有移动事件 → 读绝对位置 → 位置没变 → 模型不动」。

参考实现的做法印证了这一点：Bongo-Cat-Mver 用 DirectInput 的 `DIMOUSESTATE.lX/lY` 读相对位移，
累加进一个自维护的位置（`point.x += mouse_state_d7.lX`），而不是读绝对光标。它的
`mouse_force_move` 是 `decoration` 下的全局开关，默认关闭。

范围里有一件必须现在决定、且决定方案形状的事：**累加发生在哪一层**。

## 决策

### 1. 累加在平台 adapter 内完成，不进入 runtime

`CursorProducer` 是 latest-value 通道（`CursorSlot`），只保留最后一个待消费样本。相对位移如果在
平台侧只作为「这一帧动了」的布尔量传给 runtime，再由 runtime 累加，那么两次发布之间被合并掉的采样
会连同位移一起丢失——快速移动会比设备实际移动的距离短。因此位移必须在**发布之前**合并，也就是在
平台线程内完成。

结果是 runtime、renderer 和 `ModelSettings` 都不知道这个开关存在：平台发布的仍是同一个
`CursorSample`，只是位置来源不同；平滑、归一化和模型投影完全不变。

### 2. `CursorForceMoveState` 是平台无关的状态机，落在 `bongocat-input`

`bongocat_input::CursorForceMoveState` 持有三件事：模式本身、`CursorMotionAccumulator`、上一个采样
的绝对位置。它对外只有三个操作：

- `sync(enabled) -> bool`：读模式，模式变化时丢弃累加位置（旧位置描述的是另一个读法，不能继续推进），
  返回当前是否累加。
- `reset()`：只丢累加位置，不改模式。输入 Reset、以及平台识别出「报告位置而非位移的设备」时调用。
- `advance(absolute, delta, viewport) -> CursorPosition`：推进一个采样并给出要发布的位置。

放在 `bongocat-input` 而不是各自平台，是因为这段策略对两个平台是同一件事，而平台文件在对方平台上
根本不参与编译。放在共享 crate 让它有单元测试，也避免两份实现分叉。

`CursorMotionAccumulator` 单独拆出来，是因为「以绝对位置为种子、按位移推进、夹在 viewport 内」这段
几何与「模式与上一个位置」是两件事，前者可以脱离后者测试。

### 3. 第一个采样与 viewport 变化重新对齐，且不叠加该采样的位移

累加器的第一个采样和任何 viewport 变化都直接从绝对位置开始，**不**再加上这一次的 `delta`：那个位移
已经体现在绝对位置里，再加一次会多走一个采样。这与 `CursorSmoother` 对首个样本和跨显示器样本的处理
是同一条规则（直接对齐目标），因此两者在 `force_move` 打开与关闭之间切换时不会产生位置跳变。

跨 viewport 必须重新对齐的理由和平滑器一致：两个显示器的坐标系原点不同，把旧显示器的位移加到新
显示器上会得到一个无意义的位置。

### 4. 累加位置夹在当前 viewport 内，饱和而不是记账

`advance` 把位置夹在 `viewport` 内。到达边缘后继续朝同方向推，位置**饱和**在边缘，而不是把多出来的
位移记在账上——后者会让用户先「还清」反向位移才能看到指针往回动。viewerport 没有可用面积（宽或高
为 0、非有限）时不夹取也不 panic：一个退化的读数不能让输入 worker 挂掉。

### 5. 报告位置的设备被识别出来并重新对齐，而不是冻结

不是所有设备都报告相对位移。Windows 的 `RAWMOUSE::usFlags` 有 `MOUSE_MOVE_ABSOLUTE` 位（远程桌面、
绝对定位设备会置位），这类包里的 `lLastX/lLastY` 是坐标而不是位移。macOS 没有对应的标志位。

- Windows：绝对包到达时调用 `reset()`，于是下一个采样从绝对光标重新对齐。跳过绝对包而不重置会让
  累加位置停在被钉住的那一点，而绝对设备的光标其实是正常移动的。
- macOS：没有标志位，于是从事实推断——**位置移动了但位移为零**说明这是报告位置的设备；被捕获的
  指针恰好相反（位置不动、位移持续到达）。这个推断只会导致重新对齐，而重新对齐的结果永远是「指针
  真正所在的位置」，所以它最多是无收益，不会引入错误位置。反过来，位置移动**且**位移非零时不移位
  （有些游戏会把光标重新居中，此时按位移累加才是对的，按位置对齐会让模型跳到屏幕中心）。

### 6. `input.mouse.force_move` 是 `InputConfig` 下的独立命名空间，runtime 只负责转发

配置落在 `input.mouse.force_move`，与 `input.gamepad` 并列：两者都是「输入在被模型看到之前怎么读」。
runtime 侧是 `CursorSettings { force_move: bool }`，通过 `SetCursorSettings` 命令进入
`RuntimeSnapshot::cursor_settings`。

runtime **不消费**这个值。消费者是 overlay session——它持有平台输入服务，而输入服务是唯一能看到设备
位移的组件。session 每帧从 snapshot 读出该值并写进服务的 `Arc<AtomicBool>`（两个平台各一个
`set_force_move`）。用原子量而不是命令通道，是因为这是每帧重放的幂等状态：丢一帧只损失一个采样，
下一帧就补回来，而加一条命令通道会让输入服务多一个必须排序的消息来源。

不放进 `OverlaySettings`：那个类型的文档把它定义成「单个 overlay 窗口的属性」，而这个字段是输入
采集策略，与窗口无关。不放进 `ModelSettings`：那个类型会交给 renderer，而这个字段与渲染无关。
`CursorSettings` 与 `GamepadAxisSettings` 是同一个形状——runtime 拥有的强类型输入设置——所以两者
放在同一个 crate、走同一条 revision-checked 设置命令。

字段带 `#[serde(default)]`（命名空间与内层字段都是），因此缺少 `input.mouse` 的旧配置按关闭读取，
不进入 ADR-0054 的「最新有效备份 → 默认配置」恢复流程。

### 7. 设置行放在 Input & interaction → Mouse 的最后一行

页面已有的三行是「忽略鼠标输入」「水平翻转鼠标跟随」「垂直翻转鼠标跟随」，都描述**模型拿到指针之后
怎么用**。这一行描述的是**指针位置从哪来**，放在三行之后读作对上面三行的一条附注：如果指针根本不动，
上面三项都没有作用对象。放在最前会读成「下面几项的前提条件」，而它并不是——关闭它时上面三行照常工作。

## 明确不做

- **不默认打开。** 打开期间由设备以外的方式移动的指针（程序化 warp、绝对定位设备、触摸板手势）不会
  被跟随；这是模式的代价，因此由用户显式选择。默认值与参考实现一致，都是关闭。
- **不用低级鼠标钩子。** `WH_MOUSE_LL` 提供的是同一个被钉住的坐标，解决不了问题，却会引入一个必须
  常驻的钩子。
- **不引入 DirectInput。** 参考实现用 DirectInput 读位移；本项目的 Windows 输入路径已经是 Raw Input，
  同一个 `RAWMOUSE` 里就有 `lLastX/lLastY`，没有理由再引入第二套设备 API（AGENTS.md §2.1）。
- **不在 runtime 侧累加。** 理由见决策 1。
- **不改变平滑、归一化或模型投影。** 这个开关只改位置来源。
- **不做「仅特定进程生效」的探测。** 是否捕获指针是前台应用的行为，逐进程探测既不必要也不可靠。

## 后果

- 在捕获指针的全屏游戏里，模型继续跟随鼠标移动；按键跟随不受影响，因为它本来就走另一条路径。
- 桌面上的正常使用不受影响：位移与绝对光标同步移动，累加位置跟着走；绝对设备与重新居中的游戏由
  决策 5 的规则处理。
- 配置、runtime 快照、设置协议与 overlay options 之间只有一份拼写；`CursorSettings` 与
  `GamepadAxisSettings` 共用同一套命令与快照通道。
- 该开关在 Windows 与 macOS 上都生效，且两平台共用同一段累加策略与同一批单元测试。

## 残余风险与待验证项（不得当作已确认）

1. **未在真实全屏游戏中实测。** 本机无法运行受捕获指针影响的游戏；逻辑有单元测试与解码测试固定，
   但「游戏中猫确实跟随鼠标」这一点没有实机证据。
2. **macOS 实机未验证。** 改动覆盖 macOS 的 `forward_latest_cursor`，但本机是 Windows，交叉检查被
   `permission-flow` 的 Swift 构建脚本挡住，因此 macOS 侧只有人工审查，没有编译或运行证据。
3. **Windows 是否真的收到被捕获指针的移动包，只有间接证据。** 报告者的「按键跟随、鼠标不跟随」说明
   `WM_INPUT` 到达了本进程；移动包与按键包走同一条投递路径，但这一点未在本机核对。
4. **macOS 上「位置移动且位移为零」这个推断未在真机验证。** 它的失效方向是安全的（最多重新对齐），
   但绝对定位设备在 macOS 上是否真的产生零位移事件没有实测。
5. **`RAWMOUSE` 字节布局只在解码测试中固定。** `usFlags` 偏移（`header_size + 0`）与
   `MOUSE_MOVE_ABSOLUTE` 取值来自 `RAWMOUSE` 的定义，未在真实远程桌面或绘图板上核对。

## 验证

- `bongocat-input`：`accumulator_seeds_from_the_absolute_cursor_then_follows_relative_motion`、
  `accumulator_clamps_inside_the_viewport_and_saturates_at_the_edge`、
  `accumulator_reseeds_when_the_viewport_changes_or_is_reset`、
  `accumulator_tolerates_a_degenerate_viewport`、
  `force_move_state_discards_the_accumulated_position_on_a_mode_change`、
  `force_move_state_reseeds_for_a_device_that_reports_a_position`、
  `force_move_state_keeps_accumulating_when_a_moving_location_also_reports_motion`。
- `bongocat-config`：`the_force_move_switch_round_trips_and_defaults_off_for_older_data`，以及
  `shared/config/fixtures` 的 `force-move-mouse` / `input-without-mouse` / `force-move-mouse-not-a-boolean`
  三个 fixture。
- `bongocat-ui-protocol`：`cursor_settings_command_preserves_typed_values`、
  `cursor_settings_default_to_the_absolute_cursor`。
- `bongocat-platform`（Windows）：`raw_mouse_decoder_reads_relative_motion_and_marks_absolute_devices`、
  `force_move_sums_relative_motion_and_ignores_absolute_packets`；macOS 侧
  `cursor_callback_slot_sums_motion_across_coalesced_samples`（见残余风险 2）。
- `bongocat-ui`：`settings.input_interaction.mouse.force_move.label` 在七个语言资源与设置窗口 smoke
  清单中都被点名。
