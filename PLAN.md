# BongoCat v2 跨机联机改造方案

> 目标仓库：`Hyperbola-QAQ/BongoCat`（fork 自 `ayangweb/BongoCat`，master = v2.4.1）
> Wayland 基础：PR #1114（`linux-wayland`，head `b4901ed`，+7657/−408，仍在 open）
> 开发/编译：laptop（Omarchy / Hyprland）`~/Work/Projects/BongoCat`
> 运行：PC（Arch / Hyprland）为 host，laptop 为 viewer

---

## 0. 关键结论

| 需求 | 现状 | 工作量 |
|---|---|---|
| Wayland 输入捕获 | **PR #1114 已实现**（libinput helper + 独立提权进程） | ≈0，合入即可 |
| Wayland 窗口显示 | **PR #1114 已实现**（SCTK layer-shell）；`Layer::Top` 需改 `Overlay` 才能盖全屏游戏 | 1 行 |
| 跨机联机 | **v2 完全没有**，这是唯一空白 | 主要工作量 |

之前把「Wayland」估成主要工作量，是因为误 fork 了 v1.1.0 的 Tauri 版本 —— 那个架构（GTK/WebKit + rdev）和 PR #1114（SCTK + libinput）完全不同，代码无法复用。fork 到正确的 v2 之后，这个判断不成立。

---

## 1. 为什么 v2 加联机比想象中简单

v2 的输入层已经把一切收敛成统一的领域类型（`crates/bongocat-input/src/input.rs`）：

```rust
pub struct PhysicalKey(u16);      // 物理键码，布局无关 —— 天生适合跨机传输
pub struct MonotonicMillis(u64);
pub enum   MouseButton { ... }
pub enum   InputControl { ... }    // 键盘 / 鼠标 / 手柄 的统一表示
```

链路是：

```
平台输入 (CGEventTap / Raw Input / libinput)
      ↓  platform_input.rs（含队列、去抖、reconcile、诊断计数）
   InputControl
      ↓
   overlay 渲染（live2d-render 驱动模型动作）
```

**只要在 `InputControl` 这个位置做替换，主机和渲染都不用改。**

- host：`InputControl` 产出后，多一个「旁路」把它序列化发出去
- viewer：生产者从平台输入换成网络事件流，下游完全无感

因此新增能力全部落在「边缘」，核心渲染栈、Live2D、窗口、模型管理零改动。

---

## 2. 仓库与分支策略

```
origin    git@github.com:Hyperbola-QAQ/BongoCat.git     # fork of ayangweb
upstream  git@github.com:ayangweb/BongoCat.git
```

| 分支 | 内容 |
|---|---|
| `master` | 只跟随上游 fast-forward，不留本地提交 |
| `linux-wayland` | 合入 PR #1114（含 `Layer::Overlay` 修正），作为基线 |
| `feat/multiplayer` | 联机功能，基于 `linux-wayland` |

同步：

```bash
git checkout master && git fetch upstream && git merge --ff-only upstream/master
git checkout linux-wayland && git rebase master
git checkout feat/multiplayer && git rebase linux-wayland
```

**PR #1114 若被上游合入**，`linux-wayland` 分支可直接丢弃，`feat/multiplayer` rebase 到新 master。

---

## 3. 新增模块

```
crates/
├── bongocat-net/                    ← 新增
│   ├── src/lib.rs                   协议类型、序列化
│   ├── src/client.rs                WebSocket 客户端（房间、重连、心跳）
│   ├── src/host.rs                  host 侧：InputControl → 网络
│   └── src/viewer.rs                viewer 侧：网络 → InputControl 事件流
└── bongocat-input/
    └── src/remote_source.rs         ← 新增：实现与平台输入相同的事件源接口
```

依赖（新增，全部 workspace 级）：

```toml
tokio = { version = "1", features = ["rt", "net", "sync", "time"] }
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
```

> 不引入 async 到渲染线程：网络线程独立，通过 `std::sync::mpsc` 把 `InputControl` 交给渲染，与现有输入队列的消费方式一致。

---

## 4. 协议设计

不必重新发明。`JMTeCHNoLOGY/BongoCat` 的 Go 服务端（`BongoCat-mp/server/`，MIT）已经把房间、速率限制、快照、身份都做完了，且经过测试。可以直接复用其**消息信封**，只替换 payload：

```json
{ "v": "v1", "type": "input", "payload": { "event": { ... } } }
```

payload 用 v2 的 `InputControl` 直接序列化：

```rust
#[derive(Serialize, Deserialize)]
struct RemoteInput {
    t: u64,          // MonotonicMillis，用于回放/丢包排序
    control: InputControl,
    bounds: Option<Bounds>,   // 鼠标绝对坐标需要显示器信息才能映射
}
```

