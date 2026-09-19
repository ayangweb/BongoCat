# ADR-0036: 模型导入的压缩包来源与解压边界

状态：已接受（2026-09-16）
依赖：ADR-0030（先复用既有方案）、ADR-0027（诊断包已使用 `zip` 写入器）、ADR-0011（渐进实现与发布门禁）

## 背景

到本次改动为止，用户只能通过「选择文件夹」导入模型：`ModelStore::import_with_observer` 接收一个
目录，`PreparedModel::prepare` 就地校验它，`copy_package` 复制进 store 自己的 staging 目录，最后
`rename` 原子提交。

但模型站与社区导出普遍给的是 `.zip`。两个真实样本已经暴露了这件事：

| 归档文件 | 内部根目录 | 入口 |
| --- | --- | --- |
| `经典小键盘 · 标准模式.zip` | `经典小键盘 · 标准模式/` | `cat.model3.json` |
| `送葬人 · 标准模式.zip` | `图弟 · 标准模式/` | `demomodel.model3.json` |

两点值得注意：两个归档都把包放在**一层包装目录**下面，而第二个的包装目录名与归档文件名
**并不一致**。用户要在导入前手动解压，且解压后仍然要自己去猜该选哪一层目录。

同时，Technical Design 早已写明「模型导入防止路径穿越、符号链接逃逸、压缩炸弹和覆盖现有用户
数据」，但在此之前没有任何实现对应「压缩炸弹」——因为不存在解压路径。本 ADR 记录把压缩包接成
一等来源所采用的决策，以及解压这一步的安全边界。

## 决策

### 1. 来源类型由内容识别，不由扩展名或调用方标志决定

`detect_source_kind` 只看文件系统类型与文件头：目录 → 目录来源；常规文件且以 `PK\x03\x04` /
`PK\x05\x06` / `PK\x07\x08` 开头 → 压缩包来源；其余 → `model_store_source_archive_unsupported`。

因此：归档被改名（`model.package`）仍能导入；叫 `looks-like.zip` 的**目录**仍按目录处理；`.rar`
或损坏下载得到的是一个稳定诊断，而不是"先解压失败、再报包无效"。用户不需要告诉产品他选的是
什么，产品也不需要靠字符串猜。

### 2. 压缩包不是第二个解析器

压缩包来源与目录来源**共用同一套校验**：解压写满 staging 之后，仍然调用同一个
`PreparedModel::prepare`。压缩包只是"把同样的字节放进 staging 的另一种方式"，因此不可能绕过
路径规范化、符号链接拒绝、JSON/PNG/FLAC 侧车校验、纹理尺寸上限或包级上限。

这是本 ADR 最重要的一条：任何"为压缩包单独写一遍检查"的做法都会在两侧漂移。

### 3. 解压写进 store 自己的 staging 目录

解压目标就是 `create_staging_directory` 建好的 `.importing-<id>-<pid>-<seq>`，不引入第二个临时
目录、不复用系统 temp。由此得到的性质都是既有的，不需要新机制：

- 提交仍是**一次 `rename`**；失败时 `StagingCleanup` 删掉整个 staging，store 根目录不留痕迹。
- 权限仍是 store 的 owner-only（目录 0700、文件 0600），解压出来的字节不会经过一个"更宽松的
  中间位置"。
- 配置里没有指向临时路径的引用，取消与崩溃恢复沿用既有 `recover_abandoned_operations`。

### 4. 先校验、后解压：plan → extract 两遍

`plan_archive` 只读中央目录（`by_index_raw`，不解压、不解密），在这一遍里完成：条目名规范化、
条目类型、压缩方法、加密标志、条目数、深度、单文件与整包声明字节。`extract_archive` 才真正
解压。

这样做的直接收益是**压缩炸弹在还只是一个头部数字的时候就被拒绝**：恶意归档不需要被解压一个
字节就会失败，store 也不会先建好 staging 再报错。

代价是文件被读两遍，以及两遍之间归档可能被替换。后者由第二遍的**逐条目复核**处理：索引位置的
条目名、声明大小、`is_file` 必须与 plan 一致，实际写出的字节数必须等于声明值，否则
`model_store_source_changed`。`zip` 默认校验 CRC32，损坏的载荷因此也不会被当作成功。

### 5. 包装目录剥离

归档一个文件夹必然给每个条目加一层前缀，所以"压缩这个目录"永远是 `X/...`。保留这一层会让入口
发现失败（`model_entry_missing`），因此 `strip_wrapper_directories` 把它去掉，并且**反复**去掉
（`a/b/...` 这种嵌套压缩同样常见），直到剩下的条目不再共享同一个首段。

判定规则是被刻意收窄的：

