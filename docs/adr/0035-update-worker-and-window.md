# ADR-0035: 更新 worker、更新窗口与发布说明

状态：已接受（2026-09-15；2026-09-25 修订自动检查默认值与间隔）
依赖：ADR-0034（detached minisign 信任模型）、ADR-0029（第三方库边界）、ADR-0016（匿名诊断导出）、ADR-0059（UI protocol 边界）

## 背景

ADR-0034 交付了更新链路与信任模型，并在「后续边界」中明确排除了四件事：endpoint 配置界面、
公钥注入自动化、**update worker**、**更新 UI**，以及操作系统包签名验证与失败启动恢复。

在此之前，产品里与更新相关的可见能力只有两处，且都不完整：

- 设置页有一个「自动检查更新」开关，它只把 `updates.check_automatically` 写进配置，
  **没有任何代码读取它**——打开它不会有任何行为。
- 系统菜单有一个「检查更新」入口，由 `bongocat_app::update_check_available()` 决定是否显示，
  但它的处理分支是 `SystemMenuAction::CheckForUpdates => Ok(true)`，即**点下去什么都不发生**。

也就是说：更新管线是完整且被测试覆盖的，但**没有任何调用方**。本 ADR 记录把这条链路接成
一个完整产品功能所采用的决策。

## 决策

### 1. 更新 worker 与设置 worker 分开，各自一条线程

新增 `bongocat-app::ApplicationUpdateService`：一条名为 `bongocat-update-service` 的线程独占
`UpdateRuntime`，是**唯一**触碰网络、下载和安装的组件。

不复用 `ApplicationSettingsService` 的命令循环。设置循环是串行阻塞的：把 check/download/install
放进去，会让一次百 MB 级传输期间的所有设置读写排队。两者生命周期也不同——设置服务在退出序列里
先于 overlay 结束，更新服务可以在安装完成后仍在运行。

### 2. worker 只发布状态，不推送事件

worker 把当前阶段写入 `UpdateStateHandle`（`Arc<Mutex<UpdateSnapshot>>` + revision），窗口按
`UPDATE_STATE_POLL_INTERVAL`（250 ms）读取。理由：

- 窗口关闭、销毁或渲染慢，都不会阻塞或反压 worker；下载进度是覆盖写，不是队列。
- revision 只在阶段真正变化时推进，同一个阶段重复发布不推进 revision，轮询因此不会无意义重绘。
- 命令通道（`UpdateClient` → `UpdateCommand`）保持有界且非阻塞（`try_send`），窗口不会因为
  worker 忙碌而卡住。

**关闭窗口不取消任何操作**，因为操作不属于窗口。这是刻意的：库没有取消钩子（见 §9），一个
"取消"按钮只能让 UI 停止显示，而传输仍在继续——那是在撒谎。窗口因此始终可以关闭。

### 3. UI protocol 拥有跨层 contract，app 负责适配

`bongocat-ui-protocol` 定义 `UpdateSnapshot` / `UpdatePhase` / `UpdateClient` 等全部更新面
类型，`bongocat-ui` 只负责 GPUI view 和展示策略。`bongocat-ui-protocol` 与 `bongocat-ui` 都不
依赖 `bongocat-update`。`bongocat-app` 做映射，且映射是穷尽 `match`：

- `UpdateStage` → `UpdateFailureStage`
- 14 个 `UpdateErrorCode` → UI 的同名目录

新增一个 stage 或 code 会让 `bongocat-app` 编译失败，直到它被赋予用户可见的含义。这沿用了
`SettingsRuntimeErrorCode` 等既有做法，也让 `cargo-packager-updater` 的类型不越过应用边界。
代价是两份字符串目录，由 `bongocat-app` 的测试逐项比对锁定。

### 4. 三个阶段可观测，靠拆分库调用实现

窗口需要区分「下载中 / 校验中 / 安装中」，但库的
`download_and_install()` 把三者合成一次调用。因此 runtime 改用
`Update::download_extended()` + `Update::install()` 两次调用：

```text
download_extended(on_chunk, on_finish)   读完全部字节 → on_finish → 验签 → 返回已验签的字节
install(bytes)                           写入安装位置
```

`on_finish` 在验签**之前**触发，返回之后表示验签已通过。于是三个可观测点正好落在
`UpdateEvent::{Progress, DownloadFinished, Verified}` 上，窗口的 `Downloading` / `Verifying` /
`Installing` 分别对应它们，而不是靠猜。

