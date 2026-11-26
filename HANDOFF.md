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
  1. **sequence 必须全局连续**：房间输入与通过本机门控的输入最终都经同一个 base
     `InputProducer` 分配 runtime 序号；本机被过滤的事件不得消耗这个序号，否则会误触发
     sequence-gap reset。本机 tee 的外层 producer 和网络帧使用各自的序号域。
  2. **at 必须同时钟域**：Linux 本机采集与网络到达事件都使用进程共享的
     `MonotonicMillis::now()`；远端机器的时钟不能直接用于本机 runtime。
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

## 7. OBS Alpha work in progress (2026-11-02)

**2026-11-03 Alpha model/input follow-up:** The off-screen Alpha renderer had
excluded the model's background asset, which is the model keyboard/mouse-pad
artwork, not an opaque scene background. It now uses the same asset as the
desktop renderer. A real Vulkan off-screen regression compares frames with and
without that asset and checks that transparent pixels remain. On laptop the
user's existing multiplayer configuration follows PC device 7d50a6ef and does
not display laptop's own input; the relay had only its laptop client connected.
The PC publisher was started with its existing configuration and now connects.
The user confirmed the PC keyboard works; user visual approval and end-to-end
pointer validation still need verification. The laptop release was rebuilt, and
both PC and laptop applications were subsequently started using existing config.
The user confirmed the PC keyboard worked but the laptop pointer still
controlled the pet. The local pointer was not gated by the selected source and
remote samples were mirrored back to the room. The cursor producer now forwards
local capture to the network independently of whether local display is enabled,
and a distinct remote publish path bypasses that observer. Tests cover source
selection, local suppression and no echo. The rebuilt laptop application is now running.
Follow-up input audit: suppressed local events used to consume the runtime
sequence and make later remote events appear to have gaps, resetting held state.
The local capture tee now has its own sequence domain and forwards accepted
events through the same runtime producer as remote events. Regression tests
cover both suppressed and forwarded local events interleaved with remote edges.
The sequence fix has been built but the current user session still runs the
previous executable. The shared-clock follow-up also needs a fresh release build.
Linux local capture and incoming network input now share the same process-wide
monotonic input clock; incoming remote timestamps are re-stamped on arrival.
Automated tests cover local/remote pointer and key interleaving, including
non-monotonic and sequence-gap diagnostics. User-side simultaneous mixed-source
and long-duration validation remain; do not claim complete input reliability.
A later live-session diagnostic observed the user app's main thread blocked in
an Intel Vulkan DRM timeline ioctl for over five seconds, and /proc process write
accounting rose by approximately four BGRA frames in four seconds: the existing
frame-callback gate does not prevent ~1 fps output in this state. Alpha output
now consumes an independent bounded subscription directly from runtime frame
publication; model preparation frames are excluded and the producer uses
try_lock. A separate real GPU/v4l2 smoke test delivered transparent model pixels
and twelve frames while the desktop consumer was never polled. Its temporary
production-named device was removed; the user's development device was untouched.
The rebuilt release was started by the user, who confirmed OBS is smooth.
A 60-second /proc process-write sample was consistent with about 29.93 fps;
this includes all process writes and is therefore an estimate, not a V4L2
measurement. The user explicitly skipped the eight-hour validation, so the non-invasive
sampling session was stopped after approximately three minutes; its CSV and
status remain under `/tmp/bongocat-final-soak.89H3WA/`. The partial sample is
not evidence of eight-hour stability or end-to-end latency. The optimized standalone GPU/V4L2 test captured 120 frames at approximately
30.1–30.7 fps with no desktop-consumer polling and cleaned its temporary
production-named camera. This is a short test, not a soak or latency result.
The user confirmed the model, PC keyboard and both mouse-source behaviors
work in the previous release. A non-invasive sample of that running process was stopped after discovering
the pre-fix app was outputting approximately one frame per second; samples and
status remain under `/tmp/bongocat-soak.e5Vm3y/`. It is not final-build soak
evidence. The earlier shared-clock release build was cancelled when the
independent runtime-frame subscription fix became necessary.

