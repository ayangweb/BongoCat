# ADR-0070: 手柄按键图片词表与按键层解析通道

状态：已接受（2026-09-26）

## 背景

用户报告：手柄模式下按下按键后模型没有显示对应按键图片。排查后发现的问题不是单点映射错误，
而是整条通道缺了一段，外加三处互相独立的命名缺陷。

**1. 手柄按键从来没有到达过按键层。** `bongocat_render::KeyPress` 只携带 `hid_usage: u16`，
`InputState::model_snapshot_with_filter` 遇到 `InputControl::Gamepad` 时只设置
`left_hand_down`/`right_hand_down` 或 `stick_*_down`，从不产生 `KeyPress`。手柄按钮没有 HID
Keyboard/Keypad usage，所以在这套类型里根本无法表达。后果是：手柄模式移动爪子，但**任何**按键
都不画图——与用户"部分按键不显示"的印象一致（实际是全部不显示，只是爪子动作让人以为按键生效了）。

**2. 预置模型的手柄图片名是第三方 backend 的词表。** 旧 Tauri 输入层用
`format!("{:?}", gilrs::Button)` 派生模型图片文件名，产物因此是 `South`/`East`/`West`/`North`、
`LeftTrigger`/`RightTrigger`（gilrs 语义是**肩键** LB/RB）、`LeftTrigger2`/`RightTrigger2`
（模拟扳机 LT/RT）、`LeftThumb`/`RightThumb`（摇杆键）、`DPad*`。项目自己的 `GamepadButton`
词表是 `South`/`East`/`West`/`North`/`LeftShoulder`/`RightShoulder`/`LeftTrigger`/`RightTrigger`/
`Select`/`Start`/`LeftStick`/`RightStick`/`Dpad*`。两套不一致，且旧词表里**一个主干承载两个语义**
（`LeftTrigger` 既是肩键名、又是产品扳机键名），任何"加个别名"的修法都会把其中一个按钮解析成
另一个按钮的图。

**3. 左右手归属是硬编码的，且与预置模型相反。** `bongocat-app::input_bindings_for_model` 对
`gamepad` 预置只写死两条：`South → Left`、`East → Right`。而预置模型把
`South`/`West`/`North`/`East` 和两个右扳机放在 `right-keys`，`DPad*` 和两个左扳机放在
`left-keys`——这是真实手柄布局，也是旧实现采用的判定方式（旧实现从图片所在目录推手）。旧实现的
`lefthand`/`righthand` 语义与此完全一致，Mver 转换也正是按它决定输出目录。

**4. Mver 的 XInput 按钮序号表有六个条目按错位置。** `legacy_gamepad_button_name` 写的是
8→`LeftThumb`、9→`RightThumb`、10→`DPadLeft`、11→`DPadRight`、14→`Start`、15→`Select`。
XInput（以及 gilrs 的 `Button` 顺序）是 8=Select、9=Start、10=LeftThumb、11=RightThumb、
12=DPadUp、13=DPadDown、14=DPadLeft、15=DPadRight。表是照着第三方词表的排列抄的，不是照着
按钮排列抄的，所以一个手柄按键转换出来的模型会把图装到错的按钮名下。

**5. 预置模型根本没有 `Select`/`Start`/两个摇杆键的图。** 即使通道修好，这四个按钮也只能保持惰性。

## 决策

- **`KeyPress` 改为携带带标签的身份。** `bongocat_render::KeyIdentity` 是
  `Keyboard(u16)` | `Gamepad(GamepadButton)`，`KeyPress` 是 `{ key, side }`。两族共用同一个按键层
  和同样两个手目录，但不是同一类控件，不折叠进一个数字空间：手柄按钮没有 HID usage，任何"给
  手柄按钮分配一段假 usage"的做法都会让一类控件被当成另一类解析，也不会有第三种输入族能安全
  扩展。`KeyPress::keyboard` / `KeyPress::gamepad` 是唯一构造入口。
- **图片名等于 `GamepadButton` 变体名。** `GamepadButton::key_image_name()` 是 16 个名字的唯一
  来源，名字与变体逐字相同。这条不变量让类型和美术词表无法漂移：改一个变体名会同时打穿所有
  装了旧主干的模型，新增一个变体在没有名字之前画不出任何东西。契约测试遍历 16 个按钮，检查名字
  两两不同且与变体名逐字相同。
