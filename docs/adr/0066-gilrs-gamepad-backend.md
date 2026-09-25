# ADR-0066: gilrs 手柄后端与平台窄适配边界

状态：已接受（2026-09-25）；macOS callback lifetime、backend stop/join、事件队列上界、initial state、Windows WGI 焦点矩阵、compile guard 与 fork 自身回归仍阻塞完成声明

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
  `f43af45c3106e48ff131b77bf8c148d9bd5cbed2`，package 版本为 `gilrs 0.11.2` /
  `gilrs-core 0.6.8`。不使用浮动 branch、registry 版本或本地 patch。当前 commit 只是集成
  baseline，不等于 fork 已经完成发布门禁；后续修复必须形成可审计的 patch series 并再次精确
  固定 commit。`deny.toml` 只放行该精确 git source；fork 的 SDL mapping submodule 由该 commit
  固定为 `15b5e9f4abfb1c5c691c468799816755a91a2e11`。`deny.toml` 通过
  `required-git-spec = "rev"` 拒绝 branch/tag git source。workspace dependency 不启用平台 feature；
  `bongocat-platform` 的 Windows target table 启用 `wgi`。当前 fork 的 `gilrs-core` compile guard
  没有 `target_os = "windows"` 条件，macOS target table 也必须暂时启用 `wgi` 才能编译；该 feature
  不会编译 Windows backend 源，macOS 仍由 gilrs-core 选择 IOHID。fork 修复 compile guard 后应立即
  删除 macOS 的临时 feature。
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
- 一次 drain 最多消费 `256` 个 gilrs event，剩余 backlog 留在 gilrs 队列，不能让手柄洪峰阻塞
  Raw Input/CGEventTap。最大活动设备数仍为 `4`，与现有 24-key axis transport 容量一致；gilrs
  backend id 不进入项目 API，adapter 分配 `0..3` 项目 slot，每次连接只通过
  `GamepadAxisProducer::connect` 分配新 generation。
- gilrs 的位置名与项目词表并非逐字相同：left/right trigger 逻辑按钮映射为项目 shoulder，
  `LeftTrigger2`/`RightTrigger2` 的连续值同时映射为项目 trigger button 与 trigger axis。连续值采用
  产品阈值 `value >= 0.5` press、`< 0.5` release；stick 使用 `[-1, 1]`，trigger 使用 `[0, 1]`。
  `C`、`Z`、`Mode`、`Unknown` 和未知 axis 不伪造项目 identity，只进入匿名拒绝/解码计数。
- 连接成功和任何已提交的全局 input Reset 后，adapter 先重播相同 connection，再从 gilrs 已缓存的
  state 重新发布当前 held button 与六轴 latest value；不分配新 generation。backend 仍必须提供
  authoritative initial state、reset epoch 和 lossless release contract 才能完成该语义。断开先淘汰该 generation 的
  pending axis，再可靠发布 `GamepadDisconnected`。shutdown 清理项目 connection/axis state并 drop
  gilrs context，最终 Reset 仍由 input owner 发布。
- gilrs 构造返回错误或构造阶段 panic 只禁用 gamepad adapter并增加一次匿名 backend failure；键鼠
  service 继续运行。构造后的 worker health/panic 仍必须由 fork 提供可观测状态。
  旧 XInput 轮询次数/查询错误、GameController background policy 和 callback 临时 snapshot 等
  backend 专用诊断从项目 contract 删除，只保留连接、断连、容量拒绝、按钮、axis、拒绝、backend
  failure、backend event discard 与通用 input service 指标。drain 中已收集但因 runtime queue
  拒绝而未处理的 event 会计数；fork 仍需提供 reset epoch/purge，不能把该计数当作无损保证。
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

0. **P0 macOS callback lifetime：** pinned `gilrs-core` 在 `gamepad.rs` 中把临时 tuple 的地址注册为
   IOHID callback context；tuple 语句结束后指针即悬空，而 IOHID 仍会长期保留它。该问题必须在 fork
   中改为持久化 owned context，直到 callback unregister、run-loop stop 和 worker join 后才释放；在此
   之前不得运行或宣称 macOS physical input。
1. 当前 fork 的 macOS `gilrs-core` 启动 IOHID run-loop thread 后没有 stop channel、run-loop stop 或
   join handle；drop `Gilrs` 后线程仍永久运行。当前替换不得宣称 macOS clean shutdown、100 次
   restart 或无线程增长完成。adapter 已把该依赖归因反映为最终 `clean_shutdown=false` /
   `service_status=Failed`，即使键鼠 callback 与 final Reset 正常，也不能把整个 input service 标成
   clean。修复必须进入 fork，并为 stop acknowledgement、callback quiescence、bounded join 和
   100-cycle 增加测试。
