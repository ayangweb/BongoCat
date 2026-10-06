# ADR-0077：房间成员模型窗口与聊天归属

状态：接受。日期：2026-10-01。

房间内的本机成员复用主模型窗口；每个其他成员使用独立 RuntimeOwner、RenderConsumer
与 ProductOverlaySession。所有 GPU/window owner 留在产品主线程，模型加载留在设置 worker。
远端 session 不注册本机输入服务，也不播放 motion 音频。

聊天按服务端 member ID 路由，不通过昵称判定。尚未创建窗口时仅保留该成员最新消息；
离房、被踢或连接关闭时清空成员窗口计划与聊天。远端窗口退出顺序为停止 tick、解除
runtime 路由、停止 runtime 并 join、释放 renderer 与窗口。

第一步复用本机模型：成员广告提供模型 ID 和来源（preset/installed）；仅唯一匹配的本机 catalog entry 可以选择，
没有匹配或存在歧义时使用 standard 预置模型。模型获取约束于 2026-10-07 被 ADR-0080 替代：房间可自动获取并导入模型；不改变用户
自己的模型选择、配置或窗口持久化数据。模型加载失败也回退到 standard。

## 实施与退出条件

- [ ] Phase 5：成员窗口和聊天路由闭环；依赖现有 overlay/render commit contract。
  自动化覆盖 ID 路由、昵称重复、消息暂存、成员删除与清空，Windows 双客户端 smoke
  和 macOS 对应 smoke 通过后才标记平台验收完成。
  当前实现已通过 ID 路由、模型回退、屏幕内排列与 Windows 两个真实 D3D11 窗口的
  自动化验证；完整双客户端服务端联机和 macOS 实机验证尚未运行，保持未勾选。
- [ ] Phase 5：WebRTC 可靠有序键鼠边沿、latest-value 鼠标移动、溢出计数与 Reset。
  依赖成员窗口隔离；先完成数据通道契约与 loopback spike，再接入真实输入。
  离房、连接失败、重建和 shutdown 不得留下 pressed state。
  当前真实双客户端已通过实际部署服务的加入、SDP/ICE 信令、可靠边沿、反向鼠标按钮、
  独立鼠标位置、默认猫动作映射与断线 Reset 测试。尚无两台设备跨 NAT、TURN 或 macOS
  实机证据，因此平台验收保持未勾选。just room-peer-smoke 使用 BONGOCAT_TEST_SERVER_URL。

本 ADR 不把编译或合成测试当作双平台、多设备或 WebRTC 发布证据。

## WebRTC 与部署协议补充

采用 crates.io 最新稳定 webrtc 0.21.0（MIT/Apache-2.0）作为可替换的私有传输 adapter。
项目公共接口不暴露该库类型；Tokio 1.53.1 与 async-trait 0.1.92 是其 executor/handler
边界所需依赖，workspace 已有其传递依赖。webrtc-rs 当前维护 Sans-IO core 和 async driver；
Windows/macOS 使用 ring 加密后端，其 unsafe 位于依赖内部，不进入应用业务代码。
新增依赖须由完整编译、许可证/来源审计和数据通道实测验证后验收。

SDP/ICE 仅通过现有 peer:signal 转发，键鼠数据仅走 DTLS/SCTP。稳定成员 ID 排序决定
offer 发起方，避免双方同时协商；可靠有序通道传输键鼠边沿与 Reset，鼠标移动单独
使用 latest-value 路径。输入发送队列有界、溢出可观测并 Reset；窗口未就绪前不缓存
无限输入。连接关闭、离房和 shutdown 必须清空远端 pressed state。
传输由独立 worker 与有界信令队列驱动，不等待 REST 或 Socket.IO ack；信令发送留在
Tokio executor 之外，避免同步 rust_socketio 内部 executor 的嵌套冲突。每 500ms 状态校正，
2s 无输入心跳触发 Reset；控制调用和退出等待最多 2s。STUN 使用公共服务，当前不配置 TURN。

实际部署服务的 join 成功应答不含 memberId。客户端加入时提供唯一 join_token，
从服务端原样返回的 model.meta 中仅接受唯一匹配身份；有 memberId 时仍以它为准。
无法确认身份时发 room:leave 回滚真实成员，不留客户端未入房而服务端已入房的状态。
标记不写入配置，不使用昵称、成员顺序或成员数推断身份。

workspace cargo update 发现 rust-i18n-macro 4.2.4 展开会调用当前固定 rust-i18n 4.2.2
不存在的 replace_patterns_cow，阻塞 GPUI 编译。因此 lock 暂留 macro 4.2.2；owner 为
rust-i18n 上游，解除条件为项目固定 runtime 与 GPUI 依赖可一起升级并通过本地化检查。
rust-i18n-support 保持原有 4.2.4，cargo tree --invert rust-i18n-macro 可解释该约束。
