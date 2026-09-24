# ADR-0046: Overlay 颜色契约以两端都执行 linear -> sRGB 编码为准

状态：Superseded by ADR-0063
日期：2026-09-18
取代日期：2026-09-24
取代：无（修正 Technical Design §Renderer 颜色段落与两条 Phase 3/5 验收证据的结论）

> 本文记录的是曾经用于补齐 Windows/macOS 线性编码不对称的修复决策与预期实现路径；
> 当时没有完成目标硬件的像素验证。它讨论了“两端是否都执行 linear -> sRGB 编码”，
> 但没有解决“是否要匹配 Bongo-Cat-Mver 的历史 encoded-space 观感”问题；当前产品目标由
> ADR-0063 取代。

## Context

Windows 与 macOS 上同一个模型出现明显色差：中间调整体不同，纯黑与纯白相同。排查后排除了
模型资源、纹理上传和 blend 模式，差异只落在像素管道的最后一步。

两端在编码之前完全一致：模型、背景和按键 PNG 都以 sRGB texture view 采样，shader 在解码后的
linear RGB 上执行 multiply/screen、mask 与预乘，blend 因子表也相同（normal 为
`ONE / ONE_MINUS_SRC_ALPHA`，additive 为 `ONE / ONE`，multiplicative 为
`DST_COLOR / ONE_MINUS_SRC_ALPHA`）。

分叉点在写入目标：

- macOS 把 `BGRA8Unorm_sRGB` 同时设给 `CAMetalLayer` 和管线颜色附件，硬件在 store 时完成
  linear -> sRGB 编码。
- Windows 的 flip presentation model 只能创建非 sRGB 的 `B8G8R8A8_UNORM` back buffer，
  代码据此把 swapchain 固定为 UNORM —— 但 back buffer 的 render target view 也是用
  `CreateRenderTargetView(texture, None, …)` 创建的，即继承 UNORM。于是 linear 预乘值被
  直接写进一个被 DirectComposition 当作 sRGB 解释的 surface：中间调输出约等于 `v^2.2`，
  黑白不变，且 alpha 边沿按 linear 权重加权。

Windows 官方文档给出的组合正是本例缺的一半：flip model 不允许 sRGB back buffer，正确做法是
UNORM buffer + `_SRGB` render target view。仓库原先把“DirectComposition 不接受 sRGB
swapchain format”当作完整结论，导致编码步骤从未执行，而 Technical Design 与两条验收证据都
断言这组格式已经避免了两平台颜色语义漂移。

## Decision

1. **两端都必须执行最终 linear -> sRGB 编码，语义以 macOS 现有的 linear 合成路径为基准。**
   Windows 保留 `B8G8R8A8_UNORM` swapchain，改由 back buffer 的 render target view 使用
   `DXGI_FORMAT_B8G8R8A8_UNORM_SRGB`（`COMPOSITION_RENDER_TARGET_FORMAT`）承担编码。
2. **alpha-only clipping mask target 保持 linear UNORM。** mask 只携带 coverage，对它做编码
   等于二次施加 gamma。
3. **不为对齐旧版观感而退回 gamma 空间。** 当时审查的 BongoCatMver/OpenGL 参考路径没有显示
   sRGB texture view 或 framebuffer encode，因此本条选择继续采用 linear 合成（decode + linear
   运算 + encode）；该来源链与最终像素当时均未完成验证，本条修复只补齐 Windows 缺失的编码。
4. **本机不可验证的 Windows 侧改动必须留下可重复的形态证据。** 由于 `bongocat-overlay` 在
   非 Windows 主机上无法为 Windows 目标构建，涉及 D3D11 调用形态的改动应以独立探针 crate 在
   `--target x86_64-pc-windows-msvc` 下通过 check/clippy，并在合并前删除探针。

## Consequences

- 按当时的设计，同一组 sRGB 贴图应在两平台产出相同的 surface 字节，并预期消除 Windows
  以往“整体偏暗、对比更强”的观感；该结果当时没有目标硬件 readback 证据。
- 修复是格式层的，不改变 drawable 拓扑、遮罩算法、frame 调度或任何产品配置，因此不需要
  数据迁移，也不影响 `schema_version: 1`。
- 未收敛的残差是系统级显示色彩管理：macOS 由 Core Animation 把 sRGB 内容转换到显示器色彩空间，
  Windows DirectComposition 不做这一步。广色域显示器上同一组 sRGB 值仍可能观感不同，这属于
  合成器行为，不属于 renderer 契约。
- 旧版观感对齐（是否改用 gamma 空间合成）仍是独立问题。若将来决定对齐旧版，需要同时修改两端
  的纹理视图与附件格式，并同步本文第 3 条与 Technical Design。

## Verification

以下记录的是 2026-09-18 当时的中间方案；其中测试名称和格式断言已被 ADR-0063 的
encoded-space contract 取代，不能作为当前实现的验证结果。

- `windows.rs` 的 `color_formats_decode_assets_and_encode_the_composited_frame_as_srgb` 增加
  `COMPOSITION_RENDER_TARGET_FORMAT` 断言；`macos.rs` 的对应测试继续固定
  `BGRA8Unorm_sRGB`，两侧注释互相指向，防止再次单边漂移。
- 本机（macOS 26.5.2 / Apple M1 Pro / Rust 1.97.1）通过：`cargo fmt -p bongocat-overlay --
  --check`、`cargo clippy -p bongocat-overlay --all-targets --all-features -- -D warnings`、
  `cargo test -p bongocat-overlay`，以及完整 workspace 门禁的其余各项（三组严格 Clippy、
  `cargo test --locked --workspace` 738 项、`cargo check --locked --workspace --release`）。
  仓库级 `cargo fmt --all -- --check` 未执行：它在 HEAD 上已因 `bongocat-platform` 的两个
  无关文件失败，属于既有问题。
- D3D11 调用形态（descriptor 字段、view 维度、`CreateRenderTargetView` 参数类型、DXGI 常量）
  以临时独立探针在 `--target x86_64-pc-windows-msvc` 下 check 与 clippy 通过。
- 尚未完成：Windows 实机首帧、Windows job 的单元测试执行结果、同 snapshot 的两平台 readback
  色值对照。该历史任务已由 `P3-MVER-COLOR-COMPATIBILITY` 取代；在新的跨平台硬件证据完成前，
  两条任务都不得作为当前观感验收依据。
