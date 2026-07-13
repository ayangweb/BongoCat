# 右键菜单直达聊天设置 — 设计

日期：2026-07-10
分支：feat/ai-chat-bubble

## 目标

在猫咪右键菜单（及托盘菜单，两者共享 `getBaseMenu`）中新增一个"聊天设置"菜单项，点击后打开偏好设置窗口并直接定位到"聊天"Tab。

## 背景

- 右键/托盘菜单定义在 `src/composables/useAppMenu.ts`。
- 偏好设置窗口在应用启动时创建并常驻（隐藏），Tab 由 `src/pages/preference/index.vue` 的本地 `current` 索引控制，目前无法从外部指定。
- 代码库已有跨窗口事件模式：`LISTEN_KEY.SHOW_CHAT`、`UPDATE_CONFIG` 等通过 Tauri emit/listen 通信。

## 方案

沿用现有跨窗口事件模式（已否决 URL query 方案——偏好窗口 URL 固定不重载；已否决持久化 store 方案——一次性导航信号不该落盘）。

### 改动点

1. **`src/constants/index.ts`**
   新增 `LISTEN_KEY.NAVIGATE_PREFERENCE_TAB = 'navigate-preference-tab'`。

2. **`src/composables/useAppMenu.ts`** — `getBaseMenu`
   在"偏好设置..."之后新增菜单项：
   - 文案：`t('composables.useAppMenu.labels.chatSetting')`（"聊天设置"）
   - action：`showWindow(WINDOW_LABEL.PREFERENCE)` 后 `emit(LISTEN_KEY.NAVIGATE_PREFERENCE_TAB, 'chat')`

3. **`src/pages/preference/index.vue`**
   新增 `useTauriListen<string>(LISTEN_KEY.NAVIGATE_PREFERENCE_TAB, ...)`：
   `const index = menus.value.findIndex(m => m.key === payload)`，`index >= 0` 时 `current = index`（-1 时忽略）。

4. **国际化** — 5 个 locale 文件（zh-CN、zh-TW、en-US、pt-BR、vi-VN）新增
   `composables.useAppMenu.labels.chatSetting`：
   - zh-CN：聊天设置
   - zh-TW：聊天設定
   - en-US：Chat Settings
   - pt-BR：Configurações de chat
   - vi-VN：Cài đặt trò chuyện

## 错误处理

- payload 不匹配任何 tab key 时静默忽略（findIndex -1 守卫），无其他新错误路径。

## 验证

手动：右键猫咪 →"聊天设置"→ 偏好设置窗口打开且定位到聊天 Tab；托盘菜单同样生效；偏好窗口已打开但停在其他 Tab 时点击也能切换过去。
