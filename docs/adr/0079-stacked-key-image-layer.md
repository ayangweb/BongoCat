# ADR-0079: 按键层按按下顺序叠放全部按住的键

状态：已接受（2026-10-02）
补充：ADR-0042（按键必须有自己的键位图）、ADR-0060（模型包 store 边界）

## 背景

Issue #965 报告：同时按下 A 和 D 时只显示一张图片，「能不能怎么调试一下让一起显示呢」，并问设置里
为什么没有单键模式开关。issue 下的讨论把理由说清楚了——直播时手法很快，「一瞬间三四个键按出去」，
观众如果只看到一张图片就分不清是连击还是外挂。

现状是**两处各自独立地把多键折叠成单键**：

- `bongocat-runtime` 的 `InputState::model_snapshot_with_filter` 对每只手只保留一个
  `latest_left_key` / `latest_right_key`，其余按键直接丢弃，因此 `KeyPressSet` 最多两个元素。
- `bongocat-live2d-render` 的 `resolve_key_overlays` 又用 `let mut selected = [None, None]` 按手
  折叠一次，输出固定是 `[左手, 右手]`。

管道本身其实早就准备好了：`KeyPressSet` 是 64 槽的有序集合且 `push` 保留插入顺序，
`RenderSnapshot::active_keys` 是 `Vec<KeyOverlay>`，`GpuModel` 存的是 `Vec`，双平台渲染器都按
`active_keys` 顺序遍历。也就是说**只有内容被限制为 2，载体一直是可叠放的**。

z 序不需要新增任何机制：Windows 侧在画背景前把 `OMSetRenderTargets` 的 depth-stencil view 置为
`None`，整个渲染器也从不创建 `ID3D11DepthStencilState`，按键层循环因此继承那个空绑定；Metal 侧的
render pass 只挂 color attachment，从不创建 depth texture。两边都纯靠提交顺序。
**`active_keys` 的最后一项就是最上层**。

需要一并解决的还有顺序本身。`InputState::pressed` 是 `BTreeMap<InputControl, PressedRecord>`，
迭代顺序是 HID usage 升序，从不是按下顺序；唯一的顺序信号是
`PressedRecord::pressed_at`（`MonotonicMillis`，毫秒精度）。旧代码的比较是
`record.pressed_at >= at`，所以**同一毫秒内的两次按下由 usage 大小决出胜负**，与「最后按下」这句话
无关。快速连击正好落在这个情形里。

## 决策

### 1. `model.show_all_pressed_keys` 是唯一的开关，默认关闭

放在 `ModelConfig`，`bool`、默认 `false`、正向语义（`show_…` 而不是 `single_…`），设置页呈现为
「同时显示所有按下的键」开关。

默认关闭是刻意的：叠放会让一个和弦在画面上明显变"挤"，而现有用户已经习惯了单图。把可见变化交给
一次显式选择，也保证这个字段不改变任何未选择它的用户的画面。

它是 `ModelConfig` 里**唯一带 `#[serde(default)]` 的字段**。v1 解析入口是严格的（`deny_unknown_fields`
且字段缺失即失败），所以一个必填新字段会让该字段加入之前写下的每一份 `config.json` 都解析失败，
进而走「最新有效备份 → 默认配置」的恢复流程并丢掉用户设置。缺少该字段的旧文档按兼容模式加载，
不视为一次新的开启。

### 2. 按下顺序用 `pressed_at` + 可靠输入队列的 sequence 构成全序

`PressedRecord` 增加 `pressed_sequence: u64`，取自按下该控件那条边沿的 envelope sequence，并随
`apply_event` 的参数一起传入。比较键变成 `(pressed_at, pressed_sequence)`。

sequence 是进程内单调计数器，与输入队列给事件排序用的是同一个值，所以它和队列的顺序永远一致，
跨过毫秒边界也继续有效。**这同时修掉了兼容模式里的平局仲裁**：同一毫秒按下 `A` 再 `D` 时，
现在显示的是 `D`（后按下的），而不是 usage 更大的那个恰好也是后按到的巧合结果。行为文档
`shared/behavior/input-semantics.md` 原本就写的是「最后按下且仍有效」，实现此前并没有保证这一点。