要点：

- **`PhysicalKey(u16)` 布局无关**，两端键盘布局不同也不会错位 —— 这是 v2 相对 v1 的天然优势
- 鼠标位置用「物理像素 + 显示器 bounds」传，viewer 再按自己的显示器换算，和 v2 现有的多显示器逻辑一致
- 连续事件（鼠标移动）限流到 60Hz；离散事件（按键）不丢
- 服务端只做转发，不存历史（隐私）

**服务端两种选择**：

| 方案 | 说明 |
|---|---|
| A 复用 `BongoCat-mp/server`（Go） | 现成、有测试、有房间码与限流；需改 payload 类型 |
| B 自写极简 relay（Rust/Go，~200 行） | 两台机器自用，只需「房间码 + 广播」 |

家用场景推荐 B，但如果想省事且不介意多一层，A 更稳。

---

## 5. 角色语义

新增 CLI 参数 `--role`（`product_options.rs` 已有参数解析结构，顺延）：

| 角色 | 输入源 | 渲染 | 用途 |
|---|---|---|---|
| `host` | 本地平台输入 | 本地模型 | PC，正常用 + 广播 |
| `viewer` | 网络事件流 | 本地模型（被远端驱动） | laptop，OBS 采集它 |
| `both`（默认） | 本地平台输入 | 本地模型 | 原行为，不联机 |

viewer 模式下**不启动 libinput helper**，因此 laptop 端完全不需要权限、不需要 pkexec 授权，也不会有那个授权弹窗。这是这个设计最实用的一点。

---

## 6. 实施步骤

### P0 合入 Wayland 分支（半天）

1. `git checkout -b linux-wayland origin/pr-1114`
2. 应用 `Layer::Top` → `Layer::Overlay`（`crates/bongocat-overlay/src/linux/wayland.rs:802`）
3. laptop 上 `cargo build --release -p bongocat-app`
4. 验证：`hyprctl layers` 显示 `com.ayangweb.bongo-cat` 在 `level 3`；全屏应用上方可见
5. 这一步**在 PC 上已经验证过**（改完确认 overlay 层生效、截图可见），属于回归确认

### P1 协议与网络骨架（1–2 天）

1. 建 `crates/bongocat-net`，先只做单向：host 发、viewer 收、`println!`
2. 本地 loopback 验证：PC 上 `--role host`，另一终端起 viewer，看事件到达
3. 加房间码 + 断线重连 + 心跳

### P2 viewer 接入渲染（1–2 天）

1. `remote_source.rs` 实现与平台输入一致的事件源接口
2. `--role viewer` 时用它替代平台输入
3. 验证：laptop 上猫跟随 PC 按键动作；鼠标轨迹位置正确（含双屏）

### P3 打磨（按需）

- 延迟优化（批量合并、丢弃过期连续事件）
- 多远端（一个 viewer 显示多只猫 → 需要多 layer surface，改动较大，非必需）
- 配置页加「联机」栏目：服务器地址、房间码、角色
- 加密：局域网内可先用明文，跨网建议 Tailscale 或 WSS

---

## 7. 与上游兼容

改动全部集中在：

- 新增 `crates/bongocat-net/`，新增 `crates/bongocat-input/src/remote_source.rs`
- `crates/bongocat-app/src/product_options.rs`：加 `--role` 参数
- `crates/bongocat-app/src/main.rs`：按角色选择事件源（约 20 行分支）
- `Cargo.toml`：workspace 增加依赖与成员

**不碰**：`bongocat-live2d*`、`bongocat-overlay`（除 P0 那一行层级修正）、`bongocat-model*`、`bongocat-config`。

上游更新时冲突面只有 20 行左右的装配代码，且 `bongocat-net` 是独立 crate，可整体 cherry-pick。

---

## 8. 风险

| 风险 | 应对 |
|---|---|
| PR #1114 未合入，上游 master 变动导致 rebase 冲突 | 改动集中在 PR 新增文件（input-helper、linux/wayland.rs），上游较少触碰 |
| `InputControl` 序列化需要开放构造器 | `PhysicalKey::new(u16)` 等已在 crate 内，若未公开则补 `pub`（小改） |
| viewer 端无本地输入时，模型 idle 逻辑可能误判 | 用网络事件驱动同一 idle 计时器，等价于本地输入 |
| 多显示器坐标 | 沿用 v2 现有 `bounds` + 显示器查询逻辑，不新造 |

---

## 9. 待确认

1. **PC 上是否需要看到自己的猫**（若不需要，host 可 headless，省去窗口层的任何顾虑）
2. 服务端选 A（复用 Go）还是 B（自写极简）
3. laptop 端 OBS 采集方式：窗口采集（PipeWire portal）还是显示器裁切
