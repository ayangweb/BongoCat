# Bongo-Cat-Mver Frame Rate Semantics

状态：参考实现的帧率语义已冻结；对照后 1 项采纳（`overlay.maximum_fps` 契约补值域与语义）、3 项明确不采纳
记录日期：2026-09-23
审阅版本：tag `v1.6.0` = commit `4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c`，即 `docs/migration/bongo-cat-mver-reference.md` 固定的基线；上游 `SFML 2.5.1` 与 `CubismNativeSamples`

> 本文记录参考实现的**行为事实**，不授权复制、翻译或重新许可其源码。所有引用均为只读对照。
> 它是 `docs/migration/bongo-cat-mver-reference.md` 在帧率一项上的展开：固定 commit、文档优先级
> 与使用规则以该基线为准，本文件只补充帧率语义与不采纳项的细节。

目的：冻结 `Bongo-Cat-Mver` 对「最大帧率」的处理方式，作为 `next` 的对照来源。它回答两个问题——
帧率限制应该在哪里生效，以及模型动画应该如何消费时间。本文只记录**参考实现的事实与其上游依据**，
不改动任何 `next` 行为。

依据：上述 tag 与上游实现，**引用上游代码是因为关键行为不在 Mver 自己的代码里**——Mver 没有手写
任何节流循环，节流语义完全由 SFML 的窗口层决定。

## 1. 配置与应用点

| 项 | 位置 | 说明 |
| --- | --- | --- |
| 配置键 | `BongoCatMver/src/data.cpp:24` | `decoration.framerateLimit`，整数，默认 `60` |
| 用户界面 | `BongoCatMverUI/setting_cat.xaml:160` | 标题「帧率限制」；`MaxLength=3`，`TextChanged` 只留数字，`LostFocus` 空值写回 `0` 后立刻持久化（`setting_cat.xaml.cs:172`、`:190`） |
| 配置文档 | `BongoCatMverUI/tutorial/Tutorial_ConfigComparisonTable.xaml:107` | 随应用发布的配置对照表，明确写出 `0` 表示不限制及其代价 |
| 应用点 | `BongoCatMver/include/catmain.h:166` | `window.setFramerateLimit(data::cfg["decoration"]["framerateLimit"].asInt())`，全仓库唯一一处 |
| 生效时机 | `catmain.h` 的 `setWindow()` | 只在启动、`UIWM_WRITECONFIG`（UI 保存后 `PostMessage`）与 `Ctrl+R` 时调用；该函数只改窗口样式与位置，**不重建窗口**，因此限流值可以中途改而无需重启进程 |

`0` 是合法的第三态：SFML 把它折叠成「无限制」，不是「每秒 0 帧」。

## 2. 节流语义：睡眠补足帧预算的余额

`setFramerateLimit` 只记录每帧预算，真正等待发生在每次呈现：

```cpp
// SFML 2.5.1 src/SFML/Window/Window.cpp
void Window::setFramerateLimit(unsigned int limit)
{
    if (limit > 0)
        m_frameTimeLimit = seconds(1.f / limit);
    else
        m_frameTimeLimit = Time::Zero;          // 0 = 不限制
}

void Window::display()
{
    if (setActive())
        m_context->display();                   // 交换缓冲、呈现

    if (m_frameTimeLimit != Time::Zero)
    {
        sleep(m_frameTimeLimit - m_clock.getElapsedTime());
        m_clock.restart();
    }
}
```

决定成败的是 `m_clock` 何时重启：它在**上一次 `display()` 的末尾**重启（另一次在 `initialize()`）。
所以下一次读到 `getElapsedTime()` 时，它恰好等于**本帧已经花掉的时间**——睡眠只补差额，
**呈现间隔恒等于 `1/limit`**，而不是 `1/limit + 工作耗时`。Mver 的循环顺序也支持这一点：
`clearCatWindow()` → `drawCat()`（含模型求值）→ `window.display()`（`src/main.cpp:241`、`:262`）。

