# 贡献指南

非常感谢您对 BongoCat 的关注和贡献！在您提交贡献之前，请先花一些时间阅读以下指南，以确保您的贡献能够顺利进行。

## 透明的开发

所有工作都在 GitHub 上公开进行。无论是核心团队成员还是外部贡献者的 Pull Request，都需要经过相同的 review 流程。

## 提交 Issue

我们使用 [Github Issues](https://github.com/ayangweb/BongoCat/issues) 进行 Bug 报告和新 Feature 建议。在提交 Issue 之前，请确保已经搜索过类似的问题，因为它们可能已经得到解答或正在被修复。对于 Bug 报告，请包含可用于重现问题的完整步骤。对于新 Feature 建议，请指出你想要的更改以及期望的行为。

## 提交 Pull Request

### 共建流程

- 认领 issue：在 Github 建立 Issue 并认领（或直接认领已有 Issue），告知大家自己正在修复，避免重复工作。
- 项目开发：在完成准备工作后，进行 Bug 修复或功能开发。
- 提交 PR。

### 准备工作

- [Rust](https://v2.tauri.app/start/prerequisites/): 请自行根据官网步骤安装 rust 环境。
- [Node.js](https://nodejs.org/en/): 用于运行项目。
- [Pnpm](https://pnpm.io/)：本项目使用 Pnpm 进行包管理。

### 下载依赖

```shell
pnpm install
```

### 启动应用

```shell
pnpm tauri dev
```

### 打包应用

> 如果需要打包后进行调试，请在以下命令后面加上 `--debug`

```shell
pnpm tauri build
```

## Commit 指南

Commit messages 请遵循[conventional-changelog 标准](https://www.conventionalcommits.org/en/v1.0.0/)。

### Commit 类型

以下是 commit 类型列表:

- feat: 新特性或功能
- fix: 缺陷修复
- docs: 文档更新
- style: 代码风格更新
- refactor: 代码重构，不引入新功能和缺陷修复
- perf: 性能优化
- chore: 其他提交

期待您的参与，让我们一起使 BongoCat 变得更好！
