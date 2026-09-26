# ADR-0057: 模型窗口用右键拖动缩放

状态：Accepted
日期：2026-09-23
修订（2026-09-24）：缩放写回配置不再重建 HWND/NSPanel；统一改为在现有原生窗口上更新几何，
并在尺寸变化后立即填充新的 swap-chain/drawable，避免设置更新暴露未初始化透明帧。
取代：无（恢复旧版 `pre-refactor` 分支存在、`next` 尚未实现的窗口缩放交互）

## Context

`next` 的模型窗口尺寸只有一个来源：配置 `overlay.scale_percent`。在本 ADR 初始实现时，改变它意味着
**重建整个窗口**——`OverlaySessionOptions::requires_window_recreation` 把 `scale_percent` 列为重建条件，
重建会重新创建原生窗口与 GPU 资源，`GpuModel::prepare` 还会重新加载全部模型纹理。因此在这次改动之前，
调整窗口大小只有设置页滑块这一条路，而且每次都伴随一次可见的重建。

旧版（`pre-refactor` 分支 `src/pages/main/index.vue`）在主窗口上实现了这个交互：

```js
if (buttons !== 2 || !shiftKey) return          // 右键 + Shift
const delta = (movementX + movementY) * 0.5
catStore.window.scale = round(clamp(scale + delta, 10, 500))
```

即按住右键并移动指针即可改缩放，但要求同时按住 Shift，范围 `10–500`，并且直接改
`window.scale`（与设置页滑块同源）。`next` 没有移植这个交互。

同时，`next` 的右键已经被上下文菜单独占：Windows 拦截 `WM_CONTEXTMENU | WM_NCRBUTTONUP` 并转发
成 `OverlayContextMenuRequest`，macOS 用 `NSEvent` local monitor 监听 `RightMouseUp`，两者都交给
应用的 `muda` 菜单。任何新的右键交互都必须先解决这个冲突。

两个平台都没有窗口 resize 通路：渲染尺寸在窗口创建时固化进 swapchain/RTV/mask 纹理
（Windows）或 `CAMetalLayer` 的 drawable size（macOS），`overlay/src/windows/` 与
`overlay/src/macos.rs` 里此前不存在任何 resize 代码。

## Decision

- **触发**：右键按下后指针位移（欧氏距离）超过 `3px` 才算缩放；未越阈值的右键仍然是上下文菜单。
  不要求按住 Shift——旧版的修饰键要求是当时用 `mousemove` 无法区分单击与拖动的权宜做法，而
  位移阈值直接解决了同一个问题。越阈值之后即使缩放没有实际变化，这次右键也不再弹菜单。
- **映射**：`scale = clamp(按下时的缩放 + (dx + dy) * 0.5, 25, 400)`。系数沿用旧版（向右下拖动
  放大、向左上拖动缩小，右上/左下相互抵消），范围收敛到当前配置契约
  （`OverlayConfig::scale_percent`、`OverlaySettings::is_valid`）而不是旧版的 `10–500`：拖动结果
  必须总能被配置接受，否则一次拖动会产生一个存不进去的值。位移始终相对按下点测量，因此缓慢
  拖动与手抖都不会改变判定。
- **起点**：按下时的缩放由窗口**当前宽度**反算（`width / base * 100`，同样钳制到 `25–400`），
  而不是读配置里的 `overlay.scale_percent`。窗口几何与配置并不总是同步——用户拖动过一次、
  显示器 DPI 变了、或者手工编辑过配置——用配置值当起点会让第一次指针移动就把窗口跳到另一个
  尺寸。反算让拖动始终从用户看到的大小继续。
- **锚点**：窗口左上角保持不动，与旧版 `setSize` 的观感一致。Windows 用
  `SetWindowPos(..., SWP_NOMOVE)`；macOS 的 `NSWindow` frame 原点在左下角，所以按“顶边不动”
  修正 origin.y，两者在屏幕上等价。
- **实时跟随**：拖动过程中窗口逐帧改尺寸，渲染器**就地** resize，不重建窗口、不重载纹理。
  Windows 走 `IDXGISwapChain1::ResizeBuffers` + 重建 RTV/staging/每个 mesh 的 mask target；
  macOS 走 `setFrame:display:` + 重设 drawable size + 重建 mask 纹理。其余与尺寸无关的资源
  （device、pipelines、composition graph、模型纹理、顶点/索引缓冲）全程保留。