- 只有当**所有**条目都以同一个首段开头时才剥离；
- 任何一个**文件**直接位于归档根就取消剥离（根目录本身就是包）；
- 一个**目录**条目位于根且等于该首段时不取消剥离——那正是包装目录在声明自己。

最后一条是必要的：`X/` 这个目录条目在每个"压缩文件夹"产物里都存在，如果它取消了剥离，功能
在真实归档上完全不工作。这个缺陷在开发中真实出现过，由
`wrapper_prefix_requires_one_directory_shared_by_every_entry` 固定。

### 6. 归档工具元数据被丢弃

`__MACOSX/**` 与 `._*` / `.DS_Store` / `.localized` / `Thumbs.db` / `desktop.ini` 不参与包：
它们既不参与包装目录判定，也不被解压。理由与 `ModelStore::list` 忽略同样文件的理由一致——它们
属于文件管理器，不属于模型。副作用是 `__MACOSX/` 与真正的包目录并列时，包装目录仍能被正确
识别（否则这一常见形态会直接导入失败）。

如果一个模型真的引用了被丢弃的 `._*` 文件，它会得到"资源缺失"，这是诚实的结果：包确实不完整。

### 7. 上限：给"容器"一个自己的界

`ModelPackageLimits` 新增 `maximum_archive_bytes`（默认 1 GiB，与 `maximum_package_bytes` 同值）。

不能复用 `maximum_package_bytes`：那条上限约束的是归档**产出**的包，在归档读取器解析中央目录
**之前**就要用它挡住输入；而"用 `Stored` 存一个 8 字节包上限的归档"本身可能远大于 8 字节。
把两者合并会让包字节上限对归档来源永不可达。也不能复用 `maximum_file_bytes`，否则单文件上限
同样变得不可达。

此外有一个独立的**结构上限**：条目数超过 `maximum_file_count × 4` 直接拒绝。乘数给目录条目与
被丢弃的元数据条目留出余量，同时保证"声明海量条目"的输入不会进入逐条目循环。

### 8. 诊断码：新增一个，复用其余

新增 `ModelStoreDiagnostic::SourceArchiveUnsupported`（`model_store_source_archive_unsupported`），
只在"这个文件根本不是可用归档"时使用：无法作为 zip 打开、加密、不支持的压缩方法、条目类型
既非文件也非目录、容器超出 `maximum_archive_bytes`、无条目、只有目录没有文件。

其余情形刻意复用既有码，因为它们描述的是同一件事：

| 情形 | 码 |
| --- | --- |
| 条目路径绝对 / 穿越 / 平台前缀 / 重复 / 文件与目录冲突 | `SourceEntryUnsupported` |
| 条目是符号链接 | `SourceSymlinkUnsupported` |
| 条目类型是设备/FIFO 等 | `SourceEntryUnsupported` |
| 深度、条目数、单文件或整包字节超限 | `SourceChanged` |
| 实际字节数与声明不符、归档在两次读取之间变化 | `SourceChanged` |

`SourceChanged` 用于上限与声明不符，是因为目录来源在同一处也用它（"源不再匹配已校验的形态"），
并且对归档而言，声明值就是归档自己的元数据，不一致就是源自相矛盾。这保持了"用户可见结果是
一组稳定码"的收敛性，而不是为同一结果造第二套词汇。

settings 层把新码映射到既有 `SettingsErrorCode::ModelImportSourceUnsupported`
（"模型来源包含不支持的项目"），因此没有新增用户可见错误码。

### 9. 依赖：`zip` 的 `deflate-flate2`，并显式依赖 `flate2`

`zip =8.6.0` 已在 workspace 依赖里（诊断包用它写 `.zip`），但当时是 `default-features = false`
且只走 `Stored`，没有解压能力。真实模型归档用 deflate（两个样本的压缩方法都是 8），因此打开
`features = ["deflate-flate2"]`。

`zip` 对 `flate2` 声明的是 `default-features = false`，自己不选后端；而 `flate2` 一个后端都没
选中时以 `compile_error!` 编译失败。因此 `bongocat-model` **直接**依赖 workspace 的
`flate2 =1.1.10`，让 `rust_backend`（miniz_oxide，纯 Rust）成为本 crate 在任何构建图里的确定
事实，而不是依赖"某个兄弟 crate 恰好打开了默认 feature"——那只在测试构建里成立。

其余 feature 一律不开（无 AES、无 bzip2/zstd/lzma/ppmd/deflate64）：模型归档不需要它们，而未
开启的压缩方法会以稳定诊断被拒绝（这也是"加密归档"的拒绝路径）。按 ADR-0030 与 `AGENTS.md` §9，
两个 crate 的版本都在当次用 `cargo info` 核对了 crates.io 最新非 yanked 稳定版：`zip` 8.6.0
（`9.0.0-pre3` 是预发布，不采用）、`flate2` 1.1.10。

