# 贡献指南

Native Rewrite 使用 Rust 2024。请先阅读仓库根目录的 `AGENTS.md`、技术设计和实施 TODO，确认
改动所属阶段与验收门槛。

## 开发环境

- Rust `1.97.1`，包含 `clippy` 和 `rustfmt`。
- macOS 或 Windows 平台工作需要在对应系统完成 smoke 验证；Linux 不是首发目标，也不作为 Native workspace 编译门槛。

Native 产品 workspace 位于仓库根目录，不需要 Node.js、pnpm、Tauri 或 Web 前端工具链。

```text
just dev
just check
just build
```

也可以在仓库根目录直接执行 Cargo 命令。Development 与 Production 存储根隔离，禁止在运行时
切换构建环境或读取历史 Tauri/Pinia 配置。

## 构建与打包

`just build` 是唯一的构建入口，它把参数转发给 `crates/bongocat-packaging`；该 crate 编译产品、
写 build provenance，并把 bundle 与安装器生成交给 `cargo-packager`。本地与 CI 使用同一个入口，
所以不存在"本地能构建、CI 不能构建"的两套逻辑。

不要重新引入 `scripts/` 或自维护的 `.nsi`，也不要把平台判断、文件复制、`Info.plist` 注入、
`.dmg` / NSIS 组装或版本号解析写回 `Justfile`、CI workflow 或文档。决策背景见
`docs/adr/0033-build-packaging-and-release-toolchain.md`，构建入口约定见 `AGENTS.md` §12.1.1。

除 Rust toolchain 外还需要 `just` 与 Python 3（`tools/record-native-provenance.py` 写 provenance）。

## 提交

提交信息遵循 Conventional Commits，例如 `feat: add model import validation`。提交前运行与改动范围
相称的格式化、Clippy、测试和平台 smoke；不要提交构建产物、用户数据或未验证的平台声明。

历史 Vue/Tauri 实现只在远端 `master` 与 `pre-refactor-tauri` 分支中保留，不能重新接入 Native
产品依赖图。
