# ADR-0074: 验证流水线按缓存作用域与构建产物拆分

状态：已接受（2026-09-28）

## 背景

`.github/workflows/verify.yml` 此前只有一个 `Test workspace (${{ matrix.os }})` 作业
承担两个平台的全部验证：静态门禁、release 构建、所有 smoke、macOS 打包。实测
（PR #1055，Windows 53.6 分钟、macOS 44.9 分钟）里有两处纯粹的结构性浪费：

1. **每个新 PR 都从零编译。** 流水线只在 `pull_request` 上触发。GitHub 的 cache 作用域
   按 ref 划分：PR 只能读取基线分支创建的缓存，而它自己写入的缓存落在
   `refs/pull/<n>/merge`，之后任何运行都读不到。仓库的 cache 清单证实 20 条记录全部
   挂在 `refs/pull/*/merge` 上，没有一条在 `master`。于是每个新分支的第一次运行都
   `Cache not found`，clippy / test / release check / release 构建全部冷跑。
2. **同一份 release 依赖图被编译两遍。** storage-injection smoke 用
   `--target-dir target/storage-test-injection` 单独构建，而 `storage-test-injection` 是
   `bongocat-app` 的叶子 feature，不激活任何依赖上的 feature。第二个 target 目录因此与前
   面的 release 构建零共享：实测在 Windows 多编译 538 个 crate、耗时 14.2 分钟，在 macOS
   多编译 575 个 crate、耗时 8.4 分钟。

这两项都不是门禁成本：`just check` 要求的门禁集合没有变化，减少的是重复的编译和串行
等待。

## 决策

### 1. 只在 `pull_request` 上运行；master push 与手动触发都不保留

这一条是**先加上、实测无效、再撤掉**的，过程本身是结论的一部分。

最初加 `push: master` 的理由不是验证 master（PR 必须通过才能合并，那在验证上是冗余的），
而是缓存：GitHub 的 cache 按 ref 分域，PR 只能读基线分支（`refs/heads/master`）与自己
（`refs/pull/<n>/merge`）的条目，合并后该 PR 的条目就留在一个此后无人读取的作用域里。
同一 PR 内部的复用确实有效——`docs/refresh-readme-status-bar` 的三次连续运行里，
第 1 次 `Cache not found`、`macos-spikes` 16.9 分钟，第 3 次 `Cache restored from key`、
2.9 分钟——但**跨 PR 无效**，而开发者等待的正是每个新 PR 的第一次运行。

**但它没有兑现。** 加了 master 触发之后实测：

| 运行 | `Test workspace (windows)` | 原因 |
| --- | --- | --- |
| 某 PR 的第 2 次（同 PR 作用域有缓存） | 6.4 分钟 | 热 |
| master push | 19.0 分钟 | master 作用域缺该条目 |
| 下一个新 PR 的第 1 次 | 19.0 分钟 | 冷 |

仓库 cache 预算 10 GB，实际占用 **10.69 GB（26 条）**，已超。GitHub 在写入新条目时按 LRU
逐出，被挤掉的恰好是最大的两条——`Windows-workspace` 1.88 GB 与 `macOS-workspace`
1.72 GB——它们在 master 作用域里**根本不存在**。而且每个打开的 PR 都会在自己作用域里存一
整套 master 的副本，占用 ≈ `(1 + PR 数) × 单套大小`，所以同时开两个 PR 就必然再次超预算。
PR #1061 的 `product-smoke` 日志直接写了 `Cache not found`，尽管 master 作用域里那条
`Windows-product` 存在：两次运行并发，master 那次还没写完。

也就是说 master 触发**从未真正交付过预热**，只是让每次合并后多出一轮看起来莫名其妙的验证。
维护者据此决定撤掉它，接受 PR 冷跑约 20 分钟。`workflow_dispatch` 一并去掉。

保留的：`concurrency` + `cancel-in-progress`，同一 PR 上被后续 push 取代的运行不必烧分钟。

`tools/tests/test_verify_documentation_filter.py` 断言解析出的触发事件**恰好只有
`pull_request`**——它读的是 YAML 解析结果而不是文件文本，所以记录这段历史的注释可以照常
提到 `push`。将来若要重新加回，必须是一个有意识的决定。

### 2. `Test workspace` 拆成四个作业，按产物而非按平台切分

