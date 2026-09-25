# ADR-0066: gilrs 手柄后端与平台窄适配边界

状态：已接受（2026-09-25）；fork 已补齐 callback lifetime、bounded queue/epoch、authoritative reset、bounded shutdown、compile guard 与 xinput 回归修复；Windows WGI 焦点矩阵、双平台物理设备与长期证据仍阻塞完成声明

## 背景

Native Rewrite 此前在 `bongocat-platform` 内分别维护 Windows XInput 轮询和 macOS
`GCExtendedGamepad` callback。两套代码重复承担设备发现、按钮映射、连接 generation、axis
归一化和 callback 生命周期，且产品兼容面被 XInput 0–3 slot 与 Apple extended profile 限制。手柄兼容、
驱动修复和平台 backend 问题应集中维护在第三方边界，而不是继续在 BongoCat 内扩展两套系统 API。

项目已有强类型 `InputEvent`、可靠 producer、generation-keyed gamepad-axis latest-value transport 和
runtime dead-zone；这些是产品语义，不能由第三方库类型替代。按 ADR-0030 的复用阶梯，应选择成熟的
跨平台 gamepad backend，再只保留 BongoCat 所需的窄适配。

## 决策

- 根 workspace 精确固定 `https://github.com/ayangweb/gilrs` 的 commit
  `035a1cac7a784ec0447d9baba0de02efa6937d22`，package 版本为 `gilrs 0.11.2` /
  `gilrs-core 0.6.8`。该 commit 包含 callback context ownership、bounded queue/epoch、authoritative
  reset、WGI/XInput bounded shutdown、macOS IOHID stop/join、target-scoped compile guard 与 xinput
  extreme-axis regression 修复；后续修复仍必须形成可审计的 patch series 并再次精确固定 commit。
  `deny.toml` 只放行该精确 git source；fork 的 SDL mapping submodule 由该 commit 固定为
  `15b5e9f4abfb1c5c691c468799816755a91a2e11`。`deny.toml` 通过 `required-git-spec = "rev"` 拒绝
  branch/tag git source。workspace dependency 不启用平台 feature；`bongocat-platform` 的 Windows target
  table 启用 `wgi`，macOS target table 保持 featureless 并由 gilrs-core 选择 IOHID。
- Windows 使用 gilrs 的 `wgi` backend；macOS 使用 gilrs 的 IOHID backend。BongoCat 不再声明
  `objc2-game-controller`，不直接加载 XInput DLL，也不自行维护 SDL mapping、hat/D-pad 解码或设备
  backend。
- `GilrsGamepad` 是 `bongocat-platform` 私有且双平台共用的窄 adapter。`gilrs::GamepadId`、
  `Button`、`Axis`、`Event` 和错误只在该文件内出现；runtime/UI 只接收项目自有的
  `GamepadConnection`、`GamepadButton`、`GamepadAxis`、`InputEvent` 和
  `GamepadAxisSample`。
- builder 关闭 gilrs 默认 jitter/dead-zone filter、force feedback 和
  `SDL_GAMECONTROLLERCONFIG` 环境映射，保留 fork 内置 SDL mapping，并关闭自动 state update 后由
  adapter 显式更新。D-pad axis 使用 gilrs 自带 `axis_dpad_to_button`，BongoCat 不复制 hat 解码。
  产品 dead-zone 仍只由 runtime 的 `GamepadAxisSettings` 决定。
- 一次 drain 最多消费 `256` 个 gilrs event，剩余 backlog 留在 gilrs 的 bounded backend/high-level pending
  queue，不能让手柄洪峰阻塞 Raw Input/CGEventTap。最大活动设备数仍为 `4`，与现有 24-key axis transport
  容量一致；gilrs
  backend id 不进入项目 API，adapter 分配 `0..3` 项目 slot，每次连接只通过
  `GamepadAxisProducer::connect` 分配新 generation。
- gilrs 的位置名与项目词表并非逐字相同：left/right trigger 逻辑按钮映射为项目 shoulder，
  `LeftTrigger2`/`RightTrigger2` 的连续值同时映射为项目 trigger button 与 trigger axis。连续值采用
  产品阈值 `value >= 0.5` press、`< 0.5` release；stick 使用 `[-1, 1]`，trigger 使用 `[0, 1]`。
  `C`、`Z`、`Mode`、`Unknown` 和未知 axis 不伪造项目 identity，只进入匿名拒绝/解码计数。
- 连接成功和任何已提交的全局 input Reset 后，adapter 调用 gilrs reset epoch，清空 fork queue 与
  cached state，要求 backend 重播 authoritative held button 与六轴 latest value；不分配新 generation。
  fork overflow 通过强类型 `BackendOverflow { dropped }` 事件可见，adapter 计数并触发同一 reset/reseed
  恢复。断开先淘汰该 generation 的 pending axis，再可靠发布 `GamepadDisconnected`。shutdown 清理项目
  connection/axis state并消费 gilrs 的 bounded shutdown acknowledgement，最终 Reset 仍由 input owner
  发布。
