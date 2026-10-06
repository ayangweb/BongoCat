# ADR-0079：Release 编译采用 ThinLTO 与并行代码生成

状态：接受。日期：2026-10-06。

## 背景

用户报告 `bongocat-app(bin)` 构建需要二十余分钟。原 release profile 使用
FatLTO、一个 codegen unit，并关闭增量编译；二进制阶段包含整个依赖图的跨 crate
优化，不能将该阶段的等待全部归因于安装包组装。目前没有可比的 FatLTO 本机计时报告。

## 决策

release profile 改为 `lto = "thin"`、`codegen-units = 16`。
保留 `opt-level = 3`、`panic = "abort"`、符号裁剪以及非增量发行构建。
复用 Cargo/LLVM 的优化与并行编译能力，不新增依赖、linker 或第二条构建入口。
`just build` 继续通过 packaging crate 执行同一 release profile；开发和发行环境隔离不变。

[Cargo 官方文档](https://doc.rust-lang.org/cargo/reference/profiles.html)说明 ThinLTO
比 FatLTO 的执行时间明显更低，更多 codegen units 可以并行处理 crate。
实际构建耗时、二进制大小和运行性能仍由本项目测量，不承诺固定提速比例。
此次 profile 变化会使已有 release 编译缓存失效，首次构建包含依赖重编译。

## 验证与退出条件

在 Windows x64 上通过唯一打包入口 `just build` 验证完整编译与安装包组装，记录
墙钟耗时和产物大小；通过 packaging contract 与格式检查验证入口和平台集合未改变。
执行 Windows Development smoke 验证产品启动和退出。
macOS 构建、双平台运行性能/空闲 CPU、GPU 帧耗时和 soak 仍需对应平台实测，不能从
Windows 编译推断。此次构建测量不作为 FatLTO/ThinLTO 的同条件性能对比。

## 本次结果

- Windows `just build` 成功：profile 改变后的首次完整打包 267.858 秒，
  无源码变化的缓存打包 17.068 秒；详见
  [构建耗时记录](../benchmark/windows-release-build-thin-lto.md)。
- `cargo update --workspace` 未更新包版本；`cargo metadata --locked --no-deps` 通过。
- packaging contract 30 项通过；Windows `just dev-smoke` 通过（设置
  `CARGO_BUILD_TARGET=x86_64-pc-windows-msvc`，复用同 target 的 release 缓存）。
- `just check` 串行重跑通过。执行时设置 `__COMPAT_LAYER=RunAsInvoker`，
  解决 Windows 拒绝启动 update 测试 executable 的错误 740；无提权或产品代码修改。
  首轮遇到该执行错误，并行编译期间重试还出现一次 FPS 时序测试失败，
  编译结束后的串行重跑通过，不能将有竞争负载的测试视为性能基准。
- 纯构建配置调整不新增用户功能或产品行为声明，CHANGELOG 不新增条目。
- 下一项未完成验证为 macOS 构建和双平台运行性能对比，相关发布门禁不据此关闭。
