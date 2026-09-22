# ADR-0037: 在应用内导入 BongoCatMver 模型

状态：已接受（2026-09-17）
依赖：ADR-0030（先复用既有方案）、ADR-0036（模型来源按内容识别、压缩包与目录共用校验；
**该 ADR 已于 2026-09-22 撤回**）、ADR-0011（渐进实现与发布门禁）

修订（2026-09-22）：ADR-0036 撤回后，本文**决策 4「归档来源不解压」**连同它描述的
`ArchivePlan` / `MverSource::Archive` 一并失去实现——Mver 源现在只从用户选中的文件夹就地读取，
`MverSource` 退化为一个持有已 canonicalize 根路径的 struct。其余决策（转换边界、多模型产出、
键位图合成、重编码、诊断码）**全部不变**。归档侧的读取上限逻辑（用声明大小在解压前判定）随实现
删除，但 `LEGACY_RESOURCE_MAXIMUM_BYTES` 仍然约束目录侧的单次读入。

## 背景

`Bongo-Cat-Mver` 是 BongoCat 的上游原版实现（`docs/migration/bongo-cat-mver-reference.md`
固定了参考 commit）。它的模型格式与 BongoCat 不同，而且在两处不一致：

| | BongoCatMver | BongoCat |
| --- | --- | --- |
| Live2D 包位置 | `<源>/img/<模式>/cat_model/` | 包根（入口发现只看根目录） |
| 键位图 | 每个键分成 `hand/*.png` 与 `keyboard/*.png` 两层，另有一张与模式一一对应的背景和封面 | 每个键一张合成图，按 HID usage 解析出的名字放在 `resources/{left-keys,right-keys}` |
| 键位表 | 根 `config.json` 的 `standard`/`keyboard`/`gamepad` 三个 section | 无需键位表，模型只声明 Live2D 资源 |
| 一个"模型" | 每种输入模式各自一个 Live2D 包 + 一套键位图 | 一个目录 = 一个模型 |

历史上这件事由外部工具 `BongoCat-Converter`（TypeScript/浏览器）完成：用户选中整个 Mver 应用
文件夹，工具按模式产出 `BongoCat - <模式>` 目录，再在 BongoCat 里逐个导入。这要求用户离开产品、
理解两个格式的差异、并在两次产物之间手动搬运文件。用户要求把这一步内置到产品里：导入时直接
识别 BongoCatMver 模型并触发转换，转换产出的模型进入既有模型存储与管理流程。

三个只有看真实数据才会发现的事实决定了本 ADR 的形态：

1. **三种模式的键位表不在同一个编码空间里。** `standard`/`keyboard` 用 Windows 虚拟键码
   （`49` = `1`，`65` = `A`），`gamepad` 用 XInput 按键序号（`10` = 十字键左，`0` = A）。同一
   个 `13` 在前者是 `Return`、在后者是十字键下。
2. **一个源最多对应三个模型**，因为三种模式带的是三份不同的 `.moc3`（参考样本里分别是
   `demomodel.moc3`/`demomodel2.moc3`/`demomodel3.moc3`），无法合并成一个包。
3. **`gamepad.lefthand` 与 `righthand` 共用同一份 `keyboard` 图集**，右手的图集下标从左手的
   长度继续（`lefthand.length + index`），而不是从 0 重新开始。

## 决策

### 1. 检测仍按内容识别，且要求两条独立证据

`ModelStore::inspect_source` 复用 ADR-0036 的 `detect_source_kind`（目录 / zip 签名），然后问该
来源是否带 Mver 键位表。认定条件有两条，缺一不可：

* 根 `config.json` 存在且能解析成 legacy 的 section 形状；
* 这些 section 里至少有一个模式真的在 `<资源根>/<模式>/cat_model/` 下**恰好**有一个
  `.model3.json`。

第二条让误判几乎不可能：转换后的 BongoCat 包把入口放在包根，不在 `cat_model/` 下，两者从外部
无法混淆。`config.json` 读不到、或解析失败、或找不到任何带模型的模式，一律回退到普通包导入，
由那条路径给出它自己的诊断——检测是**试探性**的，不负责报错。

资源根优先取 `img/`（Mver 应用的实际布局），不存在时取源根本身，以容纳把模式文件夹直接摊在根上
的模型包。

### 2. 一个源产出多个模型，每个模型独立安装

每个模式转换成一个 BongoCat 包，各自分配一个随机 UUID v4 存储键，各自写一条 `installed_models`
元数据记录，因此三种模式成为模型列表里三个可以独立启用、改名、删除的普通条目。

