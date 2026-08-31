# ADR-0013: Startup Item Capability and Environment Isolation

状态：Accepted
日期：2026-08-31

## Context

Native Rewrite 支持 macOS 12+，但 `SMAppService.mainAppService` 只在 macOS 13+
可用。Development 与 Production 又固定使用相同 Bundle ID；若开发构建直接注册 main
app login item，会覆盖同一用户的生产启动项系统状态。Windows 的当前用户启动项没有同样
的 Bundle ID 限制，但仍必须按构建环境隔离名称与目标命令。

启动项是可选平台能力，失败不能阻止 runtime、overlay 或设置窗口启动。UI 需要区分未启用、
已启用、路径过期、等待系统批准和当前环境不支持，而不能把所有情况压缩成一个布尔值。

## Decision

- 共享平台 API 使用稳定的 `StartupItemEnvironment`、`StartupItemState` 和
  `StartupItemError`，不暴露 registry handle、Objective-C object 或第三方错误类型。
- Windows 使用当前用户 `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run`，
  Development 与 Production 使用不同 value name。启用时写入当前 executable 的带引号绝对
  路径和 `--run-seconds 0`；状态检测必须区分完全匹配与 stale value；禁用只删除当前环境的
  value，不读取或修改另一环境。
- macOS 13+ Production `.app` 使用 `SMAppService.mainAppService`。状态映射保留
  not-registered、enabled、requires-approval 和 not-found；`not-found` 是可显式尝试注册的
  预注册状态，不在 adapter 或 UI 提前判为无效。注册/取消注册错误只向上返回稳定匿名 code。
- macOS 12 返回明确的 unsupported capability，应用其余功能继续运行。不得回退到已废弃的
  `SMLoginItemSetEnabled`、自行写 LaunchAgent 或修改用户登录项数据库。
- macOS Development 构建只报告 unsupported build environment，不注册或取消注册
  `mainAppService`，从而不改变 Production 的系统登录项状态。真实变更 smoke 使用临时签名的
  Production `.app` 和隔离测试用户/CI runner，并从 `/Applications` 下的临时唯一安装目录
  启动，使 LaunchServices 与 `SMAppService` 验证的是安装态 main application。
- 启用、禁用只能由显式用户 command 触发；正常启动只读取状态。平台失败不改变配置，也不
  阻塞 input/runtime/renderer。

## Consequences

- macOS 12 用户可使用产品核心功能，但设置页会明确显示登录启动不可用。
- 开发构建不能直接演练 macOS 注册变更；平台 contract test 与 Production bundle smoke 负责
  覆盖，公开发布前仍需签名构建和 System Settings 批准路径实测。
- Windows 安装位置变化会显示 stale，而不是误报 enabled；用户再次启用可原子覆盖当前环境
  value。
- 启动项状态属于平台 snapshot，不进入配置 schema，也不由 runtime 动画状态持有。

## Verification

- 纯 contract test 覆盖所有 state/error code、环境命名和 Windows command 编码。
- Windows 临时当前用户 value smoke 覆盖 disabled -> enabled -> stale -> disabled，并保证只
  触及当前环境 value。
- macOS 12 availability probe 不引用缺失 class；macOS 13+ Production bundle smoke 从
  `/Applications` 的临时安装目录覆盖 status、register/unregister 和 requires-approval 映射，
  结束时恢复原状态并注销/删除临时 bundle。首次 `not-found` 注册后取消注册允许规范化为
  `not-registered`，因为两者都不留下启用的登录项。
- 双平台 smoke 后运行完整 Native format、Clippy、workspace test 和 release check。
