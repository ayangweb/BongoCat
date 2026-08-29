# Phase 0 Rust Model Package Spike

状态：三个预置 model3、cdi3、motion3、exp3 与静态关联资源解析通过；Cubism Core、行为求值和 renderer 未进入本 spike
日期：2026-08-30

## Hypothesis

在不依赖 Cubism SDK、平台 API 或 GPUI 的前提下，Rust 可以先完成模型包发现、model3 强类型解析、引用路径安全、资源存在性、关联 JSON 边界和 PNG 尺寸预检，并为后续 Core wrapper 生成确定的资源索引。

## Scope

`spikes/model-package/` 是独立 Rust 2024 workspace，使用 `#![forbid(unsafe_code)]`。它负责：

- 要求包根目录恰好包含一个 `.model3.json`；
- 解析 model3 v3 的 moc、texture、display info、expression、motion/sound、physics、pose、user data、group 和 hit area；
- 强类型解析 cdi3 v3 的 parameter、parameter group 和 part，拒绝重复 ID、悬空/成环 group；
- 强类型解析 motion3 v3 的 curve target、segment encoding、时间边界、fade、user data 和 Meta 计数，并强类型解析 exp3 的类型、参数、fade 与 Add/Multiply/Overwrite blend；
- 规范化 `/` 与 `\\`，拒绝绝对路径、Windows 盘符、`..` 和 canonical root 之外的符号链接；
- 有界读取 JSON、文件和整个包，在图片解码/GPU 分配前检查 PNG IHDR 与尺寸；
- 索引 `resources/background.png`、`cover.png`、`left-keys` 和 `right-keys`，显式输出旧版 mode heuristic；
- 扫描完整包并报告总文件数、总字节数和未引用文件；
- 输出不含绝对源路径的稳定、可序列化 Rust 类型与 snake_case 诊断码。

默认限制为：单边纹理 8192、JSON 16 MiB、单文件 512 MiB、包 1 GiB、4096 文件和 32 层目录。限制作为显式输入传入，不读取配置或进程环境。

## Non-goals

- 不调用或模拟 Cubism Core，不校验 `.moc3` consistency。
- 不求值 motion、expression、physics 或 pose。
- 不解码纹理、不创建 GPU 资源、不绘制 Live2D。
- 不复制模型到数据目录，不修改源包或当前活动模型。
- 不把 spike 直接当作 Phase 4 产品 parser；Phase 0 结束后需按评审结论 promote、replace 或 delete。

## Dependencies

直接依赖精确固定为 `serde 1.0.229`、`serde_json 1.0.151`，测试使用 `tempfile 3.27.0`。三者均是 2026-08-29 crates.io 最新非 yanked 稳定版，许可证均为 MIT OR Apache-2.0；依赖用途仅限强类型 JSON 与隔离临时 fixture，公共 API 不暴露第三方类型。停止维护时可分别替换 JSON codec 与标准库临时目录 helper，不影响模型索引 contract。

## Reproduction

```text
cargo fmt --manifest-path spikes/model-package/Cargo.toml -- --check
cargo clippy --manifest-path spikes/model-package/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path spikes/model-package/Cargo.toml --locked
cargo check --manifest-path spikes/model-package/Cargo.toml --locked --release
cargo check --manifest-path spikes/model-package/Cargo.toml --locked --release --target x86_64-pc-windows-msvc
cargo check --manifest-path spikes/model-package/Cargo.toml --locked --release --target aarch64-pc-windows-msvc
cargo run --manifest-path spikes/model-package/Cargo.toml --locked -- src-tauri/assets/models/standard
```

规范化预置结果位于 `shared/fixtures/model-fixtures/preset-model3-index.json`。测试会从仓库中的三个只读预置目录重新生成内存索引并与该文件精确比较；golden 不会在测试中自动更新。

## Environment And Results

本地验证环境为 macOS 26.5.2 build 25F84、Apple Silicon arm64、`aarch64-apple-darwin`、rustc/cargo 1.97.1。格式、Clippy、12 项单元/fixture 测试、release check 和 license/source policy 通过；`x86_64-pc-windows-msvc` 与 `aarch64-pc-windows-msvc` release check 同样通过。

