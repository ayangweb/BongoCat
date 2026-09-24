# ADR-0065: Runtime-Owned Random Model Behavior Scheduler

状态：已接受（2026-09-25）

## 背景

模型同时声明 motion 和 expression。设置页需要一个正交的开关和时间间隔，让用户在不让
GPUI 或 renderer 自己决定业务行为的情况下启用周期性随机播放。两个字段必须进入当前 v1
配置、typed settings protocol 和 runtime，而不能作为 UI-only 草稿或由 renderer 读取配置文件。

## 决策

- `model.random_behavior_enabled` 是唯一的启用门禁，默认 `false`；
  `model.random_behavior_interval_seconds` 默认 `30`，持久化范围为 `1..=3600`。
- `bongocat-runtime::RandomBehaviorSettings` 是 runtime 的唯一设置类型。
  `SetRandomBehaviorSettings` 携带 expected config revision 对应的强类型 command，并在
  `RuntimeSnapshot` 中回显；非法间隔在 runtime 和 config 边界都拒绝。
- runtime 从当前已成功提交的 model index 声明的 motion 与 expression 中均匀选择一个；
  这是合并后的行为列表等权，不是先以 50% 选择 motion 再以 50% 选择 expression。第一次选择等待
  一个完整间隔。成功模型切换、设置变更和重新启用都会重锚定时器；长暂停只选择一次，不追赶补发，
  时钟回退不会制造过去的触发。
- 自动 motion 使用 `Idle` priority，不替换正在进行的 `Normal` 或 `Force` 产品 motion。
  expression 仍遵守现有 expression 淡入、替换和模型 commit 清理语义。空行为列表是 no-op。
- 选择器是 runtime 内部的固定 seed PRNG；生产 seed 只用于进程内变化，测试可以注入 seed
  并用 `MonotonicClock` 得到可重复序列。renderer、GPUI Entity 和平台 callback 不参与选择。
- settings 页面把两个字段放在 Interaction 的模型分组中；间隔控件在开关关闭时置灰但保留
  已保存的值。配置写入仍使用原子、revision-checked 的 settings service。

## 验证

- `bongocat-config` fixture 覆盖默认 round-trip 和非法间隔；协议测试覆盖 typed command。
- runtime scheduler 单元测试覆盖固定 seed、间隔等待、禁用/模型切换重锚和时钟回退；另一个
  Core-backed runtime 集成测试验证模型在一个间隔后确实产生 motion 或 expression。固定 seed 的
  Core-backed 构造器仍属于后续测试基础设施，不把生产 seed 当作可重复性证据。
- 仍需双平台实机观察、长时间随机序列/时钟变化 soak，以及正式发布门禁；本 ADR 不把
  scheduler scaffold 宣称为完整 Live2D 兼容或平台发布证据。

## 后果

随机行为不再由第三方 `easy-live2d` helper 或旧版配置隐式继承；Native v1 明确拥有该产品
行为。配置初始关闭不会改变现有启动观感，用户打开后需要等待一个完整间隔才看到第一次选择。
