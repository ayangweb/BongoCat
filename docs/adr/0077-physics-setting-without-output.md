# ADR-0077: 接受没有输出项的物理设置

状态：已接受（2026-09-29）
补充：ADR-0060（模型包 store 边界）、ADR-0061（Live2D playback 边界）

## 背景

`bongocat-model` 的 `validate_physics_resource_value` 曾要求每个 `PhysicsSetting` 同时有
输入、输出和至少两个顶点，缺输出项即以 `physics3 settings require input, output, and at
least two vertices` 拒绝整个包。

这条规则比格式本身严格。Live2D 官方 `physics3.json` schema
（`Live2D/CubismSpecs` 的 `FileFormats/physics3.json.md`）把 `Output` 列为 required **键**，
但对数组长度没有任何要求；Cubism Editor 导出的模型里，"模拟了粒子但不写回任何参数"的
设置项是正常存在的。

用户报告的模型正是这种情况。`cat.physics3.json` 的 32 个 `PhysicsSettings` 里有 16 个
`"Output": []`，名字是「头发Y轴」「翅膀物理」「耳坠」「呆毛2」等——正是编辑器里搭好、随后
把输出接到别处的设置项。三个模式的标准/键盘/手柄版本都是这个形态（16/15/14 个），而这个
`cat.physics3.json` 与原始 MVer 模型 `img/standard/cat_model/` 下那份逐字节相同
（`c6722ed4f8b815b4`），说明转换过程没有改动它，问题一直在校验侧。旧版本能导入，是因为它
走官方 Cubism Framework 的解析，那条路径同样不要求非空输出。

结果是：模型包校验通过前就失败，用户看到的是「模型包无效」，而模型本身完全可用。

## 决策

### 1. 输出项可以为空，模拟照做但不写回

删除 `setting.outputs.is_empty()` 这一项要求，保留输入项非空和至少两个顶点。

理由不是"放宽一点试试"，而是求值器本来就按这个语义实现。`bongocat-live2d` 的
`PhysicsRuntime` 用 `setting.outputs.len()` 分配每个设置的输出缓冲区
（`crates/bongocat-live2d/src/physics.rs` 的 `reset`），`step` 与 `interpolate` 都按输出
列表迭代：列表为空时循环体不执行，粒子照常模拟，结果无处可写。这正是官方 Framework 的行为
——`csmUpdatePhysics` 为每个设置计算粒子，再逐个 `Output` 写回，没有输出就没有回写。

因此空输出是**惰性**而不是**错误**：它描述的是"这个设置驱动不了任何参数"，而不是"这个
设置坏了"。

### 2. 保留的两条要求仍然有牙齿

- **输入项非空**：没有任何输入的设置无从反应，等于没有刚体。
- **至少两个顶点**：一个顶点挂不出摆锤，`reset` 也要靠前一个顶点递推初始位置。

两者都继续拒绝，且各自的反例被测试固定。

### 3. Meta 计数仍然照实校验

`TotalOutputCount` 为 0 是合法的，计数一致性检查（`Meta` 声明值必须等于数组实际长度）
不变。这条检查防的是元数据自相矛盾，与输出项是否为空无关。

## 后果

- 报告的模型与另一个同病模型（`魈二代 · 标准模式`）现在都能导入，行为为
  `Ready`，物理照常求值。
- 与 Cubism Framework 的一致性提高：两条路径现在对同一个文件给出同一个答案，而校验层不再
  比被参照的实现更严格。
- 只放宽了输出项。字段级规则（权重范围、`VertexIndex` 边界、Meta 计数、Id 唯一性等）
  一条未动，因此这不是一次泛化的"宽松化"，而是一处定向修正。
- 回归测试分两层：`bongocat-model` 固定"空输出合法、缺输入/缺摆锤仍拒绝、混合 rig 正常"，
  `bongocat-live2d` 固定"空输出设置被跳过而旁边的有效设置照常驱动参数"——后者防止跳过逻辑
  退化成"整个 rig 变成 no-op"而测试仍然通过。
