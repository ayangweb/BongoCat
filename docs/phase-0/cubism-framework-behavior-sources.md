# Cubism Framework Behavior Sources

状态：R5 `5-r.5` 行为来源已固定；Rust 实现许可仍阻塞发布
记录日期：2026-08-30

> 本文是工程溯源与测试设计，不构成法律意见，也不授权复制、翻译或重新许可 Live2D 源码。

## 1. Pinned Source Tree

当前本地验证使用 Cubism Native Framework R5 tag `5-r.5`，commit
`145155d2c5bdd8d23475cef9cc3ab46d3220190c`。Samples tag `5-r.5` 为 commit
`b8024738f108e6003e4925193e8d5ec04cd18196`。

`tools/inspect-cubism-sdk.py` 会在维护者合法取得的 SDK ZIP 中校验以下关键文件的 Git blob SHA，并同时输出每个文件的 SHA-256。Git blob SHA 用于证明文件属于固定 tree；最终 SDK archive SHA-256 仍是发布供应链门禁。

| 行为域                   | R5 文件                                                    | Git blob SHA                               |
| ------------------------ | ---------------------------------------------------------- | ------------------------------------------ |
| model3 resource setting  | `Framework/src/CubismModelSettingJson.cpp`                 | `8b9fa84d5d74a0882b2d5f20322862606207c6a6` |
| breath                   | `Framework/src/Effect/CubismBreath.cpp`                    | `9312b1f96b25380670856f9cecc3dee33ea9ad02` |
| eye blink                | `Framework/src/Effect/CubismEyeBlink.cpp`                  | `7b67806753b76cac1fd053ed899ff761aa0156b4` |
| pose                     | `Framework/src/Effect/CubismPose.cpp`                      | `fcb88823d17466359f87c7e2a88e309fc54b19c4` |
| expression               | `Framework/src/Motion/CubismExpressionMotion.cpp`          | `5f79270126c487c38075a853d0081c974b081060` |
| motion evaluation        | `Framework/src/Motion/CubismMotion.cpp`                    | `702f85a1a4057dc695eba47088f9338409937bce` |
| motion3 parsing          | `Framework/src/Motion/CubismMotionJson.cpp`                | `6cd35be1923a26014c5bd155ecad9eccbc9cd1e2` |
| physics evaluation       | `Framework/src/Physics/CubismPhysics.cpp`                  | `5cb44241c1f3faeb6dcac7463c0c18eab9dac431` |
| physics3 parsing         | `Framework/src/Physics/CubismPhysicsJson.cpp`              | `8cfdc05564e24ece369035fdf90fb546b94d90c6` |
| renderer common contract | `Framework/src/Rendering/CubismRenderer.cpp`               | `ce008f9148b1fd591d077ab90a963da9431ac08c` |
| D3D11 renderer           | `Framework/src/Rendering/D3D11/CubismRenderer_D3D11.cpp`   | `917b46ba352f4e80566c07369baba3d703ec54fb` |
| D3D11 effect shader      | `Framework/src/Rendering/D3D11/Shaders/CubismEffect.fx`    | `bbaca13cbbcfb9b184e6e8a5e63f40e99619f217` |
| Metal renderer           | `Framework/src/Rendering/Metal/CubismRenderer_Metal.mm`    | `d46eddfb748669a55e6c47ea22bfba5774ec4504` |
| Metal shader set         | `Framework/src/Rendering/Metal/Shaders/MetalShaders.metal` | `696adec0e2e38e1fa83d499f1369c388bab1576a` |

平台 Samples 只用于观察官方装配顺序，不作为 BongoCat 架构模板：

| 平台  | Sample source                                           | Git blob SHA                               |
| ----- | ------------------------------------------------------- | ------------------------------------------ |
| D3D11 | `Samples/D3D11/Demo/proj.d3d11.cmake/src/LAppModel.cpp` | `7e9a02c5e650a84e64e50db1569fc576a9906a81` |
| Metal | `Samples/Metal/Demo/proj.ios.cmake/src/LAppModel.mm`    | `70773176cd224fc8ed3db8d62c0c5ffaba365dcf` |

## 2. Behavior Contracts to Reproduce

