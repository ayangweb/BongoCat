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

### 1. `verify.yml` 同时在 `pull_request` 与 `master` push 上运行

**master 的运行不是为了验证 master。** PR 必须通过才能合并，因此 master 上的代码必然已经
验证过，再跑一遍在验证上是冗余的。它存在的唯一理由是产生 PR 读得到的缓存。

GitHub 的 cache 按 ref 分域，PR 只能读取基线分支（`refs/heads/master`）与自己
（`refs/pull/<n>/merge`）的条目。合并后该 PR 的条目就留在一个此后无人读取的作用域里。
实测（`docs/refresh-readme-status-bar` 的三次连续运行）：

| 运行 | cache 状态 | `macos-spikes` 耗时 |
| --- | --- | --- |
| 该 PR 第 1 次 | `Cache not found` | 16.9 分钟 |
| 该 PR 第 3 次 | `Cache restored from key` | 2.9 分钟 |

即**同一 PR 内部复用有效**（省 14 分钟），**跨 PR 无效**。而开发者等待的正是每个新 PR 的
第一次运行。没有 master 上的运行，就永远不存在 PR 能读到的作用域，仓库里 20 条 cache
记录全部挂在 `refs/pull/*/merge`、没有一条在 `master`，就是这一点的直接证据。

这同时解决第二个问题：每个合并的 PR 会在 10 GB 预算里留下 7–9 GB 死条目（Windows 与
macOS 的 `target/` 各约 3.2 GB，加上 spikes），三条 PR 就占满预算并把其余条目 LRU 挤掉。
master 键每次运行都被访问，能稳定留在预算里，死条目则被自然淘汰。

代价是每次合并后 master 多消耗一次完整套件的 runner 分钟。该代价由维护者明确接受，且
不进入任何 PR 的等待时间。

同一 ref 上被取代的运行用 `concurrency` + `cancel-in-progress` 取消：它的结果必然被下一
次 push 覆盖。

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
编译器，第二次是空操作（实测 0 秒）。现在 stable 检查显式使用 `cargo +stable`，并与
minimum 检查分处 `target/toolchain-stable` 与 `target/toolchain-minimum`，两个编译器不会
互相使缓存失效。这让一个此前静默失效的门禁真正生效，代价是该作业冷跑时多编译一次
workspace（约 3 分钟，不在任何作业的关键路径上）。

### 6. `spikes/model-package` 不再在 Linux 上重复跑

`model-package-platforms` 已经在两个发布平台上运行它的 fmt、clippy、test 与 release
check，Linux 上的一份是严格子集，从 `contract-spikes` 的矩阵里移除。

### 7. 失败证据契约从固定数字改为按作业推导

`tools/tests/test_failure_evidence_workflow.py` 原先断言 `verify.yml` 里恰好有 10 个
`Collect redacted failure evidence`。拆分后这个数字会立刻过期，而过期的数字无法区分"某个
作业丢了证据步骤"和"某个作业本来就没有"。测试改为解析 `jobs:` 块、断言每个作业各自持有
一对证据步骤。

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

- PR 的墙钟时间从"冷编译 + 全部串行"变成"增量编译 + 并行"。首次在一个新分支上运行时
  仍然是冷的，此时关键路径是 `product-smoke`（两个平台的 release 构建）。
- 拆分后的作业各自 `checkout` 并各自 restore 缓存，runner 分钟数会上升；换来的是墙钟
  时间和每个 PR 的反馈速度。
- 每次合并到 master 会额外触发一次完整验证。它不增加 PR 等待时间，作用是填充缓存；见
  决策第 1 条。
- 13 个作业各自持有失败证据上传，`retention-days: 7` 与脱敏工具不变。
- 本 ADR 只改变 CI 的执行方式。`just check`、`just build` 与本地开发流程不变。

## 已接受的残余风险

1. **依赖版本变化会击穿缓存**：`Cargo.lock` 变化时新键必然冷跑一次，之后由 master 的运行
   重新填充。这是缓存的固有性质，不是本决策的缺陷。
2. **master 的重复验证是已知冗余**：PR 通过才合并，master 再验证一次在验证强度上没有
   增量。保留它是为了缓存，代价是每次合并多一次完整套件的 runner 分钟。
3. **10 GB 预算是共享的**：其他 workflow（release、`macos-spikes`、`windows-gpui-spikes`）
   与本流水线的键竞争同一预算。短期 PR 遗留的缓存条目会被 LRU 逐出；稳态下 master 键因
   每次运行都被访问而保留。若某个作业反复变冷，应当先收缩它的缓存路径（尤其是重复的
   `~/.cargo/registry`）而不是继续加键。
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

### 预期关键路径（以 PR #1055 的冷跑实测步骤耗时推算）

| 作业 | 改动前 | 改动后 |
| --- | --- | --- |
| Windows 关键路径 | `Test workspace (windows-latest)` 53.6 分钟 | `product-smoke (windows-latest)` 约 20 分钟 |
| macOS 关键路径 | `Test workspace (macos-latest)` 44.9 分钟 | `packaging-smoke` 约 14 分钟 |

推算口径：每个作业的耗时等于其步骤在 PR #1055 中的实测值之和（全部为冷缓存），storage
构建按本机实测的 8 个 crate + 链接估算。master 缓存预热之后，增量重编的只有本次改动
触及的 crate，关键路径进一步降到个位数到十分钟量级。实际数字需要在第一次跑出带缓存的
运行后复核。
