# Phase 0 Model Resource Inventory

状态：静态清单与 Rust model3/资源索引完成，运行时兼容待 Cubism spike
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

用户模型不得直接提交到仓库。可分发的运行时 fixture 必须使用取得分发授权的样本；静态导入 preflight 使用人工合成的最小目录。

2026-08-28 对本机旧版数据目录只执行结构统计，未复制或记录用户模型 id、名称和内容：

| 指标                                                   |           观察值 |
| ------------------------------------------------------ | ---------------: |
| 自定义模型根目录                                       |               13 |
| 文件                                                   |              558 |
| 总大小                                                 |       约 360 MiB |
| 单文件最大值                                           | 50,760,792 bytes |
| model3 / physics3 / pose3                              |      13 / 13 / 0 |
| motion3 / exp3                                         |          78 / 42 |
| 符号链接                                               |                0 |
| 缺少 resources/background/cover/left-keys 的模型根目录 |                0 |

该样本说明 importer 必须处理大文件和大量 motion，且不能把预置模型“不含 physics/pose”的结论推广到自定义模型。统计不代表所有用户模型都符合相同目录完整性。

## 合成异常模型 fixture

`shared/fixtures/model-fixtures/cases.json` 固定六类导入 preflight 行为：

| 用例                    | 预期结果 | 稳定诊断                           |
| ----------------------- | -------- | ---------------------------------- |
| 缺失 moc                | reject   | `model_moc_missing`                |
| 损坏 model3 JSON        | reject   | `model_json_invalid`               |
| 非 ASCII 和空格路径     | accept   | 无                                 |
| 32768 x 32768 纹理头    | reject   | `model_texture_dimension_exceeded` |
| `..` 引用逃逸模型根目录 | reject   | `model_reference_escapes_root`     |
| 多个 model3 入口        | reject   | `model_entry_ambiguous`            |

Phase 0 暂定单边纹理尺寸上限为 `8192`。超大纹理 fixture 只保存紧凑的 PNG 十六进制头，并在临时目录物化；validator 必须在完整图片解码和 GPU 分配前拒绝。所有合成包只验证入口发现、JSON 解析、引用解析和纹理头限制，既不包含有效 Cubism moc，也不得传入 Cubism Core。它们不能证明 drawable、motion、expression、physics、mask 或 renderer 兼容。

运行 `python3 tools/validate-fixtures.py` 会复制每个源 fixture 到隔离临时目录、比较精确诊断并在退出时清理物化文件。验证不得修改源包或当前活动模型。

## Spike 验收

三个预置模型分别完成：

1. model3 解析和路径验证；
2. moc consistency/model creation；
3. texture、drawable order、blend 和 mask 绘制；
4. motion、expression 和音效；
5. 输入参数驱动；
6. 100 次 load/switch/destroy 无持续资源增长。

## Legacy Core Compatibility Baseline

`shared/fixtures/model-fixtures/legacy-core-baseline.json` 冻结旧应用当前 Web Core 对三个预置 moc 的可重复观察。来源 Core 为 `public/js/live2dcubismcore.min.js`，SHA-256 `25ae938cb4fe282ce189b357bcc97e603d1e1f7ec78bf04150d401c23cdc792f`，运行时版本为 `5.1.0`（`0x05010000`），其最高 MOC enum 为 `MocVersion_50`。

| 模式     | moc enum        | consistency | parameters | parts | drawables | canvas             |
| -------- | --------------- | ----------- | ---------: | ----: | --------: | ------------------ |
| standard | `MocVersion_40` | true        |         37 |    10 |        21 | 612 x 354，PPU 354 |
| keyboard | `MocVersion_40` | true        |         34 |    10 |        19 | 612 x 354，PPU 354 |
| gamepad  | `MocVersion_40` | true        |         42 |    14 |        25 | 612 x 354，PPU 354 |

运行 `node tools/inspect-legacy-cubism-models.mjs --check` 会先校验 Web Core 与三个 moc 的固定 hash，再在隔离 VM 中创建并释放 Moc/Model，最后与 snapshot 精确比较。工具不接受任意模型、不修改资源，也不接入新应用运行时。

该 snapshot 是未来 Native R5 spike 的回归 oracle，不是 R5 兼容证据。新 sys/safe wrapper 必须对同一组固定 moc 产生相同 moc version、基础计数和 canvas；drawable 数据、mask、motion、expression、physics、renderer 与重复销毁仍按 Cubism spike 单独验收。

## Rust Model Package Index

`spikes/model-package/` 以纯 Rust 强类型解析三个预置 `cat.model3.json`，并在读取 Cubism Core 前验证所有 model3 引用：moc、纹理、display info、expression、motion、音频、physics、pose 和 user data。`resources/background.png`、`cover.png` 与左右键图片也进入同一规范化索引。

解析器统一把 `\\` 转为 `/`，拒绝绝对路径、盘符、`..`、跨根符号链接和符号链接目录递归；关联 JSON 有 16 MiB 上限并要求 object 顶层，单文件上限为 512 MiB，包上限为 1 GiB/4096 文件/32 层目录。PNG 在解码或 GPU 分配前只读取签名与 IHDR，并执行 8192 单边上限。

`shared/fixtures/model-fixtures/preset-model3-index.json` 冻结完整索引。Rust 测试同时重跑六类合成异常包，并额外覆盖跨根符号链接和目录深度。三个预置包分别解析出 71/75/28 个文件，字节数与本清单一致；每个包均有 3 张模型纹理、3 个引用 expression、2 个 motion group，且明确报告未被 model3 引用的 `exp_1.exp3.json` 与 `exp_2.exp3.json`。

该索引只完成包发现、静态解析、路径安全和资源存在性验证。moc consistency/model creation、motion/expression/physics/pose 求值、D3D11/Metal 绘制和销毁压力仍属于 `P0-CUBISM` 的未完成门禁。
