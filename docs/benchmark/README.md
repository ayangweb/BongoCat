# Benchmark Records

本目录保存可重复的性能测试方法和结果。每份记录至少包含：

- 构建 commit 和 release/debug 配置
- 操作系统、CPU、GPU、内存和显示器/DPI
- 模型、窗口尺寸、目标 FPS 和输入脚本
- 预热、样本数、测量工具和原始数据位置
- p50/p95/p99、误差来源和结论

macOS Metal overlay 的第一个单机 draw-time diagnostic baseline 见
[`macos-overlay-frame-timing-90e0aa7.md`](macos-overlay-frame-timing-90e0aa7.md)
及其 raw CSV。它只验证当前 preview 的有界计时器，未覆盖发布性能矩阵。
