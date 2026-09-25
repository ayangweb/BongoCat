# Bongo-Cat-Mver Reference Baseline

状态：行为参考已固定
初始记录日期：2026-08-30
最后核对日期：2026-09-24

## 1. 固定来源

- 仓库：<https://github.com/MMmmmoko/Bongo-Cat-Mver>
- 固定 commit：`4da0b9468ad3b6ffaa096eba3f080501d6ab0b5c`
- 本项目关系：BongoCat 的上游原版实现与产品行为参考，不是 Cargo dependency、
  vendor source 或当前架构模板。

后续考古必须先确认所观察源码的 commit。若需要改用更新 commit，先记录 diff、
行为变化和采用理由，不用浮动的默认分支结论覆盖本基线。

## 2. 优先查阅入口

| 问题                  | 参考文件                                           | 需要观察的证据                                                            |
| --------------------- | -------------------------------------------------- | ------------------------------------------------------------------------- |
| Cubism model 生命周期 | `BongoCatMver/src/myUserModel.cpp`                 | model3 资源装配、motion/expression/physics/pose、renderer 创建与销毁      |
| 模型布局与绘制        | `BongoCatMver/src/myUserModel.cpp`                 | `SetupFromLayout`、MVP、texture binding、premultiplied alpha、`DrawModel` |
| standard 模式         | `BongoCatMver/src/mode/mode98_live2d_standard.cpp` | Core update/draw 与背景、设备、手部、按键和音效资源的组合顺序             |
| 应用与窗口循环        | `BongoCatMver/src/main.cpp`                        | 窗口创建、消息/绘制循环、模式切换和 shutdown                              |
| 帧率与动画时间        | `BongoCatMver/include/catmain.h`、`src/main.cpp`   | 窗口帧预算、呈现节拍与动画时间源                                          |
| 输入采集              | `BongoCatMver/src/input*.cpp`、mode 文件           | down/up 来源、设备状态查询、模式映射和丢失 release 场景                   |
| 模型资源约定          | `BongoCatMver/model`、`BongoCatMver/img`           | 预置模型、背景、键帽、鼠标/手柄资源和目录关系                             |

文件名随上游版本变化时用 `rg` 搜索相关 API 或产品字段，不凭记忆推断行为。

帧率限制的完整语义——窗口帧预算与呈现节拍、动画时间源与帧率的解耦、以及明确不采纳的做法——见
`docs/phase-0/mver-frame-rate-semantics.md`；该文档只是本基线在帧率一项上的展开，commit 与使用规则
仍以本文件为准。

## 3. 已确认与待核对的渲染证据

固定 commit 的源码树能直接确认：

- `myUserModel.cpp` 通过官方 Cubism Framework 的 OpenGL renderer 绘制模型。
- model3 layout 交给 model matrix 处理；调用方再组合 projection/MVP。
- texture 按 model setting 的 index 绑定，并通过 `LAppTextureManager::CreateTextureFromPngFile`
  交给外部依赖加载。
- `myUserModel.cpp` 在 `PREMULTIPLIED_ALPHA_ENABLE` 未定义时调用
  `IsPremultipliedAlpha(false)`；但该宏是否在实际 Mver build 中定义，固定 commit 没有提供
  足够的构建配置证明。
- clipping、drawable order 和 blend 交由官方 renderer；standard/gamepad 等最终画面还会叠加
  背景、设备和按键资源，组合顺序在 mode 层。

该 commit **不包含** `LAppTextureManager` 实现、官方 OpenGL shader、Cubism Framework/SFML
版本锁定或完整 build flags，因此不能仅凭它断言实际二进制的纹理上传格式、shader 数学和所有
背景/按键 compositor 细节。另行固定的 Cubism Native Framework R5 `5-r.5` 行为来源
（见 `docs/phase-0/cubism-framework-behavior-sources.md`）确实展示了普通 `GL_RGBA` render target
与 encoded-space shader 参考路径，但它是独立的 R5 oracle，不是该 Mver 二进制来源的直接证明。

