# BongoCat 跨机联机 · 设计定稿

> 仓库：`Hyperbola-QAQ/BongoCat`（fork of `ayangweb/BongoCat`）
> 基线：`linux-wayland` 分支（PR #1114 + `Layer::Overlay` 修正），已编译验证
> 状态：设计已确认，待实现

---

## 0. 已定决策

| 项 | 决策 |
|---|---|
| 角色模型 | 不使用 host/viewer 枚举，改为「推送」+「展示」两个独立能力 |
| 默认行为 | 展示目标默认为 `[self]`，即未选择设备时回退 host 模式 |
| 多猫显示 | 多进程，每个展示目标一个渲染进程 |
| 服务端拓扑 | 中心化星型 relay |
| relay 形态 | 与 GUI 同一个二进制，`--relay` 子命令 |
| 地址配置 | 下拉预设 + 手动输入都支持 |
| 跟随逻辑 | 「跟随本地服务器」为默认且不可编辑；「手动指定」可覆盖 |
| UI 结构 | 三部分：客户端 / 展示设备 / 服务端 |

---

## 1. 总体架构

```
                ┌─────────────────────────────────┐
                │  relay（同一二进制的 --relay）    │
                │  token 分组 · 纯广播 · 不存储     │
                └───┬──────────────────────┬──────┘
                    │                      │
      ┌─────────────┴───────┐    ┌─────────┴──────────┐
      │ PC（主实例）         │    │ laptop（主实例）    │
      │ publish ✔           │    │ publish ✘          │
      │ display [pc-01] ✘   │    │ display [pc-01] ✔  │
      │  → 无窗口           │    │  → 渲染进程 ×1     │
      └─────────────────────┘    └────────────────────┘
```

**进程角色只有两种：**

- **主实例**：读配置、连 relay、负责本机输入捕获、按需 fork 渲染子进程
- **渲染子进程**：`--render-only --target <device-id>`，不捕获输入、不读配置，只订阅一个设备并渲染

一台机器上：主实例 1 个 + 渲染子进程 N 个（N = 远端展示目标数）。本机自己的猫由主实例直接渲染，不额外 fork。

---

## 2. 配置模型（`config.json` 新增 `network` 节）

```json
{
  "network": {
    "device": {
      "id": "pc-01",
      "name": "PC"
    },
    "client": {
      "enabled": true,
      "source": "follow_local",
      "url": null,
      "token": null,
      "publish": true,
      "display": ["self"]
    },
    "server": {
      "enabled": true,
      "listen": "dual_stack",
      "address": null,
      "port": 8787,
      "token": "AUTO_GENERATED"
    }
  }
}
```

字段语义：

| 字段 | 取值 | 说明 |
|---|---|---|
| `device.id` | 字符串 | 首次启动生成 `主机名-abcd`（4 位随机），持久化，可手改 |
| `client.source` | `follow_local` \| `manual` | `follow_local` 时 `url`/`token` 被忽略 |
| `client.url` | `ws://...` | 仅 `manual` 生效 |
| `client.token` | 字符串 | 仅 `manual` 生效；`null` 表示沿用 `server.token` |
| `client.publish` | bool | 是否推送本机输入 |
| `client.display` | 数组 | `["self"]` 默认；`[]` = headless；`["pc-01"]` = 只看远端；`["self","pc-01"]` = 混合 |
| `server.listen` | `local_only` \| `ipv4` \| `dual_stack` \| `custom` | 预设四档 |
| `server.address` | 字符串 | 仅 `custom` 生效 |

**跟随规则的实现**：`follow_local` 时，实际连接地址由 `server.port` 推导为 `ws://127.0.0.1:{port}`，token 直接取 `server.token`。UI 上该字段只读并显示推导结果。

`server.enabled` 时，若 `server.token` 为空则自动生成（32 字节随机 → base64url，约 43 字符）。

---

## 3. 协议

WebSocket，JSON 文本帧。`v` 为协议版本，不匹配直接拒绝并回 `error`。