- **词表是产品的，不是 backend 的。** `bongocat-platform` 把每个 backend 控制映射到项目枚举，
  gilrs 类型不越过 adapter 边界（ADR-0066），因此没有任何环节需要 backend 名字。
- **预置 `gamepad` 模型按产品词表重命名。** `DPad*`→`Dpad*`、
  `LeftTrigger`→`LeftShoulder`、`LeftTrigger2`→`LeftTrigger`、`RightTrigger`→`RightShoulder`、
  `RightTrigger2`→`RightTrigger`。目录归属不变（仍是真实手柄布局）。这是对用户已有模型的
  可见变化，因此记入 CHANGELOG 的用户可见修复。
- **导入时把旧主干改写为产品名。** `bongocat-model-store::key_names` 在 staging 副本上重写，
  与键盘侧既有的 `AltGr`→`AltRight`、`Return`→`Enter`、`Function`→`Globe`、`Backslash`→`BackSlash`
  是同一套机制、同一个唯一合法的位置（用户的源目录是只读输入）。
- **手柄词表不提供运行时旧名候选。** 键盘旧名可以作为末位候选，是因为 `AltLeft`/`AltRight`
  这样的 canonical 名和旧名 `Alt` 不冲突。手柄不是这样：旧词表里 `LeftTrigger` 已经是肩键的
  名字。给 `LeftTrigger`（扳机键）加 `LeftTrigger2` 旧名候选，会让一个装过归一化的旧包在按
  扳机时画出肩键的图；反过来给 `LeftShoulder` 加 `LeftTrigger` 候选，则会让一个已经用产品名
  写 `LeftTrigger.png` 表示扳机的包在按肩键时画出扳机的图。两种方向都会产生"看起来能用但按错键"
  的静默错误，比缺图更糟。归一化是唯一安全的答案。手工拷进 store、绕过归一化的目录只保留
  词表本身不冲突的那部分（四个面部键、两个菜单键）可达。
- **一个包整体判定为旧词表或新词表。** 归一化按目录判定：目录里出现一个只在旧词表里存在的主干
  （`LeftTrigger2`、`LeftTrigger2` 的右侧、`LeftThumb`、`RightThumb`、`DPad*`）就整体按旧词表
  重写。判定必须读目录真实条目名而不是探测路径——Windows 和 macOS 的路径不区分大小写，探测
  `DPadUp.png` 在装着 `DpadUp.png` 的目录里也会成功，那样每个包都会被判成旧词表。
- **只改大小写的条目真的改名。** `Backslash`→`BackSlash` 和 `DPadUp`→`DpadUp` 在大小写不敏感的
  文件系统上源和目标是同一个文件，直接 `rename` 是空操作；这两条经由同目录临时名完成两步改名，
  于是归一化在所有平台都生效，而不只在大小写敏感的文件系统上。临时名只存在于 store 自己的
  staging 副本里，两次 rename 之间被中断的导入由 store 的 abandoned-import 恢复删除。
- **Mver 的 XInput 序号表按真实按钮顺序重写。** 0–3 面部键、4/5 肩键、6/7 模拟扳机、8/9 菜单键、
  10/11 摇杆键、12–15 方向键（上/下/左/右），输出产品自己的 16 个按钮名。逐条断言这张表，并断言
  每一个输出名都是某个 `GamepadButton` 的图片名。
- **左右手由模型自己的目录决定。** `gamepad_hands_for_model` 遍历 `GamepadButton::ALL`，把按钮
  绑定到提供它图片的那一侧，`left-keys` 是左爪、`right-keys` 是右爪。这与旧实现一致，也让 Mver
  转换按 `lefthand`/`righthand` 落盘的结果直接可用，不再需要为每个转换模型单独配一张手表。
  同一按钮在两个目录都有图时归左手（键盘侧整块主键区也归左手）；产品每个控件只投影一只手，
  而没有预置模型会这样做。
- **缺图按钮保持惰性，摇杆键保留自己的参数。** 没有图片的按钮不绑定、不产生爪子动作、不产生
  按键层（ADR-0042）。`LeftStick`/`RightStick` 另外驱动 `StickLeftDown`/`StickRightDown`，
  与爪子状态互相独立；模型若提供 `LeftStick.png`，L3 依然同时得到按键层。