### 10. UI：一个来源一个按钮

原生面板没有"选文件夹或文件"的统一形态（macOS 是 `pick_folder` 与 `pick_file` 两个不同面板，
Windows 的 common item dialog 同样区分），所以 Models 页面提供两个按钮：**选择文件夹**与
**选择压缩包**，各自的 tab index、焦点句柄与 AccessKit 节点齐备。

不做的替代方案：一个按钮加一个模式开关（用户要猜当前是什么模式）、或一个按钮先弹选择再弹面板
（多一步且同样要说明）。**由按钮决定来源种类，由字节决定它究竟是什么**。

压缩包按钮的对话框只过滤 `.zip` 作为便利，**不作为判据**：`validate_selected_archive` 只要求
"绝对路径 + 真实常规文件"，因此被改名的归档仍可通过，而"不是归档"由 store 报稳定码——对话框
层拒绝一个能用的归档，会把可导入的文件变成对话框级失败。

状态文案按所选来源区分（`models.import.archive.*`），不再把"压缩包"说成"文件夹"。挑选过程本身
与来源无关的三条文案（取消、取消后保留上次选择、选择器必须在 UI 线程）上移到
`models.import.picker.*`，因为它们描述的是选择器而不是文件夹。

### 11. picker 模块与稳定码改名

`bongocat-platform` 的 `directory_picker` 模块改名为 `model_source_picker`，
`DirectoryPickerOutcome` / `DirectoryPickerError` 改为 `ModelSourcePickerOutcome` /
`ModelSourcePickerError`，稳定码前缀由 `directory_picker_*` 改为 `model_source_picker_*`，
示例改名为 `examples/model_source_picker_smoke.rs` 并新增 `--kind directory|archive`。

理由是这次改动**引入**了一处不准确：让"压缩包选择器"返回 `DirectoryPickerError` 是错的。与之
相邻的既有模块名一并修正，代价是 4 个文件里的机械替换与一次示例改名。

### 12. `zip` 归档内容与第三方模型不进入仓库

两个真实样本是第三方模型（含 Live2D 模型数据），不提交到仓库。真实归档的验证改为一个由环境
变量驱动、未设置即跳过的测试：
`imports_the_archive_samples_named_by_the_environment`（`BONGOCAT_MODEL_ARCHIVE_SAMPLES` 指向一个
放 `.zip` 的目录）。仓库内的永久测试用测试期生成的归档（真 deflate 流）覆盖结构与拒绝面。

## 明确不做

- **`.rar` / `.7z` / `.tar.gz`**：只有 zip 被真实需求驱动。新增格式需要各自的边界分析（tar 的
  符号链接与硬链接、gzip 的单流特性、7z/rar 的许可证），不在本 ADR 范围。
- **加密归档**：没有密码输入面，也没有需求。加密条目被拒绝。
- **新增"解压中"进度阶段**：typed 操作契约的 stage 集合（prepare/copy/validate/commit）不变，
  解压计入 `Copying` ——它描述的是"把来源字节物化进 staging"，对归档而言就是解压。新增阶段会
  让 model/ui/app 三处的单调性契约与测试同时改，收益只是一个更细的动词，不值得。
- **把归档解压到已安装目录**：`next` 不允许任何就地更新语义；导入永远是"新 id + 新目录"。
- **压缩包内的多模型选择**：一个归档 = 一个模型，仍是入口发现规则（0 个 → 缺失，≥2 个 → 歧义）。

## 残余风险与待验证项（不得当作已确认）

1. **中央目录的分配没有在解析前被界住**。`maximum_archive_bytes` 限制输入文件大小，但
   `ZipArchive::new` 会在返回**之后**我们才拿到 `len()`；在此期间它已按中央目录声明分配。条目数
   上限（`× 4`）是在那之后生效的。最坏内存未测量，也没有针对"1 GiB 输入 + 海量声明条目"的测试。
2. **真实样本只有两个**，且都是单层包装、deflate、无加密、条目数 < 70。zip64、data descriptor、
   非 UTF-8 条目名、大小写混合扩展名只有合成测试覆盖。
3. **Windows 未实机验证**：本机是 macOS。归档解压路径本身是平台无关的（`std::fs` + `zip`），
   但 `SourceArchiveUnsupported` 在 Windows 上的真实触发路径（例如从资源管理器拖出的 zip）没有
   实测；`just check` 的 Windows 部分按仓库约定在 Windows 实机/CI 上运行。
