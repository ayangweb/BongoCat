# ADR-0066: gilrs 手柄后端与平台窄适配边界

状态：已接受（2026-09-25）；fork 已补齐 callback lifetime、bounded queue/epoch、authoritative reset、bounded shutdown、compile guard 与 xinput 回归修复；2026-09-26 追加 macOS IOHID worker run-loop 空转修复（空闲 CPU 98% → 0%）；2026-10-02 追加 WGI 焦点门控读取修复与 WGI raw analog axis 取值域修复；2026-10-05 追加 WGI raw element 顺序修复与 SDL 轴槽位翻译的实测回退（见 Issue #1099）；双平台物理设备与长期证据仍阻塞完成声明

## 背景

BongoCat 此前在 `bongocat-platform` 内分别维护 Windows XInput 轮询和 macOS
`GCExtendedGamepad` callback。两套代码重复承担设备发现、按钮映射、连接 generation、axis
归一化和 callback 生命周期，且产品兼容面被 XInput 0–3 slot 与 Apple extended profile 限制。手柄兼容、
驱动修复和平台 backend 问题应集中维护在第三方边界，而不是继续在 BongoCat 内扩展两套系统 API。

项目已有强类型 `InputEvent`、可靠 producer、generation-keyed gamepad-axis latest-value transport 和
runtime dead-zone；这些是产品语义，不能由第三方库类型替代。按 ADR-0030 的复用阶梯，应选择成熟的
跨平台 gamepad backend，再只保留 BongoCat 所需的窄适配。

## 决策

- 根 workspace 精确固定 `https://github.com/ayangweb/gilrs` 的 commit
  `9a5ee3d6c36db25871f6ea67eafaf09c637a0c50`，package 版本为 `gilrs 0.11.2` /
  `gilrs-core 0.6.8`。该 commit 包含 callback context ownership、bounded queue/epoch、authoritative
  reset、WGI/XInput bounded shutdown、macOS IOHID stop/join、target-scoped compile guard 与 xinput
  extreme-axis regression 修复；再追加 macOS IOHID worker 的 run-loop 空转修复：worker 曾以 null
  mode 调用 `CFRunLoopRunInMode`，CoreFoundation 会立即返回而不等待，使无条件创建的空闲
  backend 持续占满一个 CPU 核；修复传入真实 run-loop mode，空闲 CPU 由 98% 降到 0%，
  `reset` 仍在约 8ms 内 ack、`shutdown` 约 105ms join，并新增 `idle_backend_does_not_spin`
  回归测试；再追加 WGI 焦点门控读取修复：Windows 只把 mapped 的
  `Windows.Gaming.Input.Gamepad` 读取投递给拥有前台窗口的进程，而 gilrs 在
  `Gamepad.FromGameController` 成功（即 XInput 设备）时固定选它，使这类设备只在 BongoCat
  窗口获得焦点时才有输入，HID 设备不受影响因为它们走 `RawGameController`。修复改为设备只要
  暴露任何 raw report 就优先用 `RawGameController`，mapped 读取仅作为完全无 raw report 时的
  回退；该选择同时决定 element 列表、`AxisInfo` 取值域和 SDL mapping 查询键，因为三者都描述
  raw 布局；再追加 WGI raw analog axis 的取值域修复：backend 曾用 SDL 的预居中换算
  `(value * 65535.0) - 32768.0` 生成 raw sample，同时把 `EvCodeKind::Axis` 声明为 `i16`
  取值域，而 gilrs 会再归一化一次，导致静止摇杆落在 `i16::MIN` 并被读成 `-1.0`、
  负方向一半行程被钳制为最大偏移，且映射到按键的模拟扳机按有符号域读取时静止值正好是半程、
  压在 axis-to-button 阈值上。最后追加 WGI raw element 顺序修复（Issue #1099）：
  `native_ev_codes` 沿用上游 gilrs 的 evdev 下标，而 `RawGameController` 按 Windows Gaming
  Input 自己的顺序报告元素，`Mapping::default` 又按 element code 身份匹配，于是四个面键整体错位
  一位、把 D-pad 报告成按键的设备十字键四个方向全部落到表外而失效、两个模拟扳机分别落到右摇杆
  Y 轴和左扳机上——静止摇杆因此正好压在 axis-to-button 阈值上而让被它驱动的按键逐采样抖动，
  这就是 Switch 模式下"模型鬼畜地疯狂按下所有按键"。修复分两半：`native_ev_codes` 改带 WGI
  原始下标（面键 `SOUTH/EAST/WEST/NORTH` = 0..3，D-pad 落在 10..13，Guide 之后的元素排在
  15 之后以免借用别的按键下标），`Gamepad::axes` 则把 raw element 按 SDL 映射的槽位顺序
  呈现（六个轴时 `[0, 1, 4, 2, 3, 5]`），使 `lefttrigger:a2` 解析到左扳机而不是右摇杆 X 轴；
  非六轴设备保持自身顺序，因为没有任何来源描述它。后续修复仍必须形成可审计的 patch series、
  推送到 fork `master` 并再次精确固定 commit。
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

