# ADR-0098: macOS 应用菜单与窗口动作

状态：已接受
依赖：ADR-0068（设置窗口销毁与进程内导航记忆）

## 决策

- 应用层通过 GPUI Kit 公开的菜单和 action API 安装 macOS 顶部菜单与快捷键；不新增依赖、
  全局快捷键注册或 AppKit 窗口管理实现。
- `⌘W` 关闭当前设置或更新窗口，设置复用 `SettingsView::close`，后台产品继续运行；
  `⌘Q` 复用 `request_product_quit`，先提交待保存设置，再执行产品 shutdown。
- 最小化、缩放、隐藏应用、隐藏其他应用与显示全部使用框架公开操作；`⌘,` 复用设置单例入口。
  窗口动作延后到事件分发结束执行，避免重入当前窗口借用。
- 编辑菜单复用组件已有 action，不复制输入处理；菜单及快捷键共用 action，文案跟随产品语言。
- 保留已有 Dock、应用激活、reopen 和窗口销毁规则。

## 依据与验证

GPUI Kit 0.7.3 明确由应用负责 quit/close actions，macOS 后端从 keymap 生成原生菜单快捷键。
来源：[GPUI Kit](https://docs.rs/gpui-kit/0.7.3/src/gpui_kit/lib.rs.html)、
[macOS 菜单后端](https://docs.rs/crate/gpui-pre-macos/0.3.9/source/src/platform.rs)。

验收范围包括本地化、设置关闭后销毁与重建、后台 frame loop 连续运行，以及 macOS 实机菜单、
窗口快捷键、输入框、弹窗和快捷键录制。其他平台不注册这些菜单与绑定。

已通过八种语言校验、`just check`、`just dev-smoke` 及 15 项 changelog/platform cfg
contract 测试；实机已验证中文菜单、
菜单关闭与隐藏、`⌘W`、`⌘Q`、`⌘M`、`⌘H` 和 `⌘,`，关闭后后台继续运行、重新开窗
与最小化恢复正常。smoke 曾输出 Metal drawable 警告，但正常退出。
更新窗口、输入框编辑菜单、弹窗与快捷键录制仍待完整人工验收。