**2026-11-03 decision superseding the older lifecycle notes below:** the user
removed automatic boot recovery and the separately installed privileged copy.
Install/uninstall now invoke the current app through pkexec for each operation;
no service or privileged copy is created. After reboot, settings reports that
the camera must be reinstalled and the user authorizes installation again.
The laptop had no old service or helper to migrate. Root lifecycle tests passed
including a simulated new boot, foreign-node conflict, occupied device and
cleanup; real pkexec install/uninstall passed with two authorization prompts
using a temporary terminal agent and the unprivileged development executable.
The rebuilt optimized application also passed the real pkexec install/uninstall
test with exactly two authorization prompts.
Running a user-writable app as root trusts that exact executable at authorization
time. Real reboot, graphical authorization, OBS model review and dual-machine
input remain unverified.

The laptop spike confirmed AR24 transparent and partial-alpha composition in OBS;
see `docs/benchmark/obs-alpha-spike.md`. The test producer and device were removed.
The proposed implementation boundaries are in `docs/adr/0083-linux-obs-alpha-output.md`.
Configuration has `alpha_output.enabled`, defaulting to false for old files;
schema, fixtures and tests were updated. An independent off-screen renderer and
bounded frame transport now write AR24 to a dedicated V4L2 device, with typed
runtime status and a persisted settings command. A real GPU/loopback smoke test
checked transparent, opaque and partially transparent pixels and thread shutdown.

A restricted development/production device helper is in progress. On laptop, an
isolated root-owned test binary passed repeated installation, udev ownership,
simulated reboot restoration and repeated uninstallation; the test artifacts
were removed and the pre-existing camera nodes remained. Ordinary-user probes
also correctly detected the development device before install, after install,
and after uninstall. A typed settings request now returns fresh system status.
A later real-device lifecycle test verified that uninstall rejects an open device
and simulated boot-time number collision refuses a foreign loopback device without
removing it; the test cameras and management files were cleaned. An initial test
used an overlong sentinel label that the kernel truncated; it failed, its test
device was explicitly identified and removed, and a shortened-label rerun passed.
Separately, with the actual application binary installed as a disposable helper,
`systemctl start bongocat-alpha-development.service` recreated a manually removed
camera and restored its user permission; uninstall then removed the resources.
This exercises the service entry point but is not a real reboot test. A separate production bootstrap test copied a
root-owned test executable from the fixed installed application path, then passed
repeated install and uninstall and cleaned all test resources. The unprivileged
entry point calls only fixed helper operations through pkexec; graphical polkit UI
and explicit graphical authorization rejection still need end-to-end testing. Development builds can bootstrap from a root-owned installed application or use a
separately provisioned root-owned development helper; they cannot bootstrap from
a user-writable debug executable. The root-owned helper persists after camera
uninstall; its safe version-upgrade/replacement behavior still needs explicit
design and testing before an application upgrade can be considered covered. The Alpha group is now connected in the model-window settings page, including
driver/module/tool/device/output status, refresh, install/uninstall, switch and
retry. The typed management command serializes operations; uninstall disables the
persisted switch and waits for an overlay-owner completion token after the output
worker joins before invoking the helper. A service test confirms shutdown cancels
an unacknowledged stop and no device operation runs early. Unanswered authorization
is cancellable while pkexec retains the caller's real uid; root-side tool calls now
have per-command timeouts. Simulated cancellation and the actual development
lifecycle passed; graphical polkit interaction and shutdown timing still require
verification. An SSH/TTY pkexec test found polkit 128's internal agent fails with
`No session for cookie`. A separate unprivileged `pkttyagent` then verified
real pkexec authentication, application-level install/uninstall and cancellation
of an unanswered authorization; temporary binaries, agent and device were removed.
The laptop currently has no detected graphical polkit authentication agent, so
normal GUI authorization needs one installed and running before end-to-end GUI
testing. No packages or system authentication configuration were changed. The
Alpha page's install/uninstall button clicks, minimum 800×600 scrolling and missing
driver disabled state now have GPUI tests. Actual reboot, graphical agent, OBS
model composition, multiplayer and performance validation remain.

