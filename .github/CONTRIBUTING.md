# 贡献指南

Native Rewrite 使用 Rust 2024。请先阅读仓库根目录的 `AGENTS.md`、技术设计和实施 TODO，确认
改动所属阶段与验收门槛。

## 开发环境

- Rust `1.97.1`，包含 `clippy` 和 `rustfmt`。
- macOS 或 Windows 平台工作需要在对应系统完成 smoke 验证；Linux 只用于共享 crate 检查。

Native 产品 workspace 位于仓库根目录，不需要 Node.js、pnpm、Tauri 或 Web 前端工具链。

```text
just dev
just check
```

也可以在仓库根目录直接执行 Cargo 命令。Development 与 Production 存储根隔离，禁止在运行时
切换构建环境或读取历史 Tauri/Pinia 配置。

## 提交

提交信息遵循 Conventional Commits，例如 `feat: add model import validation`。提交前运行与改动范围
相称的格式化、Clippy、测试和平台 smoke；不要提交构建产物、用户数据或未验证的平台声明。

历史 Vue/Tauri 实现只在远端 `master` 与 `pre-refactor-tauri` 分支中保留，不能重新接入 Native
产品依赖图。
