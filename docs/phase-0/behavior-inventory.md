# Phase 0 Behavior Inventory

状态：静态功能范围与优先级已冻结，行为细节待 Windows/macOS 实机确认
基线 commit：`44f44bc`
记录日期：2026-08-28

## 优先级

- `P0 首发`：Windows/macOS 首发必须具备；可以修复旧缺陷，但产品能力不能静默消失。
- `P1 首发后`：首发后补齐，不阻塞最小可发布版本。
- `不迁移`：只作为行为对照或反例，不进入 Native Rewrite 产品实现。

优先级描述产品能力，不承诺复制旧实现。列为 `P0 首发` 的行为仍须通过 fixture、平台 spike 或实机验收后才能宣称兼容。

## 功能矩阵

| 领域       | 行为                                                | 优先级    | 源码结论                                             | 待确认                          |
| ---------- | --------------------------------------------------- | --------- | ---------------------------------------------------- | ------------------------------- |
| 主窗口     | 透明、无边框、默认跳过任务栏                        | P0 首发   | 主窗口配置为透明、无装饰、无阴影并默认跳过任务栏     | 原生 alpha、任务栏与阴影        |
| 主窗口     | 显示/隐藏、置顶、穿透                               | P0 首发   | 配置、快捷键和菜单可直接控制                         | 双平台层级、焦点和恢复          |
| 主窗口     | 左键拖动                                            | P0 首发   | 主窗口按下后调用窗口拖动                             | 穿透开启时的临时操作入口        |
| 主窗口     | 10%-500% 缩放和 10%-100% 透明度                     | P0 首发   | 设置页、托盘菜单和 Shift+右键拖动可修改              | 逻辑尺寸、Retina/DPI 和手势     |
| 主窗口     | 圆角                                                | P1 首发后 | 旧版通过内容裁剪百分比实现                           | 原生窗口边缘与背景资源一致性    |
| 主窗口     | hover 延迟隐藏                                      | P1 首发后 | 光标进入后隐藏内容并临时穿透，离开后恢复             | 多显示器边界与丢失离开事件      |
| 主窗口     | 保持在屏幕内                                        | P0 首发   | 移动/缩放后按光标所在显示器边界 clamp                | 显示器移除和负坐标 fallback     |
| 窗口状态   | 恢复主窗口与设置窗口的位置和尺寸                    | P0 首发   | 旧版保存两个窗口的物理 x/y/width/height              | 稳定显示器 id 与 DPI 变化       |
| 主窗口菜单 | 打开设置、显隐、穿透、缩放、透明度、重启和退出      | P0 首发   | 主窗口右键菜单与托盘共用基础菜单                     | 置顶窗口弹出菜单层级            |
| 键盘       | 按键图片按 left/right key 目录分手                  | P0 首发   | 同一手只保留最后一个 pressed key                     | 多键同手产品语义确认            |
| 键盘       | 不支持的 F 键降级为 Fn，左右修饰键降级为通用键      | P0 首发   | 根据当前模型资源名动态 fallback                      | PhysicalKey 命名全集            |
| 键盘       | CapsLock 短暂触发                                   | P0 首发   | 当前固定 100ms 自动释放                              | 锁定键跨平台状态语义            |
| 键盘       | 每次 KeyDown 创建 3 秒自动释放 timer                | 不迁移    | Windows 旧兜底会破坏长按且不能证明真实释放           | 由 KeyUp、reconcile、Reset 替代 |
| 鼠标       | 左右按钮驱动专用模型参数                            | P0 首发   | 左键映射 Left，旧版把其他所有按钮映射为 Right        | 中键和侧键独立语义              |
| 鼠标       | 非 Left 按钮全部当作 Right                          | 不迁移    | 会把中键和侧键错误映射为右键                         | 新协议显式枚举或忽略            |
| 鼠标       | 当前显示器归一化坐标驱动角度和眼球参数              | P0 首发   | X/Y 映射 parameter range，Z 使用 X/Y 乘积            | 混合 DPI、负坐标与边界          |
| 鼠标       | 位置指数平滑、忽略鼠标和水平镜像                    | P0 首发   | 60 FPS 基准 decay 为 0.75，距离 <0.5 时收敛          | 确定性 tick 与 mirror fixture   |
| 手柄       | 按钮、D-pad 和 trigger 驱动左右按键图片             | P0 首发   | gamepad 模式启停独立监听                             | dead-zone、断开复位和重连       |
| 手柄       | 双摇杆显示、按下和 XY 参数                          | P0 首发   | 使用 `CatParamStick*` 系列参数                       | 多手柄选择与 profile 差异       |
| Live2D     | model3、moc、texture 和 cdi 加载                    | P0 首发   | 旧版选择目录中的第一个 model3                        | 新版多入口必须拒绝并诊断        |
| Live2D     | motion group/index、normal priority 与 lock group   | P0 首发   | motion 可预览并绑定快捷键                            | priority、并发和停止语义        |
| Live2D     | expression 按 index 切换                            | P0 首发   | expression 绑定使用 model id + index                 | 稳定资源 id 与模型切换          |
| Live2D     | physics、pose、fade、EyeBlink 和 parameter 更新     | P0 首发   | 预置模型不含 physics/pose，自定义模型会包含 physics  | 授权真实样本和 Framework 依据   |
| Live2D     | motion sound 和最大 FPS                             | P0 首发   | 默认音效开、60 FPS                                   | 音频后端、无声资源与不可见降频  |
| 模型       | standard、keyboard、gamepad 三个预置和自定义模型    | P0 首发   | 启动时重建预置项并保留自定义项                       | 用户模型 schema 与稳定 id       |
| 模型       | 目录选择、拖放、多选导入并复制到当前环境数据根      | P0 首发   | 旧版直接复制选择目录                                 | 安全 preflight、取消和冲突      |
| 模型       | 通过 right-keys 文件名静默推断 keyboard/gamepad     | 不迁移    | `East` 文件存在即判断 gamepad，否则判断 keyboard     | 新版显式分类、预览或诊断        |
| 模型       | 删除、切换、预置保护和在文件管理器显示              | P0 首发   | 预置不可删除；自定义模型可删除                       | prepare/commit 与删除事务       |
| 模型       | 删除失败后仍从模型列表移除                          | 不迁移    | `finally` 无条件更新列表，可能制造磁盘/配置不一致    | 失败必须保留原模型              |
| 快捷键     | 显示猫、打开设置、镜像、穿透和置顶                  | P0 首发   | 只在 Pressed 执行；修改时旧版先解绑旧值              | 冲突检测和事务回滚              |
| 行为快捷键 | motion/expression 自动分配和用户编辑组合键          | P1 首发后 | primary + Shift/Alt 与数字/字母分层                  | 稳定行为 id 与冲突策略          |
| 托盘       | 可隐藏的托盘/菜单栏图标与动态状态菜单               | P0 首发   | 菜单包含设置、显隐、穿透、缩放、透明度、更新和退出   | 图标隐藏后的可恢复入口          |
| 系统       | 任务栏图标与托盘/菜单栏图标分别显示或隐藏           | P0 首发   | 设置页分别保存 taskbar/tray visibility               | 双入口均隐藏时的恢复路径        |
| 系统       | 登录启动、单实例、正常重启/退出和结构化日志         | P0 首发   | 旧版由 Tauri 插件提供                                | 原生失败状态和 shutdown         |
| 权限       | Windows 管理员状态提示                              | P0 首发   | 非管理员时提示退出，但旧版没有原地提权               | 默认不提权及高权限应用输入差异  |
| 权限       | macOS Input Monitoring 状态、请求和再次引导         | P0 首发   | 旧版只处理 Input Monitoring，没有 Accessibility 流程 | 拒绝、撤销、重启和 TCC 差异     |
| 设置       | 设置页关闭时隐藏、可从托盘/快捷键/单实例唤回        | P0 首发   | CloseRequested 被拦截并隐藏                          | GPUI 窗口重建与焦点             |
| 设置       | 首次启动依次初始化配置、模型、窗口恢复和系统 locale | P0 首发   | UI 等待窗口恢复完成后显示                            | 部分失败时的 degraded 页面      |
| 设置       | 原样复刻旧 Vue/Ant Design 页面结构和视觉            | 不迁移    | 旧布局只作为功能入口证据                             | GPUI 使用新 design system       |
| 更新       | 自动/手动检查、进度、稍后安装和安装后重启           | P0 首发   | 自动检查周期为 24 小时                               | HTTPS、签名、channel 和回滚     |
| 更新       | HTTP endpoint、嵌入请求凭据和旧 updater payload     | 不迁移    | 旧配置允许不安全传输且前端带固定 access key          | 新协议只允许 HTTPS 与签名       |
| 外观       | system/light/dark 与五种既有语言                    | P0 首发   | 首次语言从系统 locale 获取，不支持时回退英文         | GPUI 字体、输入法和布局         |
| 错误       | 模型、更新、文件和快捷键失败具有用户可见反馈        | P0 首发   | 旧版多为未分类字符串 toast/dialog                    | 稳定 error code、重试和详情     |
| 诊断       | 复制脱敏 app/platform 信息、打开日志目录和反馈链接  | P1 首发后 | About 页已有复制、日志和外部链接入口                 | 诊断包内容与隐私                |
| 配置       | 读取、探测、导入或 alias 旧 Tauri/Pinia 配置        | 不迁移    | 仅保留只读考古 fixture                               | Native schema 从 v1 开始        |
| 配置       | 原样恢复旧物理像素坐标和旧模型 id                   | 不迁移    | 旧状态不具备稳定显示器 id，且 Native 不导入旧配置    | 新状态使用逻辑坐标和自有 id     |
| 平台       | Linux 窗口、输入、托盘、安装包和旧条件分支          | 不迁移    | Linux 只属于首发后单独能力评估                       | 不进入 Windows/macOS 首发范围   |