2. WGI 与 IOHID backend 当前使用 unbounded `std::sync::mpsc`，gilrs 还维护内部 event deque。每个
   BongoCat tick 的 256-event drain 只保护键鼠服务延迟，不能证明长期 backlog 有界。fork 必须提供
   bounded queue、明确 overflow 事件/计数和 reset 语义；在此之前不能完成手柄 edge 零丢失门禁。
3. gilrs 文档提示 Windows Gaming Input 可能需要关联且获得焦点的窗口。BongoCat Raw Input window 为
   hidden，overlay 默认为 click-through，设置窗口也不保证聚焦。必须在 Windows 10/11 实机验证
   settings 开关、焦点/失焦、click-through、启动时已连接、重连和多手柄；若 WGI 无法在这些状态可靠
   投递，不能改回 BongoCat 内 XInput workaround，而应修复或更换 fork backend。
4. macOS IOHID callback/context 与 raw handle 的安全不变量仍需随上述 stop/join 修复一起复核，并补
   callback panic/禁用/权限变化后的静止证明。
5. WGI backend 虽有 stop channel 和 `Drop` join，但当前 join 没有有界 deadline，worker 的
   registration/conversion/send 失败路径含 panic/expect，join failure 只记录日志而没有稳定
   acknowledgement。无设备 smoke 只能证明本次 context 能返回，不能证明异常路径和 2 秒 shutdown
   contract；修复必须进入 fork。
6. gilrs high-level state 在 backend 初始时可能为空；当前 adapter 的 snapshot 只能重播 gilrs 已
   缓存的 state，不能保证进程启动或重连时立即得到 authoritative held button/axis。fork 必须
   提供初始状态读取或完整 snapshot，并覆盖 held-at-startup、held-at-reconnect 和迟到事件。
7. fork 的可选 `xinput` feature 自身仍有 extreme-axis regression；该 feature 不是本产品路径，
   但在把 fork 当作可审计 baseline 前必须修复或在 ADR 中明确接受其范围。
8. `gilrs-core` 当前对 `xinput`/`wgi` 的 compile guard 缺少 `target_os = "windows"` 条件，导致
   macOS 在 `default-features = false` 时也必须暂时解析 `wgi` feature。该 manifest workaround
   只为绕过 fork 的 build-time guard，不启用 Windows backend 源；应在 fork 中修正 guard 后删除。

这些阻塞不授权在 BongoCat 内复制 backend，但禁止把当前状态写成双平台手柄完成或 stable 可发布。

## 验证

- 共享 adapter 单元测试固定 16 个项目按钮、trigger 连续值与独立 axis、trigger 有限范围、四设备
  上限/容量拒绝、slot 复用 generation、runtime stop 错误和 Reset 后同 generation 重播。
- Windows `cargo check/clippy/test` 与 WGI 无设备 context 初始化/关闭 smoke 已在本地通过；
  Windows target 解析 `wgi`。macOS target 由于 fork compile guard 暂时也解析 `wgi`，但不编译
  Windows backend 源；`x86_64-apple-darwin` cross-check 已证明 IOHID adapter 类型边界。物理 WGI
  矩阵、fork guard 修复、真实设备、物理 profile、热插拔和生命周期矩阵继续作为发布门禁。
- 原 standalone XInput/GameController 手柄 probe、依赖、命令和 CI smoke 已删除；键鼠 Raw Input /
  CGEventTap spike 保留。

## 被拒绝方案

- **继续维护平台 XInput/GameController backend**：拒绝，因为它重复设备发现、mapping 和生命周期
  修复面，并继续把驱动兼容问题锁在 BongoCat。
- **让 runtime/UI 直接消费 gilrs 类型**：拒绝，因为它破坏强类型项目 contract、第三方替换边界和
  runtime 单一状态所有权。
- **在 BongoCat 内修复 gilrs 的 macOS 生命周期、队列或 WGI 焦点问题**：拒绝；这些是 fork/backend
  问题，应在 fork 修复、测试并精确升级 commit。
- **用 crates.io `gilrs 0.11.2` 代替 fork**：拒绝；当前需要 fork 中与 D-pad/平台行为相关的修复，且
  后续兼容修复统一在 fork 维护。
