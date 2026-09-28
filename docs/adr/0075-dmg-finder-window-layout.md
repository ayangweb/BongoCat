# ADR-0075: 磁盘映像的 Finder 窗口布局与卷图标

状态：已接受（2026-09-29）
补充：ADR-0033 第 4 条（`.dmg` 由操作系统磁盘映像工具产出）

## 背景

ADR-0033 把 `.dmg` 的构建从 `cargo-packager` 收回到 `crates/bongocat-packaging`，用
`ditto` + `hdiutil` + `codesign` 产出镜像。当时镜像里只有 bundle 和一个指向
`/Applications` 的符号链接，没有任何 Finder 元数据，因此挂载后的窗口完全由 Finder
的默认值决定：按内容自适应的大小、48 px 图标、条目按名称排列、工具栏和状态栏都在，
标题栏和路径栏显示的是通用的磁盘映像图标。

Tauri v2 的默认 `.dmg` 不是这样。它的 `bundle.macOS.dmg` 默认值是

| 配置 | 默认值 |
| --- | --- |
| `windowSize` | `660 × 400` |
| `appPosition` | `180, 170` |
| `applicationFolderPosition` | `480, 170` |
| `background` | `null` |
| `windowPosition` | `null` |

并且它把窗口交给 `create-dmg` 脚本去排（`--icon <app> 180 170 --app-drop-link 480 170
--window-size 660 400 --volicon <icns> --hide-extension <app>`）。Tauri 没有传的
`--window-pos`、`--icon-size`、`--text-size` 落到 `create-dmg` 自己的默认值
（`WINX=10`、`WINY=60`、`ICON_SIZE=128`、`TEXT_SIZE=16`），所以它的默认镜像实际是
660×400 的窗口、128 px 图标、16 pt 标签、app 在左 `Applications` 在右、无工具栏与状态栏、
路径栏保留，并且卷有应用图标。

这些数字里没有一个来自本仓库，因此在打包侧也必须有一处明确的来源。

## 决策

### 1. 布局由构建期写入的 `.DS_Store` 描述，不经过 Finder

窗口布局不在镜像的文件系统里。Finder 在打开卷时读取卷根的 `.DS_Store`，从里面取出窗口
大小、图标位置和图标视图设置；没有这个文件就用它自己的默认值。`create-dmg`（以及 Tauri）
的做法是挂载镜像后用 AppleScript 驱动 Finder 排窗口，Finder 顺手把 `.DS_Store` 写出来。

本项目不走这条路，理由是它在无人值守环境里不成立：

- AppleScript 控制 Finder 需要图形登录会话和一次"自动化控制 Finder"的授权；`create-dmg`
  自己为此提供 `--skip-jenkins`，Tauri 在 `CI=true` 时就会传它——也就是说 Tauri 的 CI
  产物本来就没有这套布局。
- 授权被拒绝时脚本中途失败，留下一张排了一半的镜像。
- 顺带一提，ADR-0033 已经记录了 `create-dmg` 在当前 macOS 上连挂载解析都做不完整。

因此 `crates/bongocat-packaging` 直接写卷根的 `.DS_Store`，写的就是 `create-dmg` 那份
`template.applescript` 会让 Finder 记下的内容：`bwsp`（窗口 bounds 与要隐藏的 Finder
部件）、`icvp`（图标视图设置，含图标与标签尺寸）、`vSrn`（视图设置版本），以及两个
`Iloc`（app 与 `Applications` 的图标坐标）。数值取自上面的 Tauri 默认值与 `create-dmg`
默认值，集中在 `finder_store::WindowLayout::default()` 一处。

路径栏刻意不隐藏：它在窗口底部标出卷名与卷图标，Tauri 也没有隐藏它，而
`template.applescript` 里没有对应语句。

### 2. `.DS_Store` 编码器由本仓库实现

`crates/bongocat-packaging/src/finder_store.rs` 写这个文件。格式是 buddy allocator 的
B-tree：4 字节文件头、32 字节 `Bud1` prelude、三个块（指向树根的目录块、记录全部落在
同一个 4096 字节叶页里的树、allocator 的块表）。`bwsp` 与 `icvp` 的值是 binary plist，
由 `plist` 写出。

调研过的两个第三方选择都没有采用：

