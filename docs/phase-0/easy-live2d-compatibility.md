# easy-live2d Compatibility Boundary

状态：旧产品依赖面与 Native Rewrite 兼容边界已冻结
记录日期：2026-08-29

> 本文记录 BongoCat 的产品行为，不授权复制、翻译或重新许可 easy-live2d 或 Cubism Framework 源码。Native Rust 实现仍受 `cubism-framework-behavior-sources.md` 的许可门禁约束。

## 1. Audited Baseline

旧应用在 `package.json` 声明 `easy-live2d ^0.4.4`，当前 `pnpm-lock.yaml` 固定为 `easy-live2d 0.4.4`、`pixi.js 8.18.1` 和 `@pixi/sound 6.0.1`。lockfile 中 easy-live2d tarball integrity 为：

```text
sha512-3/PNWXng0vJcNm4y7pj6Rkux6KA6t/ZaSx50uVEDUpO9a50A+AGLRdG0E5vYKlFpWTOeZ/NxaL497CpXwJ20BQ==
```

本次审计的本地安装产物 hash 为：

```text
dist/index.js      SHA-256 9d5bd793768739357c0011556644d715c928b05f71b593033943cbe289a9abf9
dist/index.js.map  SHA-256 146e7a76a48e71a8bd9fe1b0951e2c82b7943615a94ada76d2226218dbb6ec4f
```

easy-live2d package metadata 声明 MPL-2.0，但其 bundle 同时包含受 Live2D Open Software License 管理的 Cubism Framework 内容。因此它只能作为旧行为 oracle，不能成为 MIT Rust 实现的源码来源。

## 2. Actual BongoCat Surface

旧应用直接使用的能力只有：

| BongoCat call                | Product meaning                                |
| ---------------------------- | ---------------------------------------------- |
| `CubismSetting.redirectPath` | 将 model3 的相对资源映射到应用可读取的位置     |
| `Live2DSprite(...).ready`    | 完整加载 moc、纹理和关联资源后才允许使用模型   |
| `width` / `height`           | 按模型 canvas aspect ratio 调整 overlay        |
| `getMotions()`               | 按 model3 group/index 建立动作列表与快捷键 ID  |
| `getExpressions()`           | 按 model3 顺序建立表情列表与快捷键 ID          |
| `startMotion(...Normal)`     | 手动动作、快捷键动作和预览使用 normal priority |
| `setExpression(index)`       | 通过稳定的 model3 expression index 切换表情    |
| `getParameterValueRangeById` | 将鼠标和手柄输入映射到模型参数范围             |
| `setParameterValueById`      | 驱动左右手、鼠标按钮、鼠标跟随和双摇杆参数     |
| `Config.MotionSound`         | 启用或禁用 motion3 关联音效                    |
| `Ticker.shared.maxFPS`       | 限制旧 Web renderer 的最大帧率                 |
| `destroy()`                  | 模型切换和退出时释放旧 sprite/model/texture    |

BongoCat 显式关闭 easy-live2d 的 mouse follow，自己计算显示器归一化坐标。它不直接使用 easy-live2d 的 pointer handler、drag、hit test、random expression、random user motion、WebGL API 或 renderer object；这些不属于 Native 兼容面。

## 3. Must Preserve

以下是产品可见语义，Native Rewrite 必须以 Rust 类型和 fixture 重建：

1. model3 中的 moc、texture、motion、expression、physics、pose、cdi、user data 和 motion sound 引用在 commit 前全部验证。
2. motion identity 是 `{group, index}`，expression identity 是 model3 顺序中的 index；UI 可另显示名称，但不得用本地化文本作为稳定 ID。
3. motion 使用 normal priority，读取 motion3/model3 fade 与 effect ID；开始新的带声音 motion 时，旧 motion voice 被停止，同一时刻最多一个 motion voice。
4. expression 保留 add、multiply、overwrite 与 fade 语义；越界 index 返回稳定错误，不能只写 warning 后假装成功。
5. 外部输入写入的参数是持久 override。每个 deterministic tick 必须先完成 motion、expression、eye blink/breath、pose 和 physics，再应用 override，最后提交 Core model update；否则持续按键、鼠标按钮或静止摇杆会在下一帧被覆盖。
6. 不存在的 parameter 返回 `None`/typed diagnostic；存在的 parameter 暴露 Core min/max，输入映射后再由模型范围 clamp。
7. texture 使用预乘 alpha，renderer 保留 drawable order、opacity、culling、blend、multiply/screen color、mask 和 inverted mask 语义。
8. 模型 ready 只在 Core model、所有必需资源和 GPU texture 都可用后发布；失败必须返回 typed error，不能留下永远 pending 的 ready state。
9. 模型销毁顺序保持 GPU resources -> Model -> Moc backing bytes；音频、motion/expression owner 和异步加载任务也必须停止或 join。

