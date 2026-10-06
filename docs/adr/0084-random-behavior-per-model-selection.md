# ADR-0084: 随机播放按模型勾选参与的动作和表情

状态：已接受（2026-10-06）
补充：ADR-0079（新增配置字段的可选读取约定）、ADR-0066（按任务组织的设置信息架构）
需求：Issue #1108

## 背景

Issue #1108 报告：「设置 > 模型行为 > 随机播放动作或表情」已经能选范围（关闭 / 仅表情 / 仅动作 /
表情和动作）并设间隔，但只要选了某一类，当前模型该类的**所有**动作和表情都会被随机到。有些动作和表情
与整体画风不符，用户现在只能把整个随机播放关掉，或者把间隔调得很大来降低出现频率。

这不是缺少一个筛选器，而是范围与成员被压进了同一个控件：mode 用一个枚举表达「哪一类」，于是「这一类里
的哪一个」无处安放。加一个布尔「排除」列表会把同一件事表达两次——正向勾选与排除项在换模型后必然留下
残留的排除项，而正向列表是用户心里的问题。

同时已经存在的边界让这件事比看起来更微妙：

- 随机播放走 `renderer.set_expression` / `start_motion`，不经过 command 队列，因此它天然不会进入
  `model.last_expressions`（ADR 既有边界）。新筛选必须留在同一条路径上，否则自动行为会开始改写用户的
  记忆集合。
- 一个模型的行为按**声明顺序**列出，`behavior_id` 是配置里唯一的拼法（快捷键绑定已经用它）。成员选择
  因此必须用 `behavior_id` 而不是下标：包更新后声明顺序可能变，下标会让同一个选择指向另一个行为。
- 设置窗口的每一行、每一个 document 都要能被搜索和解释，而这一行的行数取决于模型声明了多少行为。

## 决策

### 1. `model.random_behavior.included` 是每模型一行的列表，`null` 与空列表是两个状态

`Option<Vec<RandomBehaviorInclusion>>`，每行是 `{ model: ModelIdentity, behavior_ids: Vec<String> }`。
默认值 `null`，字段带 `#[serde(default)]`。

`null` 与 `Some(vec![])` 必须可区分，因为它们回答不同的问题：

- `null`：**没有人做过这个选择**，因此模式允许的每一个行为都参与。这也是该字段加入之前每一份文档的
  含义，所以缺字段的旧文档按无筛选加载，而不是被严格 v1 入口拒绝后走「最新有效备份 → 默认配置」。
- 空行：**这个模型什么都不自己播**，设置页在界面上写明这一点。

不带 `#[serde(default)]` 的必填新字段会让该字段加入之前写下的每一份 `config.json` 解析失败，
进而走恢复流程并丢掉用户设置——与 ADR-0079 §1 同一理由。

一个模型在列表里没有自己那一行时同样什么都不播。因此**第一次写入这个列表时必须给每一个模型建行**：
否则用户在一个模型上点一下勾选框，会让所有没打开过的模型一起哑掉。这是一个由「无行即不播」推出的
实现约束，不是可选项。

每行的 `behavior_ids` 用与快捷键绑定相同的 `behavior_id` 写法，并按现有 `ModelBehaviorParseError`
规则校验：一个不是行为的字符串、同一行里重复的一个，都是文档缺陷而不是「反正选不到」的行为。一个模型
最多一行——两行会让答案取决于解析顺序。

### 2. 筛选在 runtime，命令携带模型身份

新增 `RuntimeCommand::SetRandomBehaviorInclusion(Option<RandomBehaviorInclusion>)`，其中
`RandomBehaviorInclusion` 是 `{ model: ModelId, model_origin: ModelOrigin, behavior_ids: BTreeSet<String> }`。

**它不并入 `RandomBehaviorSettings`**，因为两者形状与寿命都不同：mode 与间隔是描述调度的两个标量、
`Copy`、只在启动和设置变更时换；成员集合是拥有所有权、按模型归属的集合，把它塞进 `Copy` 的设置里会
迫使热路径上的 scheduler 也背上一次 `Arc` 克隆。分开之后每个值只有一种形状，两条命令也不会彼此覆盖。

模型身份随集合一起走是**关键**：runtime 用 `belongs_to` 判定它是否属于当前生效的模型，不属于就当作没有
筛选。少了这一层，一次迟到的选择会作用到「恰好此刻在屏幕上」的那个模型上——正是共用一份设置时会出现的
故障。身份同时也是「不发布任何东西」的依据：文档整体为 `null` 时上一次的选择因为身份不匹配本来就等于
无筛选，所以配置里从未用过这个功能的用户在切换模型时不会产生任何 runtime 流量。

Application 在每次 `prepare_model` / `select_model` 之后为新模型重新发布并等待；`set_random_behavior_inclusion`
写完配置后发布的是**当前生效模型**那一行，而不是用户刚编辑的那一行——发布后者会让屏幕上那个模型保持
无筛选，正是用户在另一个模型的页面上勾选却看到没反应的原因。

### 3. mode 与成员是两个独立筛选器，任何一方都能把候选集清空

`poll` 先按 mode 收窄类别，再按成员集合收窄，然后才抽取。只勾了动作而 mode 是「仅表情」时什么都不播，
**不会**退回播一个被 mode 排除的动作：mode 是用户对「播什么类别」的决定，不是两个集合之间的排序
（与既有 mode 语义一致）。