失败阶段不再靠调用方推断：`UpdateError` 现在带一个 `stage` 字段，由 runtime 在失败点设置。
库把下载与验签合在一条错误里，所以下载阶段的错误按 code 再分一次
（`SignatureInvalid` → Verify，其余 → Download，未识别的按 Download 处理——不宣称载荷已通过认证）。

### 5. 重启要求是平台事实，不是运行时判断

`restart_required_after_install()` 返回 `cfg!(target_os = "macos")`：

- **macOS**：库整包替换 `.app`（`remove_dir_all` + `rename`），运行中的进程此后执行的是已被删除的
  文件。`preset_root()` 虽然只在启动时读取，但 `PresetModelCatalog::list/load` 是**惰性读盘**的，
  所以继续运行会让预设模型切换直接失败。因此 macOS 在安装成功后**自动重启**：先按 §5.3 的顺序
  完成产品 shutdown，再 `exec` 新的可执行文件。
- **Windows**：库把载荷交给 NSIS 安装器后立即 `process::exit(0)`，`Installed` 在本平台不可观测；
  安装器的 `/R` 负责重启。窗口因此没有重启按钮。

**自动重启由应用侧看门狗触发，而不是由窗口触发。** 窗口可以随时关闭（关闭不取消操作），如果重启
挂在窗口上，用户在传输途中关掉窗口就会留下一个执行着已删除文件的进程。因此应用每 25 ms 观察一次
发布出来的阶段：看到 `Installed { restart_required: true }` 后等 `UPDATE_RESTART_DELAY`（1200 ms，
只为让窗口把结果显示出来）再重启；窗口上的「立即重启」按钮只是把这个等待缩短，两条路径共用一个
`update_restart_started` 标志，因此只重启一次。

### 6. 自动检查由 GPUI 侧驱动，并在发现更新时打开窗口

开关与间隔值在用户配置里，只有设置服务读得到，所以调度放在 `main.rs` 的 GPUI 侧，而不是 worker 里：
启动后等 10 秒（不与启动争网络和窗口），之后按配置中的整小时数等待下一次检查。自动检查开关
默认 `false`，因此新配置不会主动发起检查；用户显式打开后才启动该调度。间隔默认 `24` 小时，
接受 `1..=8760`；调度器以最近一次实际派发为期限锚点，并通过只读取自动更新设置的轻量轮询重新
安排下一次检查，因此修改间隔会重排期限。关闭自动检查不会清空该值，手动检查也不受它影响。该补充取代本 ADR 最初
记录的“固定 24 小时且间隔未持久化”状态。

发现可用更新且窗口未打开时**打开更新窗口**。替代方案（什么都不做）会让这个开关变成一个
不可观测的设置；这与 §8.4 里"开关存在但无行为"的旧状态没有本质区别。窗口是单例，重复检查
不会叠窗口。

### 7. 发布说明随 manifest 一起走

`crates/bongocat-packaging` 的 `--merge-manifests` 增加 `--release-notes <file>`，把说明写进共享
`latest.json` 的顶层 `notes`；runtime 把它透传为 `UpdateRelease.notes`，窗口在「更新内容」区域
渲染，并另给一个指向 `releases/tag/v<version>` 的链接。

- **不新增第二次网络请求**：说明和版本、载荷地址来自同一份 manifest，检查更新时一次拿到。
- **不新增依赖**：说明由发布工作流用 `gh api .../releases/generate-notes` 生成（工作流本来就用
  `--generate-notes`），只是改为先生成再同时喂给 manifest 和 `gh release create --notes-file`，
  因此发布页和客户端看到的说明不会分叉。
  - 补充（2026-09-16）：**这一条已被取代**。`generate-notes` 给的是两次 tag 之间的提交摘要（PR 与
    commit 列表），而本项目对外发布的是手写的双语 changelog；两者内容不同，让发布页显示提交摘要
    就等于放弃了 `CHANGELOG.md` / `CHANGELOG.zh-CN.md` 作为事实来源。现在由
    `bongocat-packaging --extract-release-notes <file>`（`just release-notes`）按产品版本号从两份
    changelog 中各取同一条目，按「英文正文 → `---` → 中文正文」合成一份文件，同一份文件仍同时喂给
    manifest 与 `gh release create --notes-file`。"一次请求拿到"与"发布页和客户端不分叉"两条结论不变，
    变的只是说明从哪来：不再是 GitHub 的提交摘要，而是仓库里那份 changelog。该步骤因此也不再需要
    `GH_TOKEN`，并且在下载发布产物之前执行。版本在 changelog 里没有条目时合成失败，发布中断——这是
    有意的门禁，理由见下一条。
