# BongoCat 贡献指南

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

感谢你帮助改进 BongoCat。请确保贡献范围集中、表达清楚，并有可重复的验证结果。

## 开始之前

- 提交新议题或拉取请求前，请先搜索已有议题和拉取请求。
- 如果要大幅改变产品行为，请先说明用户需求和预期结果，再开始实现。
- 不要在同一次改动中夹带无关的清理或格式化。
- 修改面向用户或贡献者的文档时，请在同一次改动中同步更新英文版和简体中文版。
- 公开文档应聚焦产品行为和使用流程。除非完成任务必须依赖，否则不要在其中展示内部项目代号、
  迁移过程、实现技术栈的强调信息或源码文件名。

## 开发环境

BongoCat 在 Windows 和 macOS 上开发。Linux 不是发布目标，也不是日常验证所必需的环境。

请安装 `rustup`（仓库会固定所需工具链版本）、`just` 和 Python 3，并在仓库根目录运行命令：

```text
just dev
just check
just build
just schema
```

`just dev` 和 `just dev-smoke` 也会准备官方插件：优先使用
`BONGOCAT_PLUGINS_ROOT` 指向的本地插件仓库，否则首次运行时把
`https://github.com/ayangweb/BongoCat-Plugins` 克隆到 `target/plugin-dev`，检出
`Cargo.toml` 中 SDK 所固定的 revision，只构建当前主机平台，并把生成的本地目录交给应用。
自动管理的克隆必须保持干净。要在已有插件仓库上开发（包括未提交改动），可显式指定路径：

```text
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just dev
```

使用 `just plugin-tests` 会在同一插件仓库上运行完整工作区测试，并启用需要真实插件
进程的用例；不设置插件仓库时，这些用例会显式跳过。需要验证插件仓库中尚未提交的
改动时，把 `BONGOCAT_PLUGINS_ROOT` 指向同一目录。

插件构建沿用同一套 Rust 工具链和 `just`，编排脚本还需要 Python 3 与 Git。该开发流程
不发布插件、不使用插件签名私钥，也不依赖主应用 Release。

### 插件仓库与联合调试

