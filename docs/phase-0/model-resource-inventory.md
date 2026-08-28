# Phase 0 Model Resource Inventory

状态：预置资源静态清单完成，运行时兼容待 Cubism spike
基线 commit：`44f44bc`
记录日期：2026-08-28

## 预置模型

| 模式     | 文件数 |    总字节 | left-keys | right-keys | 内容清单 SHA-256                                                   |
| -------- | -----: | --------: | --------: | ---------: | ------------------------------------------------------------------ |
| standard |     71 | 1,524,540 |        55 |          0 | `02b99308faaa94f676e9cef2b3796a672891d99e8b6fd6ceb4c080d4fbebef3f` |
| keyboard |     75 | 1,490,448 |        55 |          4 | `13c23a84f55e736d097536ab60d7fdbb370f9429d8f5d4f9b2154fdf7a42d456` |
| gamepad  |     28 | 1,206,645 |         6 |          6 | `05a9a1b3f4a2fc62893296416dfa17fd168d03d18abb7c2fdcb29efd1506bf61` |

内容清单 hash 的输入是按路径排序后的逐文件 SHA-256 文本。它用于检测 fixture 漂移，不替代发布包签名。

## Model3 共同特征

- `Version` 为 3。
- 每个模型引用 1 个 moc、3 张纹理和 1 个 cdi3 display info。
- 每个模型包含 3 个 expression。
- motion groups 为 `CAT_motion` 和 `CAT_motion_lock`，每组 2 个 motion。
- 每组第一个 motion 引用 FLAC 音频，motion fade in/out 均为 0。
- 预置 model3 没有引用 physics3 或 pose3。
- EyeBlink group 使用 `ParamEyeLOpen` 和 `ParamEyeROpen`。

## 模式能力

### standard

- 只有 left-keys；同一时刻按键图片按目录互斥。
- 包含 `CatParamLeftHandDown`、`ParamMouseLeftDown`、`ParamMouseRightDown`。
- 鼠标位置可驱动 `ParamMouseX/Y`、`ParamAngleX/Y/Z`、`ParamEyeBallX/Y`。

### keyboard

- left-keys 与 4 个方向键 right-keys。
- 包含 `CatParamLeftHandDown` 和 `CatParamRightHandDown`。
- 不包含专用鼠标按钮参数。

### gamepad

- left-keys 包含 D-pad 和左 trigger；right-keys 包含面键和右 trigger。
- 包含左右手和 `CatParamStickLX/LY/RX/RY`。
- 包含左右摇杆显示与按下参数。

## 模型目录协议

当前模型加载依赖以下约定：

```text
model-root/
  *.model3.json
  *.moc3
  *.cdi3.json
  *.motion3.json
  *.exp3.json
  optional audio
  texture-directory/*.png
  resources/
    background.png
    cover.png
    left-keys/*.png
    right-keys/*.png
```

目录可以只含 left-keys。right-keys 存在时，当前导入逻辑用文件名判断 keyboard/gamepad：包含 `East` 视为 gamepad，否则为 keyboard。该启发式必须在新 validator 中显式报告，不能静默误分类。

## 自定义模型样本策略

本机存在多种用户模型，可证明实际数据中会出现：

- physics3 引用；
- 大量自定义 motion；
- 多套 moc/cdi 文件共存；
- 缺少按键资源的模型；
- macOS metadata 文件；
- 非预置 expression 命名。

用户模型不得直接提交到仓库。后续 fixture 必须使用取得分发授权的样本或人工合成的最小目录，并覆盖：路径含空格/非 ASCII、缺文件、损坏 JSON、超大纹理、多 model3 和路径穿越。

## Spike 验收

三个预置模型分别完成：

1. model3 解析和路径验证；
2. moc consistency/model creation；
3. texture、drawable order、blend 和 mask 绘制；
4. motion、expression 和音效；
5. 输入参数驱动；
6. 100 次 load/switch/destroy 无持续资源增长。
