# BongoCat

[English](README.md) | [简体中文](README.zh-CN.md)

![BongoCat](https://socialify.git.ci/ayangweb/BongoCat/image?custom_description=&description=1&font=Source+Code+Pro&forks=1&issues=1&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)

<div align="center">
  <div>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img alt="Windows" src="https://img.shields.io/badge/-Windows-blue?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB0PSIxNzI2MzA1OTcxMDA2IiBjbGFzcz0iaWNvbiIgdmlld0JveD0iMCAwIDEwMjQgMTAyNCIgdmVyc2lvbj0iMS4xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHAtaWQ9IjE1NDgiIHdpZHRoPSIxMjgiIGhlaWdodD0iMTI4Ij48cGF0aCBkPSJNNTI3LjI3NTU1MTYxIDk2Ljk3MTAzMDEzdjM3My45OTIxMDY2N2g0OTQuNTEzNjE5NzVWMTUuMDI2NzU3NTN6TTUyNy4yNzU1NTE2MSA5MjguMzIzNTA4MTVsNDk0LjUxMzYxOTc1IDgwLjUyMDI4MDQ5di00NTUuNjc3NDcxNjFoLTQ5NC41MTM2MTk3NXpNNC42NzA0NTEzNiA0NzAuODMzNjgyOTdINDIyLjY3Njg1OTI1VjExMC41NjM2ODE5N2wtNDE4LjAwNjQwNzg5IDY5LjI1Nzc5NzUzek00LjY3MDQ1MTM2IDg0Ni43Njc1OTcwM0w0MjIuNjc2ODU5MjUgOTE0Ljg2MDMxMDEzVjU1My4xNjYzMTcwM0g0LjY3MDQ1MTM2eiIgcC1pZD0iMTU0OSIgZmlsbD0iI2ZmZmZmZiI+PC9wYXRoPjwvc3ZnPg==" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img alt="macOS" src="https://img.shields.io/badge/-macOS-black?style=flat-square&logo=apple&logoColor=white" /></a>
  </div>

  <p>
    <a href="./LICENSE"><img src="https://img.shields.io/github/license/ayangweb/BongoCat?style=flat-square" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases/latest"><img src="https://img.shields.io/github/v/release/ayangweb/BongoCat?label=release&sort=semver&style=flat-square" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img src="https://img.shields.io/github/downloads/ayangweb/BongoCat/total?style=flat-square" /></a>
  </p>

  <p>
    <a href="https://trendshift.io/developers/8507" target="_blank"><img src="https://trendshift.io/api/badge/developers/8507" alt="ayangweb | Trendshift" width="250" height="55" /></a>
    <a href="https://trendshift.io/repositories/14605" target="_blank"><img src="https://trendshift.io/api/badge/repositories/14605" alt="ayangweb%2FBongoCat | Trendshift" width="250" height="55" /></a>
    <a href="https://hellogithub.com/repository/7d23863fd4be47b39e816193ded385c9" target="_blank">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=dark" />
        <source media="(prefers-color-scheme: light)" srcset="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=neutral" />
        <img alt="HelloGitHub" src="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=neutral" width="250" height="55" />
      </picture>
    </a>
  </p>
</div>

<!-- TODO(release)：把下面两个占位替换为 BongoCat 截图。 -->

| macOS                                   | Windows                                   |
| --------------------------------------- | ----------------------------------------- |
| ![macOS](REPLACE-WITH-MACOS-SCREENSHOT) | ![Windows](REPLACE-WITH-WINDOWS-SCREENSHOT) |

BongoCat 是一款面向 Windows 和 macOS 的桌面陪伴应用。一只 Live2D 小猫住在你的屏幕上，眼神和爪子会
跟着鼠标移动，你按下的每个按键、鼠标键和手柄按键它都会有反应。你可以换上自己的模型，把窗口拖到任何
位置，需要的时候它就安静地待在一边。

灵感来自 [MMmmmoko](https://github.com/MMmmmoko) 的
[Bongo-Cat-Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver)。

## 功能介绍

- **自带你的模型。** 导入 Live2D 模型文件夹，改名、换封面，在模型库中自由切换。内置标准、键盘和
  手柄三个模型。
- **模型窗口随你摆布。** 拖动到任意位置，右键拖动缩放，缩放 25–400%、不透明度 1–100%，还可调整圆角
  和最大帧率。可以置顶、让点击穿透、保持在屏幕内，或在鼠标停留时隐藏。
- **修正朝向不对的模型。** 水平翻转模型，并可水平或垂直翻转鼠标跟随。
- **模型行为。** 打开动作音效，按间隔随机播放动作或表情，并让每个模型回到你上次在它上面使用的
  表情。
- **按输入类型分别控制。** 分别忽略鼠标、键盘或手柄输入，调整手柄摇杆和扳机死区，并在手柄连接
  或断开时自动切换模型。
- **全局快捷键。** 显示或隐藏模型窗口、打开设置、切换各类输入的忽略状态，以及播放指定的动作或
  表情。窗口快捷键和模型快捷键各有独立开关。
- **应用内更新。** 手动检查或按 1–8760 小时的间隔自动检查，在同一个窗口里完成下载、校验、安装和
  重启。
- **跟随系统外观。** 浅色、深色和跟随系统三种主题；跟随系统、简体中文和英语三种语言。
- **图标放在你顺手的地方。** macOS 菜单栏图标、Windows 系统托盘图标，程序坞和任务栏图标各有独立
  开关，还可以开机自动启动。
- **按日期分文件、有限保留的日志。** 自定义日志级别和保留天数，旧文件自动清理。
- **开源且不打扰你。** 无账号、无遥测、不收集任何数据。所有功能都可离线使用，唯一的联网行为是
  更新检查，默认关闭。

## 下载

最新版本在 [GitHub Releases](https://github.com/ayangweb/BongoCat/releases/latest)。

| 你的系统                       | 要求                   | 下载文件                      |
| ------------------------------ | ---------------------- | ----------------------------- |
| Windows                        | Windows 10 1903 及以上 | `BongoCat_<版本号>_x64.exe`   |
| macOS（Apple 芯片，M1 及以后） | macOS 12 及以上        | `BongoCat-<版本号>-arm64.dmg` |
| macOS（Intel 芯片）            | macOS 12 及以上        | `BongoCat-<版本号>-x64.dmg`   |

Windows on ARM 通过仿真运行 x64 版本。不提供 Linux 构建。

macOS 首次启动时会申请「输入监控」权限，猫咪需要它才能看到你的键盘和鼠标。如果列表里已经有
BongoCat，请先用 `−` 移除再用 `+` 重新添加，然后重启 BongoCat。在 Windows 上，如果同时有以管理员
权限运行的程序，BongoCat 会建议你也以管理员权限运行，以便继续收到输入。

## 模型转换

Bongo-Cat-Mver 的模型文件夹可以在导入时直接转换。勾选要转换的模式——标准、键盘或手柄——
BongoCat 会完成转换、用模型自身生成封面并加入模型库。

## 更多模型

你可以在 [Awesome-BongoCat](https://github.com/ayangweb/Awesome-BongoCat) 里浏览、下载更多猫咪
模型，或分享自己的创作。

## 社区交流

<table>
  <thead>
    <tr>
      <th>QQ 群 1</th>
      <th>QQ 群 2</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>
        <a href="https://qm.qq.com/q/AS3gNv2Vzy">
          <picture>
            <source
              media="(prefers-color-scheme: dark)"
              srcset="https://i0.hdslb.com/bfs/openplatform/8ecdc4982ab01b59d7731fcca3ec26631a274560.png"
            />
            <source
              media="(prefers-color-scheme: light)"
              srcset="https://i0.hdslb.com/bfs/openplatform/09f56580397063e1819c4c2ed63d07dee12720e1.png"
            />
            <img
              alt="QQ 群 1"
              src="https://i0.hdslb.com/bfs/openplatform/09f56580397063e1819c4c2ed63d07dee12720e1.png"
              height="250"
            />
          </picture>
        </a>
      </td>
      <td>
        <a href="https://qm.qq.com/q/TmltLAod2O">
          <picture>
            <source
              media="(prefers-color-scheme: dark)"
              srcset="https://i0.hdslb.com/bfs/openplatform/473c522487ff33e0f32b15466aeb0734f17161c8.png"
            />
            <source
              media="(prefers-color-scheme: light)"
              srcset="https://i0.hdslb.com/bfs/openplatform/d5ae8c5af6ae1d0a1f066705ee822d1287384cf6.png"
            />
            <img
              alt="QQ 群 2"
              src="https://i0.hdslb.com/bfs/openplatform/d5ae8c5af6ae1d0a1f066705ee822d1287384cf6.png"
              height="250"
            />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>

## 赞赏

每一份认可都值得被珍视！赞赏随缘，心意无价，谢谢你的支持 ❤️

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://i0.hdslb.com/bfs/openplatform/e7438bff14cdfb6bfd0feacbb482f99ea4093294.png" />
  <source media="(prefers-color-scheme: light)" srcset="https://i0.hdslb.com/bfs/openplatform/da55cc3ec1556580c91e59f589792866c998c7c6.png" />
  <img alt="微信赞赏码" src="https://i0.hdslb.com/bfs/openplatform/da55cc3ec1556580c91e59f589792866c998c7c6.png" height="250" />
</picture>

## 贡献指南

感谢大家为 BongoCat 做出的贡献！如果也希望为 BongoCat 添砖加瓦，请查阅
[贡献指南](CONTRIBUTING.zh-CN.md)。

<a href="https://openomy.com/ayangweb/BongoCat" target="_blank" style="display: block; width: 100%;" align="center">
  <img src="https://openomy.com/svg?repo=ayangweb/BongoCat&chart=bubble" alt="贡献者排行榜" style="display: block; width: 100%;" />
</a>

## 从源码构建

请安装 `rustup`（仓库会固定所需工具链版本）、`just` 和 Python 3，然后在仓库根目录运行：

```text
just dev
just check
just build
```

`just build` 会生成发布包：macOS 上生成 `.app` 和 `.dmg`，Windows 上生成 x64 安装程序。
运行 `just version` 可查看产品版本号。

## 文档

- [贡献指南](CONTRIBUTING.zh-CN.md)
- [更新日志](CHANGELOG.zh-CN.md)
- [许可证](LICENSE)
