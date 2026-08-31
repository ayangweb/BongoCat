# Input Semantics

状态：Phase 0 draft
版本：1

## 时间

所有输入事件在进入 runtime 时使用单调时间。Fixture 使用从序列开始计算的整数毫秒 `atMs`；相同时间的事件按数组顺序处理。

## 边沿与状态

- `key_down`/`key_up` 和 `mouse_down`/`mouse_up` 是可靠边沿。
- 重复 `key_down` 不能增加 pressed 计数，也不能重复触发只接受首次边沿的动作。
- `key_up` 可以来自正常采集或系统状态校正，两者对业务状态等价。
- `reset` 清空所有 pressed key/button 和瞬时输入状态。
- 每个 pressed key 最终必须由 `key_up` 或 `reset` 清除。

## 可靠事件序列

平台采集器为 key/button edge 和设备生命周期事件分配进程内单调 `sequence`。状态层必须检测重复、乱序和跳号：重复或乱序事件计数后忽略；跳号表示未知边沿可能丢失，先执行安全 `reset`，再应用当前事件。序列诊断只记录异常类别与数量，不记录具体键值；鼠标移动和手柄 axis 的 latest-value 更新不要求进入该序列。

## 高频值

`cursor_moved` 和 `gamepad_axis` 可以在进入 runtime 前合并为最新值。合并不能改变 key/button edge 的顺序，也不能延迟释放事件。

可靠 edge、设备生命周期事件和 command 使用固定容量 FIFO；满载必须返回原事件并触发安全恢复，不能静默丢弃。高频 cursor/axis 使用独立 latest-value 槽位，不得占用可靠 edge 容量。

Cursor sample 使用全局逻辑坐标、光标当前所在显示器的逻辑 viewport 和进程内单调时间。输入服务启动时必须主动查询并发布当前光标位置，不能等待首次移动事件；跨显示器时使用新位置所在显示器的 viewport，不使用主显示器或 overlay 所在显示器代替。

Runtime 将光标按当前显示器归一化到模型坐标：

```text
x_ratio = (position.x - viewport.origin.x) / viewport.width
y_ratio = (position.y - viewport.origin.y) / viewport.height
pointer_x = clamp(1 - 2 * x_ratio, -1, 1)
pointer_y = clamp(1 - 2 * y_ratio, -1, 1)
pointer_z = clamp(-pointer_x * pointer_y, -1, 1)
```

该方向与现有模型语义一致：显示器左上角为 `(1, 1)`，右下角为 `(-1, -1)`。viewport 必须有限且宽高为正；无效几何和单调时间回退必须拒绝并计数，不得向模型传播 `NaN` 或无穷值。

单槽 cursor transport 的每个 accepted sample 最终必须满足以下守恒关系：

```text
published = coalesced + consumed + pending
```

`pending` 只能是 `0` 或 `1`。停止 runtime 后的新 sample 必须返回原值并计入 `rejected_after_stop`；shutdown 必须消费已接受的 pending sample，使最终 `pending = 0`。

手柄 axis 槽位以 `{device_id, connection_generation, axis}` 为 key，并限制活动 key 总数。可靠的 `device_connected` 为该连接分配单调 generation；`device_disconnected` 清空该 generation 的 runtime axis/pressed state 和尚未消费的 axis sample。重连即使复用平台 device id 也必须获得新 generation，旧 callback 的迟到 sample 只能被计数并忽略。每个 accepted sample 最终由 coalesced、consumed、disconnect discard 或 pending 之一解释；新增 key 超容量必须显式报错，不能扩成无界 map。

Axis sample 只有在对应 connection 已被 runtime 接受后才可进入模型输入；连接事件之前到达
latest-value 槽位的 sample 直接丢弃，不能在后续连接时回放。投影阶段仍需再次过滤 active
connection，防止 worker 在连接/断开边界观察到陈旧 generation。

## 物理键

Runtime 使用布局无关的稳定物理键名。左右修饰键必须区分，例如 `ControlLeft` 和 `ControlRight`。字符和 UI 显示名称由单独映射层产生。

### Canonical names

平台 adapter 必须把系统原始码映射到以下协议名称；名称不会随键盘布局、本地化或设备厂商变化：

- `PhysicalKey`：使用 `KeyA`、`Digit1`、`Enter`、`Escape`、`Space`、`Tab`、`Backspace`、`ShiftLeft`/`ShiftRight`、`ControlLeft`/`ControlRight`、`AltLeft`/`AltRight`、`MetaLeft`/`MetaRight`、`ArrowUp`/`ArrowDown`/`ArrowLeft`/`ArrowRight`、`PrintScreen`、`Pause` 等 DOM/USB 语义名；无法识别的码保留 `Unknown(<platform-code>)` 诊断值，不得映射成字符。
- `MouseButton`：`left`、`right`、`middle`、`back`、`forward`。
- `GamepadButton`：优先使用标准位置名 `south`、`east`、`west`、`north`、`left_shoulder`、`right_shoulder`、`left_trigger`、`right_trigger`、`select`、`start`、`left_stick`、`right_stick`、`dpad_up`、`dpad_down`、`dpad_left`、`dpad_right`；不能识别的位置保留设备 profile 的稳定诊断名。
- `GamepadAxis`：使用 `left_stick_x`、`left_stick_y`、`right_stick_x`、`right_stick_y`、`left_trigger`、`right_trigger`；数值归一化到 `[-1, 1]`，trigger 的无效负值由 adapter 钳制为 `0` 并计入诊断。

数字手柄按钮以 `value >= 0.5` 产生 pressed，低于阈值产生 released；重复 edge 不增加 pressed 计数。axis 和 cursor 只保留最新值，不能阻塞可靠边沿。死区由产品配置决定，adapter 不得把设备默认死区静默写入共享协议。

Windows XInput 的 0–3 user index 只作为当前连接的 `device_id`；同一 slot 断开再连接必须分配新 generation。signed thumb axis 按负半轴 `32768`、正半轴 `32767` 归一化到完整 `[-1, 1]`，trigger 按 `0..255` 归一化到 `[0, 1]`。adapter 不应用 `XINPUT_GAMEPAD_*_DEADZONE` 或 trigger threshold 常量，避免平台默认值覆盖产品配置与共享 `0.5` 按钮语义。

macOS `FlagsChanged` 必须在 event-tap callback 中结合事件自身 flags、keycode 和 callback decoder 记录的该 key 前一边沿状态，固定左右修饰键的 pressed/released 方向；同类左右键同时按住时，聚合 flag 仍为 true，但已在 decoder set 中的触发 key 表示单侧 release。decoder set 只解决平台 packet 歧义，不是 runtime pressed state，并随任何 `reset` 清空。consumer 不得在稍后 drain 时查询当前全局状态来反推旧边沿，因为同一批次可能已经包含后续 release。无法识别方向的 modifier event 触发带计数的安全 `reset`；`CGEventSourceKeyState` 只用于候选 pressed set 的周期校正。

## 手部状态

模型资源可以把多个键映射到同一只手。兼容模式下，同一手只显示最后按下且仍有效的键资源；任意映射到该手的 pressed key 都令对应 hand-down 参数为 true。

## Reset 原因

支持的原因至少包括：

- `session_lock`
- `sleep`
- `device_removed`
- `service_restart`
- `queue_overflow`
- `permission_changed`
- `test`

Reset 必须记录诊断计数和原因，但不能记录用户按键内容。