0. **Windows WGI 焦点矩阵（已在 fork 修复，实机证据仍缺）：** gilrs 文档提示 Windows Gaming Input
   可能需要关联且获得焦点的窗口。Issue #1082 证实了这个门控真实存在：BongoCat Raw Input window 为
   hidden，overlay 默认为 click-through，设置窗口也不保证聚焦，因此 mapped 读取让 XInput 模式手柄
   只在 BongoCat 窗口获得焦点时才有输入。fork 已改为优先使用不受焦点门控的 `RawGameController`，
   但仍必须在 Windows 10/11 实机验证 settings 开关、焦点/失焦、click-through、启动时已连接、重连和
   多手柄；在取得该证据前不得把焦点矩阵写成已验证，若 raw 路径在这些状态仍不可靠，应继续修复 fork
   backend，而不是改回 BongoCat 内 XInput workaround。
0b. **Windows WGI raw element 顺序（已在 fork 修复，实机证据仍缺）：** Issue #1099 报告 Xbox 兼容
   模式下面键错位、十字键无响应、左爪默认姿态错误，Switch 模式下模型疯狂按下所有按键，而 DS4
   模式正常。该症状与 `native_ev_codes` 的 evdev 下标和 WGI raw 顺序不一致完全吻合：没有 SDL
   mapping 的设备走 `Mapping::default` 时面键错位、十字键落到表外，两个模拟扳机则落到右摇杆 Y 轴
   和左扳机上；Switch Pro 只有四个轴，其第三轴（正是右摇杆 Y）因此被当成左扳机，静止时按有符号
   域归一化后正好是 `0.5`，压在 axis-to-button 阈值上逐采样抖动，这就是"鬼畜"。有 SDL mapping 的
   设备走 `parse_sdl_mapping`，其 `a2`（左扳机）同样落到 WGI 的右摇杆 X 轴。fork 已按上文把
   `native_ev_codes` 改为 WGI 原始下标，并把 `Gamepad::axes` 的呈现顺序改为 SDL 槽位顺序；轴顺序
   依据是 Microsoft 的 `Raw game controller` 文档，按键顺序依据是 Windows 手柄的实际报告顺序
   （Microsoft 不跨设备固定它，设备在注册表 `Labels\Buttons` 自报），因此仍必须在 Windows 10/11
   实机逐一确认面键、十字键、摇杆与扳机，确认 SDL mapping 命中的设备不再错位，并确认非六轴设备
   （Switch Pro、DS4）在自身顺序下的表现。切换 `crates/bongocat-platform/Cargo.toml` 的 Windows
   feature 到 `xinput` 可让 Xbox 兼容模式正常但让 Switch/DS4 完全无反应，这是 `xinput` backend
   只认 XInput 设备的预期行为，不作为回退方案。
1. **双平台物理设备与系统生命周期：** 仍需在真实 macOS IOHID 和 Windows WGI 设备上验证 held-at-startup、
   held-at-reconnect、lost-release、overflow/reset epoch、100-cycle restart、锁屏/睡眠/快速用户切换和
   长时间无增长。cross-check、纯函数测试和无设备 smoke 不能替代这些证据。
2. **macOS 物理 callback 静止证明：** fork 已拥有持久 context、run-loop stop、close 和 bounded join，但
   TCC deny/grant/revoke、自然 timeout、设备移除及 callback in-flight 的静止/恢复仍需目标系统实测。
3. **WGI 默认 mapping 的轴位置错位（fork 待修，本仓库不兜底）：** 没有 SDL mapping 的
   Windows 控制器走 `gilrs::mapping::Mapping::default`，它按 `gilrs-core` 的
   `windows_wgi::native_ev_codes` 表把位置名绑定到事件码，而该表的 axis 下标写的是
   `LSTICKX=0, LSTICKY=1, RSTICKX=2, LT2=3, RT2=4, RSTICKY=5`，与 Windows
   Gaming Input 的 raw 报告顺序不符。Microsoft 的 `Raw game controller` 文档逐字写明
   Xbox 手柄的六个 raw 轴是 `LeftThumbstickX=0, LeftThumbstickY=1, RightThumbstickX=2,
   RightThumbstickY=3, LeftTrigger=4, RightTrigger=5`。raw reading 的 element 列表来自
   `collect_axes_and_buttons`（设备自己的 `AxisCount`），`Mapping::default` 按「下标在列表里
   吗」过滤，因此 LT/RT 与右摇杆 Y 轴互相错位：`LT2=3` 实际读到右摇杆 Y，`RT2=4` 实际读到
   左手扳机（两个扳机对调），`RSTICKY=5` 实际读到右手扳机。同一张表的 button 下标也按
   evdev 排列（`BTN_WEST=0, BTN_SOUTH=1, …`），而 raw button 顺序由设备的 registry
   `Labels\Buttons` 决定、没有跨设备固定顺序，所以 button 侧无法在没有证据的情况下改。
   影响面：只覆盖 SDL database 里没有 GUID 的 Windows 控制器；有 mapping 的设备按元素下标
   解析（`parse_sdl_mapping` 用 `axes.get(from)`/`buttons.get(from)`），因此不受影响。
   需要的修法：只把 axis 六个下标改成上表的 raw 顺序，并加一条 fork 回归测试断言
   `Mapping::default` 在一个六轴十五键的 raw 设备上把 `LeftStickX/Y`、`RightStickX/Y` 绑到
   `0/1/2/3`、把两个模拟扳机绑到 `4/5`。改下标不影响 mapped reading——那条路径按事件码
   身份（`nec::AXIS_LSTICKX` 等）收发，从不读下标。
   本仓库**不**在 `gilrs_gamepad.rs` 里补偿：补偿就要在 adapter 复制一份第三方位置表，并在
   上游修好之后静默变成双重修正。fork 修复、推送 `master` 与再次精确 pin 之前，该缺陷只在
   Windows 无 mapping 的控制器上可见。

