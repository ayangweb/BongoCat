# ADR-0072: Random Behavior Becomes a Mode Enumeration

状态：已接受（2026-09-27）

## 背景

ADR-0065 给模型空闲随机播放定义了两个字段：`model.random_behavior.enabled` 门禁和
`model.random_behavior.interval_seconds` 间隔，候选集合恒为模型声明的 motion 与
expression 合并列表。这只回答了「要不要随机」和「多久一次」，没有回答「随机什么」。

用户反馈这个组合解决不了两类真实需求：只想让模型换个表情（保持它对按键的姿态响应
完全由自己控制），或者只想让它自己动一动（不改表情）。旧实现把「动作或表情」写死在
一个布尔门禁后面，用户要么两个都要，要么两个都不要。

## 决策

- `model.random_behavior.enabled` 被 `model.random_behavior.mode` 取代。mode 是单一枚举：
  `off`（默认）、`expressions`、`motions`、`motions_and_expressions`，以
  `snake_case` 持久化。关闭是它的一个取值，不再是独立布尔字段。
- 关闭是枚举而不是保留布尔门禁，是因为「开关 + 模式下拉」要用户先回答两个问题才能
  读懂一行设置，而且它允许「打开但没有可播内容」这种没人主动要求的状态。单一枚举
  让每个配置只有一个答案，也就是设置页那一行下拉显示的答案。
- 默认值是 `off`：全新配置从未被要求自己动起来，这也是 v1 文档原本描述的行为，
  因此本决策不改变任何现有配置的实际效果。
- mode 先收窄候选集合再抽取，而不是先抽取再丢弃。这样选中项一定属于用户允许的类别，
  且 `motions_and_expressions` 是一个等权池子，不是两类各占 50%。
- 模型没有声明所选类别的行为时保持无操作，不退回另一类。mode 是用户对「播什么」的
  决定，不是两个都可用集合之间的优先级。
- 间隔在 mode 为 `off` 时仍须合法（`1..=3600`，默认 `30`），因此一个把越界值寄存
  在关闭状态下的配置会被拒绝，而不是等到用户打开时才发现。
- runtime 与 settings protocol 各自拥有自己的 mode 枚举，与 log level 一样由
  `bongocat-app` 的 `config_projection` 负责四处边界转换。runtime 的枚举紧挨着它过滤的
  `ModelBehaviorSnapshot`，protocol 的枚举是窗口渲染下拉所需的完整目录。
- 设置页用统一门禁规则（ADR-0053）：mode 下拉本身只被结构性阻塞禁用，两个下拉在开关关闭
  时置灰但保留已选值 —— 这里 mode 下拉扮演门禁控件的角色，所以它走 `disables_switch()`
  语义（永远不因自身取值而禁用），间隔行走 `disables_controls()`。否则用户一旦关掉就
  再也打不开。
- 间隔与 mode 仍是同一个持久化对象、同一个 typed command。两次独立命令会让「先改 mode
  再改间隔」的快速点击用第二个命令的旧快照覆盖第一次的结果，用户的第一次选择被静默撤销；
  因此两个控件都从仍在途中的值出发，debouncer 把其余变化收敛成一次后续提交。
- 该字段直接进入当前 v1，不提供迁移、旧字段探测或 fallback。按仓库既定策略，
  仍持有 `enabled` 的旧文档按损坏处理，走「最新有效备份 → 默认配置」。

## 验证

- `bongocat-config` 覆盖四个取值都通过文档往返、持久化拼写等于文档化的
  `snake_case` 名、目录外的取值被拒而不是忽略、间隔在 `off` 下同样被校验；JSON Schema
  与全部共享 fixture 同步。
- `bongocat-runtime` 覆盖 `off` 在 24 个 seed 下从不选择行为、每个单一类别 mode 在
  24 个 seed 下从不越界、合并 mode 确实能选到两类、模型只声明一类时另一类 mode 保持
  无操作、关闭 mode 与模型 commit 都清除待触发截止时间。
- `bongocat-app` 覆盖 mode 经 config → runtime → settings 全链持久化并在重启后恢复，
  越界间隔仍使配置与 runtime 一起保持不变。
- `bongocat-ui` 覆盖四个选项在全部语言下非空、唯一且可逆，mode 与待发间隔合成同一次
  patch，`off` 保留用户已选的间隔，关闭状态下改间隔不触达服务，以及打开/关闭的连续选择
  排成一条 typed command 链；settings smoke 断言新增双语 key 非空。
- 变异验证：把 `admits` 的两个类别对调、让 `off` 也进入候选过滤、把 `is_active` 反向读取、
  或让间隔 mutator 去掉 `off` 守卫，都会让上述测试变红。
- 未运行：Windows 10 1903+ 与 macOS 12+ 实机目视确认下拉在 125/150/200% 与 Retina 下的
  排版，以及长时间随机序列 soak 与双平台 GPUI 渲染门禁。这些仍属 ADR-0065 与 ADR-0066
  的完成门禁，本 ADR 的自动化证据不替代它们。

## 后果

用户可以在「表情」「动作」「两者」之间三选一，也可以整体关闭，且默认行为与之前一致。
配置少一个布尔字段，多一个枚举字段；两者都是 v1 直接变更。`enabled` 消失后，
`RandomBehaviorSettings` 只剩 mode 与间隔，runtime 不再需要「门禁」与「模式」两个概念
去描述同一件事。