- **有界**：manifest 每次检查都会被解析，说明因此上限 32 KiB，超长在字符边界截断并追加可见标记，
  而不是让发布失败。空文件按"没有说明"处理，不写空字符串。
  - 补充（2026-09-16）：双语合成后上限仍按 32 KiB 判定，**截断发生在字符边界、不区分语言**：说明
    长到需要截断时，被切掉的会是中文那一段。当前 changelog 的条目约 6 KiB，离上限很远，因此这是
    一个已记录而未处理的边界，而不是一个已知会发生的行为。另需注意：更新窗口渲染的是整份说明，
    不做语言过滤，所以中文用户会先看到英文段、再看到中文段——这与本项目发布页一贯的排版一致，
    是刻意保留的，不是渲染缺陷。
- `pub_date` 仍然不写：库把它当可选，新鲜度由版本决定，而打包工具没有日期格式化能力。

### 8. 传输有界

`UpdaterBuilder::timeout(UPDATE_REQUEST_TIMEOUT)` = 30 分钟。transport 自身没有超时，不设界的话
一条卡死的连接会让 worker 永久阻塞。取值刻意宽松（覆盖整条载荷传输，而不是单次读取），因为它
要逃离死连接，不是约束慢链路；窗口不阻塞在传输上，可以随时关闭。

### 9. 明确不做的三件事

- **取消下载**：库在 `download_extended` 内部读完全部载荷并验签，没有 abort 钩子。要做到真取消
  只能自研下载与验签——而自研验证层正是 ADR-0029/0034 删掉的东西。因此不提供取消按钮，并在
  runtime 的文档里写明原因。
- **安装前 quiescence**：见下节。
- **操作系统包签名验证（codesign / Authenticode 的客户端校验）**：仍是独立发布门禁，本 ADR 不碰。

## 需要维护者复核的一处偏差

`AGENTS.md` §10 与 TODO §8.4 的条目是「**安装前**协调 runtime/renderer shutdown，失败可回滚」。
本实现把协调放在**载荷安装完成之后、替换进程之前**：

```text
check → download → verify → install → 按 §5.3 顺序 shutdown → exec 新构建
```

理由是三条同时成立的观察：

1. **macOS 的整包替换对运行中进程是 inode 安全的**：`rename` 后旧 inode 仍被进程持有，替换本身
   不需要进程先退出。
2. **Windows 的安装路径本来就会终止进程**：库在运行安装器后 `exit(0)`，不存在"安装时进程还在写
   文件"的窗口。
3. **先 quiescence 会让安装失败变成不可恢复**：`Application::shutdown(self)` 消耗 `Application`，
   没有对应的 restart。若在安装前就把 runtime/overlay 停掉，而安装随后失败（磁盘满、权限、
   库在 macOS 上 `remove_dir_all` 后 `rename` 失败），应用会停在"overlay 已销毁、runtime 已停止、
   只能重启"的状态。当前顺序下，安装失败时应用**完好无损**，可以直接重试。

因此当前顺序在失败路径上严格更安全，而在成功路径上同样满足"新构建启动前完成 shutdown"。
**这是一处与既有文档措辞不一致的取舍，需要维护者确认**；若不接受，应改为
"先 shutdown 再 install"，并同时为 `Application` 增加可恢复的 pause/resume 能力——那是另一项
独立工作，不在本 ADR 范围。

## 真实端点首次运行的结果（2026-09-15）

第一次在真实安装产物上运行检查更新时，窗口报 `update_release_fetch_failed`（"无法读取发布信息"）。
排查结论记录如下，因为它同时说明了两件事。

`https://github.com/ayangweb/BongoCat/releases/latest/download/latest.json` **存在且返回 200**，但内容
是**旧 Tauri 版本的 updater manifest**，不是本项目打包流程产出的那份：

| 观察 | 线上文档 | 本项目产出 |
| --- | --- | --- |
| 平台键 | `darwin-aarch64`、`windows-x86_64-nsis`、`linux-*` | `macos-aarch64`、`windows-x86_64` |
| 每个条目 | 只有 `url` + `signature` | 另有必需的 `format` |
| `pub_date` | 有 | 刻意不写 |
| 签名 | `trusted comment: signature from tauri secret key` | 本项目 minisign 私钥 |