| 作业 | 平台 | 内容 |
| --- | --- | --- |
| `workspace` | Windows + macOS | fmt、clippy、`cargo test --workspace`、`cargo check --workspace --release`、production feature check、provenance |
| `product-smoke` | Windows + macOS | `bongocat-app` release 构建与全部默认 feature 的 product/storage smoke |
| `packaging-smoke` | macOS | `just build` 的两个 bundle smoke 与未知参数拒绝 |
| `windows-platform-smoke` | Windows | startup item、missing-release recovery、D3D11 模型切换 |

切分依据是**产物是否共享**，不是步骤数量：`just build` 为显式 target triple 构建，与
`product-smoke` 不共享任何 artifact；storage smoke 必须跑在默认 feature 构建之后，因此
留在同一个作业里。四个作业的验证集合与拆分前逐条对应，没有任何 smoke 被删除。

### 3. storage-injection 构建与默认构建共用 target 目录

`storage-test-injection` 只作用于 `bongocat-app` 自身，在同一 target 目录下构建只需重新
链接该 crate（实测本机 8 个 workspace crate、零第三方依赖），构建后把二进制复制到
`target/storage-smoke/`，storage smoke 继续跑属于自己的一份产物。`product-smoke` 中所有
读取默认 feature 二进制的步骤（single-instance、product icon、system menu）因此排在
storage 构建之前。

### 4. 缓存按作业分键，路径尽量不重叠

仓库 cache 预算是 10 GB，而合并后的单个 `target/` 已经是 3.2 GB，同一份 `target/` 用两个
键各存一次会直接挤爆预算并触发 LRU 逐出。因此每个作业只存自己产出的 profile：

| 作业 | 缓存路径 | 动作 |
| --- | --- | --- |
| `toolchain` | `target/toolchain-stable/`、`target/toolchain-minimum/` | 读写 |
| `workspace` | `target/debug/`、`target/release/`（只含 `cargo check` 的 metadata） | 读写 |
| `product-smoke` | `target/release/`（含 codegen） | 读写 |
| `packaging-smoke` | `target/*/release/`（显式 triple 的产物） | 读写 |
| `windows-platform-smoke` | `target/release/` | **只读**（`actions/cache/restore`） |

`workspace` 的 `target/release/` 只含两个 `cargo check --release` 产生的 metadata，几百 MB，
不含 smoke 作业链接出来的代码；保留它是为了让 release check 不必每次重编 189 个 crate。

`windows-platform-smoke` 只增加两个测试 harness 与一个 overlay 二进制，它们是
`product-smoke` 缓存内容的子集，因此只读不写——否则同一棵 release 树要在仓库预算里再存
一份。删除第二个 target 目录同时让 Windows 的缓存条目从 3.2 GB 降到 1.9 GB 左右
（按目录构成估算，未在 CI 上实测）。

各作业缓存的 `~/.cargo/registry` 仍然重复（约 0.3–0.6 GB × 条目数）。收敛为一个共享
registry 键是明确的优化项，但在条目数与预算都稳定之前不改动，避免同时引入第二个变量。

### 5. toolchain 作业的 stable canary 改为真实检查

`rust-toolchain.toml` 把 toolchain 钉在 1.97.1，因此作业里裸跑的 `cargo check` 始终解析
到被钉住的版本——"Build workspace with current stable" 与随后的 minimum 检查跑的是同一个
编译器，第二次是空操作（实测 0 秒）。现在 stable 检查显式使用 `cargo +stable`，并单独放在
`target/toolchain-stable`，让这个此前静默失效的门禁真正生效。它在关键路径之外，冷跑多编译
一次 workspace 的代价不进入任何 PR 的等待时间。minimum 半边随后按第 7 条删除，所以本作业
现在只做 stable canary 一件事。

### 6. `spikes/model-package` 不再在 Linux 上重复跑

`model-package-platforms` 已经在两个发布平台上运行它的 fmt、clippy、test 与 release
check，Linux 上的一份是严格子集，从 `contract-spikes` 的矩阵里移除。

### 7. 三处只有重复成本、没有覆盖的步骤被删除

按 PR #1059 与 PR #1055 的实测步骤耗时逐条排查后，删掉三处：