- **`ds_parser =0.4.0`**（`.DS_Store` 解析与写入，类型化、带 HFS+ 往返测试）：许可证是
  BlueOak-1.0.0，`deny.toml` 白名单里没有，需要为此放开一条非 OSI 许可；解析器还会带进
  10 个新 crate，其中 `alias_record`（Alias 记录，也就是背景图的别名）本项目一个都用不到。
- **`cargo-codesign` 的 `ds_store` 模块**（MIT OR Apache-2.0，许可证无需放开）：它属于一个
  跨平台签名 CLI，会把 `ed25519-dalek`、`dialoguer`、`clap`、`toml` 等一起带进来，而本项目
  只需要其中一个模块的字节布局。

实际新增的依赖只有两个，且都已经在 `Cargo.lock` 里（`cargo-packager` 与其它 crate 已经
依赖它们），`Cargo.lock` 的净变化是 `bongocat-packaging` 多两行依赖名：

| 依赖 | 用途 | 替换边界 |
| --- | --- | --- |
| `plist =1.10.1` | 写 `bwsp` / `icvp` 的 binary plist | 只在 `finder_store` 内部使用，plist 类型不出现在打包工具的公共面 |
| `tempfile =3.27.0` | 卷的临时挂载点 | 只提供一个会被删除的目录 |

自研编码器的失败模式是有界的：`.DS_Store` 只影响 Finder 读到什么，不影响镜像里的文件，
所以格式写错的后果是安装器窗口退回 Finder 默认布局，而不是安装失败。

### 3. 卷图标需要一次可写挂载，因此镜像先建可写再转码

Finder 的卷图标是卷根的 `.VolumeIcon.icns` 加上卷根上的一个文件属性。属性只能写在已挂载
的可写卷上，而压缩成 `ULMO` 的镜像挂载后是只读的，所以顺序变成：

```text
hdiutil create -srcfolder <staging> -format UDRW   # 可写
hdiutil attach -nobrowse -noverify -noautoopen -mountpoint <临时目录>
SetFile -a C <挂载点>                              # 卷使用 .VolumeIcon.icns
hdiutil detach
hdiutil convert -format ULMO -o <镜像>             # ADR-0033 选定的压缩格式
codesign
```

`SetFile` 由 macOS 自身在 `/usr/bin` 提供（`create-dmg` 用的是同一个工具），不依赖
Command Line Tools；缺失时构建直接失败，而不是悄悄少一个图标。`.VolumeIcon.icns` 是
`resources/icons/logo-macos.icns` 本身，用 `SetFile -c icnC` 标成图标文件，与
`create-dmg` 的做法一致。

挂载点放在系统临时目录，而不是输出目录：实测把可写卷挂在构建树里会让 macOS 往卷中写入
`.fseventsd`（filesystem-events 日志），产物就多出一个随"当时谁在看着 checkout"变化的隐藏
文件；挂到临时目录下三次实测都没有。挂载由一个守卫对象持有，任何提前返回或 panic 都会
卸载它，构建失败不会在开发者机器上留下已挂载卷。

### 4. `icvp` 的键集必须与 Finder 自己写的完全一致

这一条是实测出来的：只写 `iconSize`、`textSize`、`arrangeBy` 等少数键时，Finder 打开
窗口仍然是 48 px 图标；把 Finder 自己写的那套键补齐（`backgroundType`、三个
`backgroundColor*`、`gridOffsetX/Y`、`gridSpacing` 也在内）之后，128 px 才生效。图标视图
的默认值显然不是"缺键时沿用已有值"，而是整条记录一起判定。

因此键集是对 Finder 的契约而不是偏好，`finder_store` 的单元测试按整份键名列表断言。

### 5. 压缩格式、文件系统与文件系统事件都不再改动

`ULMO` 与 ADR-0033 的实测结论保持一致；文件系统仍是 `hdiutil` 的默认（APFS）；镜像里
不再额外删除 `.fseventsd`，因为第 3 条已经让它的来源消失。

## 备选方案

- **引入第三方 `.DS_Store` crate**：`ds_parser` 与 `cargo-codesign` 的理由见决策第 2 条。
  两者都会为了一个约 200 行的固定记录集合扩大依赖面，而本项目能从源码读懂并测试这段布局。
- **提交一份预生成的 `.DS_Store` 二进制**：`node-appdmg` 之类的项目这样做，但文件名和
  坐标因此变成二进制里不可见的常量，改产品名或坐标就静默失效，而且契约测试无法断言它
  的内容。