库里 `ReleaseManifestPlatform::format` 是**非可选**字段，所以 18 个条目全部缺 `format` 时，整份文档在
平台查找之前就反序列化失败 → `Error::Serialization` → 当时被映射到 `ReleaseFetchFailed`。

也就是说：**更新功能本身没有缺陷，是线上还没有由新流程发布的 release。** 这也是 ADR-0034 待验证项 1
第一次被真正执行到——它暴露的是"端点被旧产物占用"，而不是"客户端有问题"。

排查过程还暴露了一个诊断粒度问题并已修正：

- `ReleaseFetchFailed` 原本同时表示"取不到发布信息"和"取到了但读不懂"。这两件事需要不同的应对
  （前者通常可重试或属运维遗漏，后者说明发布的 manifest 不是本产品的），合并成一个码会让排查必须
  读源码。
- 现在拆成两个稳定码：`update_release_fetch_failed`（`Error::ReleaseNotFound`：没有 release、没有
  manifest 资产、或非成功响应）与 `update_release_manifest_invalid`（`Error::Serialization`，以及
  `Error::Semver`——本构建自己的版本在任何请求之前就已解析，所以库返回的 semver 错误只可能来自
  manifest 的 `version` 字段）。按 ADR-0034 的说明，新增码是向后兼容的。
- 两条新增能力测试在 loopback 上固定了这个故障形态：
  `a_release_without_a_manifest_is_a_fetch_failure` 与
  `a_manifest_from_another_pipeline_is_rejected_as_unreadable`。后者不只断言失败，还做对照实验：
  给同一份文档的每个条目**只补上 `format`**，失败就从 `Serialization` 变成 `TargetNotFound`——这证明
  缺 `format` 是解析失败的真正原因，而平台键拼写是第二个独立问题。

运维含义（不属于代码范围，但必须写下来）：线上 v1.1.0 仍是旧 Tauri 版本，而工作区版本号也是 `1.1.0`。
即使把 manifest 换成正确形状，`release.version > current_version` 对已安装的旧 1.1.0 也不成立，
旧用户会被判为"已是最新"而永远不迁移。新流程的首次发布需要选一个高于旧线的版本号。

## 残余风险与待验证项（不得当作已确认）

1. **真实发布链路未跑通**：manifest 获取这一步已在真实端点上执行并完成定位（见上一节），但没有
   任何一次真实 GitHub Release 被**下载、验签、安装**过——线上没有由新流程发布的 release。窗口的
   其余状态由离线脚本化 engine 测试覆盖，真实链路仍见 ADR-0034 待验证项 1。
2. **Windows 安装路径未在本机验证**：本机是 macOS。`Installed` 在 Windows 不可观测这一点来自源码
   阅读，未实测；窗口在该平台的 `installed_relaunching` 文案因此也未被真实观察到。
3. **macOS 自动重启未实测**：`exec` 新构建这条路径需要一次真实安装才能验证；`exec` 失败时进程
   以退出码 1 结束并写 stderr，用户需要手动启动。
4. **发布说明注入未在真实发布中跑过**：`gh api .../generate-notes` 的形状、`--notes-file` 与
   manifest `notes` 的一致性只有单元测试与工作流静态检查，没有真实 tag 发布证据。说明来源已于
   2026-09-16 改为 changelog 提取（见 §7 补充），本项仍未消除：`just release-notes` →
   `just release-manifest` 已在本机端到端跑通，但没有真实 tag 发布证据，也没有在 GitHub 发布页上
   验证过合成后的 Markdown 渲染。
5. **manifest 仍无防降级保护**：说明字段不改变这一点，能替换 manifest 的攻击者仍可把客户端指向
   旧但签名有效的载荷。
6. **库的内存行为未变**：验签前把整个载荷读进内存，大产物下的峰值内存仍未测量。

## 验证

已完成（2026-09-15，本机 macOS 26.5 / aarch64）：

- `cargo fmt --all --check` 通过。
- `cargo clippy --locked --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings`
  零警告；`bongocat-app` 的 `storage-test-injection` 与 `production` 两种 feature 组合各自
  `--all-targets -D warnings` 通过。
