# Cubism Core R5 Probe

状态：macOS arm64 Core ABI 与三个预置 Moc 生命周期已通过；其他 target 与 renderer 仍待验证
记录日期：2026-08-30

## 1. Scope

本记录验证仓库外的官方 Cubism Native `5-r.5` Core 能否由 Rust raw binding
直接调用，并读取实现 renderer 所需的 Model 数组。它不验证 Framework 动作/物理
求值、GPU 绘制、其他 target ABI 或发布授权。

仓库只提交自有 `tools/cubism-core-probe/`。官方 header、Core dylib、真实生成
bindings、解压目录和编译 target 均保留在仓库外，未进入 Git 或 CI artifact。

## 2. Inputs

| Item                            | Value                                                              |
| ------------------------------- | ------------------------------------------------------------------ |
| SDK                             | `CubismSdkForNative-5-r.5.zip`                                     |
| Archive SHA-256                 | `7ff3a4bbc19c0a8728965aa522ab77eb11b252916453e68a8a78d3b71188bb12` |
| Core                            | `06.00.0001` (`csmGetVersion() == 0x06000001`)                     |
| Header SHA-256                  | `6f1802780d1eb36ff39705e0764f9eeed9b41c313a13ac155270c6f4ad51d53f` |
| Generated arm64 binding SHA-256 | `6cd53ddbb173d73a842b33a507c5c03c879adcb05a8c005730b58c1f0f061364` |
| macOS universal dylib SHA-256   | `d6b029354e47e81c1e063ad2de3cfc63bdd0b7bf3fe8dd079de17c2a4b43b27f` |
| Binding generator               | `bindgen 0.72.1`, config `cubism-core-r5-v1`                       |
| Binding config SHA-256          | `abacb15263ef79f17117551035d746d4ab1c336bcd398b61d0d26c01e12d9f77` |

主机为 Apple Silicon、macOS `26.5.2 (25F84)`、Rust `1.97.1`
(`aarch64-apple-darwin`)、Xcode `26.6 (17F113)`、Apple clang
`21.0.0 (clang-2100.1.1.101)`。证据源码为包含本文的 commit；旧模型文件只读，
运行过程没有修改模型或 SDK。

## 3. Checked Boundary

probe 对每个模型执行：

1. 按 Core 规定的 64-byte Moc 与 16-byte Model alignment 分配 Rust-owned memory；
2. 检查 Moc version/consistency，调用 revive、model size、initialize 和 update；
3. 验证 parameter、part、drawable 的 count、ID 和对应数组均可读；
4. 验证 r.5 的 packed drawable blend mode、`csmGetRenderOrders`、parent/offscreen
   index，以及 offscreen count/blend/opacity/color/mask/flag 数组；
5. 验证每个 drawable 的 vertex、UV、index、mask 指针与计数，并读取 canvas；
6. 清除 dynamic flags，按 Rust owner 的逆序销毁 Model memory 与 Moc memory。

所有 count 在构造 slice 前转换为非负 `usize`；非零 count 拒绝 null pointer；所有
累加使用 checked arithmetic；canvas、opacity 和 color 数据拒绝非有限浮点数。raw
pointer 不离开单次 `inspect_model` 调用。

## 4. Results

三个预置 Moc 各运行 `100` 次完整生命周期，每一轮的规范化 observation 必须与
首轮完全相等：

| Model    | Moc | Parameters | Parts | Drawables | Vertices | Indices | Mask refs | Offscreens |
| -------- | --: | ---------: | ----: | --------: | -------: | ------: | --------: | ---------: |
| standard |   3 |         37 |    10 |        21 |      224 |     780 |         5 |          0 |
| keyboard |   3 |         34 |    10 |        19 |      162 |     516 |         5 |          0 |
| gamepad  |   3 |         42 |    14 |        25 |      268 |     918 |         5 |          0 |

三者 canvas 均为 `612 x 354`、origin `306 x 177`、pixels-per-unit `354`；基础
count 与 `shared/fixtures/model-fixtures/legacy-core-baseline.json` 一致。旧预置 Moc
不含 r.5 offscreen，因此本次只证明零长度 offscreen API 路径正确，不能替代一个
具有 enhanced rendering/offscreen 数据的授权 fixture。

带固定 external rpath 的 release probe 经 `leaks --atExit` 再跑相同 300 次模型
生命周期，结果为 `0 leaks for 0 total leaked bytes`，peak physical footprint
`2624K`。这证明本 probe 的 CPU Moc/Model allocation 在该主机上完成释放，不证明
未来 safe wrapper、GPU texture 或 renderer 无泄漏。

## 5. Remaining Gates

- 在 Windows x64 使用对应 DLL/import library 重复 generation、compile/link、三个模型
  与 lifecycle/leak/handle smoke；
- 在 macOS x64 原生 Intel 主机重复 ABI 与模型验证，不能用 arm64 结果推断；
- Windows ARM64 等待官方 desktop Core，不使用 UWP artifact；
- 取得带 r.5 offscreen/enhanced rendering 的合法 fixture 并验证非零数组；
- 实现并评审产品 `bongocat-live2d` safe owner，随后接入 D3D11/Metal renderer；
- 完成 Framework 行为、发布授权、notice、签名和分发清单。
