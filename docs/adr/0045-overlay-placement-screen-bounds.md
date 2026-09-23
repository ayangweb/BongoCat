# ADR-0045: Overlay 放置约束改为屏幕范围并延迟收敛

状态：Accepted
日期：2026-09-18
取代：无（取代 `P6-KEEP-OVERLAY-IN-WORK-AREA` 确立的工作区约束行为，该行为未单独留下 ADR）

> 后续修订（2026-09-23）：ADR-0054 后项目不再维护 AccessKit 标签同步；字段改名的当前收口是中英文案、共享 schema、fixture 与可见控件。

## Context

当前 v1 的 `overlay.keep_inside_work_area` 约束 overlay 窗口避开工作区之外的一切区域：
Windows 用 `MONITORINFO.rcWork`、macOS 用 `NSScreen.visibleFrame`，并在创建、设置/模型重建和
每一次 frame tick 里收敛窗口原点。由此产生两类不符合预期的行为：

- 模型窗口无法覆盖任务栏、程序坞和菜单栏所在的那一条区域。用户把猫放在任务栏上方这类明显
  合理的摆放会被立刻推回工作区内。
- 收敛在每一个 frame tick 上执行，因此拖拽一结束（两个平台的原生拖动循环都会阻塞 frame
  source，所以“结束”就是在下一帧）窗口就被拉回最近的显示器。当窗口在相邻两块显示器接缝处、
  或交叠面积更大的其实是对面的显示器时，这一拉回会把用户刚拖到的位置直接抢走，跨显示器摆放
  因此难以完成。

## Decision

- 约束区域从“某一块显示器的工作区”改为**所有已连接显示器矩形的并集**：Windows 取
  `EnumDisplayMonitors` + `MONITORINFO.rcMonitor`，macOS 取 `NSScreen.frame`。允许窗口覆盖
  任务栏/Dock/菜单栏，负坐标保持有效。
- 判定“窗口完整显示”的口径是**并集覆盖**而不是“落在单块显示器内”：一个横跨两块显示器、
  但每一部分都落在某块显示器上的窗口是合法摆放，不纠正。只有窗口真正越过桌面边界时才纠正。
- 纠正目标是“与窗口交叠面积最大的显示器，无交叠时取中心距离最近者”，保持窗口尺寸不变；
  窗口大于显示器时把原点贴到显示器原点（与旧行为一致）。
- 纠正**延迟**执行：窗口连续静止 `1s`（`PLACEMENT_SETTLE_DELAY`）后才移回显示器内，期间
  任何被观测到的位移都重新计时。创建、缩放/设置重建和模型重建这些没有拖拽在途的路径仍然
  立即收敛。
- 放置检查最多缓存 `500ms`（`PLACEMENT_INSPECTION_INTERVAL`）后重新评估。这既让静止窗口不必
  每帧枚举显示器（macOS 会分配、Windows 会进窗口管理器），也让显示器拓扑变化——包括拔掉
  外接屏后窗口留在桌面外——在没有平台通知通道的情况下仍能被纠正。
- 逻辑落在平台无关的 `bongocat-overlay/src/placement` 模块，平台适配器只负责枚举显示器与
  读写窗口矩形；判定与倒计时都可以在没有 GPU 和原生窗口的情况下测试。
- 字段随语义改名：`overlay.keep_inside_work_area` → `overlay.keep_inside_screen`。当前 v1
  尚未发布，按 §4.1 直接改名，不引入 alias、迁移或旧键兼容。中英文案、共享 schema 与
  fixture、AccessKit 标签同步更新。

## Consequences

- 行为变化：任务栏、Dock 和菜单栏所在区域不再是禁区，窗口可以停留在其上。
- 行为变化：窗口被拖出桌面后会有一段可见的“停留”，随后才自动回到显示器内；这是延迟的
  有意结果，也是跨显示器拖拽可用的前提。
- 多显示器选择逻辑从此只有一份（原先 macOS 在 `bongocat-overlay` 里自算交叠，Windows 交给
  `MonitorFromRect(MONITOR_DEFAULTTONEAREST)`）；两者语义一致，行为差异只剩平台坐标原点
  方向（AppKit 底部原点、Win32 顶部原点），分类计算对两者等价。
- 失去的能力：不再保证窗口避开系统栏；`always_on_top` 开启时 overlay 可以覆盖 macOS 菜单栏。
  需要“让开任务栏”时改由用户自己摆放。
- 一个大于所有显示器的窗口永远无法“完整显示”，此时约束只会把它贴到显示器原点且不再产生
  新的移动（纠正结果与当前矩形相同即视为无事可做），不会每帧重复 `SetWindowPos`。
- 纠正发生在 frame tick 内：overlay 不可见或 frame source 已停止时不会纠正，隐藏的窗口
  保留原位置。
- 显示器热插拔靠 `500ms` 定期重评估自愈，没有监听 `WM_DISPLAYCHANGE` /
  `NSApplicationDidChangeScreenParametersNotification`；因此拓扑变化到窗口纠正之间最多有
  “重评估间隔 + 静止延迟”的延迟。若后续需要更快的响应，应加平台通知而不是缩短这两个常量。
- 未验证项：两平台真实鼠标拖拽观感、多显示器实机摆放与显示器热插拔均未实机核验（本机只有
  macOS 单显示器环境，Windows 交叉工具链在本机无法构建 overlay 的 C 依赖）。