超预算时不会雪崩也不会补帧：`sf::sleep` 对非正时长直接返回。

```cpp
// SFML 2.5.1 src/SFML/System/Sleep.cpp
void sleep(Time duration)
{
    if (duration >= Time::Zero)
        priv::sleepImpl(duration);
}
```

即工作耗时超过一帧预算时就不睡、不追赶、不突发补帧，呈现节奏退化为「能跑多快跑多快」。
这与 `next` 的 `bongocat_runtime::FramePacer`（截止时间网格、超时重锚不补发）是同一套语义，
差别只在锚点：SFML 存的是「相对上次呈现的余额」，`FramePacer` 存的是绝对 `deadline`；稳态周期一致。

## 3. 动画时间源：与帧率彻底解耦

帧率限制只决定采样密度，动画进度由模型自己的时间源决定。Mver 每帧在 Live2D 模式的 `draw()` 开头
更新一次时间：

```cpp
// BongoCatMver/src/mode/mode98_live2d_standard.cpp:231
LAppPal::UpdateTime();
```

模型随后读取它：

```cpp
// BongoCatMver/src/myUserModel.cpp:368
const csmFloat32 deltaTimeSeconds = LAppPal::GetDeltaTime();
_userTimeSeconds += deltaTimeSeconds;
_dragManager->Update(deltaTimeSeconds);     // 之后交给 Cubism 的 motion/expression manager
```

`LAppPal` 来自官方 sample，用高精度性能计数器求两次计数的差，**保存绝对时间戳而不是累加**：

```cpp
// CubismNativeSamples Samples/D3D11/Demo/proj.d3d11.cmake/src/LAppPal.cpp
void LAppPal::UpdateTime()
{
    if (s_frequency.QuadPart == 0) { StartTimer(); QueryPerformanceCounter(&s_lastFrame); s_deltaTime = 0.0f; return; }
    LARGE_INTEGER current;
    QueryPerformanceCounter(&current);
    const LONGLONG BASIS = 1000000;
    LONGLONG dwTime = ((current.QuadPart - s_lastFrame.QuadPart) * BASIS / s_frequency.QuadPart);
    s_deltaTime = (double)dwTime / (double)BASIS;
    s_lastFrame = current;
}
```

因此 60 FPS 与 30 FPS 下，同一墙钟时刻的模型姿态完全一致：帧率只改变采样密度，不改变动作进度。
注意它**没有任何步长上限**（见 5.2）。

非 Live2D 模式不是时间动画：`mode 1/2` 的手/键盘贴图由按键状态直接选择，`sf::Clock`
（`catfunc.cpp:286`）只作为「哪个键最新按下」的排序时间戳（`catfunc.cpp:245` 的 `max_time()`）。
所以两类模式都不存在「帧率改变动作速度」的可能。

## 4. 与 `next` 的对照

| 维度 | 参考实现（Mver + SFML） | `next`（2026-09-24 修复后） |
| --- | --- | --- |
| 帧预算算法 | `sleep(预算 − 已用)` | `wait(deadline − now)` |
| 稳态呈现间隔 | `= 1/fps` | `= 1/fps` |
| 工作超预算 | 不睡、不追赶 | 重锚、不补发 |
| 节流点数量 | 1（`window.display()`） | 3（runtime worker、产品 frame source、独立 overlay loop），语义一致 |
| 动画时间源 | `LAppPal` + `QueryPerformanceCounter` 差值 | 注入的单调时钟 `Duration` 差值（可测试） |
| 改设置生效 | 需要走配置重载消息 | typed command，实时生效且带 revision CAS |
| 值域 | `0`（不限制）或 `1..999` | `15..=240` |

## 5. 明确不采纳

### 5.1 `0 = 不限制` 与只挡上界的输入过滤