A dual-machine attempt exposed a reproducible startup SIGSEGV when Alpha was
enabled: GDB showed concurrent Vulkan instance initialization in GPUI and the
Alpha worker, both inside the loader (the Alpha thread was loading NVIDIA ICD).
The output renderer now reuses the overlay's existing wgpu instance and obtains
a separate adapter/device. GDB then observed a normal 12-second exit, and the
ignored `linux_alpha_startup` integration test opened the settings window,
captured a real transparent/opaque/partial-alpha frame and exited 0. The temporary
camera was removed. A two-machine trial did not verify input; do not claim the
multiplayer gate from it. A covered Wayland surface initially limited both debug
and release Alpha output to ~1 fps despite the device advertising 30 fps. The
overlay now honors its existing compositor frame callback before acquiring the
next surface image, while continuing to publish camera frames. The debug build
then measured ~11.9 fps; the ignored startup test additionally captured ten
frames within four seconds. The optimized Development build then measured about
29.9 fps for ten seconds at AR24 1280×720; at 15 seconds its process lifetime
average CPU was 21.0% and RSS was 194728 KiB. GPU utilization, end-to-end latency
and long-duration stability remain unmeasured. A V4L2 client started before the
producer completed a five-frame capture; a subsequent client reconnected and
captured 15 frames while the optimized app stayed running and exited normally.
This is a V4L2 client test, not an OBS visual check. See ADR-0083. The updated ignored startup test passed on laptop with the temporary
camera removed afterward. `cargo fmt --all -- --check`, `git diff --check`,
locale validation, and the Alpha overlay unit tests passed. Targeted clippy passed
with the newer installed Rust 1.99 toolchain's deprecated, chunks-exact-to-as-chunks
and cmp-owned lints allowed; the repository-pinned 1.97.1 and `just` are absent.
Do not present the feature as complete. Preserve uncommitted and untracked files.

On laptop, `cargo fmt --all -- --check`, `cargo test --locked -p bongocat-config`
(81 passed, one ignored), config clippy with `-D warnings`, and
`cargo check --locked -p bongocat-app` passed. Workspace tests reproduced the
three documented Linux `bongocat-update` failures. The `just` command is absent
on laptop, so schema was generated by the exact cargo command in its recipe.

2026-11-02 continuation: the V4L2 adapter uses `v4l` 0.14.0 and is wired to
the offscreen renderer, settings command and owned-device lifecycle. The GPU
loopback test was rerun after integration: it captured transparent, opaque and
partially transparent model pixels and joined the output thread; the disposable
device was verified removed. The UI's 207 tests, Alpha service tests, locale
coverage and format/diff checks passed. These do not replace the remaining
end-to-end and reboot gates listed above. A fresh debug build also ran in the
laptop Wayland session with `--run-seconds 5` and exited 0; this was a startup/
shutdown check, not a GUI or OBS acceptance test. It logged a pre-existing
missing XCompose include under ~/.local/share/omarchy. Strict platform clippy on laptop Rust
1.99 fails on an existing `bongocat-log` deprecation; with `-A deprecated`
the adapter check passed. Repo-pinned Rust 1.97.1, cargo-deny and just are absent.


### Camera identity upgrade (2026-11-03)

The user explicitly approved one authorized reinstall of the existing camera.
New state records preserve the original v1 fields and add the device-node inode,
and are root-owned but world-readable because their UID, boot ID, device number
and inode are already observable system metadata. Status and ordinary output
verify current boot, owner, rule and inode before opening; privileged uninstall
refuses a mismatched or missing inode. Existing mode-0600 five-field records
remain readable by the installer. One explicit same-boot reinstall enrolls the
matching existing device without replacing it; legacy records cannot prove
whether a same-name replacement happened before this enrollment. User accepted
the one-time step; the currently running user instance and its existing record
have not been changed. A separate privileged production-camera test passed
legacy upgrade, old-boot rejection, same-name/same-number replacement rejection
and safe uninstall; an independent GPU test using a managed production camera
also passed and cleaned its artifacts. The user must reinstall after switching
to the new release, then retry output if needed. The upgrade was also tested
with another capture handle open; the device inode stayed unchanged. The
output checks the inode of the opened file descriptor after opening as well as
the recorded device path before opening. A real managed-camera GPU test and
a wrong-inode handle test passed; all isolated production artifacts were
removed and the user's development camera and 0600 record were left intact. The
optimized build is running in tmux `bongocat-identity-release`, with log
`/tmp/bongocat-identity-release.log`. Do not stop the user's existing instance
or launch pkexec automatically; after the build, the approved one-time reinstall
still requires the user to click the settings button and authorize it.

### Settings restart and relay (2026-11-26)

The settings restart previously spawned a second process before the old relay
released its port. Linux now flushes settings and shuts down before replacing the
process with `exec`; relay binding is checked before reporting it as listening.
The settings window is also closed before releasing the overlay renderer. On the
PC, a symbolized core showed the previous exit crash in GPUI's Vulkan swapchain
destruction inside the NVIDIA driver. Four isolated exits with settings open now
completed successfully; one isolated laptop exit also succeeded. The UI and
relay tests passed, including save failure and socket replacement tests.