- **只做布局、不做卷图标**：少一次挂载，但标题栏和路径栏会继续显示通用磁盘映像图标，
  与 Tauri 默认产物仍有可见差异。
- **把镜像建成 HFS+**（`create-dmg` 的默认文件系统）：HFS+ 卷没有 filesystem-events
  日志，第 3 条的问题自然消失；代价是把已经弃用的文件系统写进新产物。
- **改用 `diskutil`**：macOS 27 对 `hdiutil create` / `attach` / `convert` / `detach` 打印
  弃用提示，建议改用 `diskutil image create` / `image attach` / `image create from`。
  本次不换工具链，见残余风险第 3 条。

## 影响

- **用户可见**：macOS 安装器窗口变成 Tauri 默认外观——660×400、128 px 图标、16 pt 标签、
  app 在左 `Applications` 在右、无工具栏与状态栏、标题栏与路径栏是应用图标。
- 镜像卷根多两个隐藏文件：`.DS_Store`（3,140 B）与 `.VolumeIcon.icns`（应用图标）。
  Finder 默认不显示它们。
- 构建过程多一次可写镜像与一次转码：磁盘上短暂多约 38 MB 未压缩镜像，`hdiutil convert`
  实测 1.6 s。
- 体积没有变大。同一 x64 bundle 实测 11,952,090 B，对照改动前一次成型的 11,990,768 B
  反而小 0.32%——两次调用压缩的差异来自转码路径本身。
- 编码器只在 Unix 上编译（`#[cfg(unix)] mod finder_store;`）：Windows 的打包不读也不写
  `.DS_Store`，而 workspace 的 `-D warnings` 门禁不接受一个无人调用的模块。

## 已接受的残余风险

1. **`.DS_Store` 是 Apple 未公开的格式**，Apple 可以随时更改。后果限于安装器窗口的外观：
   写坏了 Finder 会忽略这个文件并用自己的默认值，安装本身（拖拽 `.app` 到
   `Applications`）不受影响。ADR-0033 的退出条件不变——`cargo-packager` 换到能在当前
   macOS 上工作的 `create-dmg` revision 后，这段代码连同本 ADR 的第 1、3 条一起删除。
2. **单元测试证明的是字节，不是 Finder 兼容性。** `finder_store` 的测试用与写入器对称的
   读取器读回自己的输出，binary plist 由 `plist` 自己解码；Finder 侧是实测（见验证），
   两者不能互相替代。
3. **macOS 27 对 `hdiutil` 的这些调用打印弃用警告**，`hdiutil imageinfo` 也是。它们仍然
   工作，但换 `diskutil` 是一个独立的工具链决定，不夹带在本次外观改动里。
4. **`SetFile` 必须在宿主 macOS 上存在**。它随系统发布在 `/usr/bin`，GitHub 的
   `macos-latest` runner 与开发者机器都有；缺失时构建失败并指名这个工具。
5. **两台机器同时 `just build` 会互相破坏**（共享 `target/package/dmg-stage`）。这是
   本仓库打包入口的既有性质，本次没有改变，也不由本 ADR 授权修复。

## 验证

- `just check`：fmt、clippy（`--all-targets --all-features`，`-D warnings`）、workspace
  测试、`cargo check --release` 全绿；`python3 -m unittest discover -s tools/tests`
  85 passed。
- `just build --target x86_64-apple-darwin` 产出的镜像，挂载后用 AppleScript 读回
  Finder 实际采用的设置：窗口 `218, 569, 878, 969`（660×400）、`BongoCat.app` 在
  `180, 170`、`Applications` 在 `480, 170`、图标 128 px、标签 16 pt、排列方式
  `not arranged`、工具栏与状态栏均不可见。
- 独立实现交叉验证：`ds_parser 0.4.0` 解析镜像内那份 `.DS_Store`，`warnings: []`，五条
  记录的键值与写入值逐项一致。
- `hdiutil imageinfo` → `Format: ULMO`；`codesign --verify --deep --strict` 通过。
- `GetFileInfo -a <挂载点>` → `avbstClinmedz`（`C` 即卷使用自定义图标）。
- 挂载后卷根只有 `.DS_Store`、`.VolumeIcon.icns`、`Applications`、`BongoCat.app`。
- `tools/tests/test_packaging_contract.py` 断言布局的七个数值来自 Tauri 默认值、镜像
  构建路径不出现 `osascript`，以及 `SetFile`、卷图标与转码步骤仍在。
