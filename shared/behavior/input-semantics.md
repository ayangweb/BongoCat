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

## 高频值

`cursor_moved` 和 `gamepad_axis` 可以在进入 runtime 前合并为最新值。合并不能改变 key/button edge 的顺序，也不能延迟释放事件。

## 物理键

Runtime 使用布局无关的稳定物理键名。左右修饰键必须区分，例如 `ControlLeft` 和 `ControlRight`。字符和 UI 显示名称由单独映射层产生。

### Canonical names

平台 adapter 必须把系统原始码映射到以下协议名称；名称不会随键盘布局、本地化或设备厂商变化：

- `PhysicalKey`：使用 `KeyA`、`Digit1`、`Enter`、`Escape`、`Space`、`Tab`、`Backspace`、`ShiftLeft`/`ShiftRight`、`ControlLeft`/`ControlRight`、`AltLeft`/`AltRight`、`MetaLeft`/`MetaRight`、`ArrowUp`/`ArrowDown`/`ArrowLeft`/`ArrowRight`、`PrintScreen`、`Pause` 等 DOM/USB 语义名；无法识别的码保留 `Unknown(<platform-code>)` 诊断值，不得映射成字符。
- `MouseButton`：`left`、`right`、`middle`、`back`、`forward`。
- `GamepadButton`：优先使用标准位置名 `south`、`east`、`west`、`north`、`left_shoulder`、`right_shoulder`、`left_trigger`、`right_trigger`、`select`、`start`、`left_stick`、`right_stick`、`dpad_up`、`dpad_down`、`dpad_left`、`dpad_right`；不能识别的位置保留设备 profile 的稳定诊断名。
- `GamepadAxis`：使用 `left_stick_x`、`left_stick_y`、`right_stick_x`、`right_stick_y`、`left_trigger`、`right_trigger`；数值归一化到 `[-1, 1]`，trigger 的无效负值由 adapter 钳制为 `0` 并计入诊断。

数字手柄按钮以 `value >= 0.5` 产生 pressed，低于阈值产生 released；重复 edge 不增加 pressed 计数。axis 和 cursor 只保留最新值，不能阻塞可靠边沿。死区由产品配置决定，adapter 不得把设备默认死区静默写入共享协议。

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