**逐模式提交**，不做整体回滚：某个模式的键位图损坏时，已经转换成功的模式保留下来。理由是这些
模型彼此独立，把已经转换成功的模式一起丢弃比留下一个用户可重试的部分结果更糟。

标题是「用户标题 · 本地化模式名」（`我的猫 · 标准模式`），即社区导出模型已经在用的命名。模式名
在**截断之后再拼接**：模式名是三个模型之间唯一的区别，长来源名不能把它挤掉，所以先按标题上限
减去后缀长度裁剪来源名。

### 3. 转换写进 store 自己的 staging，与包来源共用提交尾部

`ModelStore` 的导入尾部（`PreparedModel::prepare` 校验 + 单次 `rename` 提交 + 失败清理 staging）
被抽成 `commit_installed_staging`，目录复制、归档解压与 legacy 转换三条路径共用它。因此转换
结果不可能绕过任何包校验，也不存在第二个临时位置：失败的转换不留痕，成功的转换仍是一次原子
rename。

### 4. 归档来源不解压（**已随 ADR-0036 撤回，见文首**）

目录来源就地读取；归档来源**只按需读取被命名的条目**（`ArchivePlan::read_file`，仍逐条目复核
名字/声明大小/实际字节数）。导入一个 Mver `.zip` 不会把整个应用——exe、DLL 和用不到的模式——
解压到磁盘上，只把转换真正需要的那几十个文件读进内存。

读取有独立上限 `LEGACY_RESOURCE_MAXIMUM_BYTES`（64 MiB）：它约束的是转换期临时读入的量，与
约束"可被安装的包"的 `ModelPackageLimits::maximum_file_bytes` 是两件事。归档侧在解压前用声明
大小判定，因此超限只花一次头部读取。

### 5. 键位图合成 = 最小画布上的 Porter-Duff over

合成画布取两层中较小的宽高，两层都锚在原点绘制，因此比画布大的一层被裁切而不是缩放——这与
canvas `drawImage` 的行为一致。合成用整数形式的 `over` 算子：不透明来源整体替换、全透明来源
不改变，只有抗锯齿边缘像素会四舍五入一次（浮点形式在此处最多每通道差 1 档）。

模式里根本没有 `keyboard/` 图集时，不合成，直接按字节安装 paw 图（复古模型可能已经把两层画在
一起了）。某个绑定的 paw 或配套键帽缺失时该绑定被跳过而不是让整次转换失败——与参考实现一致，
且"少一张键位图"不等于"这个模型不可用"。

### 6. 输出图使用 lossless 重编码，`oxipng` 是唯一选择

`image` 的 PNG 编码器本身不做滤波与熵编码优化，直接用它的输出会比源文件更大。生成的合成图
交给 `oxipng 10.2.1` 做**无损**重编码：位深/颜色类型/调色板/灰度缩减都保持解码后像素不变，
`optimize_alpha` 只改写全透明像素的颜色通道（肉眼不可见），交错被关闭。

测得的取舍（样本 15 张 612×354 合成图，release 构建）：

| 后端 | 总字节 | 用时 |
| --- | --- | --- |
| 仅 `image` 编码 | 160 641 | — |
| oxipng + libdeflate | 91 470（−43%） | 0.68 s |
| oxipng + Zopfli | 86 605（−46%） | 10.25 s |

Zopfli 多 5% 换 15 倍时间，对一次交互式导入不划算，因此只开 `oxipng` 的库入口（`zopfli`、
`parallel`、`binary` 三个 feature 全关）。作为对照，仓库内既有预置模型的键位图是 6.7–9.5 KB，
本次转换产出的同类文件是 4.1–8.4 KB，量级一致。

**明确排除 `imagequant`**（pngquant 的 Rust 绑定，TinyPNG 式有损量化）：它的许可证是
`GPL-3.0-or-later`（pngquant.org 的商业授权另售），与本项目 MIT 的 `deny.toml` 白名单冲突。
用户要的"保持尺寸与视觉质量基本不变"由无损重编码完全满足，不必引入许可证风险。

重编码失败时写回普通编码结果而不是让转换失败：文件在两种情况下都是合法 PNG，只有体积有差别。

### 7. 键位名必须落在产品自己的词汇表里

转换输出的文件名是 `bongocat-live2d` 从 HID usage 解析出的名字（`KeyA`、`Return`、
`BackQuote`、`ControlLeft`/`Control`……），不是第二套命名。两处刻意偏离参考实现的写法：

