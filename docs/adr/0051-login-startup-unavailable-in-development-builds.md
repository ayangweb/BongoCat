# ADR-0051: 开发构建不提供登录时启动

状态：已接受（2026-09-20）
修订：ADR-0043（仅取代其"Development 构建从此支持启动项"这一条决策）
依赖：ADR-0008（应用身份与存储环境）、ADR-0043（启动项后端统一 auto-launch）

## 背景

ADR-0043 把双平台启动项后端统一到 `auto-launch` 时，把 Development 构建一并纳入支持范围：用不同的
`app_name`（macOS plist Label、Windows HKCU value name）隔离开发与生产的注册项，开发构建因此可以
在本机演练真实的注册生命周期。契约里随之保留了 `StartupItemUnsupportedReason::BuildEnvironment`，
但后端从不产生它（ADR-0043 原文：「保留为契约变体但当前无生产者，UI 对其的处理分支继续作为防御性
路径存在」）。

决定本 ADR 形态的事实：

1. **注册项指向的是"当前可执行文件"，而它的生命周期长于进程。** 登录时启动把
   `current_exe()` 写进操作系统的启动配置，用户下次登录时由系统拉起该路径。
2. **开发构建的可执行文件是构建产物，不是安装好的应用。** 它位于 Cargo 的 `target/` 下，
   `cargo build`、`cargo clean`、切换 profile 或直接删除目录都会替换或移除它。用户在开发构建里
   打开的开关会留下一个指向已过期或不存在的二进制的登录项，而产品里没有任何界面能解释这件事。
3. **契约里已经有一条为这件事准备好的状态。** `Unsupported(BuildEnvironment)` 与
   `SettingsStartupItemState::can_set_enabled()` 早已把"这个构建不支持该能力"表达成不可变更的状态，
   UI 的处理分支也一直存在，只是没有生产者。
4. **可见开关此前与它对外声明的语义不一致。** 没有动作可做时（快照未就绪、有待处理操作、
   平台不支持），辅助功能节点报 disabled，而可见开关仍然可以点击并发出命令（用户报告的正是这类
   "能点但没意义"的控件）。

## 决策

### 1. 门禁放在应用层，按构建环境判定

`bongocat-app` 的 `system_startup_item_state()` 与 `system_set_startup_item_enabled()` 在
`BUILD_ENVIRONMENT != Production` 时直接汇报
`SettingsStartupItemState::Unsupported(BuildEnvironment)`：

- 读取方向返回 `SettingsStartupItemStatus::State(Unsupported(BuildEnvironment))`，不调用
  `bongocat-platform`，因此开发构建连"查询系统启动项"都不会发生。
- 写入方向返回 `Ok(Unsupported(BuildEnvironment))`。这是一个 no-op 而不是错误：会发送该命令的开关
  已经被禁用，而残留窗口或脚本化客户端不应收到一个用户无法处理的失败。两个方向返回同一个值，
  客户端不会观察到能力在读取与写入之间变化。

判定写成"是否为 Production"而不是"是否为 Development"，使将来新增的构建环境默认不可用，
而不是默认获得注册能力。

选择应用层而不是平台层的理由与既有先例一致：`update_check_available()`（开发构建不安装发布产物）
同样把"这个构建产物是否具备该能力"判定在 `bongocat-app`，`bongocat-update` 只被当作能力实现方使用。
`bongocat-platform` 及其 `auto-launch` 后端、`StartupItemEnvironment`、两套 `app_name` 与 opt-in
lifecycle smoke 全部不变，Production 路径继续使用它们。

### 2. UI：开关按"没有动作可做"禁用，并用 Tooltip 说明构建原因

- `StartupItemPresentation` 增加 `unavailable_hint`，只有
  `Unsupported(BuildEnvironment)` 有值；其余状态（初始化中、可重试、已启用、已禁用、需批准、
  登录项缺失、平台/系统不支持）都是 `None`。
- 可见开关只在"这个构建根本不提供该能力"时禁用，由
  `StartupItemPresentation::switch_disabled()`（即 `unavailable_hint.is_some()`）单点定义。
  这与"此刻能否操作"是两个问题：后者由 `action` 回答，仍然只由辅助功能节点声明为 disabled /
  不可点击。保持这两者分离，是为了让已发布构建在快照尚未就绪时仍把开关呈现为正常可用——
  这正是本次要求的边界。
- 开关改为手写 `gpui_kit` 的 `Switch` 元素（`SettingField::element`）而不是打包的
  `SettingField::switch`：打包字段内部固定构造 `Switch::new("check")`，无法挂 Tooltip，
  而这一行是唯一能解释"构建不支持"的地方。
- 文案复用目录里已有的 `settings.application.startup.unsupported_build`：同一句话既解释该行，
  也作为禁用开关的悬停提示，两份不会分叉。

## 明确不做