### 3.1 前置改动：给输入类型补 serde

`InputControl` 等类型当前只有 `Clone/Copy/Debug/PartialEq/Ord`，**没有 serde**。需在 `crates/bongocat-input/src/input.rs` 为以下类型加 `#[derive(Serialize, Deserialize)]`：

- `InputControl`、`PhysicalKey`、`MouseButton`、`GamepadButtonKey`、`GamepadConnection`、`GamepadButton`、`GamepadAxis`

序列化结果（external tagging）：

```json
{"Key": 30}
{"Mouse": "Left"}
{"Gamepad": {"connection": {...}, "button": "South"}}
```

> 备选是为 net crate 定义镜像类型做转换。不推荐：类型会重复，且上游新增输入类型时容易漏。

### 3.2 消息

**握手（C → S）**

```json
{ "v": 1, "type": "hello", "payload": {
    "token": "…",
    "device": { "id": "pc-01", "name": "PC", "publish": true }
} }
```

**接受（S → C）**

```json
{ "v": 1, "type": "joined", "payload": {
    "peerId": "pc-01",
    "peers": [
      { "id": "laptop", "name": "laptop", "publish": false }
    ],
    "serverTime": 1762000000000
} }
```

**输入广播（S → 同房间其他人）**

```json
{ "v": 1, "type": "input", "payload": {
    "src": "pc-01",
    "seq": 1024,
    "t": 1762000000123,
    "control": { "Key": 30 },
    "bounds": { "x": 0, "y": 0, "width": 2560, "height": 1440 }
} }
```

- `t` 为发送端单调时钟（毫秒），**仅用于该设备内部排序与丢包判断**，不跨设备对齐
- `seq` 单调递增，接收端发现跳号只记诊断，不重传（输入事件过期即无意义）
- `bounds` 仅鼠标位移/点击需要，用于坐标映射

**设备上下线（S → 全体）**

```json
{ "v": 1, "type": "peer_joined", "payload": { "id": "mac-02", "name": "mac", "publish": true } }
{ "v": 1, "type": "peer_left",   "payload": { "id": "mac-02" } }
```

**心跳**：`ping` / `pong`，客户端 15s 一次，30s 无响应判定断线并重连（指数退避 1s→30s，上限 5 次后提示）。

**错误**

```json
{ "v": 1, "type": "error", "payload": { "code": "bad_token", "message": "invalid token" } }
```

`code` 取值：`bad_version` / `bad_token` / `rate_limited` / `internal`。

---

## 4. relay 服务端

同一二进制，`--relay` 子命令。**分支必须在 GPU/窗口初始化之前返回**，不加载图形栈。

```
bongocat-app --relay --listen dual_stack --port 8787 --token <t>
```

职责：

- 校验 token，按 token 分组（**token 即房间**，不同 token 互不可见）
- 广播 `input` 给同房间其他连接
- 维护设备在线表，上下线时通知房间
- 不存储任何输入历史

实现要点：

| 项 | 做法 |
|---|---|
| 栈 | `tokio` + `axum`（WebSocket upgrade）或 `tokio-tungstenite` |
| 状态 | `Arc<RwLock<HashMap<TokenHash, Room>>>`，`Room { peers: HashMap<DeviceId, Peer> }` |
| 双栈 | `dual_stack` 时绑定 `[::]` 并显式 `set_only_v6(false)` |
| 限流 | 每连接令牌桶：连续事件合并到 120Hz，总量 500 msg/s，超出回 `rate_limited` |
| 单连接上限 | 每设备 1 条连接，重复 `hello` 踢掉旧连接 |
| 优雅关闭 | SIGTERM → 广播 `peer_left` → 关闭 |

体积估计 250–300 行。

### 4.1 部署位置：跑在 **laptop**，不跑在 PC

这个选择是架构性的，不是性能性的 —— 两种放法在延迟上完全相同（都是一次网络传输），差别在于把复杂度放在哪台机器上。