- `cargo test --locked --workspace` 全部通过。
- `bongocat-update`：27 个单元测试 + 10 个 `release_manifest_capability` 测试通过，新增 stage 归属、
  `UpdateEvent`、`unavailability()`、传输上限、发布页 URL，以及"取不到 manifest"与"manifest 读不懂"
  分成两个码的定向测试；能力测试另在 loopback 上固定了两种失败形态（无 manifest →
  `ReleaseNotFound`；旧 Tauri 文档 → `Serialization`，并用只补 `format` 的对照实验证明原因）。
- `bongocat-app::update`：26 个测试通过，其中 11 个用脚本化 engine 在**真实 worker 线程**上驱动
  状态机，覆盖 check 的三种结果、失败的 stage/code 归属、安装的四步顺序、无 release 时 install
  不改状态、不可用构建不进入管线、重启请求只被消费一次，以及 drop 会停止线程。
- `bongocat-app`（bin）：新增 `restart_delay_elapsed` 的边界测试（未到延时、刚好到、以及单调时钟
  回退时不得提前重启）。
- `bongocat-ui` 的**无头渲染测试**（`update_window::render_tests`，7 个）：这是本项目第一批真正
  渲染窗口的测试。`bongocat-ui` 的 dev-dependency 里给 `gpui-kit` 打开 `test-support`，GPUI 因此
  使用测试平台，可以在没有显示器的情况下开窗、布局、绘制，并对注册过的元素做查询与点击。
  覆盖：18 个阶段逐一绘制且 footer 只提供该阶段允许的动作（`render` 是否真的遵守 `offers_*`
  以及每个变体能否活着画完，只有这里能看出来）、检查按钮文案随阶段区分、更新内容区跟随
  manifest 的 `notes`、以及**完整链路**（共享状态 → 窗口的 250 ms 轮询 → view 快照 → 渲染）。
  为可查询，产品代码里给 footer 动作与更新内容区加了 `.test_support()`；`test-support` 关闭时
  它是恒等函数，正常构建不受影响。
- 临时模拟面板（`update_simulation.rs`，为人工逐一切换查看每种状态而临时加入）已于 2026-09-15
  整体删除：全部 `SIMULATION-HOOK` 标记清除，默认窗口高度恢复 460，面板自身 6 个测试与 3 个
  依赖面板的渲染测试一并移除，`render_tests` 只保留不依赖面板的永久用例。
- `bongocat-ui::update` / `update_window`：13 个测试覆盖错误码目录唯一性、stage 往返、阶段可用动作、
  revision 语义、客户端与共享状态的一致性，以及"每个错误码都能解析出文案而不是解析出它自己的
  key"。后者用删除 key 的方式双向反证过：两个语言都删则它失败，只删 `zh-CN` 则它通过而
  `bongocat-i18n` 的跨语言一致性测试失败——`rust_i18n` 的 `en-US` fallback 意味着"某个语言缺
  文案"由那条既有测试守卫，本测试只负责"所有语言都缺"这一种。
- `bongocat-packaging`：16 个测试通过，新增发布说明写入、空说明不写入、超长按字符边界截断。
- `python3 -m unittest discover -s tools/tests` 全部通过（57 项）。
- 端到端冒烟：用三份 fragment 跑 `just release-manifest`，产出的 `latest.json` 含 `version`、
  `notes` 与三个 `platforms` 键，Windows 条目为 `nsis`、macOS 条目为 `app`。
- 产品启动冒烟：`target/release/bongocat-app --run-seconds 12` 退出码 0、无 stderr 输出。12 秒跨越
  了 10 秒的自动检查起点，因此同时验证了更新服务能启动、自动检查在 Development 构建上被
  `unavailability()` 拦下而不发请求、以及退出序列能 join 更新 worker 而不记录失败。
- 真实端点首次运行（2026-09-15）：在真实安装产物上点击检查更新，窗口正确显示"当前版本"、真的
  发起了请求，并诚实报出失败；随后用 `curl` 取回线上 manifest 完成字段级定位（见上一节）。
  这是真实链路第一次被走到网络层，结果是"端点被旧产物占用"而非客户端缺陷。
- **未验证**：更新窗口的**视觉**（间距、字号、是否重叠）没有实机截图证据，键盘导航也没有实机
  走查；无头渲染测试证明的是"每个阶段都画得出来、该有的动作都在、文案正确"，不是"看起来好看"。
  按 §11 的 800x600 / 125-200% 缩放条件核对仍待人工完成。下载与安装仍未在真实 release 上跑通
  （线上尚无可用的新流程 release）。
