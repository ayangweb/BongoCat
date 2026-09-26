# ADR-0068: 设置窗口销毁与进程内导航记忆

状态：已接受（2026-09-25）
依赖：ADR-0002（GPUI 设置 UI）、ADR-0035（更新 worker 与更新窗口）

## 背景

设置窗口过去关闭后只隐藏，根 `Entity` 和 GPUI 窗口会一直保留到进程退出；更新窗口过去关闭后立即销毁。
两种行为不一致，也让不常打开的设置窗口长期保留窗口级资源。

设置窗口的侧边栏选择由 `gpui-kit` 的 `Settings` 组件保存在 window-keyed state 中。窗口销毁后这份
状态会随窗口一起释放；如果不做桥接，用户每次重新打开设置都会回到第一个页面。

## 决策

### 1. 设置和更新窗口关闭即销毁

- 设置窗口标题栏 close、设置窗口 toggle 和显式关闭路径统一销毁 GPUI 窗口；
- 更新窗口保持既有的关闭即销毁行为；
- 窗口销毁不停止 runtime、输入、音频、frame source、overlay 或更新 worker；
- 两次打开之间不保留窗口实例，下一次打开重新创建 GPUI window、根 `Entity` 和组件状态；
- Windows 任务栏按钮的期望可见性由 coordinator 保留，窗口不存在时先保存意图，下一次创建时应用，
  不因设置窗口已销毁而把合法的配置变更报告为失败。

### 2. 进程内记住设置侧边栏页面

`ProductCoordinator` 持有一个 `SettingsNavigationMemory`。它是进程内、非持久化的单一 owner：

1. 设置页面通过 `SettingPage::title_suffix` 的公开渲染回调报告当前一级页面；
2. 报告只更新 `SettingsNavigationMemory`，不把导航选择写入配置文件或 runtime snapshot；
3. 新设置窗口把 memory 中的 page index 传给 `Settings::default_selected_index`；
4. 应用重启重新创建 coordinator，导航回到 Appearance。

这样既利用了 `gpui-kit` 现有的 sidebar 行为，又不复制或接管第三方 `SettingsState`。导航记忆只覆盖
一级页面；页面内的草稿、搜索词和未完成表单仍随窗口销毁，不跨窗口保存。

### 3. Shutdown 与窗口回调

普通 close 的 UI 清理发生在窗口销毁前：取消快捷键捕获、清理模型拖放并请求 flush 待提交设置。
runtime、输入和后台 worker 的 shutdown 顺序不变。Windows GPUI 0.2.2 的最终 `WM_DESTROY` 兼容
退出路径仍只用于显式 Quit 后的产品 owner 关闭阶段；普通设置/更新窗口销毁必须由双平台原生 smoke
验证。

## 结果

- 不再长期保留不常打开的设置窗口资源；
- 更新窗口行为回到原有的即时销毁语义；
- 设置窗口仍能在同一进程内恢复上次一级侧边栏页面；
- 应用重启后不会恢复旧进程中的 UI 导航选择；
- 窗口级 GPUI state 不跨窗口保存，避免把草稿、搜索和异步操作意外带到下一次打开。

## 验证

- `SettingsNavigationMemory` contract 覆盖默认值、页面更新和 clone 后的进程内保持；
- 设置窗口 smoke 覆盖 close 后 handle/window 被清除、重新 open 使用新实例并恢复 snapshot；
- Windows/macOS 原生 smoke 仍需分别覆盖真实标题栏 close、窗口销毁、重建和侧边栏页面恢复；
- 显式 Quit 的双平台 shutdown 顺序和 GPUI `WM_DESTROY` 兼容门禁保持不变。