## 主要源码证据

- 窗口、菜单和持久状态：`src/pages/main/index.vue`、`src/composables/useWindowState.ts`、`src/composables/useAppMenu.ts`、`src-tauri/src/plugins/window/`。
- 键鼠和手柄：`src/composables/useDevice.ts`、`src/composables/useGamepad.ts`、`src/composables/useModel.ts`、`src-tauri/src/core/device.rs`、`src-tauri/src/core/gamepad.rs`。
- 模型和行为：`src/stores/model.ts`、`src/pages/preference/components/model/`、`src/utils/live2d.ts` 和三个预置资源目录。
- 设置与系统集成：`src/pages/preference/`、`src/composables/useTray.ts`、`src/components/update-app/index.vue`、`src-tauri/src/lib.rs`。
- 应用和发布配置：`src/stores/`、`src/locales/`、`src-tauri/tauri.conf.json` 及平台覆盖配置。

这些路径固定的是基线 commit 中的静态事实；历史源码后续变化不自动改变本矩阵，任何范围变化必须显式评审并更新本文件。

## 范围统计

| 优先级    | 数量 | 解释                                         |
| --------- | ---: | -------------------------------------------- |
| P0 首发   |   34 | 构成 Windows/macOS 可发布产品闭环            |
| P1 首发后 |    4 | 可延期且不破坏核心使用路径                   |
| 不迁移    |    9 | 历史实现缺陷、旧技术细节或明确排除的平台能力 |