空候选集保持无操作，不重排定时器也不降级到「全部」。随机 expression 继续走 renderer 而不是 command
队列，所以它依然不会进入 `model.last_expressions`。

### 4. 页面：mode 收窄的是列表，不是答案

勾选列表的行是**当前模型**按声明顺序声明的行为中 mode 允许的那些；未勾选状态读自
`random_behavior_inclusion`，`null` 画成全部勾选。行标签复用 Shortcuts 页面同一份编号（「动作 3」在两页
指同一个行为），而不是在这里另算一套。

点击发送的是**整份勾选集合**而不是单个勾选框：存储值就是集合，而空集合是用户能到达的状态，只发一个
变化会让服务猜「加」还是「删」，并让「一个都没勾选」无法表达。点击以**存储的答案**为基线再施加这一次
勾选，因此：

- mode 收窄时隐藏的行仍然留在集合里——改回 mode 时它们还在，不需要重新勾；
- 这一次勾选权威地覆盖基线，而不是与基线合并——否则在「仅动作」下取消一个动作，它会留在集合里，与
  用户刚点的框相反。

页面与 interval 行共用同一个门禁：mode 为 `off` 时整行置灰，mutator 也拒绝，两者一起读作一组控件。
一个都没勾选时列表下方显示一行说明，避免用户从「全空的复选框」猜结论。

## 后果

- 勾选框决定实际播放内容；未勾选的动作和表情不再自己出现。mode 与间隔行为不变。
- runtime 的热路径没有新增分配：筛选仍是对同一份声明列表的两次 `filter`，不构造第二份候选集合。
- `RandomBehaviorSettings` 保持 `Copy`，设置页的 model settings 分组也不变——两者都不受本字段影响。
- 自动化契约全部落在真实实现上：
  - `bongocat-runtime`（`a_selection_never_draws_outside_what_the_user_checked`、
    `an_empty_selection_leaves_the_scheduler_with_nothing_to_play`、
    `a_selection_of_the_other_kind_leaves_the_scheduler_idle`、
    `a_selection_only_applies_to_the_model_it_names`）在 scheduler 层钉住筛选、空选择、两个筛选器的
    关系与模型身份；断言覆盖 24 个 seed × 12 次抽取，因为单次抽取落在允许项上是运气；
  - `bongocat-app`（`a_written_selection_is_the_only_thing_the_scheduler_draws_from`、
    `selecting_nothing_stops_the_scheduler_while_selecting_nothing_yet_does_not`、
    `a_selection_the_mode_excludes_leaves_the_scheduler_idle`、
    `each_model_keeps_its_own_selection_across_a_switch`、
    `a_model_absent_from_a_hand_written_document_plays_nothing`、
    `the_runtime_and_the_settings_page_read_one_selection`、
    `a_model_absent_from_a_hand_written_document_plays_nothing`、
    `saving_a_selection_on_one_model_leaves_the_others_alone`、
    `writing_a_selection_sorts_it_seeds_every_model_and_rewrites_one_row`）跑通「配置 → projection → runtime 命令 →
    真实 preset 模型 → scheduler 抽取」。这一层是唯一能覆盖三处 `behavior_id` 拼法（config 解析、
    runtime 筛选、settings 页面构造）的地方：任何一处漂移都会表现为「筛选掉全部」或「没筛选」，而拼法
    本身在运行时不可见，所以测试从结果反推而不是去比较字符串；
    `automatic_draws_over` 用 **command sequence** 而不是「快照里当前是哪个动作」来识别一次抽取，因为
    motion 完成后仍留在快照里，否则「什么都没选」无法断言；
  - `bongocat-ui`（`an_absent_selection_draws_every_visible_behavior_checked`、
    `the_mode_narrows_the_list_without_unchecking_the_rest`、
    `an_off_mode_offers_no_rows_to_check`、`a_written_selection_drives_every_checkbox`、
    `an_empty_selection_draws_nothing_checked`、
    `a_click_sends_the_whole_set_including_nothing`、
    `a_click_under_a_narrower_mode_keeps_the_rows_it_hides`、
    `the_rows_are_labelled_the_way_the_shortcuts_page_labels_them`、
    `a_model_without_declared_behaviors_offers_no_rows`、
    `the_picker_is_inert_while_the_mode_is_off`、
    `a_checkbox_sends_the_whole_selection_for_the_model_it_was_rendered_from`、
    `the_random_behavior_picker_refuses_a_click_while_the_mode_is_off`）钉住列表与整份集合；
  - `bongocat-config`（`an_absent_random_behavior_selection_is_not_an_empty_one`、
    `a_random_behavior_selection_is_parsed_and_holds_one_row_per_model`）从序列化后的当前默认里删掉
    该字段模拟旧文档的字节，并钉住 `null` 与空列表的区分、解析与每模型一行；
  - `shared/config/fixtures` 新增 1 份 accept 与 4 份 reject 用例（无法解析的 id、重复 id、重复模型、
    非法模型 id）。
- 未运行实机验证：只在本机（macOS）跑了 `just check` 与 `tools/` 下的校验脚本，没有做 macOS 或 Windows
  实机 smoke。复选框列表在 800×600 与 Windows 125/150/200% 下的观感需要实机确认。
