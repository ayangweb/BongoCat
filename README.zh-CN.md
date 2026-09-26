# BongoCat

[English](README.md) | [简体中文](README.zh-CN.md)

BongoCat 是一款面向 Windows 和 macOS 的桌面陪伴应用。它会响应键盘、鼠标和手柄输入，让 Live2D
模型动起来，并提供可移动、可置顶的模型窗口。

## 当前状态

BongoCat 正在开发中，计划支持：

- Windows 10 1903+（x64）
- macOS 12+（Intel 和 Apple 芯片）

目前尚未发布稳定版本。

## 从源码构建

请安装 `rustup`（仓库会固定所需工具链版本）、`just` 和 Python 3，然后在仓库根目录运行：

```text
just dev
just check
just build
```

`just build` 会创建发布包：macOS 上生成 `.app` 和 `.dmg`，Windows 上生成 x64 安装程序。
运行 `just version` 可查看产品版本号。

## 文档

- [贡献指南](CONTRIBUTING.zh-CN.md)
- [更新日志](CHANGELOG.zh-CN.md)
- [许可证](LICENSE)

## 历史版本

重构前的实现仅保留在受保护的
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) 分支中，供历史参考。