因此，encoded-space 颜色契约目前是基于用户报告、固定 Mver 调用关系和独立 R5 参考来源的
兼容性选择；精确来源链、背景/按键上传语义和最终像素一致性仍由 TODO 的实机/readback 门禁
确认。行为清单基线 `44f44bc` 的旧版 `src/pages/main/index.vue` 另直接确认窗口 opacity 施加于
包含背景、Live2D canvas 和按键图的根容器；这属于 legacy 产品行为证据，不是固定 Mver C++ commit
对 opacity 或 shader 的直接证明。BongoCat 的 Metal/D3D11 renderer、safe wrapper、runtime
和资源 compositor 仍须按 Technical Design 的 Rust 边界实现，并遵守 ADR-0030 规定的现有方案复用顺序。

## 4. 动作与表情生命周期证据

固定提交中的 `BongoCatMver/src/myUserModel.cpp` 显示：

- `Update()` 每帧先恢复模型参数，再更新 motion，随后独立更新 expression manager；motion 与
  expression 是两个不同生命周期的层。
- `SetExpression()` 以 force priority 启动目标 expression。固定 R5 expression manager 会在最新
  expression 淡入完成后删除旧层，但保留最新层；最新 expression 没有 duration，也不会自行
  清除。因此 BongoCat 的“最新表情淡入后持续应用，直到替换、模型切换或 shutdown”是既有参考语义，
  不是需要新增的定时播放行为。
- 同一文件在 `_motionManager->IsFinished()` 为真时会启动 idle motion。因此“一次性 motion 播放
  完后保持最终姿态”不是该 C++ 文件逐字实现的行为，而是 2026-09-24 维护者明确要求并写入
  `next` 的产品决策。实现只把它与 Mver expression 的持久最新层原则对齐，不把固定提交的
  idle 自动切换误写成来源事实。

该差异只涉及产品可见的完成语义；仍遵守本文的提交固定、只作行为证据、不复制 C++ 业务代码的
规则。

## 5. 自动呼吸与 physics 证据

同一固定提交的 `BongoCatMver/src/myUserModel.cpp` 还显示了待机运动的另一条来源：

- 每次加载模型都会创建 `CubismBreath`，固定配置 `ParamAngleX`、`ParamAngleY`、`ParamAngleZ`、
  `ParamBodyAngleX` 和 `ParamBreath` 的 offset/peak/cycle，并以 `0.5` 权重通过
  `AddParameterValue` 加法写回；这不依赖 model3 是否声明 `Breath` 组。
- `Update()` 在 motion、expression 和产品拖动之后调用 breath，再对 model3 声明的 `physics3`
  执行 `Evaluate()`，最后调用 Core update。physics 的输入/输出、粒子延迟、移动性以及其链接
  Framework 的时间步/插值语义共同产生头发等部件的待机飘动；固定仓库没有包含 SDK 版本或
  Framework 源码，不能把 R5 的 fixed-step 细节直接归给 Mver。
- 因此“模型没有 motion 文件”不等于“没有待机动画”；本仓库当前的 Rust physics3 v3 求值和
  reference breath 只能以固定提交的结构证据与本地真实模型诊断为依据，不能把整个 Cubism
  Framework 兼容性宣称为完成。

## 6. 使用规则

遇到输入、模型、渲染、窗口或模式行为问题时：

1. 先在远端 `pre-refactor-tauri` 分支、fixture 和该固定 Mver commit 中找到实际证据。
2. 区分“产品可见语义”和“旧技术实现细节”；只把前者写入当前 contract。
3. 按 ADR-0030 评估现有方案，再在当前平台 API、Rust owner 和强类型 runtime 边界内实现。
4. 为结论增加 fixture、snapshot、截图或实机复现，不能仅以“原版这样写”验收。
5. 若 Mver、远端 legacy 行为和 Technical Design 冲突，Technical Design 是架构事实
   来源；产品语义冲突写入 TODO/ADR 并明确选择，不静默猜测。

禁止直接复制 C++ 业务实现、把 SFML/OpenGL/DirectInput 重新引入生产依赖，或用
旧版全局状态和线程模型绕过当前 runtime、input、renderer 与 platform 边界。