The changed source and both changelogs were synchronized to the laptop. A PC
development build was tested on both machines; its laptop copy is temporary at
`/tmp/bongocat-verified-pc-build`. A separate test room on port 18787 confirmed
the new PC instance followed laptop input only (user observation); both test
instances were removed. The user then approved switching the live instances.
Both old processes were quit through their tray actions and the tested build is
now running on both machines; laptop relays on 8787 and PC has reconnected.
The previous laptop binary is preserved at
`target/release/.bongocat-before-restart-fix.NmsVkz`. The user confirmed the
authorized camera reinstallation and that the live PC cat follows only laptop
input. The development camera state is ready, its recorded inode matches
`/dev/video6`, and the new laptop process has that device open. OBS compositing
was not rechecked during this task. The release
build mentioned in the earlier 2026-11-03 note was not found and must not be
assumed to be running.

### Two-way UI restart test (2026-11-26)

`tools/tests/manual_multiplayer_restart_ui.py --run` uses the real settings
windows and mouse controls on both hosts. It enables local and peer display,
then clicks each restart button, checks for a new network socket and no new
core dump, and checks the laptop relay and both clients after reconnecting.
The script passed its restart, socket and configuration checks on the current
PC/laptop pair, but those checks do **not** prove the requested two-cat behavior.
The user clarified on 2026-11-26 that each screen must show two independent cats,
one per source. At the time of that test, one renderer still merged local and
remote input.
Each configuration currently includes `self` and the other device; both clients
publish, laptop serves port 8787, and laptop Alpha's enrolled camera remains
available. The script is host-specific and uses temporary virtual mouse
daemons; it does not simulate keyboard input or write configuration directly.

### Single-process dual-window VM test (2026-11-26)

The user chose one application instance per host. ADR-0084 describes the
separate peer runtime and Wayland window. Both Omarchy VMs ran the optimized
Development build with Vulkan llvmpipe and one application instance plus the
existing input helper. Each screen showed the two cats side by side. QEMU
keyboard and relative mouse events sent separately to each guest affected only
that guest's local cat and the other guest's corresponding peer cat. Both
settings-page restarts reused the application PID, recreated the room sockets,
and restored the connection. The laptop listener on TCP 18787 changed inode
from 53232 to 55654, and neither guest recorded an application core dump.
The user's direct two-screen review remains pending.

### Live multiplayer settings VM acceptance (2026-11-26)

On the authorized Omarchy guests, the Linux settings screen applied sharing and
local display changes with the original app PIDs. An incorrect room key kept
the previous connection and saved key, retained the edit, and displayed the
localized room error. An occupied TCP port kept the old listener and saved port
and displayed the relay error. After releasing the port, laptop applied 18788
and its Follow Local client rejoined. PC changed the room address through the
connection editor and joined the new port. Both application PIDs stayed the same
through the changes. After changing ports, keyboard and mouse injection in both
directions affected only the expected source on each screen.

The model-window switch hid and restored both cats while the room stayed
connected. With the device selection empty, re-enabling model windows left
both cats hidden. Both app processes quit via the tray action with status 0,
no new core dumps and the listener released. Starting again from the isolated
Development configs rejoined 18788. The temporary firewall exception was
removed after the test. The screenshot and test log are in
`/tmp/bongocat-vms.5xpZPD` on PC.

On the final UI revision, the key was masked by default. After laptop quit,
PC continued to show its local cat and kept laptop selected while hiding the
remote cat. The remote cat returned on reconnect in the same PC process.
The first screenshot taken before the client's retry still showed the offline
state. The returned-state screenshot was taken after verifying the established
socket. The software Vulkan ICD limits this evidence to behavior and layout.

The PC guest also verified the room editor at 800×600. The room key remained
masked and its field aligned with the address field. The test ended through
the tray action with status 0 and the guest returned to 1280×800. The
managed relay shutdown and same-port rebind test passed 50 consecutive runs
with Linux `SO_REUSEADDR`. The unit test for rejecting occupied live ports
still passed.

The final optimized Development build also passed a live change from an IPv4
listener to dual stack on the same port, 18788, with both guests connected.
The listener inode changed from 153914 to 164634. The local and remote
clients rejoined while both application PIDs stayed the same. The isolated
config saved the new binding, both tray exits returned 0, and the temporary
firewall rule was removed. See `editor-final-same-port.log` and
`editor-final-same-port-exit.log` in the PC test evidence directory.