* `0x08`：参考表写作 `BackSpace`，产品运行时与预置模型都写作 `Backspace`，采用产品写法。
* `gamepad` 使用 XInput 序号 → 产品预置 gamepad 模型已经装载的名字
  （`DPadDown`/`LeftTrigger`/`LeftTrigger2`/`South`/`RightTrigger`…）。转换结果与
  `resources/models/gamepad/{left-keys,right-keys}` 的现有文件集合**逐个同名**。

无法命名的控制码（鼠标按键、产品没有 overlay 的键）不产出图片，也不报错：它本来就没有可显示的
键位图，产出 `undefined.png` 之类的东西比不产出更糟。

### 8. 诊断码：新增一个 store 码，复用既有用户可见码

新增 `ModelStoreDiagnostic::SourceConversionFailed`
（`model_store_source_conversion_failed`，`ALL` 由 12 增至 13），只用于"已经认出这是
BongoCatMver 源，但无法把它变成 BongoCat 包"：键位图不是可读 PNG、合成图无法编码、绑定引用的
资源超出读取上限、请求的模式不在此源中。

它刻意与 `InvalidPackage` 区分：后者是"按包读入并且包校验失败"，前者从未成为包。settings 层
把它映射到既有 `SettingsErrorCode::ModelImportSourceUnsupported`（沿用 ADR-0036 §8 的做法），
因此没有新增用户可见错误码；具体原因留在 `detail`/`resource` 里，进入日志与诊断包。

### 9. 进度跨模型保持单调

settings 的 `report_progress` 会丢弃回退的更新，而 store 每个模型都从 `Preparing` 与 0 重新
开始。`Application` 因此在跨模型处折叠进度：累计已完成模型的文件数与字节数、并对 stage 取
最大值后再上报。于是用户看到的是一个不倒退、且最终等于三个模型总和的一条序列，而不是卡在
第一个模型的终值上。

### 10. 无新增 UI 入口

不新增按钮，也不新增"选择要转换哪些模式"的选择面：检测发生在服务层（UI executor 不做阻塞
文件与模型解析，见 Technical Design §5.1），用户手里的东西决定发生什么。Models 页面只把
`models.installed.description` 改写为说明"BongoCatMver 模型会在导入时自动转换"。
`Application::import_models*` 是唯一的导入入口，原来的单模型入口被它取代，避免存在一条会绕过
转换的旁路。

## 明确不做

- **模式选择**：一次导入转换源中所有带可用 Live2D 包的模式。参考工具默认只转标准模式，但本
  产品没有理由替用户丢掉另外两个已经躺在源里的模型。
- **鼠标按键 overlay**：`standard.mouse_left/right/side` 与 `mouse*.png` 不转换。产品当前没有
  鼠标按键的 overlay 通道（`InputControl::Mouse` 不产生 `KeyPress`），参考实现同样忽略它们。
- **`face/`、`sounds/`、`arm*.png`、`tablet*.png`**：参考实现不处理，产品格式里也没有对应位置；
  模式自带音效已经通过 `cat_model/` 里的 `Sound` 引用随包复制。
- **有损量化 / 降分辨率 / 减帧**：用户明确要求不牺牲画质，且 GPL 有损库被许可证排除。
- **`.rar`/`.7z`**：只有 zip 被真实需求驱动，与 ADR-0036 的范围一致。
- **把 Mver 源就地"升级"成 BongoCat 模型**：`next` 不做就地更新，导入永远是"新 id + 新目录"。
- **修改运行时让 F1–F12 逐键解析**：转换输出按 `F1`…`F12` 命名以保留模型自己的区分度，但
  `bongocat-live2d::key_name_candidates` 目前把功能键统一回退到 `Fn` 别名，因此逐键 F 图**暂时
  不可达**。这是运行时的既有缺口，不由本次改动引入；未改运行时是因为仓库内没有任何数据能验证
  这个改动（预置模型只有 `Fn.png`，真实样本的键位表也不含功能键），先记录为后续项。

## 残余风险与待验证项（不得当作已确认）

1. **真实样本只有一份**（用户提供的 `bongo_cat_mver_0.1.6_64`，三模式齐全、deflate 归档、
   `<img>/<模式>` 布局）。把模式文件夹摊在根上、缺少 `keyboard/` 图集、非 UTF-8 文件名、
   非 ASCII 路径只有合成测试覆盖。
2. **Windows 未实机验证**：本机是 macOS。转换路径本身平台无关（`std::fs` + `image` + `oxipng`），
   但 `oxipng` 依赖 `libdeflater`，它会用 `cc` 编译 libdeflate 的 C 源码；本机 macOS 构建通过，
   Windows MSVC 与交叉编译的 C 工具链未验证（与仓库既有的 ring 交叉编译限制同类）。
