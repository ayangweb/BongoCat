# HANDOFF — 本 fork 的当前状态与接手指南

> 面向下一个接手的 AI agent。工作规范见 `AGENTS.md`，架构与取舍见 `DESIGN.md`，
> 待办见 `TODO.md`——本文只回答三件事：**现在是什么状态、怎么继续干活、哪些坑已踩过**。
> 状态类信息会过期（本文写作于 2026-11-02），以 git log 和两台机器的实况为准。

## 1. 这个仓库是什么

`ayangweb/BongoCat` 的 fork，在 **PR #1114（Wayland 支持，分支基点 `b4901ed`）** 之上新增
**跨机联机**：两台机器各跑一份，键鼠事件经 WebSocket 中继同步，对端的猫在本地屏幕上同步
演出。分支 `feat/multiplayer` 包含 Wayland 基座 + 联机全部改动（29+ 提交），**未推 GitHub**。

- 上游：`ayangweb/BongoCat`；origin 指向 fork `Hyperbola-QAQ/BongoCat`，upstream 指向上游
- 两台部署机运行同一分支；PR #1114 尚未被上游合并，所以本 fork 暂不能整支合回上游
- 测试基线：`cargo test --workspace` **1085 passed / 3 failed**——3 个失败全部在
  `bongocat-update`（上游只为 macOS/Windows 发版，Linux 必然失败，不是回归）

## 2. 机器与拓扑

| 机器 | 主机名 | 访问 | 角色 |
|---|---|---|---|
| PC | `hyqaq-pc` | **沙箱宿主本机**（无 ssh，文件系统直接共享） | 发布端：键鼠采集 → 房间 |
| laptop | `hyqaq-laptop` | `ssh laptop`（配置在 `~/.ssh/config`） | 开发/编译主仓库 + relay + 观察者 |

- 主开发仓库在 **laptop** `~/Work/Projects/BongoCat`；PC 侧同名路径是 rsync 镜像（部署目标）。
- 沙箱运行在 PC 上：`hyprctl` 直接可用（需 `XDG_RUNTIME_DIR=/run/user/1000`）；
  **沙箱的 pkill/kill 杀不到宿主进程**——PC 侧进程管理必须
  `hyprctl dispatch 'hl.dsp.exec_cmd("pkill -x bongocat-app")'` 走宿主执行。
- 两台的用户路径相同（`/home/hyperbola/...`），脚本可以两边通用。

## 3. 构建 / 部署 / 启动 / 验证

### 构建与部署

```bash
# laptop（主仓库）
cargo build --release
# PC（先同步源码，再本机构建——见 §5 的 LD_PRELOAD 要求）
rsync -a --delete --exclude target -e "ssh ..." laptop:'~/Work/Projects/BongoCat/' ~/Work/Projects/BongoCat/
cargo build --release   # 需要 sandbox-rust-build 技能的四件套，见 §5
```

**改完必须重编两台**——「功能无效」的假阴性有一半来自跑在旧二进制上。

### 启动（当前以参数模式演示；GUI config 驱动等效）

```bash
# laptop：relay（headless）+ 观察者（猫显示 PC 的键鼠）
./target/release/bongocat-app --relay --port 8787 --token <key>
./target/release/bongocat-app --watch --publish-url ws://127.0.0.1:8787 --publish-token <key>
# PC：发布端（键鼠共享出去，本机猫隐藏）
./target/release/bongocat-app --hide --device-id <id> --publish-url ws://<laptop-ip>:8787 --publish-token <key>
```

GUI 配置驱动等效（config 已持久化）：两台普通启动即可，重启后生效（页面上有说明）。

### 真机输入注入（验证联机的唯一可靠手段；脚本已定版）

- PC：uinput 注入脚本（真实 evdev 事件；**已于 2026-11-02 按用户要求清理**，需要时重写，
  本沙箱对 `/dev/uinput` 的 ioctl 被 seccomp 拦，须 `hyprctl dispatch exec_cmd` 在宿主跑，
  且不要放仓库内）
- 诊断方法：临时在 `net_publish`/`input_tee`/`runtime apply` 加 stderr 打印再重编，
  用完删除（历史上有两轮靠这个定位）

## 4. 联机架构要点（改动前必读）

- **crate 分工**：`bongocat-net`（relay + client 协议 + 进程级状态）、`bongocat-input-helper`
  （特权/无特权输入采集子进程）、`overlay/linux/input_tee.rs`（本机输入的镜像门控）、
  `runtime/input_state/apply.rs`（事件最终消费，含两个严格守卫）。
