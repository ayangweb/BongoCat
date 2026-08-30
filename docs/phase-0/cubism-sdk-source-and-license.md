# Cubism SDK Source and License Gate

状态：本地 SDK/hash 已固定；书面许可与发布清单仍阻塞 stable
记录日期：2026-08-30

> 本文记录工程门禁，不构成法律意见。许可证的最终解释、Expandable Application 的认定和发布授权只能由 Live2D 或合格的法律顾问确认。

## 1. Version Decision

当前本地实现以 **Cubism 5 SDK for Native R5**（`5-r.4.1`，2025-07-17）作为固定验证版本。该版本对应 Core `05.01.0000`。

| 内容                     | 固定来源                                                                        | 固定引用                                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Native Framework         | [Live2D/CubismNativeFramework](https://github.com/Live2D/CubismNativeFramework) | tag `5-r.4.1`，commit `f426fc4f19852da74480e5aefe5cb99d90fd5d70`                                             |
| Native Samples           | [Live2D/CubismNativeSamples](https://github.com/Live2D/CubismNativeSamples)     | tag `5-r.4.1`，commit `51b4bc561ecda87045580c01324d2f7c6eec7642`                                             |
| Core header 与平台二进制 | [官方 Native SDK 下载页](https://www.live2d.com/en/sdk/download/native/)        | `CubismSdkForNative-5-r.4.1.zip`；SHA-256 `b71ec6bafc6578cce3b4cbbaa42a1cb51ae6eb477557932b02d22af957e733c7` |

两个 GitHub release 都没有附带 release asset。Samples 仓库只提交 Core 的 README、CHANGELOG、LICENSE 和 `RedistributableFiles.txt`，明确声明 Core 二进制不在仓库中。不能从第三方镜像、旧安装包或未知来源补齐 Core。

### 1.1 Local inspection evidence

2026-08-30 使用仓库中的 offline inspector 和固定 archive hash 得到：

| Artifact                           | SHA-256                                                            |
| ---------------------------------- | ------------------------------------------------------------------ |
| `Core/include/Live2DCubismCore.h`  | `0564a03edd0d56b90bac704bbbcc4e560b3d3d9000b49a0bd5d9cb886b414022` |
| Windows x64 `Live2DCubismCore.dll` | `c4599835b0349fcae774cedc6dbd0057743fa503d4cc0af8f9022ca3dd845634` |
| Windows x64 import `.lib`          | `88a3428b466d85776b9d50e38b1e7daa9aa6f270d1bea549d5ae0acadaa3865f` |
| macOS arm64 static library         | `b37198e8fa6e1cfc64300fa9ae42721e5c45889bfa15d6ee57389836c6acbe84` |
| macOS x64 static library           | `0489f2e2b07f208501f1bab3b8fc175cb6fa8dd50d8936b97b4506336cf94390` |
| macOS universal dylib              | `0cddec2342f983be3814ec5bf2159f580a28de818d2eef8d44b39ba98e80d896` |

本机只把 macOS universal dylib 解压到临时目录；`file`/`lipo` 确认同时包含 arm64
和 x86_64，动态调用 `csmGetVersion()` 返回 `0x05010000`，
`csmGetLatestMocVersion()` 返回 `5`。临时二进制、header、Framework 源码和
inspector JSON 均未复制到仓库。Windows ABI、第二机器复核和真实模型 lifecycle
仍保持未完成。

下载页要求下载者先阅读并同意 Live2D Proprietary Software License Agreement 与 Live2D Open Software License Agreement，并写明下载或启动软件即表示同意。因此 AI、CI、bootstrap 脚本和构建脚本均不得代替维护者勾选协议、绕过下载页或自动获取 ZIP。

## 2. Core Binary Matrix

R5 `Core/README.md` 和 `Core/RedistributableFiles.txt` 给出以下首发相关能力：

| Rust target               | R5 Core artifact                                                                                       | Phase 0 disposition                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------- |
| `x86_64-pc-windows-msvc`  | `Core/dll/windows/x86_64/Live2DCubismCore.dll` 与 import `.lib`；另有 MSVC 141/142/143 static variants | 首发候选；先验证 DLL + import library                            |
| `i686-pc-windows-msvc`    | `Core/dll/windows/x86/` 与 MSVC 141/142/143 static variants；DLL 调用约定为 `__stdcall`                | **产品范围外**；Native Rewrite 不构建或发布 x86                  |
| `aarch64-pc-windows-msvc` | 无 desktop Windows ARM64 artifact；只有 experimental UWP ARM64 DLL                                     | **发布阻塞**；ARM64 是产品目标，但 UWP DLL 不能替代 desktop Core |
| `aarch64-apple-darwin`    | `Core/lib/macos/arm64/libLive2DCubismCore.a`；另有 macOS bundle/dylib                                  | 首发候选；先验证 architecture-specific static library            |
| `x86_64-apple-darwin`     | `Core/lib/macos/x86_64/libLive2DCubismCore.a`；另有 macOS bundle/dylib                                 | 首发候选；需 Intel 实机与发布链验证                              |

`RedistributableFiles.txt` 列出的 Core 文件才是 Proprietary Software License 下的可再分发候选，并要求按 Live2D 提供的原样形式分发。BongoCat 不修改、重打包为另一种库格式或从二进制反向生成源码。最终选择 static 或 dynamic linkage 之前，还要验证代码签名、notarization、更新替换、崩溃符号和最终安装包中的许可文件。

## 3. License Gate

### 3.1 Core 与 Framework 是两套许可边界

- Cubism Core 使用 Live2D Proprietary Software License。
- Native Framework 和 Native Samples 属于 Live2D Cubism Components，使用 Live2D Open Software License；它不是 MIT/Apache/BSD 类宽松许可证。
- 官方 `NOTICE.md` 包含 `©Live2D`。发布 notice、终端用户条款和 trademark 展示方式必须在发布许可确认后形成明确清单。

BongoCat 的 Rust 源码不能直接复制、翻译或机械移植 Framework C++ 实现后仍默认按仓库 MIT 许可证发布。在 Live2D 书面答复或法律评审明确边界前，Framework 源码只能用于识别待验证行为，不能作为可直接移植的实现素材。若纯 Rust motion/expression/physics/pose 实现无法在许可边界内完成，Phase 0 必须给出 NO-GO 或明确条件，不能静默引入长期 C++ 业务 bridge。

### 3.2 Expandable Application 是发布阻塞

Proprietary Software License 将“通过增加或组合文件/数据，使用或生成不定数量模型”的派生应用列为 `Expandable Application`。BongoCat 支持用户持续导入自定义模型，工程上应保守地按该定义适用处理。

协议写明：

- 发布 Expandable Application 前，需要预先向 Live2D 申请并获得批准；
- 获批后还需要签署适用于 Expandable Application 的单独 Publication License Agreement；
- 一般用户、教育机构和年销售额低于 1000 万日元的小规模主体豁免不适用于 Expandable Application；
- Live2D 保留最终认定权。

因此在获得 Live2D 书面结论前：

- 可以继续不公开分发的本地技术验证和产品实现；
- 不得发布包含 Cubism Core/Framework 的 Native Rewrite 安装包；
- 不得将“开源”“免费”或当前收入规模当作自动豁免；
- Phase 0 退出结论最多为 `GO WITH CONDITIONS`，且发布授权必须是显式阻塞条件。

维护者联系 Live2D 时应一次性确认：

1. BongoCat 的任意用户模型导入是否属于 Expandable Application；
2. MIT 开源的 Rust 应用通过原始 C API 调用 Core 是否允许；
3. 独立实现 model3、motion、expression、physics 和 pose 时，哪些规范或 Framework 行为可以作为实现依据；
4. Windows DLL/import library 与 macOS static library 的发布、签名和更新方式；
5. 必须随源码、安装包和应用 UI 提供的 license、notice、终端用户保护条款和 attribution；
6. Publication License Agreement 的申请主体、费用、地域、分发渠道与版本更新条件。

## 4. Controlled Acquisition

合法取得 SDK 后执行以下流程：

1. 维护者在官方页面自行阅读并接受当时有效的两份协议，下载固定版本；当前文件为 `CubismSdkForNative-5-r.4.1.zip`。
2. ZIP 保存在仓库外的受控开发目录，不提交到 Git，不复制到 CI cache 或公开 artifact。
3. 运行 `python3 tools/inspect-cubism-sdk.py /absolute/path/CubismSdkForNative-5-r.4.1.zip --expected-sha256 b71ec6bafc6578cce3b4cbbaa42a1cb51ae6eb477557932b02d22af957e733c7`。
4. 首次验证记录 ZIP SHA-256、Core version、每个目标文件的路径/大小/hash，并由另一位维护者或独立机器复核。
5. 维护者只在仓库外的受控目录准备 `Core/include/Live2DCubismCore.h`，使用 inspector 报告中的 header SHA-256 运行 `tools/cubism-bindgen`；真实 bindings 与 provenance 仍留在仓库外。
6. 第二位维护者使用记录的 bindgen/libclang/target 配置独立生成并比较 output/config hash，再执行对应 Core 的 compile/link/ABI smoke；详见 `cubism-binding-generation.md`。
7. 当前 expected SHA-256 已写入本文；后续运行使用 `--expected-sha256` 拒绝漂移，第二人或第二机器复核仍待完成。
8. 只有在发布授权结论完成后，才设计私有 SDK 缓存、构建时复制和安装包 notice 生成；构建与 CI 默认保持离线。

检查器只读取 ZIP central directory 和所需文件并计算 hash，不解压文件、不接受许可、不联网。它拒绝路径穿越、绝对路径、反斜杠路径、重复路径、符号链接、加密 entry 和异常膨胀 archive，并按 `cubism-framework-behavior-sources.md` 校验关键 Framework 文件属于固定的 R5 Git tree。

## 5. Evidence Still Required

以下事项尚未完成，不能由版本/许可文档替代：

- 当前官方 ZIP SHA-256 已记录；第二来源复核仍待完成；
- 已建立合成输入的可重复 raw binding 契约；仍缺 R5 header 真实 hash、授权后的生成、双人审阅与 ABI smoke；
- Windows x64 和 macOS arm64/x64 Core 的真实加载、版本查询和 ABI smoke；
- 三个预置 moc 的 consistency、model 创建、drawable 读取和析构；
- motion、expression、physics、pose 的独立 Rust 实现依据及许可书面结论；
- Live2D 对 BongoCat Expandable Application 与公开发布方式的书面答复；
- 最终安装包中 Core、license、notice、签名和更新方式的审计。

## 6. Sources

- [Cubism SDK for Native download](https://www.live2d.com/en/sdk/download/native/)
- [Live2D Proprietary Software License Agreement](https://www.live2d.com/eula/live2d-proprietary-software-license-agreement_en.html)
- [Live2D Open Software License Agreement](https://www.live2d.com/eula/live2d-open-software-license-agreement_en.html)
- [Cubism SDK Release License](https://www.live2d.com/en/sdk/license/)
- [Cubism Native Framework 5-r.4.1 release](https://github.com/Live2D/CubismNativeFramework/releases/tag/5-r.4.1)
- [Cubism Native Samples 5-r.4.1 release](https://github.com/Live2D/CubismNativeSamples/releases/tag/5-r.4.1)
