# ADR-0086: 帧 smoke 校验不再要求「采到一个半透明像素」

状态：已接受（2026-10-07）
补充：ADR-0055（导入模型的封面由渲染截取）、ADR-0030（实现决策阶梯）
需求：用户报告——两个 Mver 模型同时导入后，后导入的那个启用时频繁提示「无法启用所选模型」，连续点击多次才偶尔成功；单独导入任一个则正常。

## 背景

用户给的两个模型是同一次 BongoCatMver 转换的产物，逐字节相同（只有 `resources/cover.png` 不同），所以
「哪个模型先导入」不是变量：真正决定成败的是**切换时那一帧的合成结果**。

两个事实把问题钉在渲染侧的 smoke 校验上：

- 产品日志（`production/logs/application-2026-10-07.log`）只有一条去重后的失败记录：
  `model/activation_failed | phase=selection | reason=runtime_command_failed`。`record_once` 会去重，
  所以「频繁提示」在日志里就是这一条；`RuntimeCommandFailed` 对应的都是 runtime 侧对本次 sequence
  报出的失败，不是模型包、store 或配置问题。
- 模型本身能启用：实测该模型 `drawables=300`、`masked_drawables=106`、`textures=1`，封面（导入时走
  **同一条** `GpuModel::prepare` + 首帧校验路径，见 ADR-0055）截取成功，所以「模型损坏」这个解释不成立。

根因在 `bongocat-overlay::frame::validate_frame_smoke`。它在 drawable 上按 17×17 固定网格采样，
原先要求 5 件事同时成立，其中一条是 `translucent_pixels > 0`——**采样点里必须至少有一个 `0 < alpha < 255`
的像素**。这条判据把「有没有抗锯齿」当成了「这一帧是不是一张真的图」，而抗锯齿是美术和网格落点的属性，
不是渲染正确性的属性：一张只有单张 8192² 纹理、alpha 边缘偏硬、在画布中占比较小的模型，合成后每个采样
点都可能是透明或全不透明。此时校验失败，而这条校验的位置决定了后果：

- 模型切换时它在 `create_overlay` 里先于 `report_model_commit` 执行（`replacement.draw(true)`），
  失败即 `reject_model_commit` → runtime 发布 `GpuPreparationFailed` → app 映射为
  `RuntimeCommandFailed` → 设置页显示「无法启用所选模型」。
- 因为它随姿态、光标位置和按键图变化（同一模型在封面截取那次通过、在切换这次不通过），所以表现为
  「点很多次偶尔成功」。用户观察到的「后导入的那个更容易失败」只是当时正在切到它。

本机复现（macOS arm64，真实 Metal，把两个已安装模型放进一个模型目录用 `--switch-cycles` 切）：

| 对象 | drawables / masked / textures | 切换结果（修复前） |
| --- | --- | --- |
| 用户的模型 | 300 / 106 / 1 | 3/3 次都在**第一次**切换失败：`Metal renderer readback found no translucent alpha coverage` |
| 仓库预置模型 | 21 / 5 / 3 | 通过 |

## 决策

### 1. 删掉 `translucent_pixels > 0` 这一条，保留其余三条

`validate_frame_smoke` 现在只要求：

- 至少一个透明像素——模型之外确实还没被画过，能区分「一片不透明的垃圾表面」；
- 至少一个模型像素（不透明 + 半透明 > 0）——确实画了东西；
- 至少两种可见颜色——是一张图，不是一块纯色填充。

这三条已经完整覆盖这条校验真正要回答的问题（「这块表面是不是一张图」）。删掉的那一条既不能补充这个
答案（未初始化的垃圾表面照样能满足它，纯色填充则被第三条拦住），又会让合法模型失败。`FramePixelStatistics`
仍统计 `translucent_pixels`，它只作诊断数据，不再作判据。

### 2. 保留「校验失败即拒绝这次模型提交」的语义

没有把失败改成「当作临时不可用去重试」。真正的空表面（无模型像素）在无合成器环境里是稳定状态，
ADR-0055 残余风险 5 已经把「失败」当作正确信号；无限重试只会把它变成卡住的窗口。本次只消除误判。

