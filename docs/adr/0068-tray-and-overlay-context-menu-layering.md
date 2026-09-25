# ADR-0068: 托盘与模型窗口右键菜单复用

状态：已接受（2026-09-25）

修订：2026-09-25 根据产品反馈调整为托盘与模型窗口右键共用同一套菜单；取代本文早期“拆分两棵菜单树”的方案。

## 背景

托盘菜单和模型窗口右键菜单都代表 BongoCat 的模型窗口控制面。此前曾尝试按入口拆成不同的菜单树，
但这会让同一组模型窗口操作在两个入口出现不同的层级和状态，增加维护与验收成本。

本次调整只改变菜单树的复用方式，不改变应用状态的所有权。平台仍只产生强类型 `SystemMenuAction`，
overlay 仍只发送右键请求，runtime、settings service 和 renderer 不感知菜单树。

## 决策

### 一棵 popup 根，一个 owner

`bongocat-platform::SystemMenu` 是唯一的托盘/菜单 owner，持有 `TrayIcon`、一个 `muda::Menu` 根、
一个模型窗口 `Submenu`、一套菜单项、`MenuEvent` receiver 和强类型事件队列。托盘图标和模型窗口右键
都展示这同一棵菜单树：

```text
设置
────────
模型窗口
  ├─ □ 隐藏模型窗口
  ├─ □ 鼠标穿透
  ├─ □ 始终置顶
  └─ □ 鼠标悬停时隐藏
────────
检查更新（仅在更新能力可用时创建）
────────
退出 BongoCat
```

菜单不再提供源码、重启、版本、缩放或透明度行。源码与版本已经在设置的 About 页面提供；缩放由设置页
和右键拖动提供，透明度留在设置页。检查更新仍由构建/channel 事实决定是否创建，不显示永久禁用的空行。

### 动作和状态边界

两个入口共用同一批 native item 实例和同一组稳定 action id，由同一个 `action_for_menu_id` 映射到
`SystemMenuAction`。应用 action 分发和 revisioned settings command 不增加菜单专用弱类型协议。

菜单 presentation 仍由 settings snapshot 驱动：

- 显隐使用偏好设置已有的 `settings.overlay.hide_model_window.label`，开关和菜单 check item 都以
  “已隐藏”为选中值；启动默认未选中，因此模型窗口默认可见。
- 鼠标穿透、置顶和鼠标悬停隐藏分别复用偏好设置已有的
  `settings.overlay.click_through.label`、`settings.overlay.always_on_top.label` 和
  `settings.overlay.hide_on_mouse_hover.label`。
- 显隐是 runtime 会话状态，不写入 `config.json`；菜单与设置页共享同一个 runtime snapshot 投影。
- 若 check item 的 command 失败，应用重新读取当前 snapshot 并回写 presentation，避免 native menu
  保留用户点击产生的乐观勾选状态。

### 生命周期不变量

- `TrayIcon` 字段必须先于菜单根、`Submenu` 和菜单项声明；显式 shutdown 先隐藏托盘，再按既定顺序停止
  input/runtime/frame/renderer/overlay。
- overlay 右键只使用真实 Windows HWND 或 macOS content `NSView`，不借用托盘隐藏窗口，也不在 overlay
  内创建第二个 owner。
- 菜单根、模型窗口子菜单和所有更新/文字/勾选操作都在平台 UI 主线程执行。菜单 tracking 期间只允许把
  事件送入既有有界队列，不能在 callback 内销毁 `SystemMenu` 或 overlay。
- `muda` 和 `tray-icon` 的版本、features 与替换边界继续由 ADR-0031 固定；第三方类型不进入公共业务 API。

## 验证

- 平台 crate 单测固定当前 action id 集合，并断言显隐、穿透、置顶、鼠标悬停隐藏、设置、更新和退出
  action 均存在；已删除的源码、重启和版本 id 不会产生 action。
- 菜单布局 contract 断言托盘和模型窗口右键共用同一组根项，并且模型窗口子菜单包含四个 check item。
- locale 双向 key 守门确保 `navigation.model_window.title`、偏好设置已有的显隐/穿透/置顶/鼠标悬停文案，
  以及 About 已有的 `update.about.label` 可解析；菜单不新增重复文案 key。
- macOS/Windows release system-menu smoke 继续覆盖托盘显隐、设置恢复、显隐 action、runtime snapshot
  变化和有序退出；它不替代真实 popup 展开与点击验证。
- 发布前仍需在两个目标平台实机确认：托盘与模型窗口右键展示同一层级、子菜单展开、cursor 定位、
  DPI/Retina、点击外部关闭、菜单项 action 派发、Explorer/菜单栏恢复和 shutdown 清理。

## 替换边界

替换点只有 `crates/bongocat-platform/src/system_menu_native.rs` 与 overlay 的 `HasWindowHandle` 实现。
升级 `muda`/`tray-icon` 时必须重新验证共用菜单根、菜单项父级关系、析构顺序、同一个 `MenuEvent`
receiver、Windows GUID/tooltip 缺陷、主线程约束、macOS `NSView` 生命周期和两个平台的实机菜单层级；
不能退回由 overlay 持有菜单或把第三方类型泄漏到 runtime/UI。