- **三条独立正交的开关**：`publish`（本机输入发不发房间）、`show_self`（本机输入喂不喂本地
  猫）、`draw`（画房间里谁的输入）。display 列表同时决定 show_self 与 draw。
- **两个运行时守卫是联机 bug 的历史重灾区**（`runtime/input_state/apply.rs`）：
  1. **sequence 必须全局连续**：runtime 收到两个 sender（房间帧走 base producer、本机帧走
     tee），任何 sender 用独立计数都会被 OutOfOrder/Duplicate 判定整批丢弃。新 sender 必须
     经 `InputProducer::with_submitter` 共享 base 的计数器。
  2. **at 必须同时钟域**：所有 stamper 用 `wall_millis()`（UNIX_EPOCH 毫秒）；任何进程
     uptime 式时间戳会触发 NonMonotonicTime reset。
  3. **`frame.src` 必须有人填**：tee 不知道本机 id（填空串），relay 原样转发（不 stamp），
     所以发布端 bridge 在发送前盖 `join.device_id`——动协议时别把这个职责弄丢。
- 指针轨迹走**独立通道**：`CursorProducer` 槽位 + observer 镜像（`cursor.rs`），经
  `kind::POINTER` 传归一化 0..1 坐标，接收端用单位视口还原（多屏映射未做）。
- relay 的 `kind::POINTER` 分支目前**不过限流器**（LAN 假设）；若上公网需要先补。

## 5. 踩坑清单（每一条都真实烧过时间）

1. **沙箱 pkill 自杀**：`pkill -f bongocat-app` 的命令行含自身 → 杀掉自己的 shell，同一条
   命令里的后续步骤全部静默不执行。PC 侧清理用宿主 `pkill -x bongocat-app`。
2. **PC 多实例同 id 互踢**：残留实例用同一 device id 反复 join → relay 软退役互踢循环，
   污染一切诊断。启动前先 `pkill -x` 清干净，并用宿主侧 `pgrep` 落盘确认。
3. **rsync --delete 会删 PC 侧新建的脚本**：验证/启动脚本必须放在 rsync 排除路径
   放仓库内必被 rsync --delete 删掉（历史教训：曾因此连吃两轮假阴性）。验证用的临时脚本已于 2026-11-02 按用户要求全部清理，下次需要时重新生成并放仓库外。
4. **沙箱构建四件套**：`LD_PRELOAD=noop_time.so` + `TMPDIR=target/tmp` +
   `RUSTUP_TOOLCHAIN=stable` + 后台运行（见 sandbox-rust-build 技能；PC 构建必须）。
5. **`/tmp` 每次沙箱调用清空**：跨命令的临时文件放 `~/.cache/` 或 workspace。
6. **locale JSON 有手写紧凑行**（如 `linux_tray`）：用 `json.dump` 全量重写会展开它们，
   i18n diff 会出现噪声——改 locale 用文本锚点替换，或改完把紧凑行还原。
7. **ssh 命令里嵌 heredoc**（不带引号的 `<<EOF`）：远程 zsh 会展开 heredoc 里的反引号——
   传脚本一律走 `ssh 'python3 -' < 本地文件`。
8. **grim 会卡死**：Hyprland screencopy 被残留会话挂住后 `timeout` 都救不回，截图验证
   不可依赖；用日志/状态落盘代替。
9. **git checkout ≠ 干净基线**：HEAD 里可能已含部分"待应用"改动——补丁脚本的存在性
   检查要用精确锚（`grep` 具体代码行），不要用宽泛的首行。
10. **locale 双向覆盖测试**：catalog 键必须被源引用、源引用必须在 7 个 locale 都存在；
    新增键后跑 `cargo test -p bongocat-i18n`。

## 6. 当前进度与下一步

- 已落地：Wayland（PR #1114 基座）、联机全链路（键盘+鼠标轨迹）、设备自动发现、
  地址项合并、状态板重设计、无特权 helper（input 组直启）、指针限频修复。
- 待办（`TODO.md`，5 项）：per-device 几何、皮肤同步自定义模型、其他属性跟随、
  UI 联机配置的逐路径测试、v4l2loopback Alpha 输出供 OBS（v4l2loopback 优先，评估见其条目）。
- 已知既有失败：`bongocat-update` 3 个测试（上游无 Linux 发版）；`grim` 截图挂起
  （Hyprland screencopy 层，与本仓库无关）。
