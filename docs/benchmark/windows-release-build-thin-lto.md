# Windows release 构建耗时：ThinLTO

日期：2026-10-07。基于 `e43d4d6ccd91c2510bc0486624e2f4f676aa81aa` 的未提交工作树，
包括既有联机/WebRTC 改动与 ADR-0079 profile 调整；不是该 commit 的干净源码构建。

## 环境与方法

- Windows 11 IoT 企业版 LTSC，10.0.28000，x86_64。
- AMD Ryzen 7 5700X3D，8 核 / 16 逻辑处理器，约 16 GiB 内存。
- rustc 1.97.1，`x86_64-pc-windows-msvc`。
- `opt-level = 3`、`lto = "thin"`、`codegen-units = 16`、`incremental = false`。
- 连续两次 `just build`，均为 Production NSIS 打包；用 PowerShell Stopwatch 测量墙钟。
- 首次因 profile 改变触发 release 依赖重编译，保留现有下载缓存与构建工具缓存，未执行 clean。
- 第二次不改变源码，测量缓存命中后的完整打包。每种条件仅一个样本，不能计算可靠分位数。
- 该测量不涉及 GPU、模型窗口或输入脚本；运行性能未采样。

## 结果

| 条件 | 完整打包耗时 | Cargo release 编译报告 |
| --- | ---: | ---: |
| profile 变化后的首次构建 | 267.858 秒 | 4 分 11 秒 |
| 源码未变的缓存构建 | 17.068 秒 | 1.06 秒 |

原始测量值见 [CSV](data/windows-release-build-thin-lto.csv)。两次构建均成功。
产物 executable 为 34,777,600 bytes；NSIS 安装器为 12,362,547 bytes。

用户报告旧配置耗时二十余分钟，但没有同条件原始数据；不能据此计算提速倍数。
这些样本也不代表每次源码变化后的构建耗时，缓存构建主要测量安装包组装。
系统背景负载、缓存、磁盘和热状态均会影响结果。

macOS 构建与双平台运行性能、空闲 CPU、帧耗时、安装升级卸载和 soak 未覆盖。

验证：30 项 packaging contract、Windows Development `just dev-smoke` 与串行
`just check` 通过。检查使用 `__COMPAT_LAYER=RunAsInvoker` 处理测试 executable
启动时的 Windows 错误 740；并行编译时曾出现一次 FPS 时序失败，串行重跑通过。
完整检查输出保存在本机忽略目录 `target/packaging-speed-check.log`，不作为可移植基准数据。