### 3. 折叠只保留在 runtime 的投影里，renderer 不再按手折叠

`resolve_key_overlays` 改为对每个 press 解析出一个 overlay 并**保持顺序**。兼容模式的投影本来就是
「左手先、右手后」，所以逐个解析的结果与旧的 `[None, None]` 折叠**逐项相同**，兼容模式的画面不变。

两个按住的键可能解析到同一张图（`Fn` 家族回退、小键盘复用主键区图）。这种情况只画一次，保留先
解析到的那个在底层：重复绘制看起来一模一样，只是多一次 draw call，而先按下的那个本来就该在下面。

### 4. 手部参数与显示模式无关

只要有任意一个绑定到该手的键仍按住，`CatParamLeftHandDown`/`CatParamRightHandDown` 就是 true，
两种模式都一样。叠放是显示选择：它改变画几张图和叠放顺序，不改变爪子对单个按键的反应，也不触碰
pressed state、释放路径、reconciliation 或 Reset。

### 5. 溢出丢最早按下的那几个

`KeyPressSet` 的 64 槽上限原本就存在（防止设备把内存撑大），叠放模式让每个槽位都可能真的画出一张图。
投影在排序后只取最后 `KeyPressSet::CAPACITY` 个：**堆叠的底部是历史，顶层是用户此刻正在看的**，
所以宁可丢最早按下的。上限以 `KeyPressSet::CAPACITY` 命名暴露，而不是让投影硬编码数组长度。

## 后果

- 打开开关后，一个和弦在按键层里逐键可辨认，且最后按下的键在最上层；关闭时画面与升级前一致。
- 兼容模式的同一毫秒平局改为按 sequence 仲裁，符合行为文档既有措辞。这是一处可观察的行为修正，
  只在两次按下落在同一毫秒时出现。
- 渲染器没有新增任何状态：叠放完全由 `RenderSnapshot::active_keys` 的顺序表达。
- 自动化契约分四处固定，且都在真实实现上：
  - `bongocat-runtime`（`the_stacked_mode_keeps_every_held_key_oldest_first`、
    `the_stacked_mode_does_not_change_which_hands_are_down`、
    `the_stacked_mode_drops_the_oldest_presses_past_the_layer_capacity`）钉住全序、平局与溢出；
  - `bongocat-live2d-render`（`every_press_gets_its_own_overlay_in_the_order_the_runtime_supplied`、
    `two_presses_that_share_one_image_draw_it_once`）钉住不重排序与同图只画一次；
  - `bongocat-app`（`a_chord_draws_one_overlay_per_key_with_the_newest_on_top`）跑通
    「按键 → 投影 → overlay 解析 → render frame」的完整链路，并在同一测试里对比开关两态；
  - `bongocat-config`（`a_configuration_written_before_the_key_layer_could_stack_still_loads`）
    从序列化后的当前默认里删掉该字段，模拟旧文档的字节，钉住旧数据仍按兼容模式加载。
- 共享 fixture 没有新增用例。`shared/fixtures/expected-state` 的 `activeKeyOverlays` 在
  `crates/bongocat-runtime/tests/shared_input_fixtures.rs` 里按 `BTreeSet` 比较，是**成员集合**而不是
  绘制顺序；`spikes/fixture-runner` 也独立实现了一遍「一手一图」。为了一个只有开启后才存在的模式去
  改 fixture 的 `context` 语法并同步两个 oracle，会把「输入状态机的边沿语义」这份 fixture 契约扩成
  「渲染顺序契约」。叠放顺序改由上面四处直接覆盖真实实现的测试固定，其中 app 那一处覆盖的正是
  fixture 声明为成员的同一份 `active_keys`。
- 未运行实机验证：本次只在本机（Windows）跑了 `just check` 与 `tools/` 下的校验脚本，没有做
  Windows 实机 smoke 或 macOS 实机 smoke，叠放的实际观感需要实机确认。