## 后果

- 与该模型同类的模型（单张大纹理、硬 alpha 边缘、画布留白多）现在第一次启用即可成功，不再需要连点。
- 预置模型与既有行为不变：仍要通过同三条判据，`just` 的预览与封面截取路径不受影响。
- 自动化契约：
  - `bongocat-overlay` 新增 `frame_smoke_accepts_a_frame_without_a_semi_transparent_sample`
    （只有透明 + 不透明采样时通过，钉住本 ADR），原拒绝用例改为钉住三条判据各自的消息；
  - `frame_smoke_requires_transparent_and_antialiased_model_coverage` 保留，半透明采样仍然算作合法采样。
- `Renderer readback` 的读回与统计口径不变，双后端（Metal / D3D11）共用同一个函数，Windows 侧同步生效。

## 明确不做

- **不调网格密度或改成自适应采样**：网格是 smoke 的固定口径，加密只会把「有时采不到」变成「更晚才采不到」，
  不改变判据本身的错误。
- **不给这条校验加重试次数**：issue 要求的是找出冲突原因，加重试是把误判藏起来。
- **不改 `reject_model_commit` 的语义**，也不动 `RUNTIME_TIMEOUT`（app 侧 2 s / overlay 侧 250 ms）。
- **不在本次处理 8192² 纹理与 106 张按 drawable 尺寸分配的 mask 纹理带来的显存占用**：实测单次 overlay
  已占约 620 MiB，切模型时新旧两份并存。这是独立的资源问题，见残余风险 1。

## 残余风险与待验证项

1. **该模型的显存占用偏高且未被本次改动影响**：`drawables=300`、`masked_drawables=106`，每张 mask
   纹理按 drawable 尺寸分配；实测 `device.currentAllocatedSize()` 单次约 620 MiB，切换期间新旧两份并存。
   本机（Apple Silicon 统一内存）没有失败，低显存机器与 Windows 侧未验证。
2. **产品 UI 路径未实机点击验证**：复现走的是 `bongocat-overlay --switch-cycles` 的切换预览
   （`sync_frame` + `draw(true)`），失败信息来自与产品相同的 `validate_frame_smoke`。产品
   `ProductOverlaySession` 走 `create_overlay` + `reject_model_commit`，两处的差别是错误处理方式而不是
   判据，但「设置页点启用后不再报错」这一点仍需要一次实机确认。
3. **Windows 后端未实机验证**：`validate_frame_smoke` 是平台无关的共享实现，Windows 侧只在本仓库
   交叉编译视角下成立（见 ADR-0055 残余风险 3 的同类限制）。
4. **`switch-cycles` 预览自带的显存增长断言本身不稳定**：修复后切该模型时它仍会以
   `Metal allocation grew from 655622144 to 655884288 bytes` 报错，增量 256 KiB（0.04%），预置模型也会
   偶发。这是诊断预览的断言口径问题（Metal 分配器不是单调的），产品路径没有这条断言；本次未处理。

## 验证

已完成（2026-10-07，本机 macOS arm64，真实 Metal 设备）：

- 复现与回归证据都来自同一个一次性探针（`bongocat-overlay` 的 `examples/switch_probe`，用完已删除），
  它把两个已安装模型放进一个模型目录并调用公开的 `run_model_switch_preview`：
  - 修复前 3/3 次在第一次切换失败，错误为
    `Metal renderer readback found no translucent alpha coverage`；
  - 修复后 3/3 次**全部 6 次切换提交成功**（只有上面残余风险 4 的断言报错，而该断言在循环之后才执行，
    因此它报错本身就证明每次切换都已提交）；同一探针对仓库预置模型仍通过。
- `cargo fmt --all -- --check` 通过；`cargo clippy --locked -p bongocat-overlay --all-targets --all-features
  -- -D warnings` 通过；`cargo test -p bongocat-overlay --locked` 全绿（78 项）。
- **未运行**：Windows 实机、设置页实机点击启用、低显存机器、`--settings-window-smoke`。
