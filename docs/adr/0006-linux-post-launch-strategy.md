# ADR-0006: Linux Is a Post-Launch Evaluation

状态：Accepted
日期：2026-08-28

## Context

BongoCat Native Rewrite 的首发目标是 Windows 和 macOS。Linux 的透明窗口、窗口层级、全局输入、托盘和桌面集成能力会随 X11、Wayland、portal 与 compositor 改变，不能从 Windows/macOS backend 的结果推断等价支持。

同时，runtime、配置、模型、动画和行为 fixture 本身不需要依赖具体桌面系统。首发实现不应为了未验证的 Linux 能力统一平台 backend，也不应让共享业务类型依赖 Win32、AppKit 或 GPU handle。

## Decision

Linux 不进入首发支持矩阵，不阻塞 Windows/macOS 的 Phase 0、实现和发布。

共享 Rust crate 保持平台无关：业务接口使用项目自有类型，平台能力通过 adapter 和 capability reporting 提供。Windows 使用 D3D11，macOS 使用 Metal；首发不为了 Linux 预先引入统一窗口、输入或 GPU abstraction。

Linux 只在首发后按独立 spike 评估。评估必须分别覆盖 X11 与 Wayland，并允许结论是某些桌面环境、compositor 或全局输入能力不受支持。尤其不得承诺 Wayland 下存在与 Windows Raw Input 或 macOS CGEventTap 等价的全局键鼠捕获。

## Consequences

- 首发 CI 可以对平台无关 crate 执行 Linux `cargo check`，但该结果不代表 Linux 产品支持。
- 新增共享业务 API 时不得暴露 HWND、Objective-C 对象、X11 handle、Wayland object 或 backend 专用 GPU 类型。
- Linux backend 可以在首发后新增，不要求修改已冻结的输入、配置和模型语义。
- 若 Linux 平台能力不足，产品必须显式降级或限制支持范围，不能用静默丢失输入代替能力声明。

## Verification

Phase 1 CI 对共享 crate 执行 Linux compile check。首发后按照 TODO 的 Linux backlog 建立 X11/Wayland 能力矩阵，并在全局输入、透明 overlay、渲染和系统集成均达到门槛后，另行提交支持决策 ADR。