```
PC ──(一条 WS)──► laptop[relay + 渲染]
```

选 laptop 的理由：

- **消费端即中心**：laptop 同时是 relay 和 viewer，PC 退化成纯发布者，只需要一个 `--publish-url`，本机不起额外进程、不开额外端口
- **PC 侧零负担**：PC 是游戏机，不该承担"给别的设备转发"的角色，也不该成为别人的地址依赖
- **扩展自然**：以后加第三台设备，全部连 laptop 即可，配置只在一处

对配置的直接后果：

| 机器 | `server.enabled` | `client.source` |
|---|---|---|
| laptop | `true` | `follow_local`（自动 `ws://127.0.0.1:8787`） |
| PC | `false` | `manual`，填 `ws://<laptop-ip>:8787` |

注意 laptop 需放行 8787 入站；PC 连的是 laptop 的局域网地址，不是 `127.0.0.1`。

反过来只在一种情况下成立：**laptop 不是常开的那台**。若录制是偶尔为之，而 PC 一直开着，则由 PC 承担 relay 更稳。

---

## 5. 客户端实现

### 5.1 接入点

输入链路的唯一汇聚点是 `InputProducer.submitter: Arc<dyn InputSubmitter>`（`crates/bongocat-input/src/input.rs:535`）。

**推送**：在创建 `InputProducer` 处（`crates/bongocat-overlay/src/linux/mod.rs:164`）注入装饰器：

```rust
struct TeeSubmitter {
    inner: Arc<dyn InputSubmitter>,
    tx: Sender<RemoteInput>,     // 有界队列，满了丢连续事件而非阻塞
}
impl InputSubmitter for TeeSubmitter {
    fn submit(&self, ev: InputEvent) -> Result<(), InputSubmitError> {
        let _ = self.tx.try_send(to_remote(&ev));   // 旁路，失败不影响主链路
        self.inner.submit(ev)
    }
}
```

关键约束：**旁路必须非阻塞**。网络卡顿绝不能反压到输入主链路，否则猫会卡住。

**展示**：`--render-only` 模式下不调用 `start_linux_input()`，改为启动一个线程从 WS 收事件，构造 `InputEvent` 后走同一个 `InputProducer` 提交。渲染层完全无感。

### 5.2 坐标映射

远端 `bounds` 描述的是发送端显示器（多屏时为所有显示器并集）。接收端按自己的主显示器尺寸等比映射，并夹取到边界内。若两端分辨率不同，按比例缩放而非直接使用绝对像素。

### 5.3 线程模型

```
主线程        GUI / 渲染
输入线程      平台输入 → submitter
网络线程      tokio runtime（一个，进程内共享）
桥接          网络线程 → 有界 mpsc → 渲染消费
```

---

## 6. 多进程渲染

主实例启动流程：

1. 读配置
2. `server.enabled` → 先拉 relay 子进程（`--relay`），等待端口就绪（最多 3s）
3. `client.enabled` → 建立 WS 连接
4. `display` 含 `self` → 主实例自己开窗口，输入走本地
5. `display` 中的远端 ID → 逐个 fork：
   ```
   bongocat-app --render-only --target <id> --url <resolved> --token <t> --device <self-id>
   ```

子进程约定：

- 不读配置文件（参数全部显式传入），避免与主实例竞争写
- 不捕获输入，不申请 pkexec
- 退出时由主实例回收；主实例退出前先发 SIGTERM 给所有子进程

失败处理：某个子进程崩溃，主实例记录并最多重启 3 次，仍失败则在 UI 该设备行标注「渲染失败」。

---

## 7. UI（设置页「联机」标签）

### 7.1 客户端

```
[开关] 启用联机

服务器   ( ) 跟随本地服务器   ( ) 手动指定
         └ follow: ws://127.0.0.1:8787            [只读]
         └ manual: [ws://192.168.1.20:8787     ]  (含下拉历史/预设)

Token    [••••••••••••••••••••] [显示] [粘贴]

[开关] 推送本机输入
```