- **依赖面变化**：本次给 `bongocat-ui` 增加了一个 **dev-dependency**（`gpui-kit` 的
  `test-support` feature），只影响测试构建。它把 `proptest` 及其传递依赖（`bit-set`、`bit-vec`、
  `rusty-fork`、`wait-timeout`、`quick-error`、`convert_case`、`proptest-macro`）带入 lock；
  `proptest` 本就在 workspace 依赖里，因此没有引入新的依赖家族。按 §9 跑了完整 `cargo update`，
  只升了 `camino 1.2.5 → 1.2.6` 与 `rustls 0.23.44 → 0.23.45` 两个补丁版本。
- **正式依赖变化**：更新内容渲染 Markdown 新增了一个正式依赖 `pulldown-cmark =0.13.4`
  （当时 crates.io 最新稳定版，MIT，`default-features = false` 去掉 `getopts` CLI 解析与 `html`
  渲染器——说明渲染为 GPUI 元素，不产出 markup）。纯解析器、无 I/O、无 `unsafe`，停止维护时的
  替换边界是自写一个受限于标题/列表/强调/链接/代码块的极简解析器；GitHub 风格表格刻意不支持。

已完成（2026-09-16，说明来源改为 changelog 提取，本机 macOS / aarch64）：

- `bongocat-packaging`：23 个测试通过（原 16 个 + 新增 7 个）。新增覆盖：
  条目按二级标题匹配（正文提及的版本、`###` 子标题、围栏代码块里的标题都不参与）、两种标题写法
  与"版本必须是完整 token"（`<version>-rc.1`、`<version>.1`、`11.1.0`、`##1.1.0` 都不匹配）、
  CRLF 归一、缺失条目的错误信息含该文件实际记录的版本、`--extract-release-notes` 与构建/合并选项
  互斥、仓库两份 changelog 记录的版本列表一致且非空。
- 端到端：`just release-notes` → `just release-manifest` 用三份 fragment 跑通，产出的 `latest.json`
  的 `notes` 为 6168 字节，结构为「英文条目 → `---` → 中文条目」，不含版本标题，远低于 32 KiB 上限。
  另单独验证了门禁：版本在 changelog 里没有条目时命令以退出码 1 失败，stderr 为
  `CHANGELOG.md has no release notes for <version>; it documents <列表>`。
- `tools/tests/test_release_changelog_contract.py` 新增 6 个用例；`python3 -m unittest discover
  -s tools/tests` 共 63 项。
- **既有失败（非本次引入）**：`test_product_version_contract.py` 的
  `test_runtime_and_ui_use_the_compiled_product_version` 在 `next` 上失败——该测试要求当前工作区版本
  字面量不出现在指定源文件里，而 `crates/bongocat-update/src/runtime.rs` 的测试夹具 URL 里有三处
  `v1.1.0`（1144、1149、1172 行，`HEAD` 版本即如此，本次未改动该文件）。夹具用哪个版本号不影响它
  要固定的形状，但修与不修属于该测试自己的取舍，未在本次改动范围内。本次新增的测试数据因此一律
  使用合成版本号（`9.9.9` 一类），避免新写死一个"当前版本"。
- **未验证**：合成后的说明在 GitHub 发布页上的 Markdown 渲染没有实机证据；真实 tag 发布仍未跑过
  （见残余风险 4）。

当前修订验证（2026-09-25）：

- `bongocat-config`、`bongocat-ui-protocol`、`bongocat-ui` 的定向/全量单元测试，独立
  `config-store` contract（含两个恢复测试），Draft 2020-12 schema、locale 校验、定向 Clippy 和
  `cargo check --workspace --release` 通过。
- 调度器纯函数测试覆盖 `1` 小时、默认 `24` 小时、自定义 `48` 小时、最大 `8760` 小时、从实际派发
  锚点重排期限和轻量轮询上限；配置服务测试覆盖 `48` 小时持久化、关闭开关保留值、stale revision、
  `0`/`8761` 拒绝以及重启恢复。
- 当前 Windows 环境的全量 workspace 测试仍有一个未触及的模型封面路径规范化失败；工具测试另有
  symlink 权限和路径分隔符环境失败，均不改变本项配置/调度证据。
