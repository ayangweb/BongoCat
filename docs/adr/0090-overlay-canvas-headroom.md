# ADR-0090: 悬浮窗为画布上方保留固定余量

状态：已接受（2026-09-29）

依赖：ADR-0057（右键拖动缩放）、ADR-0063（encoded-space 兼容颜色管线）

取代：无

## 背景

实机反馈与 `crates/bongocat-live2d/examples/canvas_bounds_probe` 对预置 `standard` 模型的探针结果：

- 升天表情（`Param5`）的天使环网格顶部在画布顶边之上 **1.4%**（画布高度），黄色光晕向上延伸到 **22.4%**。窗口高度严格等于画布宽高比（`model_window_dimensions`），画布之外的内容全部被窗口边缘裁掉，光环顶部与光晕上半段不可见。
- 同一模型把 thug life 的隐藏道具（墨镜网格，`opacity = 1`）停在画布上方 **23.1%–96%**，靠窗口裁剪隐藏；把窗口加高超过约 23% 会在待机状态暴露这些道具。上界 23.1% 因此是余量的硬上限。

渲染变换（`model_transform`）在窗口比画布比例更高时把模型纵向居中，加高的部分会一半落在画布上方、一半落在下方；只改高度不改变换就得不到"全部余量在画布上方"。

## 决策

1. **高度带上限的固定余量**：`model_window_height_for_width` 在画布宽高比高度之上增加 `CANVAS_HEADROOM_FRACTION = 0.20`。取值介于光环/光晕所需（1.4% / 22.4%）与停放道具暴露阈值（23.1%）之间。**该常量与本套预置模型的两个实测边界绑定，不是可随意调整的通用值**；对没有画布外效果的模型，它只是一段透明余量。
2. **画布贴底**：`model_transform` 在 `viewport_aspect <= model_aspect`（纵向有余量）时把画布锚定到窗口底边（`offset_y -= 1 - scale_y * model_height / 2`；无余量时该项为零）。余量全部位于画布上方。两端实现相同。
3. **窗口位置保持底边**：画布贴底意味着窗口底边不动则模型不动。模型尺寸变化引起的窗口高度变化保持底边：
   - Windows（顶边坐标）：`repair_stale_overlay_height` 把顶边上移 `旧高 - 新高`。
   - macOS（`NSPanel` origin 在底边）：保留 origin，只替换高度。
   两处修复都只在"已保存尺寸与当前尺寸规则不一致"（包含 1px 容差）时生效，即由旧规则保存的窗口状态在下次启动时一次性修复。
4. **不改模型资源、schema 与用户配置**。缩放（设置页与右键拖动）、模型切换都复用既有函数，自动带上新规则。

## 影响

- 每个模型窗口多出 20% 画布高度的透明顶带；模型的大小与屏幕位置不变（见决策 3），透明区域 click-through，无视觉影响。`keep_inside_screen` 与 hover 隐藏的判定区域随窗口变大。
- 右键拖动缩放的尺寸映射变为 `宽度 × 比例 × 1.2`；拖动观感与 ADR-0057 相同（左上角锚定不变）。
- 依赖"窗口裁剪"隐藏停放道具的模型（仓库内三个预置模型）在 20% 余量下不暴露；任何调大 `CANVAS_HEADROOM_FRACTION` 的改动都必须重新实测停放几何。
- portrait 画布模型的余量按画布比例放大（如 350x700 → 840 高）。

## 未验证项与残余风险

- macOS 的变换与创建修复只做了代数推导与单元测试，未在 macOS 实机验证（本机为 Windows）。
- 上述几何数据来自本机 Core 探针与贴图 UV 定位，不是厂商文档；不同模型的停放/越界几何未知。
- 其他第三方模型若把道具停在画布上方 20% 以内，会随余量暴露；本仓库三个预置模型不会。

## 验证

已完成：

- `cargo test --locked -p bongocat-overlay`：81 项通过（含新增的尺寸余量、底边修复与贴底变换用例）。
- `cargo fmt --all -- --check` 与
  `cargo clippy --locked -p bongocat-overlay --all-targets --all-features -- -D warnings` 通过。
- 实机（Windows 10 / x86_64，开发构建，显示缩放非 100%）四态截图：待机（模型位置不变、停放道具未露出）、升天（光环与光晕完整可见）、雷电（无黑块、余量区域无残留）、thug life（道具照常入画）。
- 窗口状态修复端到端：构造旧尺寸规则保存的窗口状态（高 172）重启，窗口按新规则修复为高 207，底边（`y + height`）保持不变。
- Release 全量构建通过（`cargo build --locked -p bongocat-app --release [--features production]`）。
- `cargo test --locked --workspace` 通过（含全部测试二进制与 doc-tests）。

未运行：macOS 构建与实机（本机为 Windows）。