官方插件和插件 SDK 由独立的
[`BongoCat-Plugins`](https://github.com/ayangweb/BongoCat-Plugins) 仓库维护。只修改插件实现、
插件界面或插件业务逻辑时，请在插件仓库创建分支和拉取请求，不要把插件源码复制回本仓库，也
不需要提升本仓库的 SDK revision。

有上游写权限的维护者可以直接克隆 `ayangweb/BongoCat-Plugins`。其他开发者需要先在 GitHub
上 fork `BongoCat-Plugins`，然后克隆自己的 fork，并把上游添加为 remote：

```text
git clone https://github.com/<your-name>/BongoCat-Plugins.git
cd BongoCat-Plugins
git remote add upstream https://github.com/ayangweb/BongoCat-Plugins.git
git fetch upstream
git switch -c fix/plugin-topic upstream/master
```

修改并验证后，先推送到自己的 fork，再向上游仓库提交拉取请求：

```text
git push -u origin fix/plugin-topic
```

在 GitHub 上从 `<your-name>:fix/plugin-topic` 向 `ayangweb/BongoCat-Plugins:master`
创建拉取请求。不要把提交直接推到上游，也不要通过拉取请求修改上游的 `master`。

主应用仓库遵循同样的权限规则：维护者可以直接克隆上游；其他开发者需要 fork
`ayangweb/BongoCat`，从上游 `master` 建分支，推送到自己的 fork 后向上游提交拉取请求。

#### 只修改插件实现

这类改动不需要修改主应用依赖。把主应用仓库中的插件路径指向自己的插件 fork checkout，
即可同时使用未提交的插件改动进行调试和完整插件测试：

```text
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just dev
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just plugin-tests
```

显式指定的插件 checkout 可以包含未提交改动；由 `just dev` 自动管理的
`target/plugin-dev/BongoCat-Plugins` 必须保持干净，并固定在 `Cargo.toml` 声明的 SDK revision。

#### 修改 SDK、进程协议或宿主接口

这类改动会同时影响插件仓库和主应用，需要两个拉取请求，并先合并插件仓库的改动。插件拉取请求
尚未合并时，可以在主应用本地临时把 SDK 依赖指向插件 checkout，用于验证两端协议是否一致：

```toml
[workspace.dependencies]
bongocat-plugin-sdk = { path = "../BongoCat-Plugins/crates/bongocat-plugin-sdk" }
```

这里的路径是主应用仓库内的本地路径示例，实际路径由开发者自行调整。也可以临时指向贡献者 fork
的完整 commit。此类依赖修改和对应的 `Cargo.lock` 变化只用于本地联调，不得提交到主应用拉取
请求。

插件仓库改动合并后，在主应用中把依赖恢复为上游仓库，并把 `rev` 固定为合并后的完整 40 位
commit SHA：

```toml
[workspace.dependencies]
bongocat-plugin-sdk = { git = "https://github.com/ayangweb/BongoCat-Plugins", rev = "<full-upstream-commit-sha>" }
```

随后执行 `cargo update --workspace` 同步 `Cargo.lock`，运行 `just check`，并用合并后的插件
checkout 运行 `just plugin-tests`。主应用不能合并到贡献者 fork 的 commit；必须使用上游仓库
中已合并的完整 revision。主应用通过 Git revision 使用 SDK，因此不要求先创建插件 Tag 或
Release；插件发布仍由插件仓库独立完成。

在 Windows 上构建时，请确保使用 MSVC 工具链（形如 `stable-x86_64-pc-windows-**msvc**`，
而非 `stable-x86_64-pc-windows-**gnu**`）。本项目依赖 Live2D Cubism Core，而该库的 Windows 版
官方仅提供 MSVC 格式的 `.lib`，因此 Windows 上必须使用 MSVC 工具链。可使用 `rustup show` 查看
当前工具链，其会显示在 `active toolchain` 栏；若不是 MSVC 工具链，请通过 `rustup override set`
切换为 MSVC 工具链。

在 macOS 上构建还需要 Xcode 自带的 Swift 工具链，因为「输入监控」引导流程链接了一个 Swift 静态
库（ADR-0078）。SwiftPM 拒绝加载 tools version 高于本机工具链的包清单，而该包声明的是 6.1；
`rust-toolchain.toml` 不覆盖它，所以请保持 Xcode 为较新版本——否则失败会表现为
`permission-flow` 构建脚本里的一条 `swift build` 报错。

可使用 `just dev-smoke` 验证设置窗口生命周期。修改配置或窗口状态类型后，运行 `just schema`
重新生成已提交的 JSON Schema 契约。涉及平台特性的改动还必须在每个受影响的操作系统上验证；
在其他主机上完成构建或测试不能代替目标平台的 smoke 验证。

## 验证要求

提交拉取请求前：

- 代码改动需运行 `just check`。
- 开发过程中可先运行定向测试，提交前再运行完整检查。
- 条件允许时，应为改动后的行为新增或更新自动化测试。
- 可见界面改动需提供截图，并注明截图所用的窗口尺寸、缩放比例、主题和平台。
- 用户可见文案发生变化时，需要同步更新两种语言版本。

不能仅凭编译成功就宣布平台功能完成。拉取请求中应记录未运行的平台检查和剩余风险。

## 构建与打包

`just build` 是唯一受支持的构建和打包入口。不要在其它工具、工作流或脚本中新增一套并行的打包
流程。不要提交构建产物、用户数据、本地环境文件或签名密钥。

## 提交与拉取请求

提交信息请遵循 [Conventional Commits](https://www.conventionalcommits.org/)，例如
`feat: add model import validation`。

拉取请求应说明：

- 要解决的用户可见问题和预期行为；
- 本次改动的范围；
- 已运行的验证命令及结果；
- 涉及的平台、权限，以及未能运行的检查；
- 行为涉及视觉变化时提供截图或录屏。

如果改动影响用户可见行为、默认值、兼容性、支持平台或升级说明，除维护者另有决定外，请同时更新
`CHANGELOG.md` 和 `CHANGELOG.zh-CN.md`。

## 隐私与安全

不得在议题、拉取请求、日志、截图或测试数据中包含真实用户路径、按键序列、剪贴板内容、私有模型
数据、凭据或签名密钥。

## 历史版本

重构前的实现仅保留在受保护的
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) 分支中。该分支仅供历史参考；
请勿修改或重新接入其中的实现。
