# ADR-0071: Gamepad-Connection Model Auto Switch

状态：已接受（2026-09-26）

## 背景

模型目录按输入模式分为 `standard`、`keyboard` 和 `gamepad` 三类。用户在手柄和键鼠之间
切换时，需要手动到模型库重新选择模型；旧版没有对应能力，`next` 是按维护者决定新增的
产品行为。

三条约束决定了它不能只做成 UI 行为：

- 模型加载是阻塞文件与 GPU 工作，UI executor 不执行；模型切换还必须走
  prepare → validate → commit 两阶段提交，只有 Application owner 能发起。
- 手柄连接状态的事实来源是 runtime 的 `InputState`：它按 connection generation 处理
  可靠 `GamepadConnected`/`GamepadDisconnected`，并在 Reset 后清空。上层只能读它的
  `connected_gamepad_count`，没有第二条判定链。
- 配置写入归 settings service worker 独占，platform 层不得直接改 overlay 或设置状态。

因此需要一个「谁观察、谁决定、谁写入」的完整通路，而不是在设置页轮询。

## 决策

- 配置是 `model.gamepad_auto_switch`：`enabled`（默认 `false`）、
  `connected_model` 和 `disconnected_model`。两个目标都是完整 `ModelIdentity` 或
  `null`，`null` 是默认值，含义是「上次在该输入族上使用过的模型」。
- 页面位置是 Input & interaction 的 Gamepad 分组末尾：门禁开关紧接在它控制的两个
  下拉之上，分组内已有的忽略手柄输入、摇杆与扳机死区保持原顺序。
- 两个下拉的第一个选项恒为「上次使用的手柄模型 / 上次使用的非手柄模型」，其余选项
  只包含该状态真正可用的模型：连接方向只列 `gamepad` 模式，断开方向只列其它模式，
  且只列 `Ready` 条目。已配置但不再可选的目标（模型被删或失效）以模型自己的显示名
  追加在末尾，避免控件显示为空而用户看不出设置指向哪里。
- 「上次使用」是 Application 的会话状态，不是配置：每次成功激活模型后按其输入模式
  记入对应族。手工选择与自动切换产生的激活一视同仁，因为这条记忆描述的是「屏幕上曾
  经是什么」，不是「谁要求的」。用户没有用过某一族时该方向无目标，切换不发生。
- 观察者是产品 frame source：它每帧已经读取 runtime snapshot，因此在
  `connected_gamepad_count` 的「0 ↔ >0」发生跨帧变化时向 settings client 发送一次
  `GamepadConnectionChanged`。第一帧只建立基线；已被系统上报的连接会在这之后形成真实
  跳变。发送失败（队列满）保留待发状态，下一帧重试。
- `GamepadConnectionChanged` 不携带状态。settings service 处理时重新读取 runtime 的
  `connected_gamepad_count`，因此重复通知、迟到通知和被补发的通知都落在同一结果上，
  不存在按过期观察切换的路径。
- 决定者是 settings service worker：门禁关闭、该方向无目标、目标已是当前模型时直接
  不做动作；否则复用既有 `Application::select_model`，因此配置提交、行为快捷键分配、
  失败回滚、`selected_model` 持久化和两阶段提交语义与用户手动选择完全一致，没有第二条
  切换路径。
- 自动切换是普通模型选择，所以它的结果进入 `model.selected_model`，重启后恢复用户最后
  看到的模型。删除导入模型时，与该模型同族的会话记忆和两个配置目标都在同一次提交中
  一起清除，不留下指向已删除文件的引用。
- 设置页用统一门禁规则（ADR-0053）：开关本身只被结构性阻塞禁用，两个下拉在开关关闭
  时置灰但保留已选值。

## 验证

- `bongocat-config` fixture 覆盖默认关闭 + 两个 `null` 目标、可移植 id 的接受与拒绝，
  JSON Schema 与 Draft 2020-12 fixture validator 同步。
- `bongocat-app` 覆盖两条链：`the_gamepad_auto_switch_follows_the_last_used_model_of_each_family`
  验证门禁关闭不动模型、启用后按族记忆、显式目标优先、切换被持久化；
  `a_gamepad_connection_notice_switches_the_model_the_settings_service_owns` 走完
  「frame source 通知 → settings service 读取 runtime 答案 → 模型落定」并断言
  `selected_model` 与诊断计数。
- `bongocat-app` 单元测试固定 frame source 的过渡检测与重试，以及删除模型后两个配置
  目标被清除、内置同 id 模型不受影响。
- `bongocat-ui` 覆盖两个下拉的模式过滤、只列 `Ready`、`null` 恒为首项、悬空目标可见
  且不重复、空目录仍提供默认项；settings smoke 断言新增双语 key 非空。
- 变异验证：把 `connected_gamepad_count > 0` 改成 `>= 0`、把 `enabled` 门禁反向读取、
  或让下拉列出全部模式，都会让上述测试变红。
- 未运行：Windows 10 1903+ 与 macOS 12+ 实机手柄热插拔矩阵、长时间反复插拔、锁屏与
  睡眠中的连接变化，以及双平台高 DPI/Retina 目视检查。这些仍是 ADR-0066 的完成门禁，
  本 ADR 的自动化证据不替代它们。

## 后果

手柄成为「换一个模型」的显式动作，而不需要用户在模型库里找当前模式。默认配置保持关闭
且不预设任何模型，因此新装应用不会因为插上手柄而突然换模型；用户打开开关后，第一次
连接若该族还没有使用记录，切换不发生，直到用户自己用过一次该族模型或在下拉里显式
指定。设置文件因此仍然只有一个模型选择事实来源：自动切换与手工选择写同一处。
