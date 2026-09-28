# ADR-0005: Cubism Core FFI Boundary

状态：Accepted, pending SDK and license validation
日期：2026-08-28

## Context

BongoCat 需要加载既有 `.moc3` 模型。官方 Cubism Core 以平台二进制形式提供，应用必须控制其版本、所有权、分发和错误边界。

## Decision

通过窄 Rust sys layer 调用官方 Cubism Core，并在其上建立 safe Rust wrapper：

```text
Cubism Core binary -> raw bindings -> Moc/Model wrappers -> model evaluation
```

原始指针不得离开 wrapper。Rust owner 必须保证 Moc、Model 和 backing buffer 的存活与析构顺序。BongoCat 的动作、表达式、物理、资源和状态逻辑保留在 Rust 模块中。

## Consequences

- 构建清单必须记录 SDK/Core 版本、来源、hash、架构和许可证。
- FFI callback 不执行阻塞工作，不允许 panic 穿越边界。
- 模型切换采用 prepare/validate/commit，失败保留当前模型。
- 不通过扩大非 Rust bridge 来绕过兼容性问题。

## Verification

Phase 0 使用三个预置模型验证 moc/model 生命周期、drawable 数据、motion、expression、physics/pose 需求和重复销毁，并形成 SDK 分发结论。