### 7.2 展示设备

```
[✓] 本机 (pc-01)
[✓] laptop       在线 · 3ms
[ ] mac-02       离线
[ ] studio-pc    在线 · 12ms

提示：全部取消 = 无窗口模式（仅推送）
```

列表数据来源：relay 的 `joined` / `peer_joined` / `peer_left`，每项显示在线状态与往返延迟（由 `ping/pong` 测得）。

### 7.3 服务端

```
[开关] 启用服务器

监听     [下拉] 仅本机 / 局域网 IPv4 / 双栈 (IPv4+IPv6) / 自定义
         └ 自定义: [0.0.0.0          ]
端口     [8787]

Token    [自动生成的值          ] [复制] [二维码]

状态：已监听 [::]:8787 (双栈) · 1 台设备在线
```

### 7.4 状态栏（贯穿三部分）

```
● 已连接 ws://192.168.1.20:8787 · 房间 2 台 · 上行 18 msg/s · 下行 0 msg/s
```

连接异常时此处变为橙色并给出原因（`bad_token` / `timeout` / `refused`）。

---

## 8. 实现顺序

| 阶段 | 内容 | 验证方式 |
|---|---|---|
| P1 | 给 `InputControl` 等补 serde；建 `crates/bongocat-net`（协议类型 + 编解码单测） | `cargo test -p bongocat-net` |
| P2 | `--relay` 子命令 + 房间广播 + 心跳 | 两台终端手连，`websocat` 观察 |
| P3 | 主实例推送（TeeSubmitter）+ 连接管理 + 配置读写 | laptop 起 relay，PC 只推送，laptop 端打印事件 |
| P4 | `--render-only` 子进程 + 坐标映射 | laptop 显示 PC 的猫 |
| P5 | UI 三部分 + 状态栏 | 设置页手动操作全流程 |
| P6 | 多目标并发、崩溃重启、headless 托盘 | 三设备场景 |

P1–P4 完成即达到你的核心需求（laptop 看 PC 的猫），P5/P6 是产品化。

---

## 9. 风险与待定

| 项 | 说明 |
|---|---|
| serde derive 触碰上游文件 | 改动约 7 行 derive，冲突面极小；上游若新增输入类型需补一行 |
| `submitter` 注入点 | 需确认 `InputProducer` 构造处可外部传入装饰器；若不可，需在上游加一个 `with_submitter`（约 10 行） |
| 多进程 GPU 开销 | 每个渲染进程一套 GPU 上下文；3 个以内无压力，需实测显存 |
| headless 无退出入口 | **已确认可行**：v2 自带 `LinuxSystemTray`（`main.rs:166` 导入、`2519` 持有），headless 时用托盘退出/打开设置，无需补开发 |
| 跨分辨率坐标 | 目前按比例映射；若两端宽高比不同会产生偏移，需实测调整策略 |
| token 明文传输 | 局域网可接受；跨网必须 wss 或 Tailscale，UI 上需明示 |

---

## 10. 不在本期范围

- 鼠标轨迹拖尾（需在 overlay 内新增绘制层）
- 手柄数据跨机传输（协议已预留，先只做键鼠）
- 公网直连 / NAT 穿透
- 多房间并行订阅

---

## 11. 实施进度（实测）

### 11.1 已落地

| 阶段 | 内容 | 状态 |
|---|---|---|
| P1 | 输入类型补 serde；`crates/bongocat-net`（`protocol` / `relay` / `client` / `token`） | 完成，`cargo test -p bongocat-net` 通过（含真实 loopback socket 的端到端房间测试） |
| P2 | relay：星型中继、双令牌桶限流、设备 id 冲突软退役、心跳 | 完成；`curl` 实测得到 `101 Switching Protocols` |
| P3 | `TeeSubmitter` 旁路推送；卡键兜底（5s 静默释放）；`--publish-url/-token/--device-id` | 完成 |
| P4 | `--watch` 跟随远端；`--hide` headless；`Layer::Overlay` 修正 | 完成，laptop 实测跟随 PC 按键（三帧截图 md5 不同） |
| P5 | 配置节 `network` + 设置页「联机」三部分 + 状态 | 进行中 |

