# ADR-0063: 模型渲染采用 encoded-space 兼容颜色管线

状态：Accepted
日期：2026-09-24
取代：ADR-0046
修订（2026-09-24）：根据多层模型在逐 drawable 透明度下出现重影的实机反馈，presentation opacity
改为在内部 drawable 合成完成后由平台最终 surface 统一施加。

## 背景与证据边界

用户反馈 Windows 上的模型颜色与 Bongo-Cat-Mver 明显不同。固定上游 commit
`4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c` 的源码树能直接确认以下有限事实：

- `myUserModel.cpp` 使用官方 Cubism Framework 的 OpenGL renderer，并通过
  `LAppTextureManager::CreateTextureFromPngFile` 加载模型纹理。
- `myUserModel.cpp` 的 `PREMULTIPLIED_ALPHA_ENABLE` 条件分支在宏未定义时调用
  `IsPremultipliedAlpha(false)`。
- drawable 的 clipping、order 和 blend 交给官方 renderer；背景、设备和按键资源还在 mode 层
  另行组合。
- 行为清单基线 `44f44bc` 的旧版 `src/pages/main/index.vue` 将 `window.opacity / 100` 施加到包含
  背景、Live2D canvas 和按键图的根容器；这支持“窗口 opacity 属于最终 surface”的产品语义，
  但它不是固定 Mver C++ commit 对 opacity 实现细节的证明。

该 commit **不包含** `LAppTextureManager` 的实现、官方 OpenGL shader、完整 build flags，也没有
固定 SFML 或 Cubism Framework 的实际版本。因此不能仅凭 Mver commit 断言最终二进制的纹理上传
格式、shader 数学、背景/按键上传语义或精确像素结果。

项目另行固定了 Cubism Native Framework R5 `5-r.5` 的行为来源
（`docs/phase-0/cubism-framework-behavior-sources.md`）。其中 OpenGL 参考路径使用普通
`GL_RGBA` render target 与 encoded-space shader 颜色处理，但它是独立的 R5 oracle，不是 Mver
二进制来源的直接证明。
本 ADR 因用户报告、上述有限 Mver 证据和独立参考路径而选择兼容性方向；来源链和像素等价仍是
TODO 的验收门禁，不被本 ADR 的 Accepted 状态掩盖。

## 决策

1. **两端统一采用 encoded-space compatibility contract。**
   - Windows：模型、背景和按键纹理使用 `DXGI_FORMAT_R8G8B8A8_UNORM`；composition back
     buffer 与 render target view 使用 `DXGI_FORMAT_B8G8R8A8_UNORM`。
   - macOS：模型、背景和按键纹理使用 `MTLPixelFormat::RGBA8Unorm`；CAMetalLayer 和
     color attachment 使用 `MTLPixelFormat::BGRA8Unorm`。
   - alpha 仍按 premultiplied alpha 输出；clipping mask 仍使用 alpha-only UNORM target。
2. **不在任一平台的 shader 中插入 sRGB decode/encode，也不对 blend color 做隐式转换。**
   两端的 shader、filter、预乘和 blend factor 由同一平台无关 contract 驱动，避免只修一端。
   模型、背景和按键先按模型/资源 alpha 完成内部合成；窗口 presentation opacity 不进入逐
   drawable 的 fragment alpha，而是在最终 surface 合成后由 Windows DirectComposition visual
   effect 或 macOS `NSPanel` 统一施加一次。这样多层 Live2D 不会各自衰减并产生重影。
3. **消费 Core 的 `double_sided` 状态。** D3D11 和 Metal 都对非 double-sided drawable（包括
   mask source）执行背面剔除；两端明确以 counter-clockwise 为正面。水平镜像会反转三角形
   winding，因此镜像时改用相反的剔除面；double-sided drawable、背景和按键 overlay 不剔除。
4. **这是兼容优先的产品决策，不是 encoded-space 线性光度正确性的声明。** 将来若要切换到
   linear-light，必须新增独立决策，同时修改两个 backend、pixel contract、截图基线和技术
   设计，不能只改 Windows 的 RTV 或只改 Metal 的 drawable。
5. **不改变模型资源、schema 或用户配置。** 这是 renderer-only 行为修复，不需要迁移、版本
   分支或旧配置转换。

## 影响

- 在来源链尚未完全核实的前提下，优先选择能解释用户色差、且不改变模型资源的产品兼容语义；
  最终是否与实际 Mver 像素一致仍由硬件 readback 决定。
- 两端使用相同的 encoded RGBA 输入和 blend state；presentation opacity 在内部 drawable 合成
  完成后由各自平台的最终 surface 机制统一施加，避免自动 gamma 转换或逐层衰减造成单边漂移。
- macOS Core Animation 和 Windows DirectComposition 仍可能在广色域显示器上对最终 surface
  做不同的系统级显示转换；renderer 不承诺跨系统色彩管理绝对一致。
- 不把 Cubism Framework 的 C++ 源码、shader 文本或厂商二进制复制进产品；实现只使用项目
  自有 contract，并遵守 Cubism 许可边界。

## 当前验证与待完成门禁

已完成的自动化验证：

- `cargo fmt --all -- --check`；
- `cargo clippy -p bongocat-overlay --all-targets --all-features --locked -- -D warnings`；
- `cargo test -p bongocat-overlay --locked`（当前 72 项通过）；
- `cargo test -p bongocat-render --locked`（16 项通过）与
  `cargo test -p bongocat-live2d-render --locked`（3 项通过）；
- `cargo check -p bongocat-overlay --target x86_64-apple-darwin --locked`；
- `cargo check --workspace --release --locked`；
- Windows `bongocat-overlay` model-switch preview（包含 D3D11/DirectComposition visual
  opacity effect 的创建、提交和资源切换）通过；这只是 renderer lifecycle smoke，不是 Mver
  像素验收；
- 共享 culling decision、blend-factor contract 和两端 format contract 单元测试。

完整 `cargo test --workspace --locked` 当前在 `bongocat-app` 测试二进制中报告 136 passed、1 failed：
`settings::tests::service_renames_and_covers_a_model_of_either_origin` 的 Windows 路径分隔/前缀断言；
命令随后停止，不能把它写成整个 workspace 已通过。`cargo clippy --workspace --all-targets
--all-features` 还会触发仓库现有的 `storage-test-injection` 与 Production 互斥 feature
`compile_error!`，因此 workspace 静态检查使用不含 `--all-features` 的命令并单独记录该限制。

尚未完成、因而不能宣称兼容验收通过：

- Mver 实际 build 的 Cubism/SFML 版本、texture manager 和 shader provenance；
- Windows/macOS 目标机首帧、模型截图与同 snapshot readback；
- encoded RGB、premultiplied alpha、mask、blend、presentation opacity 和颜色 tolerance 的
  跨平台对照；
- mipmap 生成/缩小过滤、背景与按键层顺序及复杂 mask channel parity；
- 真实 GPU/驱动、显示器色彩管理和 mirror/culling 的硬件验证。

在这些证据完成前，`P3-MVER-COLOR-COMPATIBILITY` 与相关 TODO 保持未勾选。