1. **toolchain 作业的 minimum 半边。** `rust-toolchain.toml` 钉住 toolchain，所以这一半与
   stable canary 跑的是同一个编译器；而 `workspace` 作业已在两个平台上用 clippy
   `--all-targets`、test 与 release check 编译了整个 workspace。严格子集，删除。
2. **四个 spike 作业里的 `cargo check --release`。** 这些作业的 smoke 跑的是自己
   `cargo build` 出来的 **debug** 可执行文件，release profile 的产物没有任何东西执行过：
   macOS 作业 4.95 分钟，两个 Windows GPUI 作业各约 3 分钟。产品自身的 release profile 由
   `workspace` 的 `cargo check --workspace --release` 与 product smoke 的真实 release 构建覆盖。
   `spikes/model-package` 的 release check 保留：它没有 smoke，那个检查是它唯一的 release
   信号。`spikes/input-windows` 的两条 `--release` 压力运行也保留，那是 issue #47 的证据。
3. **packaging smoke 的第二次 `just build`。** 两个 bundle 的差异只有编译进去的
   `BUILD_ENVIRONMENT` 常量与 bundle 内 provenance 的 `environment` 字段：reopen smoke 断言的
   是 bundle id、图标、签名和 LaunchServices 握手，两项都不读；environment 到 feature 的映射
   已由 `tools/tests/test_packaging_contract.py` 静态断言。留下的是"CI 不再端到端启动一个
   Development bundle"——"bundle 能在 runner 上启动"仍由同作业的 startup-item smoke 覆盖，
   它按 ADR-0051 必须使用 Production bundle。

删除 1 与 2 不在关键路径上，只减分钟；删除 3 把 packaging 作业从 18 分钟降到约 14 分钟。

### 8. 失败证据契约从固定数字改为按作业推导

`tools/tests/test_failure_evidence_workflow.py` 原先断言 `verify.yml` 里恰好有 10 个
`Collect redacted failure evidence`。拆分后这个数字会立刻过期，而过期的数字无法区分"某个
作业丢了证据步骤"和"某个作业本来就没有"。测试改为解析 `jobs:` 块、断言每个作业各自持有
一对证据步骤。

### 9. 文档改动跳过编译作业，但只有 `fixtures` 永远运行

编译器占了流水线约 93% 的时间，而 README、ADR、issue 模板这类改动一行代码都碰不到。
新增 `filter-paths` 作业判定"这次改动是否**只**碰了文档"，为 `true` 时其余作业全部跳过，
关键路径从约 9 分钟降到 16 秒。

三条让它不变成静默失守的约束：

1. **单向判定。** 过滤器只能*证明*改动与代码无关才能跳：识别不出的路径、读不到的 diff、
   空的改动集一律判为 `false`（全跑）。反过来那种"作业的输入变了才跑"的写法是**开口**的
   —— 将来新增一个顶层目录不匹配任何输入表，于是所有作业跳过，一次没人验证的改动带着
   全绿的 run 合并。这个方向是本决策里最容易写错的一步。
2. **门禁在作业级，不在 workflow 级。** 跳过的作业报告为 *skipped*，能满足 required check
   而不会卡住；workflow 级 `paths:` 一个都不匹配时**整个 workflow 都不运行**，required check
   永远不上报，PR 永久等待。`workflow_dispatch` 是"我怀疑跳多了"时的手动全量入口。
3. **`fixtures` 永远运行。** 它执行的契约测试会读 `verify.yml`、`release.yml`、两份 changelog、
   `justfile`、`macos/Info.plist`、`deny.toml`、`.github/dependabot.yml` 与
   `docs/product-runtime.md`——所以"改文档"不等于"不需要验证"。`.github/workflows/` 也因此
   不在文档白名单里：改流水线必须由这条流水线自己验证。

`tools/changed-paths.py` 承担判定，`tools/tests/` 里有两条契约测试守护：一条覆盖分类本身
（含"空改动集判为非文档"这个失败方向），另一条断言 **`verify.yml` 里每个作业必须要么在
永远运行的白名单里，要么带显式 `if:`**——新增作业时不可能"忘了决定它能不能跳"就把门禁关掉。

## 备选方案

- **只删步骤不改结构**：删掉 `cargo check --workspace --release` 或某个 smoke 可以立刻省
  时间，但那是降低门禁强度。已接受的残余风险清单里没有任何一条授权这样做。