`0` 是第三态，需要 `FramePacer` 能表达「无上限」（等待恒为 0）。`next` 目前是 `15..=240` 闭区间，
且 UI 只挡上界（`MaxLength=3` 允许填 `1`，动作几乎不动）——下限 15 更合理。是否引入「不限制」
属于产品能力取舍，未确认前不新增。

### 5.2 动画时间的单帧步长上限（评估后决定不加）

两边的动画时间都是「绝对时间戳求差」，因此一次长间隔——系统睡眠、应用挂起、或模型窗口隐藏一段时间
后重新显示——会作为**一个巨大的步长**到达：一次性 motion 直接进入 completed 并固定在包含自然
fade 权重的完整终点样本、
expression 淡出瞬间结束、motion UserData 跨过的每个时间戳仍按有效播放模式发出（只有单次有界批次
超过上限时才跳过并计入 `skipped_occurrences`）。一个自然的
想法是给单帧步长设上限（例如 `100 ms`），让动画「接着演」。

**不采纳的具体阻塞**：这会与 Technical Design 已冻结的契约冲突。该契约要求淡入淡出按
「从过渡起点起算的绝对经过时间」求值，使**同一时间点在任何帧率下得到同一个 alpha**；把动画时间轴
改成按上限推进的累加时间后，同一墙钟时刻的 alpha 会随是否发生过卡顿而不同，而且 1 s 的 clip 在
卡顿时会播超过 1 s。

**代价可测**：`bongocat-runtime/src/lib.rs` 的回归用 `clock.set(...)` 的大步长驱动动画到达某个时刻——
`from_secs(2)` 与 `from_secs(10)` 两次一次性 motion 完成、1 s 淡出用 1.5 s 跳步完成，另有 6 处在
400–1000 ms 之间取中间帧。改为按上限推进后，这些用例必须全部重写成多步序列，否则断言会失去原意。

**当前行为不是缺陷，而是该契约的推论**：一个巨大的步长意味着「那一瞬间动画就是处于终点状态」。
一次性 motion 因此直接进入 completed，并在后续每帧默认值恢复后继续应用其终点参数、part opacity
与 model opacity；它不是被「跳过」，也不会被清回 idle。显式 stop fade 同样可以在一次大步长内
完成。这里记录的是**评估后维持绝对时间语义**。

**触发条件**（满足任一再评估）：出现可复现的用户可见跳变（需要实机证据，而非推断）；或产品显式
要求「隐藏期间动画时间不流逝」。届时的取法是**只钳制 step、同时把「同一时间点同一 alpha」这条
契约改成「同一动画时间点同一 alpha」**，两者不能同时成立。

### 5.3 其它不照抄的实现细节

- **自测 FPS 是错的**：`FPStimeer.restart()` 在循环顶部（`src/main.cpp:95`），读数在
  `src/main.cpp:246`，而节流睡眠在 `src/main.cpp:262` 的 `window.display()` 里——测量窗口把要量的
  那件事排除在外，量到的是「工作耗时的倒数」（工作 1 ms 就显示约 1000 FPS）。量帧率必须量
  **两次 present 的间隔**。`next` 的回归用「发布帧数 / 时间窗」，是对的。
- **忙等**：`while (!data::init());`（`src/main.cpp` 启动处）在配置读取失败时空转。
- **设置只走重载路径**：UI 保存 → `PostMessage(UIWM_WRITECONFIG)` → 重读配置 → `setWindow()` 才
  重新应用帧率；`next` 的 typed command 更强，不采纳这种「设了但要等重载」的路径。

## 6. 采纳结果

- **本轮已采纳**：`maximum_fps` 的配置契约补上值域与语义说明（`shared/config/contract.md`），
  对齐参考实现把帧率的含义与代价写进随产品发布的配置文档这一做法。
- **上一轮已等价**：差额睡眠 / 超时不追赶 / 单一节流语义 / 动画用绝对时间差，见
  `bongocat_runtime::FramePacer` 与 `docs/implementation-todo.md` 第 48 项。