3. **`gamepad` 的键位图目前不可达**：`bongocat-runtime` 只对键盘按键产生 `KeyPress`，手柄按键
   仅置 `left_hand_down`/`right_hand_down`/`stick_*_down`。转换输出与预置 gamepad 模型同名，
   但两者在当前运行时下都只是随包携带的资源。这是既有缺口，不由本 ADR 引入，也不在本 ADR 修复。
4. **64 MiB 的读取上限没有基于实测压力测试**，取值依据是"真实模型的单文件是几 MB 量级"。
5. **Zopfli 之外的压缩收益未再评估**：只测了 libdeflate 与 Zopfli 两条后端，没有试
   `oxipng` 的高阶 preset 或 filter 集合调参。
6. **`cargo update` 顺带升了 3 个无关传递依赖**（`granit-parser` 1.2.1 → 1.3.0、
   `serde-saphyr` 1.2.0 → 1.3.0、`redox_users` 0.5.2 → 0.5.3）。这是 `AGENTS.md` §9 要求的完整
   `cargo update` 的结果，不是本功能需要。

## 验证

已完成（2026-09-17，本机 macOS / aarch64）：

- `bongocat-model` 83 测试（`mver.rs` 新增 18 个单元测试 + `store.rs` 新增 5 个）：检测的两条
  证据与"配置了模式却没有模型"的跳过、模式文件夹摊在根上的布局、左右手共用键盘图集的
  `lefthand.length + index` 下标、没有 `keyboard/` 图集时按字节复制、合成后包根布局与
  `[128, 0, 127, 255]` 的合成像素、`over` 算子边界（不透明 / 全透明 / 半透明 / 空画布）、
  lossless 重编码在缩小文件的同时保持**每一个 alpha > 0 的像素逐通道不变**、鼠标键与非法码被
  跳过、缺 layer 时跳过绑定、重复绑定只写一次、符号链接被拒绝、归档来源不解压即转换、
  超限资源被拒绝、两个编码空间的键名。
- **真实模型验证**：`cargo run -p bongocat-model --example model_conversion_smoke -- --source
  /Users/ayang/Downloads/bongo_cat_mver_0.1.6_64`（也不带 `BONGOCAT_MVER_SAMPLE` 时由
  `converts_the_legacy_sample_named_by_the_environment` 覆盖）。逐模式结果：
  `standard` → 3 张纹理、15 张键位图（`Num1..Num7`/`KeyQ`/`KeyE`/`KeyR`/`Space`/`KeyA`/`KeyD`/
  `KeyS`/`KeyW`，与 `config.json` 的 15 条绑定逐个对应）、31 文件 / 1 081 672 字节 / 0.87 s；
  `keyboard` → `left-keys`{`Control`,`KeyR`,`Shift`} + `right-keys`{四方向键}、23 文件 /
  1 015 987 字节 / 0.46 s；`gamepad` → `left-keys`{`DPadDown`,`DPadLeft`,`DPadRight`,`DPadUp`,
  `LeftTrigger`,`LeftTrigger2`} + `right-keys`{`East`,`North`,`RightTrigger`,`RightTrigger2`,
  `South`,`West`}、28 文件 / 1 082 753 字节 / 0.74 s。gamepad 的两个集合与仓库内
  `resources/models/gamepad/{left-keys,right-keys}` 的文件名**逐字符一致**；源目录未被修改。
- `bongocat-app` 124 测试（新增 4 个）：一个 Mver 源导入出 3 个模型、3 个不同 UUID、标题为
  「我的猫 · 标准模式/键盘模式/手柄模式」、三种模式各自的键位图落位与标准模式没有 `right-keys`、
  源目录未被写入、合并目录出现 3 条 installed；跨模型进度单调且终值等于三个模型文件数与字节数
  之和；标题在拼接模式名后仍不超上限且模式名保留；进度折叠的单元语义。
- `bongocat-i18n` 4 测试通过（两个 locale 的键与占位符完全一致），新增
  `models.legacy.mode.*` 与改写后的 `models.installed.description`。
- `cargo fmt --all --check`、三组 clippy（workspace `--all-targets --all-features` 与
  `bongocat-app` 的 `storage-test-injection`/`production`）、`cargo test --locked --workspace`
  （全部二进制全绿）、`cargo check --locked --workspace --release` 全部通过。

**未运行**：Windows 编译与实机导入、`NSOpenPanel`/Windows 对话框的实机交互（本次没有改动选择器）、
任意 UI 实机点击（本次只在服务层与文案层改动，Models 页面的控件集合未变）。
