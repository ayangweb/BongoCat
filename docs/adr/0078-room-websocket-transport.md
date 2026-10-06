# ADR-0078：房间连接直接使用 WebSocket

状态：接受。日期：2026-10-06。

## 决策

房间 Socket.IO adapter 直接使用 WebSocket transport，不先进行 HTTP polling
握手或 polling → WebSocket upgrade。复用 rust_socketio 0.6.0 的
`TransportType::Websocket`，不新增依赖、配置字段或平台差异。

## 依据与边界

rust_socketio 默认 `TransportType::Any` 先进行 polling 握手。Vercel 的
[WebSocket 文档](https://vercel.com/docs/functions/websockets)明确要求 Socket.IO
客户端直接使用 WebSocket transport。浏览器 HTTP 健康检查不能证明该连接路径可用。

服务端必须支持 WebSocket；仅支持 polling 的服务端不在该连接协议范围内。
namespace `/bangocat`、默认 transport path `/socket.io/`、ack 超时和断线清理保持原有协议。

该决策只解决连接传输，不保证 Vercel 多实例房间一致性。当前服务端房间存在进程内存中；
Vercel 不保证不同连接抵达同一实例，且连接受函数最大运行时间限制。
跨实例状态协调与断线重连后的房间恢复仍需独立实现与验证。

## 验证与退出条件

使用现有本地服务端入房集成测试验证直接 WebSocket 的连接、ack 与离房清理。
已通过 `cargo fmt --all -- --check`、`cargo clippy --locked -p bongocat-app --all-targets -- -D warnings`
和 `cargo test --locked -p bongocat-app --lib multiplayer::`（8 passed，4 ignored）。
指定本地服务端后单独运行 `create_room_enters_as_host_and_refreshes_lobby_against_server -- --ignored`
通过，覆盖连接、建房、大厅刷新与离房。其余双客户端/WebRTC 集成测试本次未运行。
线上需另行复测两个客户端的连接、入房、聊天、信令与断线清理；未取得该证据前
不得宣称 Vercel 联机部署整体完成。
