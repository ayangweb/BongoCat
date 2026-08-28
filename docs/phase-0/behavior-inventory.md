# Phase 0 Behavior Inventory

状态：源码考古完成第一轮，待 Windows/macOS 实机确认
基线 commit：`44f44bc`
记录日期：2026-08-28

## 优先级

- `P0`：Windows/macOS 首发必须兼容。
- `P1`：首发后补齐，不阻塞最小可发布版本。
- `Reference`：只作为迁移或行为对照，不进入新 runtime。

## 功能矩阵

| 领域       | 行为                                                     | 优先级 | 源码结论                                                  | 待确认                         |
| ---------- | -------------------------------------------------------- | -----: | --------------------------------------------------------- | ------------------------------ |
| 主窗口     | 透明、无边框、默认跳过任务栏                             |     P0 | 两个窗口在应用配置中声明，主窗口默认透明/置顶             | 原生 alpha 与任务栏行为        |
| 主窗口     | 显示/隐藏、置顶、穿透                                    |     P0 | 配置变化直接控制窗口                                      | 双平台层级与焦点               |
| 主窗口     | 缩放 10%-500%、透明度、圆角                              |     P0 | 缩放同时改变窗口尺寸；右键+Shift 拖动可缩放               | 原生 overlay 手势              |
| 主窗口     | hover 延迟隐藏                                           |     P1 | 光标进入窗口后改变透明度并临时穿透                        | 多显示器边界语义               |
| 主窗口     | 保持在屏幕内                                             |     P0 | 移动/缩放后按光标所在显示器边界 clamp                     | 显示器移除 fallback            |
| 窗口状态   | 保存两个窗口的物理位置与尺寸                             |     P0 | `app.windowState[label]` 持久化 physical x/y/width/height | 稳定显示器 id 与 DPI 迁移      |
| 键盘       | 按键图片按 left/right key 目录分手                       |     P0 | 同一手只保留最后一个 pressed key                          | 多键同手产品语义确认           |
| 键盘       | 不支持的 F 键降级为 Fn，修饰键降级为通用键               |     P0 | 基于模型资源名动态 fallback                               | 物理键命名规范                 |
| 键盘       | CapsLock 短暂触发                                        |     P0 | 当前固定 100ms 自动释放                                   | 新 runtime 的锁定键语义        |
| 键盘       | Windows 自动释放兜底                                     |     P0 | 当前每次 KeyDown 建 timer，默认 3 秒                      | 由 reconciliation 替代正常路径 |
| 鼠标       | 左/右按钮驱动 `ParamMouseLeftDown`/`ParamMouseRightDown` |     P0 | 非 Left 统一落到 Right                                    | 中键/侧键产品语义              |
| 鼠标       | 光标在当前显示器中归一化后驱动角度/眼球参数              |     P0 | X/Y 映射 parameter range，Z 使用 X/Y 乘积                 | 混合 DPI 与 mirror fixture     |
| 鼠标       | 位置使用指数平滑                                         |     P0 | 60 FPS 基准 decay 为 0.75，距离 <0.5 时收敛               | Rust 精度与 tick 规范          |
| 手柄       | 按钮与轴事件驱动按键图片和参数                           |     P0 | 模型切到 gamepad 时启动监听                               | dead-zone/断开复位             |
| 手柄       | 双摇杆显示、按下和 XY 参数                               |     P0 | `CatParamStick*` 系列参数                                 | 多手柄选择策略                 |
| Live2D     | 加载首个 model3，列出 motion/expression                  |     P0 | 路径重定向到模型目录                                      | 多 model3 拒绝/选择规则        |
| Live2D     | motion 使用 normal priority                              |     P0 | motion group/index 可绑定快捷键                           | priority 与 lock group 语义    |
| Live2D     | expression 按 index 切换                                 |     P0 | expression 绑定使用 model id + index                      | 稳定资源 id                    |
| Live2D     | motion sound 和最大 FPS                                  |     P0 | 默认音效开、60 FPS                                        | 音频后端与不可见降频           |
| 模型       | 三个预置模式 + 自定义模型                                |     P0 | 预置总是重新插入，自定义模型保留                          | 用户模型 schema                |
| 模型       | 目录选择/拖放导入、复制到 app data                       |     P0 | right-keys 文件名推断 mode                                | 安全验证、取消、冲突           |
| 模型       | 删除、切换、在文件管理器显示                             |     P0 | 删除失败后当前逻辑仍从列表移除                            | 新实现必须事务化               |
| 快捷键     | 显示猫、打开设置、镜像、穿透、置顶                       |     P0 | 注册时先解绑旧值，只响应 Pressed                          | 冲突回滚和平台规范             |
| 行为快捷键 | motion/expression 自动分配组合键                         |     P1 | primary + Shift/Alt 与数字/字母分层                       | 稳定绑定迁移                   |
| 托盘       | 动态显示显隐、穿透、缩放、透明度、更新、退出             |     P0 | 状态变化时重建菜单                                        | 设置窗口关闭后生命周期         |
| 系统       | 启动项、权限状态、单实例、日志                           |     P0 | 由多个系统插件提供                                        | 原生实现与错误状态             |
| 更新       | 自动/手动检查、进度、安装后重启                          |     P0 | 自动检查周期 24h                                          | HTTPS、签名和回滚              |
| 外观       | auto/light/dark 与五种语言                               |     P0 | 首次语言从系统 locale 获取                                | GPUI 文本和布局验证            |
| 诊断       | 复制 app/platform 信息、打开日志目录                     |     P1 | About 页提供                                              | 脱敏与导出包                   |

## 已确认的行为风险

1. Windows KeyDown 后通过 timer 自动释放，不能证明真实按键状态，且长按语义受 delay 影响。
2. pressed key 图片按“手目录”互斥，不是任意多键叠加；fixture 必须保留该产品语义或明确修改。
3. 模型删除在文件删除失败时仍会从列表移除，新实现不得复制该错误行为。
4. 模型 mode 通过 right-keys 内容推断，属于启发式协议，需要显式验证与诊断。
5. 当前窗口状态保存物理坐标，跨 DPI/显示器恢复需要新规范。
6. 当前更新、模型和输入错误多以字符串传播，新 runtime 需要稳定 error code。

## 尚需实机确认

- Windows：管理员/非管理员、PixPin、Win+L、UAC、Raw Input 覆盖和混合 DPI。
- macOS：权限拒绝/授予/撤销、Spaces、全屏辅助、Retina 和睡眠恢复。
- 双平台：三个预置模型的实际动作、表情、鼠标映射、音效与窗口尺寸。