- **输入来源门禁对两族同样生效。** `ignore_keyboard` 让键盘连同它的按键层一起消失，
  `ignore_gamepad` 让手柄连同它的按键层一起消失；`model.ignore_gamepad` 打开时手柄按钮不贡献
  按键层。共享 fixture 现在逐 checkpoint 声明 `activeKeyOverlays`，因此省略声明等于声明空列表，
  漏写会被测试抓到。

## 后果

- 手柄模式下按下面板键、肩键、扳机和方向键都会显示对应图片，并抬起该手爪子；不画图的是模型
  没有图片的按钮（预置模型缺 `Select`、`Start`、两个摇杆键）。
- 已安装的旧版游戏手柄模型：只要经过一次导入，归一化会把旧主干改成产品名并从按 F 失效变为可用。
  手工拷入 store 的旧目录按上面"不冲突的那部分可达"的规则降级。
- 预置模型文件名变化会影响手工放置同名文件的用户，CHANGELOG 记录为用户可见修复。
- `KeyPress` 的字段从 `hid_usage` 变为 `key`，`bongocat-render` 的公共 API 有一处不兼容变更；
  它不进入配置或持久化格式，`schema_version` 仍是 1。
- 剩余门禁不变：Windows WGI 焦点矩阵与双平台物理设备、profile、热插拔和生命周期矩阵仍是
  ADR-0066 的完成门禁，本次修复不提供任何实机证据。

## 验证

- `bongocat-input`：16 个按钮的名字两两不同、与变体名逐字相同、都是可移植文件主干。
- `bongocat-render`：手柄身份与键盘身份永不相等；`KeyPressSet` 的去重与容量对两族一致。
- `bongocat-live2d-render`：预置 `gamepad` 模型的每一个有图按钮都能从它所在的那一侧解析到正确
  文件；没有图的按钮不解析到任何东西。
- `bongocat-runtime`：绑定的按钮同时产生爪子和自己的按键层；未绑定的按钮完全惰性；摇杆键的
  参数独立于爪子；两个来源门禁各自带走自己的按键层。
- `bongocat-app`：`gamepad_button_presses_reach_the_render_frame_as_key_overlays` 是整条链路的
  端到端断言——在真实激活的预置模型上按下面板键、肩键、扳机和方向键，逐个断言 runtime 快照里的
  按键层、爪子方向，以及 renderer 收到的帧确实从模型自己的文件解析出该按键图；没有图片的四个按钮
  断言为惰性。手绑定由模型目录得出，预置 `gamepad` 模型的四个面部键是右手。
- `bongocat-model-store`：旧 gamepad 主干归一化到产品名且两个扳机/肩键的图不互换；已经是产品名
  的包不被改写；只改大小写的条目在所有平台真的改名；XInput 序号表逐条等于真实按钮顺序。
- **Mver 转换与预置模型同名**：一个绑定全部 16 个 XInput 按钮的 legacy 源经真实转换后产出 16 个
  互不重名的产品名文件，每个都在自己的手列表指定的目录里、携带该按钮自己的合成图，且共享
  keyboard 图集的右手偏移落在正确的下标上。`bongocat-app` 的导入测试另外断言转换结果的键位图
  集合逐个等于产品按钮名，并按目录绑定到对应手——这正是"预置模型改名"与"转换输出命名"必须一致
  的那一条。
- 共享 fixture 逐 checkpoint 断言 `activeKeyOverlays`，`gamepad-reconnect-reset` 与
  `input-recovery-lifecycle` 覆盖手柄；`keyboard-modifiers-and-repeat` 顺带固定"同一只手只画最后
  按下的那个键"。
- `spikes/model-package` 的 `preset-model3-index.json` 同步更新并逐字段相等。
- 变异验证：把 `InputState` 的手柄手绑定改成永远 `None`，以及把
  `GamepadButton::key_image_name` 的 `LeftTrigger` 改成 `LeftTrigger2`，两层各自把
  `bongocat-app` 端到端、`bongocat-runtime`、`bongocat-live2d-render`、`bongocat-input`、
  `bongocat-model-store` 和共享 fixture 的对应测试全部变红。