上述“最后应用 override”是旧库特有但已被 BongoCat 产品依赖的行为；它必须进入固定时间的参数 snapshot fixture，不能仅凭画面观察验收。

## 4. Intentional Changes

以下旧实现细节或缺陷不进入兼容范围：

| Legacy behavior                              | Native decision                                                        |
| -------------------------------------------- | ---------------------------------------------------------------------- |
| 目录中选择第一个 `.model3.json`              | 多入口模型包拒绝并返回稳定诊断                                         |
| 使用 JSON5 读取 model3                       | 按 Cubism model3 JSON 协议严格解析；非标准 JSON5 扩展不承诺兼容        |
| 加载前先 destroy 当前模型                    | 使用 prepare -> validate -> commit；失败保留当前可用模型               |
| loader 内部吞掉错误并继续 ready 流程         | 错误沿 typed result 返回，取消关联任务并释放已准备资源                 |
| 全局 `Config` 与共享 `Ticker`                | runtime snapshot 携带实例级设置，frame source 由 renderer owner 管理   |
| WebView asset URL 重定向                     | Rust model package 解析规范化路径并传给平台 renderer，不暴露 URL       |
| WebGL texture cache 与 Pixi scene graph      | Windows D3D11/macOS Metal 各自管理 GPU 资源，不复刻 Pixi object model  |
| easy-live2d pointer follow、drag 和 hit test | overlay/input/runtime 负责对应产品行为；未被 BongoCat 使用的库功能删除 |
| 随机 idle motion 和 random expression helper | 只有行为规范显式请求时才运行，不继承第三方默认值                       |
| console warning 和字符串异常                 | 使用稳定 error code、匿名诊断计数和用户可恢复状态                      |
| browser audio global singleton               | Rust audio service 明确 owner、抢占、停止、失败和 shutdown 语义        |

`maxFPS` 仍是产品设置，但其实现从 Pixi ticker 限制改为平台 frame source policy。不可见、occluded、degraded 和正常显示可使用不同 cadence，runtime 的单调 tick 与 renderer present cadence 不能混为一体。

## 5. Acceptance Matrix

| Contract               | Required evidence                                                 |
| ---------------------- | ----------------------------------------------------------------- |
| resource load          | 三个预置模型及异常 package fixture；失败不替换当前模型            |
| motion enumeration     | 每个预置的 group/index/file/fade/sound normalized snapshot        |
| expression enumeration | model3 order/name/file snapshot 与越界错误测试                    |
| update order           | 同一固定 tick 中交换 order 会产生差异的参数 golden                |
| persistent override    | key/button/axis 只写一次后跨多个 tick 保持，release/reset 后清除  |
| motion sound           | enabled/disabled、无 sound、解码失败、motion 抢占和 shutdown      |
| renderer               | 双平台相同 render command trace；像素允许声明过的误差             |
| lifecycle              | prepare failure、100 次 switch/destroy、取消中加载和有序 shutdown |

三个预置模型可以覆盖 model3、motion、expression、sound、EyeBlink 和 renderer 基础路径，但不含 physics3/pose3；后两项继续要求独立、已授权 fixture。

## 6. Source Evidence

- BongoCat adapter：`src/utils/live2d.ts`。
- 模型、鼠标与参数映射：`src/composables/useModel.ts`。
- 手柄参数映射：`src/composables/useGamepad.ts`。
- 模型切换、motion/expression 入口：`src/pages/main/index.vue`、`src/stores/model.ts`。
- 依赖版本与 integrity：`package.json`、`pnpm-lock.yaml`。
- easy-live2d installed metadata/API/source map：固定 hash 的 `node_modules/.../easy-live2d/package.json`、`dist/index.d.ts` 和 `dist/index.js.map`，仅作本地审计输入，不提交其内容。

完成本清单不表示 Cubism Native R5 已加载、Rust Framework 行为已授权或三个模型已渲染。它只冻结旧库迁移的产品兼容边界，后续 Core/Framework/renderer spike 仍须逐项提供真实证据。