- **把 macOS 打包 smoke 留在原作业**：省掉一个作业，但把 13 分钟的 `just build` 串行接在
  两条链后面，macOS 仍然是 40 分钟量级。
- **`Swatinem/rust-cache` 之类的第三方 cache action**：底层仍是同一个 GitHub cache 后端，
  同样的 10 GB 预算和同样的 ref 作用域限制，解决不了根因。
- **让 storage smoke 与默认 smoke 分处两个作业**：可以并行，但两个作业都要各自持有 release
  树，缓存占用翻倍；留在同一作业里共享 target 目录既省时间又省空间。

## 影响

- PR 的墙钟时间从"冷编译 + 全部串行"变成"冷编译 + 并行"，约 50 分钟降到约 20 分钟。
  **每个新 PR 的第一次运行都是冷的**，缓存只对同一个 PR 内的后续 push 有效。
- 拆分后的作业各自 `checkout` 并各自 restore 缓存，runner 分钟数会上升；换来的是墙钟
  时间和每个 PR 的反馈速度。
- 只改文档的改动只跑 `fixtures`，约 16 秒（见决策第 9 条）。
- 14 个作业各自持有失败证据上传，`retention-days: 7` 与脱敏工具不变。
- 本 ADR 只改变 CI 的执行方式。`just check`、`just build` 与本地开发流程不变。

## 已接受的残余风险

1. **PR 冷跑是常态，关键路径约 20 分钟**。曾尝试用 master push 触发预热，实测未兑现
   （决策第 1 条），维护者接受这个时间。同一 PR 内继续 push 会命中该 PR 自己的作用域，
   实测 `Test workspace (windows)` 从 19.0 分钟降到 6.4 分钟。
2. **10 GB 缓存预算已经超**（实测 10.69 GB / 26 条），条目在被写入时 LRU 逐出，最大的两条
   `*-workspace-*` 反复被挤掉。这是 master 触发失效的直接原因，也意味着**缓存不能作为时间
   承诺的依据**。若将来要重新尝试预热，必须先缩小单套体积：把重复十几遍的
   `~/.cargo/registry` 收敛成每个 OS 一条共享键是第一优先，其次是去掉 `workspace` 键里的
   `target/release`（那两个 release check 的 metadata，值 3.3 分钟）。
3. **master 没有任何独立验证**：只有 PR 触发，master 也没有分支保护。绕过 PR 直接推
   master 不会被验证。这是"只保留 PR 触发"的直接代价。
4. **`packaging-smoke` 不与 `product-smoke` 共享缓存**：它为显式 triple 单独构建，冷跑时
   多花约 9 分钟，但该作业不在关键路径上。

## 验证

- 流水线结构由 `tools/tests/` 的契约测试守护：每个作业必须持有且仅持有一对失败证据步骤，
  `retention-days: 7`、`collect-failure-evidence.py` 的脱敏入口与 `verify.yml` 中的
  `x86_64-pc-windows-msvc` 断言保持不变。
- 本机实测共用 target 目录后的 storage-injection 构建只重新编译 8 个 workspace crate、
  零第三方依赖（对照：改动前在 Windows CI 上是 538 个 crate、14.2 分钟）。
- 拆分后各作业的验证集合与拆分前逐条对应：Windows 与 macOS 的每一个 smoke 步骤都仍在
  workflow 中，且读取默认 feature 二进制的步骤全部排在 storage 构建之前。
- `python3 -m unittest discover -s tools/tests -p 'test_*.py'`：73 passed。

### 关键路径（实测）

| | 改动前 | 改动后 |
| --- | --- | --- |
| 冷跑（每个新 PR 的第一次） | `Test workspace (windows-latest)` 53.6 分钟 | 约 20–28 分钟，关键路径 `Smoke product (windows-latest)` |
| 同一 PR 的后续 push | 53.6 分钟 | 约 20 分钟（该 PR 作用域有缓存） |
| 只改文档 | 53.6 分钟 | 约 16 秒，只跑 `fixtures` |

改动前的数字取自 PR #1055 的逐步实测。改动后的冷跑区间来自合并后两次真实运行
（20.1 与 28.6 分钟），差异来自 10 GB 缓存预算下哪些条目幸存——这正是残余风险第 2 条。
**不要再把缓存命中当作时间承诺**：只有同一 PR 内的后续 push 才是可预期的，那个场景实测
`Test workspace (windows)` 为 6.4 分钟。
