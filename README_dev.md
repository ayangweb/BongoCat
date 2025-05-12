## BongoCat 开发者指南

本文档为 BongoCat 项目的开发者提供指导，包括项目架构、开发环境设置和启动方式。

## 灵感来源

本项目灵感来源于 [MMmmmoko](https://github.com/MMmmmoko) 大佬的 [Bongo Cat Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver)。由于原项目仅支持 Windows，作为一名深度 macOS 用户，我希望在自己的设备上也能使用这款可爱的 Bongo Cat，因此决定开发一个适配 macOS 的版本。

此外，得益于 Tauri 框架强大的跨平台能力，本项目不仅支持 macOS，还可在 Windows 和 Linux 上运行，让更多用户都能与这只可爱的猫咪互动！

![demo.gif](static/demo.gif)

## 项目架构

BongoCat 是一个基于 [Tauri](https://tauri.app/) 的桌面应用程序，前端使用 [Vue 3](https://vuejs.org/) 和 [Vite](https://vitejs.dev/) 构建。

主要目录结构如下：

- **`src/`**: 包含所有前端 Vue.js 代码。
  - `main.ts`: Vue 应用的入口文件。
  - `App.vue`: 根 Vue 组件。
  - `assets/`: 存放如 CSS、图片等静态资源。
  - `components/`: 可复用的 Vue 组件。
  - `composables/`: Vue 组合式 API 函数，用于封装和复用有状态逻辑。
  - `pages/`: 页面级 Vue 组件。
  - `plugins/`: Vue 插件或与 Tauri 交互的特定插件逻辑。
  - `router/`: Vue Router 的路由配置。
  - `stores/`: Pinia 状态管理模块。
  - `utils/`: 通用工具函数。
- **`src-tauri/`**: 包含所有后端 Rust 和 Tauri 相关的代码。
  - `Cargo.toml`: Rust 项目的配置文件，用于管理依赖。
  - `tauri.conf.json`: Tauri 应用的核心配置文件，定义窗口、插件、构建选项等。
  - `src/main.rs` (或 `src/lib.rs`): Rust 应用的入口，Tauri 应用的构建器和命令定义。
  - `icons/`: 应用图标。
  - `plugins/`: (如果存在) 自定义的 Tauri 插件。
- **`public/`**: 存放不会被 Vite 处理的静态资源，例如 Live2D 模型和核心 JS 文件。
- **`scripts/`**: 包含一些辅助构建或开发的脚本。
- **根目录文件**:
  - `package.json`: Node.js 项目配置文件，管理前端依赖和脚本。
  - `vite.config.ts`: Vite 构建工具的配置文件。
  - `uno.config.ts`: UnoCSS 配置文件。
  - `tsconfig.json`: TypeScript 配置文件。

# 贡献指南

非常感谢您对 BongoCat 的关注和贡献！在您提交贡献之前，请先花一些时间阅读以下指南，以确保您的贡献能够顺利进行。

## 透明的开发

所有工作都在 GitHub 上公开进行。无论是核心团队成员还是外部贡献者的 Pull Request，都需要经过相同的 review 流程。

## 提交 Issue

我们使用 `https://github.com/ayangweb/BongoCat/issues` 进行 Bug 报告和新 Feature 建议。在提交 Issue 之前，请确保已经搜索过类似的问题，因为它们可能已经得到解答或正在被修复。对于 Bug 报告，请包含可用于重现问题的完整步骤。对于新 Feature 建议，请指出你想要的更改以及期望的行为。

## 提交 Pull Request

### 共建流程

- 认领 issue：在 Github 建立 Issue 并认领（或直接认领已有 Issue），告知大家自己正在修复，避免重复工作。
- 项目开发：在完成准备工作后，进行 Bug 修复或功能开发。
- 提交 PR。

### 准备工作

- `https://v2.tauri.app/start/prerequisites/` : 请自行根据官网步骤安装 rust 环境。
- `https://nodejs.org/en/` : 用于运行项目。
- `https://pnpm.io/` ：本项目使用 Pnpm 进行包管理。

### 下载依赖

```shell
pnpm install
```

### 启动项目

```shell
pnpm tauri dev
```

### 打包项目

```shell
pnpm tauri build
```

## Commit 指南

Commit messages 请遵循 https://www.conventionalcommits.org/en/v1.0.0/ 。

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
