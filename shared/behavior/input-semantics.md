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

Runtime 以可注入单调时钟在逻辑坐标中平滑最新目标。平滑沿用旧版在 60 FPS 下每帧保留
`0.75` 剩余距离的语义，并按实际时间步长换算：

```text
alpha = 1 - 0.75 ^ (elapsed_seconds * 60)
current = current + (target - current) * alpha
```

插值后与目标的逻辑距离小于 `0.5` 时直接收敛。首个有效样本不从屏幕中心插值；viewport
变化时直接对齐新目标，避免把旧显示器坐标系带入新显示器。没有新 cursor sample 时 runtime
仍在周期 tick 推进现有目标，renderer 和模型参数只消费平滑位置；原始最新 sample 继续用于
transport accounting 和诊断。

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

- `PhysicalKey`：是布局无关的稳定物理键 identity（承载 `u16` HID usage）。**同一个物理键有两套互不通用的名字**，改任何一处之前先确认改的是哪一套：

  - **键位图名**：模型美术的文件主干，事实来源是 `bongocat-live2d::key_name_candidates`。模型作者按这套名字把图片放进 `resources/<side>-keys/<名>.png`。例：`KeyA`、`Num1`、`Enter`、`Escape`、`Space`、`Tab`、`Backspace`、`ShiftLeft`/`ShiftRight`、`ControlLeft`/`ControlRight`、`AltLeft`/`AltRight`、`MetaLeft`/`MetaRight`、`UpArrow`/`DownArrow`/`LeftArrow`/`RightArrow`、`LeftBracket`、`RightBracket`、`BackQuote`、`SemiColon`、`BackSlash`、`Dot`、`PrintScreen`、`Pause`。Apple 地球键的键位图名是 `Globe`（见下一条）。
  - **快捷键名**：设置页热键文本，事实来源是 `bongocat-config::NAMED_SHORTCUT_KEYS`，沿用 DOM `KeyboardEvent.code` 风格。例：`ArrowUp`/`ArrowDown`/`ArrowLeft`/`ArrowRight`、`BracketLeft`、`BracketRight`、`Backquote`、`Semicolon`、`Backslash`、`Period`；数字的 canonical 形式是 `1`，`Digit1` 只是被接受的别名。

  ⚠️ **只有键位图名能解析到图片**。照快捷键名做出 `Digit1.png` 或 `ArrowUp.png` 不会被任何键画出来：同一物理键在两套名字下的拼写可能只差大小写（`BackQuote` vs `Backquote`）或词序（`UpArrow` vs `ArrowUp`）。无法识别的平台码保留 `Unknown(<platform-code>)` 诊断值，不得映射成字符。
- 唯一不在 HID Keyboard/Keypad 页（`0x07`）的物理键是 Apple 地球键（Fn 键）：它是厂商页 `0xFF` 的 usage `0x03`（`KeyboardFn`）折叠成 `0xff00 | usage` = `0xff03`，协议名 `Globe`。**键位图名是 `Globe`，不是 `Fn`**：`Fn` 是模型可为 F1–F24 提供的共享回退图名，一个图片名只能有一个语义（ADR-0049）。该键的旧名 `Function` 只作为末位候选别名，用于兼容未经导入归一化的包。Windows 的 Fn 键由键盘固件处理、Raw Input 从不报告它，因此没有 Windows 映射。
- `MouseButton`：`left`、`right`、`middle`、`back`、`forward`。
- `GamepadButton`：只产生标准位置名 `south`、`east`、`west`、`north`、`left_shoulder`、`right_shoulder`、`left_trigger`、`right_trigger`、`select`、`start`、`left_stick`、`right_stick`、`dpad_up`、`dpad_down`、`dpad_left`、`dpad_right`；backend 的额外/未知位置不伪造 identity，只进入匿名诊断。
- `GamepadAxis`：使用 `left_stick_x`、`left_stick_y`、`right_stick_x`、`right_stick_y`、`left_trigger`、`right_trigger`；stick 数值归一化到 `[-1, 1]`，trigger 数值归一化到 `[0, 1]`，非有限或越界值拒绝并计数。

数字手柄按钮以 `value >= 0.5` 产生 pressed，低于阈值产生 released；重复 edge 不增加 pressed 计数。axis 和 cursor 只保留最新值，不能阻塞可靠边沿。死区由产品配置决定，adapter 不得把设备默认死区静默写入共享协议。

双平台手柄 adapter 固定使用 `ayangweb/gilrs`：Windows 为 WGI，macOS 为 IOHID。adapter 关闭 gilrs 默认 jitter/dead-zone filter、force feedback 和环境 mapping，保留 fork 内置 SDL mapping，并使用 gilrs 自带 D-pad axis-to-button 转换。gilrs 的 `LeftTrigger`/`RightTrigger` 位置映射为项目 shoulder，`LeftTrigger2`/`RightTrigger2` 的连续值同时产生项目 trigger button edge 与 trigger axis；后者按产品 `>= 0.5` 判定，不使用 gilrs 默认阈值代替。非有限或越界 trigger 值在改变 pressed state 前拒绝并计数。backend device id 不进入项目协议；adapter 分配最多四个 `device_id`，每次连接/重连通过 axis producer 获得新 generation。全局 Reset 成功后以同一 generation 重播 connection、gilrs 已缓存的当前 held button 和六轴，不重新分配连接；进程启动/重连时的 authoritative initial state 仍需 gilrs backend 提供。

macOS listen-only tap 必须创建在 HID 层 head 位置（`kCGHIDEventTap` + `kCGHeadInsertEventTap`，与 rdev 的 listen 一致）；实测 macOS 26 的 session tail 位置收不到右 Shift 的释放 `FlagsChanged`，HID head 位置能收到全部修饰键的完整事件对。

macOS `FlagsChanged` 必须在 event-tap callback 中固定 down/up 方向，按优先级依次判定：

1. 事件 flags 相对上一个 `FlagsChanged` 事件的设备位（flags 低 8 位，每个物理修饰键一个独立 bit）发生跳变时，以设备位方向为准；设备位是物理键私有状态，左右修饰键天然区分，同侧兄弟键按住导致家族 flag 不清零时仍能识别单边沿。
2. 否则使用家族 flag 位跳变方向。
3. 否则按 callback 记录的该 key 前一边沿交替。HID head tap 下右 Shift 的按下/释放事件对完整、设备位跳变正常；CapsLock 的 `AlphaShift` 位反映锁存状态而非物理边沿，必须依赖交替回退（该回退同时兜底 session tail 等异常环境下观察到的事件缺失序列）。

decoder 状态只解决平台 packet 歧义，不是 runtime pressed state，并随任何 `reset` 清空；周期校正强制释放候选 key 时必须同步清除 decoder 中对应 key 的记录，使交替回退重新对齐。consumer 不得在稍后 drain 时查询当前全局状态来反推旧边沿，因为同一批次可能已经包含后续 release。无法识别方向的 modifier event 触发带计数的安全 `reset`；`CGEventSourceKeyState` 只用于候选 pressed set 的周期校正，且对右侧修饰键键码（54/60/61/62，实测按住时也返回 false）必须同时查询家族主键码（55/56/58/59）。

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