统计用于防止实现期静默扩缩范围；拆分或合并矩阵行时必须同步更新。

## 已确认的行为风险

1. Windows KeyDown 后通过 timer 自动释放，不能证明真实按键状态，且长按语义受 delay 影响。
2. pressed key 图片按“手目录”互斥，不是任意多键叠加；fixture 必须保留该产品语义或明确修改。
3. 模型删除在文件删除失败时仍会从列表移除，新实现不得复制该错误行为。
4. 模型 mode 通过 right-keys 内容推断，属于启发式协议，需要显式验证与诊断。
5. 当前窗口状态保存物理坐标，跨 DPI/显示器恢复需要新规范。
6. 当前更新、模型和输入错误多以字符串传播，新 runtime 需要稳定 error code。
7. 旧 updater 允许 HTTP endpoint，并把固定请求凭据放在前端源码；Native Rewrite 不得复制。
8. 旧设置页面、Tauri plugin API 和 Pinia 字段名不是兼容面，只保留为功能入口证据。

## 尚需实机确认

- Windows：管理员/非管理员、PixPin、Win+L、UAC、Raw Input 覆盖和混合 DPI。
- macOS：权限拒绝/授予/撤销、Spaces、全屏辅助、Retina 和睡眠恢复。
- 双平台：三个预置模型的实际动作、表情、鼠标映射、音效与窗口尺寸。