### 11.2 与本章前面描述的偏差

设计稿里 P3/P4 只写了命令行参数。实际实现把**配置也当作一等入口**：

- `config.json` 的 `network` 节（`device` / `client` / `server`）由设置页编辑；
- 启动时的**生效计划** = 命令行覆盖配置（`plan()` in `net_publish.rs`）：
  - `--publish-url` 出现即强制「手动指定」地址来源；`--watch` 强制跟随任意设备；
  - 其余取 `network.client` / `network.server`；
  - `client.source = follow_local` 时地址固定为 `ws://127.0.0.1:<server.port>`，房间密钥取 `server.token`——否则一份文档的两半会描述两个不存在的房间。
- `server.enabled` 为真时 relay 与宠物**同进程**运行（独立线程），所以 laptop 一个进程既是服务端又是展示端；
  `--relay` 仍然是无窗口的独占模式，用于只需要中转的机器。
- 服务端开启但没有房间密钥时，启动时生成并**写回配置**（`ensure_network_server_token`），
  否则每次重启都会换钥匙，所有对端都要重新配一遍。

### 11.3 生效时机（当前取舍）

改设置页的联机部分**需要重启 BongoCat 才生效**，页面上有明确说明。原因：relay 是一个绑定
到进程生命周期的 socket，房间在启动时只 join 一次；运行中重读配置会留下一个仍监听旧端口的
relay。CLI 参数仍然是即时生效的，所以它不受影响。

### 11.4 状态显示

`bongocat-net` 带一个进程级状态快照（`status.rs`）：一个进程最多服务一个 relay、最多加入一个
房间，所以状态是「取一份当前值」而不是订阅流——设置窗口每秒轮询 revision，不需要在输入管线
里传句柄。状态包括：relay 的监听地址、房间连接状态（未连接/连接中/已连接 N 台/重连中/已失败）、
以及当前正在跟随哪台设备。

「已连接 N 台」的 N 在设置页单独占一行（房间内其他设备）：relay 在回答 join 时给出已经在房间里
的设备数，之后的 `peer_joined` / `peer_left` 再增减。N=0 是有效值而不是空——它表示「已进房间，
对面还没来」，和「未连接」是两件事，所以只有未连接/连接中/重连中/已失败才显示「不在房间内」。

### 11.5 与上游的冲突面

- `crates/bongocat-input/src/input.rs`：只加 derive 和一个 `submitter()` 访问器；
- `crates/bongocat-overlay/src/linux/wayland.rs`：`Layer::Top` → `Layer::Overlay`（Hyprland 下 Top 会被全屏窗口遮蔽）；
- 新增 `crates/bongocat-net/`，不改动任何既有 crate 的公共 API（`BindMode` 新增 `Custom(IpAddr)` 是唯一例外，且是加法）。

### 11.6 已知的既有失败（非本次引入）

`cargo test --workspace` 在本 fork 上剩 3 个失败，全在 `bongocat-update`：该 crate 只为
macOS/Windows 发版，`HOST_TARGET_TRIPLE` 没有 Linux 变体，所以
`host_target_is_one_of_the_shipped_combinations`、
`availability_requires_a_production_channel_and_a_signing_key`、
`unavailability_names_the_gate_that_closed` 在 Linux 上必然失败。与联机改动无关，未改。

另一处已修：`settings::tests::projection::the_running_system_reports_the_capability_it_actually_gates_input_behind`
在上游只认 `input_monitoring`（macOS）和 `administrator`（Windows）。本 fork 跑在 Linux 上，
实际值是 `linux_libinput`，因此已把 Linux 的能力名加进断言。
