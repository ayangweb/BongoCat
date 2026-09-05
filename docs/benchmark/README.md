# Benchmark Records

本目录保存可重复的性能测试方法和结果。每份记录至少包含：

- 构建 commit 和 release/debug 配置
- 操作系统、CPU、GPU、内存和显示器/DPI
- 模型、窗口尺寸、目标 FPS 和输入脚本
- 预热、样本数、测量工具和原始数据位置
- p50/p95/p99、误差来源和结论

Phase 0 已建立 macOS GPUI settings spike 的首个可重复基线，见
`data/gpui-settings-macos-248a770-startup.csv`、
`data/gpui-settings-macos-248a770-idle.csv` 和对应的 spike 记录。它不代表
完整产品、原生 overlay 或 Live2D runtime 的性能；未附环境和方法的数据
不得用于宣称性能提升。
