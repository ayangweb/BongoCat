# ADR-0064: Human-readable bounded application logging

状态：已接受（2026-09-24）

## 背景

BongoCat 已由 `bongocat-log`、`bongocat-app::ApplicationLogHandle` 和
`bongocat-live2d::CoreLogHandle` 提供环境隔离、固定事件字段、文件权限、按 UTC 日/文件大小轮转、
目录级容量上限、Core callback 队列和 panic 脱敏。现有输出仍采用 application/Core JSONL；机器解析
较严格，但普通用户难以直接阅读，现有事件目录也只覆盖少量生命周期与导出失败路径，不能充分支持
模型、配置、输入、窗口、权限、更新和跨层交互排障。

本决策不重新引入第二套日志框架。现有 writer 已拥有项目需要的生命周期、隐私、容量和非阻塞 panic
边界；缺口是输出格式、可配置过滤、更多有界事件以及 application/Core 对同一文本格式与 retention
policy 的复用。

## 决策

### 1. 单一人类可读文本方案

application 与 Cubism Core 日志都使用 UTF-8 单行文本，不再写 JSONL。每条记录固定包含：

```text
<UTC timestamp> <LEVEL> [<module>] <stable-code> | <message> | <bounded context>
```

时间使用带 `Z` 的 UTC 时间戳；级别为 `ERROR`、`WARN`、`INFO`、`DEBUG` 或 `TRACE`；module、stable
code 和 message 分离，便于用户阅读和开发者检索。文件名统一使用 `.log`：

- `application-YYYY-MM-DD.log`
- `cubism-core-YYYY-MM-DD.log`

同一 application/Core writer 共享文本格式化、过滤、单文件大小、rotation、数量、目录总容量和 retention
实现。Core callback 仍只做有界复制与非阻塞入队，文件写入只在专用 worker 中执行。

### 2. 日志过滤与保留策略

当前 v1 配置新增：

```json
{
  "logging": {
    "level": "info",
    "retention_days": 7
  }
}
```

- `logging.level` 是写入阈值，接受 `error`、`warn`、`info`、`debug`、`trace`，默认 `info`。
- `logging.retention_days` 接受 `1..=30`，默认 `7`；配置降低后立即收敛旧文件。
- 日志首先按 UTC 日切换；单个活动文件超过 `1 MiB` 时继续切分编号文件，防止单个文件无界增长。
- 当前 `application` 与 `cubism-core` 日志目录合计最多 `8 MiB` 和 `32` 个文件；超限只删除最旧的
  已轮转文件，当前 UTC 日活动文件始终保留。
- 不提供 rotation mode 配置。日分文件是用户组织日志的稳定边界，大小上限是异常流量保护；只允许用户在
  “按天”和“按大小”之间二选一会制造无实际价值的状态组合。

### 3. 事件和级别

事件仍由 app-owned 闭合 event catalog 产生，不用自由文本作为公共日志协议。级别按以下语义分配：

- `error`：关键操作失败、panic、shutdown 失败、数据损坏或不可恢复的资源/解析错误。
- `warn`：程序继续但已降级、fallback、重试、回滚、权限不足、临时 GPU/输入服务异常。
- `info`：启动/退出、用户可见的模型/配置/窗口状态变化、更新阶段结果和恢复结果。
- `debug`：服务 start/stop、候选模型 prepare、输入 reset/reconcile 状态变化、窗口状态 flush。
- `trace`：有界、低频且不含输入内容或资源正文的开发诊断；不用于逐事件输入、逐帧渲染或下载进度。

启动、退出、配置读写/恢复、状态持久化、模型准备/切换/导入/删除、资源解析、窗口创建/显隐、
输入服务与权限、更新检查/下载/验签/安装、平台文件/网络错误和 Rust→GPUI command 失败都必须在
app/service 边界进入日志。底层返回稳定错误码后由最近的 app owner 记录一次，避免同一错误在低层、
service 和 UI 重复输出。

### 4. 隐私和性能边界

日志不得包含用户路径、真实按键序列、剪贴板、配置原文、模型正文、URL、密钥/凭据、签名材料或动态 OS 错误文本。
允许的上下文只包括闭合 stable code、匿名/可移植 model ID、origin、阶段、计数、字节数、revision、
能力/服务状态和构建版本；文本统一转义换行/控制字符并限制长度。panic hook 仍不读取 payload，
使用非阻塞记录。

默认 `info` 只写低频业务和错误边界。输入事件、frame tick、cursor/axis 样本、下载进度、轮询 snapshot
和可恢复的 temporary presentation unavailable 不逐次记录。轮转与清理失败只进入匿名 writer 统计，
不得阻止 overlay/runtime 继续运行。

### 5. Diagnostics preview

匿名 diagnostics JSON 保持不变。preview bundle 中的 application lifecycle 历史从新的 `.log` 文件
严格解析固定 code，并重新输出为脱敏的 `application-events.log`；每条固定为
`<LEVEL padded> [<module>] <code>`，不复制原始日志行、消息、上下文、路径或时间戳。输入文件或
其中任一记录无法通过共享文本语法、闭合 code/module/level 契约时，整份来源文件跳过并只增加匿名计数。
Cubism Core 原文仍不进入 preview，只保留匿名
written/dropped/rotated/pruned/bytes/file 统计。

## 影响

- 不新增第三方 logging 依赖，也不增加 GPUI/runtime/OS 类型到公共日志 API。
- application/Core 输出、文件发现、retention helper、diagnostics preview 和相关测试必须一起迁移；
  活跃代码不再保留 JSONL writer/解析分支。
- `.log` 是用户可支持格式，不承诺稳定的第三方日志解析 schema；需要机器处理时使用匿名
  `diagnostics.json` 或 preview bundle 中的严格重写记录。
- 日志设置是首版 v1 字段，遵循仓库规则直接更新 schema/default/fixture/实现，不增加迁移或旧字段
  fallback。