- gilrs 构造返回错误或构造阶段 panic 只禁用 gamepad adapter并增加一次匿名 backend failure；键鼠
  service 继续运行。fork worker 的 bounded reset/stop acknowledgement、overflow marker 和 callback
  quiescence 通过强类型 gilrs API/事件进入 adapter；旧 XInput 轮询次数/查询错误、GameController
  background policy 和 callback 临时 snapshot 等 backend 专用诊断从项目 contract 删除，只保留连接、
  断连、容量拒绝、按钮、axis、拒绝、backend failure、backend event discard 与通用 input service 指标。
- 后续手柄兼容、驱动差异、backend queue、callback 和 shutdown 修复全部在该 fork 完成并提升精确
  commit；BongoCat 不加入平行 XInput/GameController workaround。升级 fork 后必须重跑双平台 adapter
  contract、物理手柄矩阵、100-cycle restart 与长时间压力。

## 依赖审查

- `gilrs 0.11.2` 的许可证为 `Apache-2.0 OR MIT`，MSRV `1.84`，低于 workspace 的 Rust `1.97`。
- Windows WGI 和 macOS IOHID 的系统绑定及 unsafe 面积被限制在第三方包内；项目 adapter 保持
  safe Rust，第三方对象和错误不进入项目公共 API。替换边界是私有 `gilrs_gamepad.rs`，不会改变
  runtime/config/UI contract。
- 内置 SDL mapping 来自 fork 固定 submodule；该 database 有独立于 gilrs dual-license 的许可，
  构建和分发前必须把 `gilrs/SDL_GameControllerDB/LICENSE`、revision 和 attribution 纳入第三方
  合规清单。

## 已知阻塞

0. **Windows WGI 焦点矩阵：** gilrs 文档提示 Windows Gaming Input 可能需要关联且获得焦点的窗口。
   BongoCat Raw Input window 为 hidden，overlay 默认为 click-through，设置窗口也不保证聚焦。必须在
   Windows 10/11 实机验证 settings 开关、焦点/失焦、click-through、启动时已连接、重连和多手柄；若 WGI
   无法在这些状态可靠投递，不能改回 BongoCat 内 XInput workaround，而应继续修复 fork backend。
1. **双平台物理设备与系统生命周期：** 仍需在真实 macOS IOHID 和 Windows WGI 设备上验证 held-at-startup、
   held-at-reconnect、lost-release、overflow/reset epoch、100-cycle restart、锁屏/睡眠/快速用户切换和
   长时间无增长。cross-check、纯函数测试和无设备 smoke 不能替代这些证据。
2. **macOS 物理 callback 静止证明：** fork 已拥有持久 context、run-loop stop、close 和 bounded join，但
   TCC deny/grant/revoke、自然 timeout、设备移除及 callback in-flight 的静止/恢复仍需目标系统实测。

这些阻塞不授权在 BongoCat 内复制 backend，但禁止把当前状态写成双平台手柄完成或 stable 可发布。

## 验证

- 共享 adapter 单元测试固定 16 个项目按钮、trigger 连续值与独立 axis、trigger 有限范围、四设备
  上限/容量拒绝、slot 复用 generation、runtime stop 错误、Reset 后同 generation 重播和 backend
  overflow recovery。
- gilrs fork 的 queue/epoch、authoritative reset、WGI/XInput shutdown acknowledgement、macOS callback
  ownership/close 与 xinput extreme-axis test 已通过 fork 的 fmt/check/clippy/test；BongoCat Windows
  `cargo check/clippy/test`、WGI 无设备 context 初始化/关闭 smoke，以及 macOS
  `x86_64-apple-darwin` cross-check 已通过。macOS target 不再启用临时 `wgi` feature。
- 原 standalone XInput/GameController 手柄 probe、依赖、命令和 CI smoke 已删除；键鼠 Raw Input /
  CGEventTap spike 保留。物理 WGI/IOHID 矩阵、真实设备、物理 profile、热插拔和生命周期矩阵继续作为
  发布门禁。

## 被拒绝方案

- **继续维护平台 XInput/GameController backend**：拒绝，因为它重复设备发现、mapping 和生命周期
  修复面，并继续把驱动兼容问题锁在 BongoCat。
- **让 runtime/UI 直接消费 gilrs 类型**：拒绝，因为它破坏强类型项目 contract、第三方替换边界和
  runtime 单一状态所有权。
- **在 BongoCat 内修复 gilrs 的 macOS 生命周期、队列或 WGI 焦点问题**：拒绝；这些是 fork/backend
  问题，应在 fork 修复、测试并精确升级 commit。
- **用 crates.io `gilrs 0.11.2` 代替 fork**：拒绝；当前需要 fork 中与 D-pad/平台行为相关的修复，且
  后续兼容修复统一在 fork 维护。