- **尺寸来源**：`100%` 的基准尺寸是 `default_overlay_window_dimensions(canvas)`，与窗口创建时
  使用的同一个值。Windows 的拖动状态机工作在物理像素（`SetWindowPos` 的单位），基准按窗口 DPI
  换算；macOS 工作在点。尺寸由基准与缩放重新算出，而不是在旧尺寸上累加像素，因此连续拖动不累积
  舍入误差。
- **持久化**：拖动结束时把最终缩放通过新的 `OverlayInteractionSinks::resize_sender` 报给应用，
  应用提交 `SettingsCommand::SetOverlaySettings` 写回 `overlay.scale_percent`；窗口几何仍由既有
  的 placement 通路（`OverlayWindowPlacementDebouncer` → `OverlayWindowPlacementChanged`）落到
  `window-state.json`。overlay 只提出请求，配置的写入方仍然是应用。
- **幂等**：缩放写回配置后，runtime snapshot 的变化会让 frame tick 走原地尺寸更新分支。该分支先
  判断窗口尺寸是否已经等于新缩放对应的尺寸（`bounds_match_scale`，容差 `1px`），相等时不再按
  比例重算——否则拖动刚设好的尺寸会被乘第二次；需要改变尺寸时只更新现有 HWND/NSPanel 与
  swap-chain/drawable，并在返回 frame loop 前立即绘制一帧，不替换原生窗口。
- **click-through**：穿透模式下窗口返回 `HTTRANSPARENT`（Windows）或 `ignoresMouseEvents`
  （macOS），收不到指针消息，因此不可拖动，也没有右键菜单。这与既有的“非穿透模式才可拖动”
  一致，不是本次引入的限制。
- **与 hover 隐藏的互斥**：拖动进行期间 `hide_on_pointer_hover` 不再生效。该功能会把窗口淡出并
  让指针事件穿透，正好会中断窗口自己正在执行的拖动；两个平台都在计算 hover 状态时把“拖动中”
  当作“指针不在窗口内”，因此拖动期间窗口保持可见、拖动结束后恢复原有的 hover 行为。
- **平台无关部分**：判定、映射、钳制与尺寸计算落在 `bongocat-overlay/src/resize_drag.rs`，
  可以在没有 GPU 和原生窗口的情况下测试；平台适配器只负责读取指针、改原生窗口尺寸、以及把
  最终缩放报出去。

## Consequences

- 行为变化：右键不再无条件弹菜单。按住右键移动超过 `3px` 会开始缩放，松手后不弹菜单；原地
  按下松开仍然是原来的菜单。
- 行为变化：缩放范围是 `25–400%`（旧版为 `10–500%`）。这是配置契约的范围，放宽它需要同时改
  schema、fixture、设置页滑块与校验，属于另一次契约变更。
- 拖动期间每个 frame tick 都会做一次 GPU resize：Windows 会重建 render target、staging 纹理与
  全部 mask target，macOS 会重建全部 mask 纹理。这些资源都很小，但拖动时的帧率低于静止时是
  预期结果，尤其是在有 mask 的模型上。
- 拖动结束后的写回不再重建 overlay 窗口；它只让 runtime snapshot 与已经完成的窗口几何对齐。
  若设置页直接改变缩放，现有窗口会在原地调整尺寸，并立即绘制新尺寸的第一帧，避免显示未初始化
  的透明 back buffer/drawable。
- 缩放值从此有两个来源（设置页与右键拖动），两者都写入同一个 `overlay.scale_percent`，因此
  设置页滑块在拖动结束后会显示拖动结果。
- 窗口位置与尺寸仍然分离：`scale_percent` 在 `config.json`，实际几何在 `window-state.json`。若两者
  不一致（例如手工编辑配置），下一次拖动会以当前窗口宽度反推的缩放为起点，并在结束时把两者
  对齐。
- 未验证项：两平台的实机拖动观感、Windows 的 `ResizeBuffers` 路径、macOS 的 drawable resize 与
  mask 重建都未经人眼核验；当前代码已通过 Windows 构建检查，但真实拖动、显示器/DPI 切换
  与 compositor 透明帧仍需目标硬件 smoke。