Both guests have the Vulkan software driver and a test account in the input
group. On the laptop VM, v4l2loopback 0.15.4 passed the privileged identity
regression, including refusal to enroll a matching device from a partial record
without an inode. That test's device and rule were removed. The test used VM
storage and did not exercise OBS or physical GPU performance.

### Upstream 2.4.1 integration (2026-11-26)

The integration uses `ayangweb/BongoCat` master at `47a73ed1`. It keeps
upstream's plugin lifecycle, desktop window handling, dependency versions,
localizations, and configuration fields, and reapplies the fork's Linux
Wayland, multiplayer, and Alpha output paths. The Linux host has no plugin
panel adapter or native input-source plugin backend. The new French and
Russian catalogs retain their upstream wording and use English for the
fork-specific entries.

The two Omarchy VMs started from isolated copies of the previous Development
configuration and rejoined port 18788. Keyboard input from each VM animated
the matching cat on both displays. The model-window switch hid and restored
both cats, and sharing changes persisted without replacing the app process.
The optimized build rebound 18788 from dual stack to IPv4 while both app PIDs
stayed the same. Both guests quit through the tray with status 0.

A disposable Development Alpha camera on the laptop guest captured an AR24
frame with 346273 transparent, 565078 opaque and 10249 translucent pixels.
A ten-frame stream completed within four seconds, followed by a clean app
exit and camera removal. This test used software Vulkan and exercised the
helper through sudo inside the disposable guest. It did not use OBS or a
desktop authorization dialog.

The Linux workspace test subset passed 1357 tests with 17 ignored in the
serial verification. The parallel rerun intermittently hit an upstream plugin
loader fixture with Linux `ETXTBSY`. The updater still has three desktop-target assertions that fail on Linux, and
the upstream Input Source plugin has no Linux adapter. The strict Clippy
check passed on the app, config, UI, UI protocol, overlay and network crates.
The 2.4.1 Development package built successfully, and schema validation
passed for 61 config and seven window-state fixtures. Both guests started
the final staged build and rejoined the isolated room before clean exit.


### Three-device Linux smoke (2026-11-26)

The user requested a third Omarchy guest, `bongocat-test-third`, for a
three-cat test. It has a separate 64 GiB disk, identity and host key. SSH and
Wayland were verified after installation, and Vulkan llvmpipe was verified
using signed packages from the existing guest cache.

The laptop guest hosted the room. PC, laptop and third joined under three
distinct device IDs. The PC viewer displayed three cats at x=100, 525 and
950. A keyboard press on each guest animated the corresponding cat on the
PC. Removing Laptop Test and adding it back shifted its position in the
selection list, while the actual cat windows returned to their device-ID
positions. Exiting the third guest hid its cat on PC. All guests exited
with status 0 and the two temporary firewall exceptions were removed.
The PC viewer saved all three device IDs in its isolated window state.

Alpha output still draws from the primary runtime and produces one cat
per frame. Three-cat composition and separate virtual cameras are outside
the current output path.

### Single-camera group output and two-host deployment (2026-11-26)

The optimized Development 2.4.1 build was deployed to the versioned
`~/Work/BongoCat/target/package/` path on hyqaq-pc and hyqaq-laptop.
The executable and archive checksums match on both hosts. No physical app
process or virtual-camera authorization was started during deployment.

The two retained Omarchy guests exercised the final executable with a
temporary third publisher in an isolated guest data root. One V4L2 AR24
frame contained nontransparent pixels in all three expected positions,
including 1308 partial-alpha pixels and 849606 transparent pixels. After
the third publisher exited, its position was transparent and the other
two positions stayed in place. Test processes exited normally and the
test camera and firewall rule were removed. The captured frame is
`/tmp/bongocat-vms.5xpZPD/alpha-three-online.png` on PC.

After three-cat desktop verification, the user requested removal of
`bongocat-test-third`. Its domain, NVRAM, dedicated qcow2 and cloud
installation configuration media were removed. The original two
Omarchy guests remain available.

### Shared window behavior and device switches (2026-11-26)

The Linux desktop passes the current window settings to every cat. Idle
visibility is driven by each cat's own input source. The host's physical
modifier state gates click-through for all windows, independent of remote
keys. The two guest tests verified that only the remote cat woke on remote
activity and hid again after two seconds. Holding local Left Shift moved
the remote window with a drag where ordinary dragging passed through.
The final optimized build switched the remote device off and on from the
same roster row without replacing the application PID. The executable and
archive were deployed to the canonical package path on PC and laptop.
