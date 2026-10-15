# ADR-0090：Windows 公共房间

状态：接受；平台实机验收待完成。

Windows 提供显式加入、最多八人的匿名公共房间，macOS 不提供联机入口。Ubuntu 服务端只依赖平台无关协议与网络库，不扩展桌面 Linux 支持。使用明文 WebSocket，连接信息可观察且不提供身份真实性；不放宽更新功能的 HTTPS 要求。

输入 owner 只输出合并的活动标记：键盘及鼠标按钮的有效按下触发，键值、坐标、设备、原始时间和释放消息永不进入联机接口。连接有效时每 200 ms 最多发送一个独立随机拍爪，120 ms 的释放由远端可注入单调时钟决定。断线丢弃活动。

协议 v1 为严格 serde 枚举，服务端持有成员和广播序号，发送者由连接确定。八人上限包括自己；服务端内存状态重启即丢失。连接代次隔离旧事件；成员变更不可静默丢失，有界队列溢出关闭连接并重建快照。心跳 10 s、超时 30 s；网络错误退避 1/2/4/8/15 s，房间满和协议不兼容要求手动重试。

Windows 远端会话只消费模型快照，不注册 Raw Input，不启动音频；复用现有 runtime、Cubism safe wrapper 与 D3D11 owner。临时 ID 原生标签归会话所有。GPUI 只持有表单草稿和强类型联机 handle，不拥有动画或 GPU。

新增网络依赖使用 Tokio / Axum / tokio-tungstenite，MIT 许可证，成熟 WebSocket 实现；新增版本实施时通过 crates.io API 核实最新稳定非 yanked 版本。替换边界为联机 client/server crate，库类型不暴露到公共 API，业务代码 forbid unsafe。需要的 futures-util 与随机库同样精确固定并审计。客户端不启用 TLS；服务端仅分发自身二进制，无 Cubism 或模型资源。

验收包含协议隐私、房间满、慢连接、重启、断线、旧配置、窗口初始化失败、原生标签 DPI、多猫资源释放和八小时 soak。自动测试不能代替 Windows 实机或 Docker 证据。

依赖审查（2026-10-14，crates.io API）：Tokio 1.53.3（MIT，MSRV 1.71）、Axum 0.8.9（MIT，MSRV 1.80）、tokio-tungstenite 0.30.0（MIT，MSRV 1.85）、futures-util 0.3.34（MIT/Apache-2.0，MSRV 1.71）、rand 0.10.3（MIT/Apache-2.0，MSRV 1.85）均为最新非 yanked 稳定版，支持本仓库 1.97.1 工具链。库内系统 IO 与 unsafe 留在服务实现，第三方类型不离开联机边界；共享协议、客户端和服务端业务 forbid unsafe。