| Domain         | Required Rust behavior                                                                                                                                                  | Phase 0 evidence                                                                                                           |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| model3         | Validate referenced moc, textures, motion groups, expressions, physics, pose, cdi, layout and hit areas before commit                                                   | Existing static fixture plus three real presets; malformed/escaping references remain rejected before Core                 |
| motion         | Parse curve targets and segments, apply model/parameter/part curves, fade in/out, loop, events, eye blink/lip sync effects and completion at deterministic time         | Golden parameter snapshots at fixed ticks, boundary timestamps and malformed curve cases                                   |
| priority/queue | Preserve reserve/start/force semantics, callback completion and stop behavior without wall-clock dependence                                                             | Deterministic concurrent motion fixture with explicit sequence and operation IDs                                           |
| expression     | Preserve add, multiply and overwrite semantics, fade weight, replacement and overlap behavior                                                                           | Multiple expressions applied in different orders with normalized parameter snapshots                                       |
| update order   | Make ordering explicit and stable across eye blink, expression, look, breath, physics, lip sync and pose                                                                | One fixture where reordering produces a different result, checked against an approved R5 oracle                            |
| physics        | Parse physics3 inputs/outputs/vertices, normalize parameter ranges, use deterministic delta time, stabilize and interpolate consistently                                | Authorized physics3 model sampled across fixed delta sequences and large-frame recovery                                    |
| pose           | Parse groups/links, initialize parts, fade visible parts and copy linked opacity                                                                                        | Authorized pose3 fixture with group switch and exact time checkpoints                                                      |
| renderer       | Consume Core drawable/offscreen order, texture, opacity, culling, packed color/alpha blend, multiply/screen color and mask/inverted-mask data using premultiplied alpha | D3D11 and Metal capture plus normalized draw/offscreen command trace; platform pixels may differ within declared tolerance |
| lifecycle      | Keep moc bytes alive through Model, release Model before Moc, and release GPU resources after the last snapshot                                                         | Repeated load/switch/destroy and failed prepare/validate/commit tests                                                      |

BongoCat must freeze the product-visible update order as a typed Rust contract and test it. It must not let container iteration order, callback arrival time or platform renderer timing decide parameter results.

The three shipped models do not reference physics3 or pose3. They can validate motion, expression and renderer paths, but cannot satisfy physics/pose acceptance. A separately authorized fixture is required; user models found on a developer machine are not copied into the repository or CI.

## 3. License Boundary

Native Framework and Samples use the Live2D Open Software License, not the repository MIT license. Core and its header use the Live2D Proprietary Software License. `Core/RedistributableFiles.txt` lists runtime libraries but does not list the Core header, so publishing generated Rust bindings derived from that header also requires an explicit Live2D answer.

Until that answer exists:

- source paths, Git object identities, public release metadata and black-box output observations may be recorded as provenance;
- BongoCat may create product fixtures from its own behavior and distributable models;
- Framework algorithms, shaders, constants or comments must not be copied, translated or mechanically ported into MIT Rust files;
- generated bindings and SDK-derived source are not committed or distributed;
- an AI must not produce Rust by line-by-line translation of the pinned C++/shader files;
- local evaluation does not imply permission to publish an installation package or derived source.

The written Live2D request defined in `cubism-sdk-source-and-license.md` must decide one of these implementation paths:

1. an independently implemented Rust layer based on sources/specifications Live2D confirms may be used for that purpose;
2. clearly isolated Rust files distributed under Live2D-approved terms, if Live2D permits this arrangement and it remains compatible with the repository and release model;
3. `NO-GO` for the pure Rust rewrite if neither path is permitted.

A long-lived C++ Framework bridge is not an implicit fallback. Any architecture change requires a new ADR and explicit user decision.

## 4. Oracle and Review Rules

- The legacy Core baseline is only an old-version compatibility oracle; Native R5 is the release candidate oracle.
- Golden outputs contain normalized project data, counts and numeric state, never copied SDK source or user model contents.
- Floating-point comparisons declare absolute/relative tolerances per domain; pixel screenshots do not replace command/state traces.
- Every behavior implementation review links to its project fixture and approved specification basis, not just a Framework line number.
- Any R5 upgrade reruns source-tree identity, SDK ZIP hash, Core ABI, three preset models, authorized physics/pose fixtures and both renderers before changing the pin.

## 5. Remaining Gate

This source inventory is complete, but implementation authorization is not. The following remain P0 blockers:

- Live2D written clarification for Rust binding publication and Framework-derived behavior;
- second-source review of the supplied R5 ZIP hash and source/binary inspection report;
- an authorized physics3 + pose3 fixture;
- black-box R5 parameter traces for motion, expression, physics and pose;
- D3D11 and Metal command/render evidence.