4. **UI 实机交互未执行**：两个按钮、压缩包面板、`SelectionInvalid` 文案都只有无头测试覆盖。真实
   `NSOpenPanel` 选 zip 的路径需要在 macOS 上运行
   `cargo run -p bongocat-platform --example model_source_picker_smoke -- --expect-selected-any --kind archive`
   才算验证过。
5. **`maximum_archive_bytes` 的取值（1 GiB）没有基于实测压力测试**，它与包字节上限同值只是出于
   "容器通常不大于内容"的判断。
6. **本次 `cargo update` 顺带了 4 个无关传递依赖的补丁升级**（`synstructure` 0.13.2 → 0.14.0，
   连带切到 `syn 3`；`yoke-derive`、`zerofrom-derive`、`zlib-rs`）。这是 `AGENTS.md` §9 要求的
   完整 `cargo update` 的结果，不是本功能需要，但会让 diff 变大。

## 验证

已完成（2026-09-16，本机 macOS 26.5 / aarch64）：

- `bongocat-model` 60 个测试通过，其中 15 个新增（`store.rs` 10 个 + `archive.rs` 5 个），新增的
  `store.rs` 用例直接针对压缩包来源：
  同一包以目录与归档两种来源导入后 `ModelPackageIndex` **逐字段相等**（
  `zip_and_directory_sources_of_the_same_package_import_identically`，并用"包装目录名与归档名
  不一致"的形态构造）；按内容而非名字识别（无扩展名、`Stored` 归档、名为 `.zip` 的目录）；
  嵌套包装剥离与深度上限；`__MACOSX` / `.DS_Store` 被丢弃且不干扰包装识别；非归档 / 截断
  / 空 / 只有目录的归档；路径穿越（`../`、绝对、平台前缀）与重复条目（`猫//model.moc3` 与
  `猫/model.moc3` 归一化后相撞）、文件与目录冲突；符号链接条目；四类上限（文件数、整包字节、
  单文件字节、结构条目数）在解压前拒绝；进度单调 + 取消后无残留 staging 且不影响已装模型。
  另有 `archive.rs` 内 5 个单元测试覆盖包装判定、剥离、祖先冲突、条目名规范化、元数据识别。
- **真实归档验证**：`BONGOCAT_MODEL_ARCHIVE_SAMPLES=/tmp/bongocat-samples cargo test -p bongocat-model
  imports_the_archive_samples` 通过（样本 = 两个真实 zip）。逐字段结果：`经典小键盘 · 标准模式.zip`
  → 入口 `cat.model3.json`、moc `demomodel.moc3`、3 张 1024x512 纹理、cdi3、3 个表情、
  2 组动作（各 2 条，1 条带 FLAC 音轨）、31 文件 / 1 218 791 字节；`送葬人 · 标准模式.zip`
  → 入口 `demomodel.model3.json`、1 张 1024x512 纹理、cdi3、61 文件 / 791 595 字节。两个归档的包装
  目录都被剥离（安装根目录下直接是 `.model3.json`），源归档未被修改。
- `bongocat-app` 120（lib）+ 21（bin）个测试通过，新增
  `application_imports_folder_and_archive_sources_of_the_same_model`：
  同一模型分别以文件夹与 `.zip` 导入，两个 UUID id 各自独立、索引一致、目录侧标题为 `非 ASCII 模型`、
  归档侧标题为归档名去掉 `.zip`，两者都出现在 installed 目录里。
- `bongocat-ui` 115 个测试通过，新增 `model_source_display_name` 的 UTF-8 边界与大小写回归、
  目录名为 `something.zip` 时保留原名、归档草稿的状态文案为"已选择压缩包/所选压缩包不可用"、
  两个按钮的 AccessKit 节点与禁用传播。
- `bongocat-platform` 新增 `pick_model_archive` 的验证测试（归档与"不是归档的文件"都算合法选择，
  目录/文件互斥拒绝，相对与不存在路径拒绝，稳定码唯一性），示例改为支持 `--kind`。
- `cargo fmt --all --check`、三组 clippy（workspace `--all-targets --all-features`、
  `bongocat-app` 的 `storage-test-injection` 与 `production`）、`cargo test --locked --workspace`、
  `cargo check --locked --workspace --release` 全部通过。

**顺带修复的既有缺陷（与本功能无关，需要单独复核）**：`bongocat-platform` 的示例
`model_source_picker_smoke` 在 `next` 上**本来无法编译**（已用 `git stash` 在 HEAD 上复现）：
它引用 `objc2_app_kit::NSBackingStoreType`，而该类型属于 `NSGraphics` feature，清单里没有打开。
因为本次需要改这个示例，所以补上了 `"NSGraphics"`；这也是 `cargo check --all-targets` 能通过的前提。
若不接受这次修复，应单独回滚该清单行，并另行处理该示例。