远端验收 build commit 为 `7ee8acd5f2a3d4dcb7a1dbc36623cbe497aeae49`。Push run `33238204993` 与 PR run `33238206415` 各 16 个 jobs 全部通过；Linux model-package jobs 为 `99062839956`/`99062844146`，Windows 为 `99062839561`/`99062844097`，macOS 为 `99062839774`/`99062844085`。三个平台均执行 format、Clippy 和 tests，Windows/macOS 额外执行 release check。首个 runner 发现 Rust 1.98 新增的 `chunks_exact_to_as_chunks` lint 后，修复提交同时增加奇数长度 fixture hex 拒绝，再由上述两组 workflow 完整复验。

motion3/exp3 强类型验证提交 `3f8f5bc` 的 push run `33269920418` 与 PR run `33269921789` 也全部通过；Linux、macOS、Windows model-package jobs 分别为 `99146452729`/`99146456459`、`99146452630`/`99146456364`、`99146452647`/`99146456373`。

| 模式     | 文件 |      字节 | 纹理 | expression | motion group | left/right keys |
| -------- | ---: | --------: | ---: | ---------: | -----------: | --------------: |
| standard |   71 | 1,524,540 |    3 |          3 |            2 |          55 / 0 |
| keyboard |   75 | 1,490,448 |    3 |          3 |            2 |          55 / 4 |
| gamepad  |   28 | 1,206,645 |    3 |          3 |            2 |           6 / 6 |

六类共享异常 fixture 的 accept/reject 与稳定诊断完全一致。额外测试证明 model3 引用的符号链接不能逃出包根，递归扫描在超过配置深度时确定失败。CI 在 Linux contract job 重跑同一套测试；代码不含平台条件或平台 handle，Windows/macOS runner 已产生对等通过证据。

本批进一步强类型读取 6 个预置 motion 文件和 15 个 expression 文件：motion 共 12 条 curve、45 个 segment、123 个 point、0 条 user data；expression 共 15 个 parameter，其中 Add 9 个、Multiply 6 个。测试拒绝截断/未知 segment、越过 segment end 的 Bezier control time、Meta 计数漂移、非有限时间或参数、空 Id、错误 expression Type 和未知 blend，并分别返回 `model_motion_invalid` 或 `model_expression_invalid`。这些结果只证明发布资源的结构与引用可读取，不证明 Cubism motion/expression 求值、参数混合、优先级或绘制语义正确。

索引 schema v2 又把三个 cdi3 的 parameter/group/part 数量固定为 standard `37/2/10`、keyboard `34/2/11`、gamepad `42/2/15`，其中 parameter 与 part 数量和 legacy Core baseline 完全一致。解析器使用 `model_display_info_invalid` 区分 display info 损坏，并在模型提交前拒绝重复 ID、悬空或成环 group。cdi3 是可选显示元数据，不作为 motion/expression 的权威 ID 白名单；真正的跨资源 ID 校验必须在取得 Core 参数/part 表后完成，以免误拒合法但不完整的 display info。

## Success And Failure Criteria

成功要求三个预置包的 model3/cdi3/motion3/exp3 索引与 snapshot 一致，六类异常 fixture 诊断一致，损坏 cdi3 和路径逃逸在读取 Core/GPU 前被拒绝，且格式、Clippy、测试、release 和 license/source policy 全部通过。

出现以下任一情况即失败：读取源包时发生写入；绝对/遍历/跨根 symlink 被接受；超限纹理进入解码；关联 JSON 未校验；输出含绝对用户路径；任一预置包资源未被验证；平台类型或 `unsafe` 进入该 workspace。

## Disposition

当前结论为 `promote candidate`：静态 package contract 可作为未来 `bongocat-model` 的输入，但只有在 `P0-CUBISM` 完成 Native Core、Framework 行为和双 renderer 门禁后才迁移到产品 workspace。当前 spike 保持隔离，不解除 Cubism 授权、binding、Moc/Model 生命周期或绘制阻塞。