- **不删除平台契约里的 `StartupItemEnvironment`、Development `app_name` 或 opt-in smoke**：
  Production 路径仍需它们，删掉会让"环境隔离"这件事失去唯一载体。
- **不在平台层再拦一次**：应用层是唯一生产者，重复判定只会产生第二个事实来源。
- **不为该状态新增文案键**：目录里已有同一句，第二条措辞只会带来漂移风险。
- **不做自动清理**：不在启动时扫描并删除历史上的 `BongoCat Development` 登录项。产品不删除它没有
  写入过的系统配置项（ADR-0008 的环境隔离原则），该项由用户手动移除。

## 后果

- 开发构建不再写 `BongoCat Development` 登录项。**已经存在的该登录项不会被自动清理**：
  macOS 是 `~/Library/LaunchAgents/BongoCat Development.plist`，Windows 是 HKCU Run 下的同名 value。
  本机（macOS）检查过该文件不存在，因此本次没有受影响的实际注册项。
- 开发构建不再能演练真实注册生命周期。该覆盖由 Production 构建的 `--startup-item-smoke`
  （`main.rs`，本就要求 `BUILD_ENVIRONMENT == Production`）与 CI 承担。
- `StartupItemEnvironment::Development` 在平台契约中仍然存在，但生产路径不会再构造它——应用层只会把
  `Production` 传下去。这条差异由 `bongocat-app` 的测试固定。
- macOS 12 用户与 Windows 用户不受影响：门禁只看构建环境，与平台和 OS 版本无关。
- 已发布构建里可见开关的行为不变：它只在"构建不提供该能力"时禁用，开发构建之外没有状态会让它变灰。
  初始化中与有待处理操作时，辅助功能节点仍报 disabled / 不可点击，而可见开关保持可用——这与改动前
  完全一致，本次只对齐了可见开关那一种情形（开发构建）。

## 残余风险与待验证项（不得当作已确认）

1. **禁用的 `Switch` 上悬停时弹层是否真的出现，没有自动化断言。** 弹层由 gpui-kit 的 managed
   tooltip 系统拥有（`Root` 的 `TooltipOverlay` 是 `pub(crate)`，内容字段私有，弹层元素未注册
   test-support），本仓库的测试层无法观察它。已核实的是触发路径可达：gpui-base 的
   `Switch`/`SwitchTrack` 在 disabled 下只停止 mouse-down 传播并丢弃点击回调，命中测试不受影响
   （`state_style.rs` 的 disabled 只投影 `opacity`），因此轨道上的 `on_hover` 仍会触发。
   实机悬停确认留给 `just dev` / `just dev-smoke`。
2. **未在 Windows 实机确认**：本机无法执行 `cfg(windows)` 路径。本项改动落在应用层与 UI 层，
   没有改平台代码。
3. **历史上已注册的开发登录项需要用户手动移除**，产品不提供入口，也不会提示。
4. **平台侧的 Development 分支现在只被它自己的 opt-in smoke 驱动**：
   `startup_item_native::startup_item_lifecycle_smoke_restores_original_state` 仍在
   `StartupItemEnvironment::Development` 上跑 disabled → enabled → disabled，而产品路径不会再走到
   那里。保留它是因为 Production 路径需要同一套后端构造，同时它仍是"环境命名隔离仍然成立"的证据；
   代价是本仓库里能观察到 Development 注册的地方只剩这个被 `--ignored` 标记的用例。

## 验证

已完成（2026-09-20，本机 macOS / aarch64）：

- `bongocat-app`：`login_startup_is_gated_on_the_build_environment` 断言
  `startup_item_available()` 等于 `BUILD_ENVIRONMENT == Production`（同一用例在开发与
  `--features production` 两种特性下都成立）；
  `a_development_build_reports_login_startup_as_unavailable` 断言开发构建的读取返回
  `State(Unsupported(BuildEnvironment))`，且 `true`/`false` 两个写入方向都返回
  `Ok(Unsupported(BuildEnvironment))` 而不是错误。
- `bongocat-ui`：`the_startup_switch_is_disabled_exactly_where_the_build_cannot_offer_it` 对全部平台
  状态 ×`blocked` 断言 `switch_disabled()` 恰好等于"构建环境不支持"；
  `a_development_build_disables_the_startup_switch_with_a_hover_hint` 固定开发构建的悬停文案
  （中文目录原文，且与 `description` 同源）；
  `the_startup_switch_stays_operable_in_every_actionable_state` 断言已发布构建能产生的三个可操作
  状态都不禁用、都不带悬停提示。
- 设置窗口 smoke 的启动项断言不变（辅助功能节点按 `action == None` 报 disabled/clickable/focusable），
  与可见开关各自的规则都由上面两个用例覆盖。

**未运行**：实机悬停确认 Tooltip 弹层（见残余风险 1）、Windows 实机。