这些阻塞不授权在 BongoCat 内复制 backend，但禁止把当前状态写成双平台手柄完成或 stable 可发布。

## 验证

- 共享 adapter 单元测试固定 16 个项目按钮、trigger 连续值与独立 axis、trigger 有限范围、四设备
  上限/容量拒绝、slot 复用 generation、runtime stop 错误、Reset 后同 generation 重播和 backend
  overflow recovery。
- fork 侧 `raw_axis_value_is_neutral_at_rest_and_spans_the_signed_range`、
  `raw_axis_value_stays_monotonic_across_the_whole_range`、
  `a_signed_raw_element_is_read_over_the_range_its_mapping_asks_for` 和
  `mapped_axis_info_leaves_an_already_matching_range_untouched` 固定 WGI raw analog axis 的取值域
  契约：静止读数为 0、两端到取值域端点、映射成按键时改按无符号域读取。`wgi` 与 `xinput` 两个
  feature 的 `cargo test` 均通过，`cargo fmt --all -- --check` 干净；`cargo clippy
  --no-default-features --features wgi --all-targets` 无新增告警（`gilrs/build.rs` 的
  `useless_borrows_in_formatting` 告警在 `master` 上已存在，不属本次改动）。
- 该修复只由纯函数回归测试覆盖。Issue #1083 与 #1099 报告的 Switch 模式手柄"模型鬼畜"是否由取值域
  与 element 顺序两处缺陷完全解释，仍需 Windows 10/11 实机加该手柄确认；未取得该证据前不得声称手柄
  输入完成。
- gilrs fork 的 queue/epoch、authoritative reset、WGI/XInput shutdown acknowledgement、macOS callback
  ownership/close 与 xinput extreme-axis test 已通过 fork 的 fmt/check/clippy/test；BongoCat Windows
  `cargo check/clippy/test`、WGI 无设备 context 初始化/关闭 smoke，以及 macOS
  `x86_64-apple-darwin` cross-check 已通过。macOS target 不再启用临时 `wgi` feature。
- fork 侧 `a_device_with_a_raw_report_is_polled_without_the_foreground_window` 与
  `a_device_without_a_raw_report_falls_back_to_the_mapped_reading` 固定读取来源的选择规则：设备只要
  暴露任何 raw report 就走不受焦点门控的 `RawGameController`，完全没有 raw report 才回退 mapped
  读取。焦点门控本身是 Windows 行为，无法用单元测试断言；该修复仍需 Windows 10/11 实机加 XInput
手柄确认背景投递。未取得该证据前不得声称焦点矩阵已验证。
- 共享 adapter 侧新增 `the_shoulder_and_the_analog_trigger_keep_their_own_buttons_and_artwork`
  与 `only_the_analog_trigger_reports_a_continuous_axis`，逐个名字钉住第三方
  `LeftTrigger`/`RightTrigger`（肩键）与 `LeftTrigger2`/`RightTrigger2`（模拟扳机）到项目
  shoulder/trigger 按钮与美术名的对应，以及只有模拟扳机同时是连续轴；这是 ADR-0070 修过的
  互换关系的防回归。
- fork 侧 `the_position_names_carry_the_windows_gaming_input_axis_order`、
  `the_face_buttons_carry_the_windows_gaming_input_button_order`、
  `a_device_that_reports_its_dpad_as_buttons_reaches_every_direction`、
`the_raw_elements_are_exposed_in_the_order_the_device_reports_them` 与
  `every_code_the_mapped_reading_emits_is_in_the_element_list` 固定 WGI raw element 顺序契约：
  位置名携带 WGI 原始下标、十字键落回真实下标、raw element 按设备报告顺序呈现且每个下标都可达、
  mapped 回退路径发出的每个 code 都在 `BUTTONS`/`AXES` 里。五处变异（把左扳机放回 evdev 下标、把面键
  放回 evdev 下标、把十字键推回表外、让轴下标整体偏移、把某个 code 从 `BUTTONS` 里换成重复项）分别
  让对应测试变红。fork 的 `cargo test --workspace --features wgi` 与 `cargo clippy --workspace
  --all-targets --features wgi` 通过；轴顺序依据 Microsoft `Raw game controller` 文档，按键顺序
  依据实机报告顺序，两者都仍需 Windows 10/11 实机逐键确认。
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
