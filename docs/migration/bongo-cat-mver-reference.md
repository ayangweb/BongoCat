# Bongo-Cat-Mver Reference Baseline

状态：行为参考已固定
记录日期：2026-08-30

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
| 输入采集              | `BongoCatMver/src/input*.cpp`、mode 文件           | down/up 来源、设备状态查询、模式映射和丢失 release 场景                   |
| 模型资源约定          | `BongoCatMver/model`、`BongoCatMver/img`           | 预置模型、背景、键帽、鼠标/手柄资源和目录关系                             |

文件名随上游版本变化时用 `rg` 搜索相关 API 或产品字段，不凭记忆推断行为。

## 3. 已确认的渲染结论

固定版本通过官方 Cubism Framework OpenGL renderer 绘制模型：

- model3 layout 交给 model matrix 处理；调用方再组合 projection/MVP。
- texture 按 model setting 的 index 绑定，并显式配置 premultiplied-alpha 模式。
- clipping、drawable order 和 blend 由官方 renderer 完成。
- standard/gamepad 等最终画面不是只有 Live2D drawable；背景、设备和按键资源在
  mode 层按产品状态另行组合。

这些结论用于确定测试问题和预期行为。Native Rewrite 的 Metal/D3D11 renderer、
safe wrapper、runtime 和资源 compositor 仍须按 Technical Design 以 Rust 独立实现。

## 4. 使用规则

遇到输入、模型、渲染、窗口或模式行为问题时：

1. 先在当前 BongoCat legacy 源码、fixture 和该固定 Mver commit 中找到实际证据。
2. 区分“产品可见语义”和“旧技术实现细节”；只把前者写入当前 contract。
3. 用当前平台 API、Rust owner 和强类型 runtime 边界独立实现。
4. 为结论增加 fixture、snapshot、截图或实机复现，不能仅以“原版这样写”验收。
5. 若 Mver、当前 legacy 行为和 Technical Design 冲突，Technical Design 是架构事实
   来源；产品语义冲突写入 TODO/ADR 并明确选择，不静默猜测。

禁止直接复制 C++ 业务实现、把 SFML/OpenGL/DirectInput 重新引入生产依赖，或用
旧版全局状态和线程模型绕过当前 runtime、input、renderer 与 platform 边界。
