# 猫咪头顶 AI 对话气泡 Implementation Plan

> **状态：✅ 已全部实现并通过评审（分支 `feat/ai-chat-bubble`）。** Task 1–9 全部完成，逐任务两段式评审 + 整支最终评审通过。
>
> **最终评审发现的 Critical 已修：** chat 窗口缺 `core:window:allow-show/hide` 权限（`daf2f96`）。
>
> **联机（真机）验证时另发现并修复（计划/静态评审看不出，需 GUI 才暴露）：**
> - `0f28e65` `main.currentMonitor()` 报 `TypeError`——`currentMonitor` 是 `@tauri-apps/api/window` 的独立函数，不是 `WebviewWindow`/`Window` 的方法；改用 `availableMonitors()` 按物理边界判定猫所在显示器。
> - `0e0171f` 气泡文字竖排（每行一个字）——气泡是 flex 子项，`width:max-content` 不能阻止 flex 收缩到 min-content；改为「带 padding 的 block wrapper（`w-max max-w-80 p-3`）」，横向排版、到上限再换行，padding（被测量计入）替代 margin 预留阴影。
> - `33bcf13` debug 构建下为 chat 窗口开 DevTools。
> - App.vue 的 `unhandledrejection` 处理改为输出 Error 的 name/message/stack（原来 `JSON.stringify(Error)` → `{}` 把错误吞了）。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Live2D 猫咪头顶用一个独立透明穿透窗口展示自动消失的对话气泡，文字由前端 `say()` 或本地 HTTP 接口推送，三端通用。

**Architecture:** 新增一个独立的 `chat` 窗口（`/chat` 路由），它是气泡生命周期的唯一权威：监听全局 `show-chat` 事件 → 渲染文字 → 量出真实尺寸 → 动态 `setSize` 自身 → 定位到猫咪正上方 → 淡入 → 定时淡出。主窗口几乎零改动（仅 macOS 把已有的 move/resize 重发改成广播，好让 chat 收到几何变化）。配置集中在新 pinia store `ai`，通过 `@tauri-store/pinia` 跨窗口同步。进程外推送由内嵌的 `tiny_http` 本地 HTTP server 提供，复用同一个 `show-chat` 事件。

**Tech Stack:** Tauri 2 + Vue 3 (`<script setup>`) + TypeScript + Pinia (`@tauri-store/pinia`) + antdv-next + UnoCSS；Rust 后端 (`tiny_http`, `form_urlencoded`, `tauri-plugin-pinia` 的 `ManagerExt`)。

## Global Constraints

- **平台**：三端都要（macOS / Windows / Linux）。macOS 用 NSPanel 处理层级；Windows/Linux 用原生 `WebviewWindow.getByLabel('main')` 监听。
- **总开关唯一生效点**：`!aiStore.ai.enabled` 的判断只在 chat 页 `showChat()` 开头做一次，所有生产者（前端 `say()` + HTTP）共用。
- **默认时长唯一兜底点**：`ms = duration ?? aiStore.ai.duration * 1000` 只在 chat 页做。`duration` 单位毫秒；`0` 表示常驻。
- **坐标系**：`getBoundingClientRect()` 是逻辑像素；`outerPosition()` / 显示器 bounds 是物理像素。定位前 bubble 逻辑尺寸 × **猫所在显示器** 的 `scaleFactor` 转物理；`setSize` 用 `LogicalSize`、`setPosition` 用 `PhysicalPosition`。
- **i18n**：所有新文案走 5 个语言包（zh-CN / zh-TW / en-US / vi-VN / pt-BR），不硬编码。
- **HTTP 安全**：只绑 `127.0.0.1`；开关默认 **关闭**（`httpEnabled=false`）；改端口/开关/token 需重启 app 生效（不做热重载）。
- 代码风格跟随现有文件：`<script setup lang="ts">`、`@/` 别名、`reactive`/`ref`、antdv-next 控件、`ProList`/`ProListItem`。
- Rust 文件缩进 4 空格、`#![allow(deprecated)]` 跟随现有；新依赖加到 `src-tauri/Cargo.toml` 的 `[dependencies]`。

---

## File Structure

新增：
- `src/stores/ai.ts` — 气泡全部配置（跨窗口同步）。
- `src/utils/chatPosition.ts` — 纯函数定位（可测，无 Tauri 依赖）。
- `src/utils/chatPosition.test.ts` — `node:assert` 自检，`npx tsx` 运行。
- `src/composables/useChat.ts` — `say()` 推送 API。
- `src/pages/chat/index.vue` — 气泡窗口页面（生命周期权威）。
- `src/pages/preference/components/ai/index.vue` — AI 设置 tab。
- `src-tauri/src/core/server.rs` — 本地 HTTP 推送 server。

修改：
- `src-tauri/tauri.conf.json` — 新增 chat 窗口。
- `src/router/index.ts` — 新增 `/chat` 路由。
- `src/constants/index.ts` — 新增 `LISTEN_KEY.SHOW_CHAT` 和 `WINDOW_LABEL.CHAT`。
- `src/App.vue` — 注册 `aiStore.$tauri.start()`。
- `src-tauri/src/core/setup/macos.rs` — chat NSPanel + move/resize 改广播。
- `src-tauri/src/core/mod.rs` — `pub mod server;`。
- `src-tauri/src/lib.rs` — setup 阶段启动 HTTP server。
- `src-tauri/Cargo.toml` — `+ tiny_http`、`+ form_urlencoded`。
- `src-tauri/capabilities/default.json` — `+ core:window:allow-current-monitor`（防御性）。
- `src/pages/preference/index.vue` — menus 加 AI tab。
- `src/pages/main/index.vue` — 模型加载后打一次招呼。
- `src/locales/*.json` — 5 个语言包补 key。

---

## Task 1: AI 配置 store

**Files:**
- Create: `src/stores/ai.ts`
- Modify: `src/App.vue:25-46`（import + `$tauri.start()`）

**Interfaces:**
- Produces: `useAiStore()` → `{ ai: AiStore['ai'] }`，其中
  ```ts
  ai: {
    enabled: boolean; duration: number
    textColor: string; fontSize: number
    bgColor: string; bgOpacity: number
    debug: boolean
    httpEnabled: boolean; httpPort: number; httpToken: string
  }
  ```
  store id 为 `'ai'`，持久化 key 为 `'ai'`（Rust 端用 `with_store("ai", |s| s.try_get::<...>("ai"))` 读取）。

- [x] **Step 1: 写 store 文件**

Create `src/stores/ai.ts`（结构对齐 `src/stores/cat.ts`）：

```ts
import { defineStore } from 'pinia'
import { reactive } from 'vue'

export interface AiStore {
  ai: {
    enabled: boolean
    duration: number
    textColor: string
    fontSize: number
    bgColor: string
    bgOpacity: number
    debug: boolean
    httpEnabled: boolean
    httpPort: number
    httpToken: string
  }
}

export const useAiStore = defineStore('ai', () => {
  const ai = reactive<AiStore['ai']>({
    enabled: true,
    duration: 3,
    textColor: '#333',
    fontSize: 14,
    bgColor: '#fff',
    bgOpacity: 90,
    debug: false,
    httpEnabled: false,
    httpPort: 7800,
    httpToken: '',
  })

  return {
    ai,
  }
})
```

- [x] **Step 2: 在 App.vue 注册 store 同步**

修改 `src/App.vue`。在 import 区（约 19-23 行附近）加：

```ts
import { useAiStore } from './stores/ai'
```

在 `const shortcutStore = useShortcutStore()` 下面加：

```ts
const aiStore = useAiStore()
```

在 `onMounted` 里 `await shortcutStore.$tauri.start()` 之后加一行：

```ts
  await aiStore.$tauri.start()
```

- [x] **Step 3: 类型检查 + lint**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm lint`
Expected: 无 `src/stores/ai.ts` / `src/App.vue` 相关报错。

- [x] **Step 4: Commit**

```bash
git add src/stores/ai.ts src/App.vue
git commit -m "feat(ai): add ai config store with cross-window sync"
```

---

## Task 2: 气泡定位纯函数（TDD）

**Files:**
- Create: `src/utils/chatPosition.ts`
- Test: `src/utils/chatPosition.test.ts`

**Interfaces:**
- Produces:
  ```ts
  interface Rect { x: number, y: number, w: number, h: number }
  interface Size { w: number, h: number }
  function computeBubblePosition(
    main: Rect, bubble: Size, screen: Rect, gap: number,
  ): { x: number, y: number }
  ```
  全部入参/出参为 **物理像素**。`main` = 主窗口外框；`bubble` = 气泡物理尺寸；`screen` = 猫所在显示器 bounds；`gap` = 气泡与猫的间距（物理）。

- [x] **Step 1: 写失败的自检测试**

Create `src/utils/chatPosition.test.ts`：

```ts
import assert from 'node:assert'

import { computeBubblePosition } from './chatPosition'

// 屏幕：原点在 (0,0)，1920x1080
const screen = { x: 0, y: 0, w: 1920, h: 1080 }
const gap = 10

// 1. 正常居中：猫在屏幕中央 → x 水平居中、y 在猫上方
{
  const main = { x: 860, y: 490, w: 200, h: 200 }
  const bubble = { w: 100, h: 60 }
  const { x, y } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 860 + (200 - 100) / 2) // 910
  assert.strictEqual(y, 490 - 60 - 10) // 420
}

// 2. 贴左边缘 → x 夹到 screen.x
{
  const main = { x: 0, y: 490, w: 200, h: 200 }
  const bubble = { w: 400, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 0)
}

// 3. 贴右边缘 → x 夹到 screen.x + screen.w - bubble.w
{
  const main = { x: 1820, y: 490, w: 100, h: 200 }
  const bubble = { w: 300, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 1920 - 300) // 1620
}

// 4. 贴顶部、上方放不下 → 翻到猫咪下方
{
  const main = { x: 860, y: 0, w: 200, h: 200 }
  const bubble = { w: 100, h: 60 }
  const { y } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(y, 0 + 200 + 10) // 210
}

// 5. 气泡比猫宽 → 仍以猫中心对齐
{
  const main = { x: 900, y: 490, w: 100, h: 200 }
  const bubble = { w: 300, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 900 + (100 - 300) / 2) // 800
}

// 6. 气泡比屏幕还宽 → 夹取后取 screen.x，不越界
{
  const main = { x: 860, y: 490, w: 200, h: 200 }
  const bubble = { w: 3000, h: 60 }
  const { x } = computeBubblePosition(main, bubble, screen, gap)
  assert.strictEqual(x, 0)
}

// 7. 多显示器：猫在副屏（负坐标偏移）→ 用副屏 bounds 夹取
{
  const sub = { x: -1920, y: 0, w: 1920, h: 1080 }
  const main = { x: -1920, y: 490, w: 200, h: 200 } // 贴副屏左边
  const bubble = { w: 400, h: 60 }
  const { x } = computeBubblePosition(main, bubble, sub, gap)
  assert.strictEqual(x, -1920)
}

// 8. DPI=2：调用方传入的已是物理像素，函数结果应为物理坐标
{
  const main = { x: 1720, y: 980, w: 400, h: 400 } // 物理（逻辑×2）
  const bubble = { w: 200, h: 120 } // 物理（逻辑 100x60 ×2）
  const hidpi = { x: 0, y: 0, w: 3840, h: 2160 }
  const { x, y } = computeBubblePosition(main, bubble, hidpi, 20)
  assert.strictEqual(x, 1720 + (400 - 200) / 2) // 1820
  assert.strictEqual(y, 980 - 120 - 20) // 840
}

console.log('chatPosition: all assertions passed')
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && npx tsx src/utils/chatPosition.test.ts`
Expected: FAIL，报错类似 `Cannot find module './chatPosition'` 或 `computeBubblePosition is not a function`。

- [x] **Step 3: 写最小实现**

Create `src/utils/chatPosition.ts`：

```ts
export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

export interface Size {
  w: number
  h: number
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(value, max))
}

/**
 * 计算气泡左上角的物理像素坐标。
 * 所有入参均为物理像素：main=主窗口外框，bubble=气泡物理尺寸，
 * screen=猫所在显示器 bounds，gap=气泡与猫的间距。
 */
export function computeBubblePosition(main: Rect, bubble: Size, screen: Rect, gap: number) {
  let x = main.x + (main.w - bubble.w) / 2
  let y = main.y - bubble.h - gap

  x = clamp(x, screen.x, screen.x + screen.w - bubble.w)

  if (y < screen.y) {
    y = main.y + main.h + gap
  }

  return { x, y }
}
```

- [x] **Step 4: 运行测试确认通过**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && npx tsx src/utils/chatPosition.test.ts`
Expected: PASS，输出 `chatPosition: all assertions passed`。

- [x] **Step 5: Commit**

```bash
git add src/utils/chatPosition.ts src/utils/chatPosition.test.ts
git commit -m "feat(ai): add chat bubble position pure function with self-check"
```

---

## Task 3: 推送 API `say()` + 常量

**Files:**
- Modify: `src/constants/index.ts:5-13`（`LISTEN_KEY`）、`:30-33`（`WINDOW_LABEL`）
- Create: `src/composables/useChat.ts`

**Interfaces:**
- Consumes: 无。
- Produces:
  - `LISTEN_KEY.SHOW_CHAT = 'show-chat'`，`WINDOW_LABEL.CHAT = 'chat'`。
  - `say(text: string, duration?: number): Promise<void>`（`duration` 单位毫秒，省略时由 chat 页兜底默认值）。

- [x] **Step 1: 加常量**

修改 `src/constants/index.ts`。在 `LISTEN_KEY` 对象里加一项（放在 `SET_EXPRESSION` 后）：

```ts
  SET_EXPRESSION: 'set-expression',
  SHOW_CHAT: 'show-chat',
```

在 `WINDOW_LABEL` 对象里加一项：

```ts
export const WINDOW_LABEL = {
  MAIN: 'main',
  PREFERENCE: 'preference',
  CHAT: 'chat',
} as const
```

- [x] **Step 2: 写 useChat composable**

Create `src/composables/useChat.ts`：

```ts
import { emit } from '@tauri-apps/api/event'

import { LISTEN_KEY } from '@/constants'

/**
 * 全局广播一条气泡。任意页面/composable 可调用。
 * 总开关 / 默认时长 / 定位 / 动画全部由 chat 页统一处理（见 src/pages/chat/index.vue）。
 * @param duration 毫秒；省略时由 chat 页用默认时长兜底；0 表示常驻。
 */
export function say(text: string, duration?: number) {
  return emit(LISTEN_KEY.SHOW_CHAT, { text, duration })
}
```

- [x] **Step 3: 类型检查 + lint**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm lint`
Expected: 无 `useChat.ts` / `constants` 相关报错。

- [x] **Step 4: Commit**

```bash
git add src/constants/index.ts src/composables/useChat.ts
git commit -m "feat(ai): add say() push api and chat constants"
```

---

## Task 4: 注册 chat 窗口 + 路由 + 页面骨架

**Files:**
- Modify: `src-tauri/tauri.conf.json`（`windows` 数组）
- Modify: `src/router/index.ts`
- Create: `src/pages/chat/index.vue`（本任务只做最小骨架，下一任务补全逻辑）

**Interfaces:**
- Consumes: `WINDOW_LABEL.CHAT`（Task 3）。
- Produces: label 为 `chat`、URL `index.html/#/chat` 的透明穿透窗口，挂载即鼠标穿透。

- [x] **Step 1: 在 tauri.conf.json 加 chat 窗口**

修改 `src-tauri/tauri.conf.json`，在 `windows` 数组里 `preference` 窗口对象之后追加（注意前一个对象末尾补逗号）：

```json
      {
        "label": "chat",
        "url": "index.html/#/chat",
        "width": 200,
        "height": 100,
        "visible": false,
        "transparent": true,
        "decorations": false,
        "shadow": false,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "resizable": false,
        "maximizable": false,
        "focus": false
      }
```

- [x] **Step 2: 加 /chat 路由**

修改 `src/router/index.ts`。加 import：

```ts
import Chat from '../pages/chat/index.vue'
```

在 `routes` 数组里加一项：

```ts
  {
    path: '/chat',
    component: Chat,
  },
```

- [x] **Step 3: 写 chat 页面骨架**

Create `src/pages/chat/index.vue`：

```vue
<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { onMounted } from 'vue'

const appWindow = getCurrentWebviewWindow()

onMounted(() => {
  // 整窗鼠标穿透：透明空白区点不到
  appWindow.setIgnoreCursorEvents(true)
})
</script>

<template>
  <div class="size-screen" />
</template>
```

- [x] **Step 4: 跑起来确认 chat 窗口已注册（不报错、main 仍正常）**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`
Expected: app 正常启动，猫咪正常显示；无 "window label chat" 相关报错。chat 窗口 `visible:false` 所以看不到，正常。确认无误后 `Ctrl-C` 退出。

- [x] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json src/router/index.ts src/pages/chat/index.vue
git commit -m "feat(ai): register chat window, route and page skeleton"
```

---

## Task 5: chat 页面完整生命周期

**Files:**
- Modify: `src/pages/chat/index.vue`（全量替换 Task 4 的骨架）

**Interfaces:**
- Consumes: `useAiStore()`（Task 1）、`computeBubblePosition`（Task 2）、`LISTEN_KEY.SHOW_CHAT` / `WINDOW_LABEL.MAIN`（Task 3）。
- Produces: 监听 `show-chat {text, duration?}` 的完整气泡渲染/测量/定位/计时/动画。

- [x] **Step 1: 全量替换 chat 页面**

把 `src/pages/chat/index.vue` 整个替换为：

```vue
<script setup lang="ts">
import { LogicalSize, PhysicalPosition } from '@tauri-apps/api/dpi'
import { TauriEvent } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { computed, nextTick, onMounted, ref, watch } from 'vue'

import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY, WINDOW_LABEL } from '@/constants'
import { computeBubblePosition } from '@/utils/chatPosition'
import { isMac } from '@/utils/platform'
import { useAiStore } from '@/stores/ai'

interface ShowChatPayload {
  text: string
  duration?: number
}

const GAP = 8 // 气泡与猫的间距（逻辑像素），定位时 × scaleFactor 转物理

const appWindow = getCurrentWebviewWindow()
const aiStore = useAiStore()
const bubbleRef = ref<HTMLElement>()
const text = ref('')
const visible = ref(false)

let timer: ReturnType<typeof setTimeout> | undefined

// 气泡背景：hex + 透明度(0-100) 合成 rgba；窗口本身始终透明
function hexToRgba(hex: string, opacity: number) {
  const value = hex.replace('#', '')
  const full = value.length === 3 ? value.split('').map(c => c + c).join('') : value
  const r = Number.parseInt(full.slice(0, 2), 16)
  const g = Number.parseInt(full.slice(2, 4), 16)
  const b = Number.parseInt(full.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${opacity / 100})`
}

const bgRgba = computed(() => hexToRgba(aiStore.ai.bgColor, aiStore.ai.bgOpacity))

const bubbleStyle = computed(() => ({
  color: aiStore.ai.textColor,
  fontSize: `${aiStore.ai.fontSize}px`,
  background: bgRgba.value,
}))

const triangleStyle = computed(() => ({
  borderTopColor: bgRgba.value,
}))

async function resize() {
  await nextTick()

  const el = bubbleRef.value
  if (!el) return

  const rect = el.getBoundingClientRect()

  await appWindow.setSize(new LogicalSize(Math.ceil(rect.width), Math.ceil(rect.height)))
}

async function reposition() {
  const el = bubbleRef.value
  if (!visible.value || !el) return

  const main = await WebviewWindow.getByLabel(WINDOW_LABEL.MAIN)
  if (!main) return

  const [position, size, monitor] = await Promise.all([
    main.outerPosition(),
    main.outerSize(),
    main.currentMonitor(),
  ])

  if (!monitor) return

  const sf = monitor.scaleFactor
  const rect = el.getBoundingClientRect()

  const { x, y } = computeBubblePosition(
    { x: position.x, y: position.y, w: size.width, h: size.height },
    { w: rect.width * sf, h: rect.height * sf },
    { x: monitor.position.x, y: monitor.position.y, w: monitor.size.width, h: monitor.size.height },
    GAP * sf,
  )

  await appWindow.setPosition(new PhysicalPosition(Math.round(x), Math.round(y)))
}

function hide() {
  visible.value = false // 触发淡出；@after-leave 里再 appWindow.hide()
}

async function showChat({ text: nextText, duration }: ShowChatPayload) {
  // 总开关唯一生效点
  if (!aiStore.ai.enabled) return

  text.value = nextText
  visible.value = true

  await resize()
  await reposition()
  await appWindow.show()

  // 默认时长唯一兜底点；0 表示常驻
  const ms = duration ?? aiStore.ai.duration * 1000

  clearTimeout(timer)

  if (ms > 0) {
    timer = setTimeout(hide, ms)
  }
}

onMounted(async () => {
  await appWindow.setIgnoreCursorEvents(true)

  if (isMac) {
    // macOS：NSPanel 不触发原生 move/resize，由 macos.rs 广播 tauri://move / tauri://resize
    appWindow.listen(TauriEvent.WINDOW_MOVED, reposition)
    appWindow.listen(TauriEvent.WINDOW_RESIZED, reposition)
  } else {
    // Windows/Linux：原生监听主窗口几何变化
    const main = await WebviewWindow.getByLabel(WINDOW_LABEL.MAIN)
    main?.onMoved(reposition)
    main?.onResized(reposition)
  }
})

useTauriListen<ShowChatPayload>(LISTEN_KEY.SHOW_CHAT, ({ payload }) => {
  showChat(payload)
})

// 字号改变会改变气泡尺寸：可见时重新测量并定位
watch(() => aiStore.ai.fontSize, async () => {
  if (!visible.value) return
  await resize()
  await reposition()
})
</script>

<template>
  <div class="size-screen flex items-end justify-center overflow-hidden">
    <Transition
      name="fade"
      @after-leave="appWindow.hide()"
    >
      <div
        v-show="visible"
        ref="bubbleRef"
        class="relative m-3 max-w-80 w-max whitespace-pre-wrap break-words rounded-2xl px-3 py-2 leading-relaxed shadow-lg"
        :style="bubbleStyle"
      >
        {{ text }}

        <span
          class="absolute left-1/2 top-full h-0 w-0 b-6 b-solid b-transparent -translate-x-1/2"
          :style="triangleStyle"
        />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
```

> 说明：`bubbleRef` 外层用 `m-3`（margin）为阴影预留空间，`getBoundingClientRect` 不含 box-shadow，靠 margin 让窗口尺寸留白，窗口透明所以留白不可见。三角用 border 画，`top-full` 贴在气泡底部正中指向猫咪。

- [x] **Step 2: 手动验证基本展示（借后续 DEBUG 测试区前，先用临时招呼验证）**

临时验证：在 `src/pages/chat/index.vue` 的 `onMounted` 末尾临时加一行 `setTimeout(() => showChat({ text: '你好呀~测试一条比较长的文字看看换行' }), 2000)`，然后：

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`
Expected: 启动 ~2 秒后，猫咪头顶冒出气泡，3 秒后淡出消失。位置在猫正上方居中。

确认后 **删除这行临时代码**。`Ctrl-C` 退出。

- [x] **Step 3: lint**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm lint`
Expected: 无 `chat/index.vue` 相关报错。

- [x] **Step 4: Commit**

```bash
git add src/pages/chat/index.vue
git commit -m "feat(ai): full chat bubble lifecycle (measure, size, position, timer, fade)"
```

---

## Task 6: macOS NSPanel + move/resize 广播

**Files:**
- Modify: `src-tauri/src/core/setup/macos.rs`

**Interfaces:**
- Consumes: `tauri.conf.json` 里 label 为 `chat` 的窗口（Task 4）。
- Produces:
  - chat 窗口转为 NSPanel，与猫同 level（Dock）+ 同 collection behavior，`non_activating` / `can_become_key=false`。
  - 主窗口的 `tauri://move` / `tauri://resize` 改为 **广播**（`emit`），使 chat 能收到主窗口几何变化。

- [x] **Step 1: macos.rs 签名加 chat 窗口参数 + 改广播 + 建 chat panel**

修改 `src-tauri/src/core/setup/macos.rs`。

(a) 改 `platform` 签名，新增 `chat_window` 参数（在 `_preference_window` 之后）：

```rust
pub fn platform(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    _preference_window: WebviewWindow,
    chat_window: WebviewWindow,
) {
```

(b) 把 `emit_position` 内的 `emit_to(target, ...)` 改成广播 `emit`，并把 resize 分支里给 main 的 `emit_to` 也改成广播。替换原 `fn emit_position` 与 `window_did_resize` 两处：

原：
```rust
    fn emit_position(window: &WebviewWindow) {
        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        if let Ok(position) = window.outer_position() {
            let _ = window.emit_to(target, WINDOW_MOVED_EVENT, position);
        }
    }

    let resize_window = main_window.clone();
    handler.window_did_resize(move |_| {
        emit_position(&resize_window);

        let target = EventTarget::labeled(MAIN_WINDOW_LABEL);

        if let Ok(size) = resize_window.inner_size() {
            let _ = resize_window.emit_to(target, WINDOW_RESIZED_EVENT, size);
        }
    });
```

改为：
```rust
    // 广播给所有窗口（含 chat），使 chat 能跟随主窗口移动/缩放
    fn emit_position(window: &WebviewWindow) {
        if let Ok(position) = window.outer_position() {
            let _ = window.emit(WINDOW_MOVED_EVENT, position);
        }
    }

    let resize_window = main_window.clone();
    handler.window_did_resize(move |_| {
        emit_position(&resize_window);

        if let Ok(size) = resize_window.inner_size() {
            let _ = resize_window.emit(WINDOW_RESIZED_EVENT, size);
        }
    });
```

> `window.emit(...)` 走 `Emitter` trait（已 `use tauri::Emitter`），广播到所有 webview。focus/blur 两处保持 `emit_to(main)` 不动（只有 main 需要）。`EventTarget` import 若变为未使用，保留即可（focus/blur 仍用到）。

(c) 在 `panel.set_event_handler(Some(handler.as_ref()));` 之前，新增 chat 窗口转 NSPanel 的代码：

```rust
    // chat 窗口：与猫同 level + 同 collection behavior，永不抢焦点
    if let Ok(chat_panel) = chat_window.to_panel::<NsPanel>() {
        chat_panel.set_level(PanelLevel::Dock.value());

        chat_panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());

        chat_panel.set_collection_behavior(
            CollectionBehavior::new()
                .stationary()
                .move_to_active_space()
                .full_screen_auxiliary()
                .into(),
        );
    }
    // ponytail: 同层 order-front（chat 创建晚于 main，show 时在猫之上）；若层级不准再抬高 PanelLevel
```

- [x] **Step 2: setup/mod.rs 与 common.rs 传入 chat 窗口**

修改 `src-tauri/src/core/setup/mod.rs`，`default` 签名加 `chat_window`，并透传：

```rust
pub fn default(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    preference_window: WebviewWindow,
    chat_window: WebviewWindow,
) {
    #[cfg(debug_assertions)]
    main_window.open_devtools();

    platform(
        app_handle,
        main_window.clone(),
        preference_window.clone(),
        chat_window.clone(),
    );
}
```

修改 `src-tauri/src/core/setup/common.rs`，给非 mac 平台的 `platform` 加同名参数（不使用）：

```rust
pub fn platform(
    _app_handle: &AppHandle,
    _main_window: WebviewWindow,
    _preference_window: WebviewWindow,
    _chat_window: WebviewWindow,
) {
}
```

- [x] **Step 3: lib.rs 取 chat 窗口并传入 setup**

修改 `src-tauri/src/lib.rs`，在 `let preference_window = ...` 之后、`setup::default(...)` 调用处：

```rust
            let chat_window = app.get_webview_window("chat").unwrap();

            setup::default(
                &app_handle,
                main_window.clone(),
                preference_window.clone(),
                chat_window.clone(),
            );
```

- [x] **Step 4: 编译验证**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat/src-tauri && cargo build`
Expected: 编译通过（warnings 可接受）。

- [x] **Step 5: macOS 上手动验证层级 + 跟随**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`（在 macOS 上）
临时在 chat 页 onMounted 加 `setTimeout(() => showChat({ text: '层级测试', duration: 0 }), 1500)`（`duration:0` 常驻便于观察），验证：
- 气泡浮在猫之上、不被裁切。
- 拖动猫咪 → 气泡跟随（允许极轻微延迟）。

确认后删除临时代码，`Ctrl-C` 退出。

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/core/setup/macos.rs src-tauri/src/core/setup/mod.rs src-tauri/src/core/setup/common.rs src-tauri/src/lib.rs
git commit -m "feat(ai): macos chat NSPanel + broadcast main move/resize to chat"
```

---

## Task 7: AI 设置 tab + i18n

**Files:**
- Create: `src/pages/preference/components/ai/index.vue`
- Modify: `src/pages/preference/index.vue`（import + menus）
- Modify: `src/locales/zh-CN.json` / `zh-TW.json` / `en-US.json` / `vi-VN.json` / `pt-BR.json`

**Interfaces:**
- Consumes: `useAiStore()`（Task 1）、`say()`（Task 3）、`ProList` / `ProListItem`。
- Produces: preference 窗口新增「AI」tab，含全部配置 + DEBUG 测试区。

- [x] **Step 1: 5 个语言包补 key**

每个文件在 `pages.preference` 对象内加一个 `ai` 子对象，并在 `pages.main` 内加 `greeting`。

`src/locales/zh-CN.json` — `pages.main` 加 `"greeting": "你好呀~"`；`pages.preference` 加：

```json
    "ai": {
      "title": "AI",
      "labels": {
        "basic": "气泡设置",
        "enabled": "启用气泡",
        "duration": "默认展示时长",
        "textColor": "文字颜色",
        "fontSize": "文字大小",
        "bgColor": "气泡底色",
        "bgOpacity": "底色透明度",
        "http": "HTTP 接口",
        "httpEnabled": "启用 HTTP 接口",
        "httpPort": "监听端口",
        "httpToken": "校验 Token",
        "debug": "调试模式",
        "testText": "测试文本",
        "testShow": "展示"
      },
      "hints": {
        "enabled": "总开关，关闭后所有气泡都不显示。",
        "http": "开启一个本地 HTTP 接口，供外部工具（如 curl）推送气泡。仅监听 127.0.0.1。",
        "httpRestart": "改动端口/开关/Token 后需重启应用生效。",
        "debug": "开启后展开下方测试区，可手动触发气泡用于验证。"
      }
    }
```

`src/locales/zh-TW.json` — `pages.main.greeting`: `"你好呀~"`；`pages.preference.ai`：

```json
    "ai": {
      "title": "AI",
      "labels": {
        "basic": "氣泡設定",
        "enabled": "啟用氣泡",
        "duration": "預設顯示時長",
        "textColor": "文字顏色",
        "fontSize": "文字大小",
        "bgColor": "氣泡底色",
        "bgOpacity": "底色透明度",
        "http": "HTTP 介面",
        "httpEnabled": "啟用 HTTP 介面",
        "httpPort": "監聽連接埠",
        "httpToken": "驗證 Token",
        "debug": "除錯模式",
        "testText": "測試文字",
        "testShow": "顯示"
      },
      "hints": {
        "enabled": "總開關，關閉後所有氣泡都不顯示。",
        "http": "開啟一個本機 HTTP 介面，供外部工具（如 curl）推送氣泡。僅監聽 127.0.0.1。",
        "httpRestart": "變更連接埠/開關/Token 後需重新啟動應用程式才會生效。",
        "debug": "開啟後展開下方測試區，可手動觸發氣泡用於驗證。"
      }
    }
```

`src/locales/en-US.json` — `pages.main.greeting`: `"Hi there~"`；`pages.preference.ai`：

```json
    "ai": {
      "title": "AI",
      "labels": {
        "basic": "Bubble settings",
        "enabled": "Enable bubble",
        "duration": "Default duration",
        "textColor": "Text color",
        "fontSize": "Font size",
        "bgColor": "Bubble color",
        "bgOpacity": "Background opacity",
        "http": "HTTP endpoint",
        "httpEnabled": "Enable HTTP endpoint",
        "httpPort": "Listen port",
        "httpToken": "Auth token",
        "debug": "Debug mode",
        "testText": "Test text",
        "testShow": "Show"
      },
      "hints": {
        "enabled": "Master switch. When off, no bubble is shown.",
        "http": "Expose a local HTTP endpoint so external tools (e.g. curl) can push bubbles. Bound to 127.0.0.1 only.",
        "httpRestart": "Changing port/switch/token requires an app restart to take effect.",
        "debug": "Expands the test area below for manually triggering a bubble."
      }
    }
```

`src/locales/vi-VN.json` — `pages.main.greeting`: `"Xin chào~"`；`pages.preference.ai`：

```json
    "ai": {
      "title": "AI",
      "labels": {
        "basic": "Cài đặt bong bóng",
        "enabled": "Bật bong bóng",
        "duration": "Thời lượng mặc định",
        "textColor": "Màu chữ",
        "fontSize": "Cỡ chữ",
        "bgColor": "Màu nền bong bóng",
        "bgOpacity": "Độ trong suốt nền",
        "http": "Giao diện HTTP",
        "httpEnabled": "Bật giao diện HTTP",
        "httpPort": "Cổng lắng nghe",
        "httpToken": "Token xác thực",
        "debug": "Chế độ gỡ lỗi",
        "testText": "Văn bản thử",
        "testShow": "Hiển thị"
      },
      "hints": {
        "enabled": "Công tắc tổng. Khi tắt, không bong bóng nào hiển thị.",
        "http": "Mở một giao diện HTTP cục bộ để công cụ ngoài (vd: curl) đẩy bong bóng. Chỉ lắng nghe 127.0.0.1.",
        "httpRestart": "Đổi cổng/công tắc/token cần khởi động lại ứng dụng để có hiệu lực.",
        "debug": "Mở khu vực thử bên dưới để kích hoạt bong bóng thủ công."
      }
    }
```

`src/locales/pt-BR.json` — `pages.main.greeting`: `"Olá~"`；`pages.preference.ai`：

```json
    "ai": {
      "title": "AI",
      "labels": {
        "basic": "Configurações do balão",
        "enabled": "Ativar balão",
        "duration": "Duração padrão",
        "textColor": "Cor do texto",
        "fontSize": "Tamanho da fonte",
        "bgColor": "Cor do balão",
        "bgOpacity": "Opacidade do fundo",
        "http": "Endpoint HTTP",
        "httpEnabled": "Ativar endpoint HTTP",
        "httpPort": "Porta de escuta",
        "httpToken": "Token de autenticação",
        "debug": "Modo de depuração",
        "testText": "Texto de teste",
        "testShow": "Mostrar"
      },
      "hints": {
        "enabled": "Interruptor geral. Quando desligado, nenhum balão é exibido.",
        "http": "Expõe um endpoint HTTP local para ferramentas externas (ex.: curl) enviarem balões. Vinculado apenas a 127.0.0.1.",
        "httpRestart": "Alterar porta/interruptor/token exige reiniciar o app para ter efeito.",
        "debug": "Expande a área de teste abaixo para disparar um balão manualmente."
      }
    }
```

> 校验 JSON 合法：`node -e "require('./src/locales/zh-CN.json')"`（对 5 个文件各跑一次，不报错即合法）。

- [x] **Step 2: 写 AI 设置组件**

Create `src/pages/preference/components/ai/index.vue`：

```vue
<script setup lang="ts">
import { Button, ColorPicker, Flex, Input, InputNumber, InputPassword, SpaceAddon, SpaceCompact, Slider, Switch } from 'antdv-next'
import { ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { say } from '@/composables/useChat'
import { useAiStore } from '@/stores/ai'

const aiStore = useAiStore()
const testText = ref('你好呀~')

function handleTest() {
  say(testText.value)
}
</script>

<template>
  <ProList :title="$t('pages.preference.ai.labels.basic')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.enabled')"
      :title="$t('pages.preference.ai.labels.enabled')"
    >
      <Switch v-model:checked="aiStore.ai.enabled" />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.duration')">
      <SpaceCompact>
        <InputNumber
          v-model:value="aiStore.ai.duration"
          class="w-20"
          :min="0"
        />

        <SpaceAddon>s</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.textColor')">
      <ColorPicker v-model:value="aiStore.ai.textColor" />
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.fontSize')">
      <SpaceCompact>
        <InputNumber
          v-model:value="aiStore.ai.fontSize"
          class="w-20"
          :max="64"
          :min="8"
        />

        <SpaceAddon>px</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem :title="$t('pages.preference.ai.labels.bgColor')">
      <ColorPicker v-model:value="aiStore.ai.bgColor" />
    </ProListItem>

    <ProListItem
      :title="$t('pages.preference.ai.labels.bgOpacity')"
      vertical
    >
      <Slider
        v-model:value="aiStore.ai.bgOpacity"
        class="m-0!"
        :max="100"
        :min="0"
        :tooltip="{
          formatter(value) {
            return `${value}%`
          },
        }"
      />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.ai.labels.http')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.http')"
      :title="$t('pages.preference.ai.labels.httpEnabled')"
    >
      <Switch v-model:checked="aiStore.ai.httpEnabled" />
    </ProListItem>

    <template v-if="aiStore.ai.httpEnabled">
      <ProListItem
        :description="$t('pages.preference.ai.hints.httpRestart')"
        :title="$t('pages.preference.ai.labels.httpPort')"
      >
        <InputNumber
          v-model:value="aiStore.ai.httpPort"
          class="w-28"
          :max="65535"
          :min="1024"
        />
      </ProListItem>

      <ProListItem :title="$t('pages.preference.ai.labels.httpToken')">
        <InputPassword
          v-model:value="aiStore.ai.httpToken"
          class="w-48"
        />
      </ProListItem>

      <ProListItem title="curl">
        <code class="select-all break-all text-3 color-text-tertiary">
          curl "http://127.0.0.1:{{ aiStore.ai.httpPort }}/say?text=hi"
        </code>
      </ProListItem>
    </template>
  </ProList>

  <ProList :title="$t('pages.preference.ai.labels.debug')">
    <ProListItem
      :description="$t('pages.preference.ai.hints.debug')"
      :title="$t('pages.preference.ai.labels.debug')"
    >
      <Switch v-model:checked="aiStore.ai.debug" />
    </ProListItem>

    <ProListItem
      v-if="aiStore.ai.debug"
      :title="$t('pages.preference.ai.labels.testText')"
    >
      <Flex :gap="8">
        <Input
          v-model:value="testText"
          class="w-48"
          @press-enter="handleTest"
        />

        <Button
          type="primary"
          @click="handleTest"
        >
          {{ $t('pages.preference.ai.labels.testShow') }}
        </Button>
      </Flex>
    </ProListItem>
  </ProList>
</template>
```

> `ColorPicker` 用 antdv-next 自带。`// ponytail`: 若该版本无 `ColorPicker` 导出，回退 `<input type="color" v-model="aiStore.ai.textColor">`。

- [x] **Step 3: preference 页加 AI tab**

修改 `src/pages/preference/index.vue`。import 区加：

```ts
import Ai from './components/ai/index.vue'
```

在 `menus` 数组里、`shortcut` 与 `about` 之间加一项：

```ts
  {
    key: 'ai',
    label: t('pages.preference.ai.title'),
    icon: 'i-solar:chat-round-bold',
    component: Ai,
  },
```

- [x] **Step 4: 跑起来验证设置页 + DEBUG 测试**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`
打开偏好设置（托盘/右键菜单）→「AI」tab：
- 改文字颜色/底色/透明度/字号控件正常。
- 打开 DEBUG → 输入文本点「展示」→ 猫咪头顶冒出气泡。
- 改字号后再次展示，气泡尺寸自适应、仍居中贴猫头顶。
- 打开 HTTP 开关 → 出现端口/Token/curl 示例与「需重启」提示。

确认后 `Ctrl-C` 退出。

- [x] **Step 5: lint**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm lint`
Expected: 无相关报错。

- [x] **Step 6: Commit**

```bash
git add src/pages/preference/components/ai/index.vue src/pages/preference/index.vue src/locales
git commit -m "feat(ai): add AI settings tab with debug test area and i18n"
```

---

## Task 8: 模型加载后打招呼

**Files:**
- Modify: `src/pages/main/index.vue`

**Interfaces:**
- Consumes: `say()`（Task 3）、`pages.main.greeting`（Task 7）。
- Produces: 首次模型加载完成后调用一次 `say(greeting)`（受 `ai.enabled` 控制，由 chat 页判断）。

- [x] **Step 1: 主页面加首次招呼**

修改 `src/pages/main/index.vue`。

import 区加：

```ts
import { say } from '@/composables/useChat'
```

在 `<script setup>` 顶部变量区（如 `const resizing = ref(false)` 附近）加一个一次性标志：

```ts
let greeted = false
```

在 `currentModel` 的 watch 里，把末尾的 `modelStore.modelReady = true` 替换为：

```ts
  modelStore.modelReady = true

  if (!greeted) {
    greeted = true
    say(t('pages.main.greeting'))
  }
```

并确保 `t` 已可用——`src/pages/main/index.vue` 当前未引入 `useI18n`，在 import 区加：

```ts
import { useI18n } from 'vue-i18n'
```

在 `const generalStore = useGeneralStore()` 附近加：

```ts
const { t } = useI18n()
```

- [x] **Step 2: 跑起来验证启动即招呼**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`
Expected: app 启动、模型加载完成后，猫咪头顶自动冒出一句招呼（默认 3 秒后消失）。把 AI 设置里 `enabled` 关掉重启 → 不再招呼。

确认后 `Ctrl-C` 退出。

- [x] **Step 3: lint**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm lint`
Expected: 无相关报错。

- [x] **Step 4: Commit**

```bash
git add src/pages/main/index.vue
git commit -m "feat(ai): greet once on model ready"
```

---

## Task 9: 本地 HTTP 推送 server

**Files:**
- Modify: `src-tauri/Cargo.toml`（`[dependencies]` 加 `tiny_http`、`form_urlencoded`）
- Create: `src-tauri/src/core/server.rs`
- Modify: `src-tauri/src/core/mod.rs`（`pub mod server;`）
- Modify: `src-tauri/src/lib.rs`（setup 末尾启动）
- Modify: `src-tauri/capabilities/default.json`（`+ core:window:allow-current-monitor`）

**Interfaces:**
- Consumes: `ai` store 持久化数据（Task 1，Rust 端 `with_store("ai", |s| s.try_get_or_default::<AiConfig>("ai"))`）、`show-chat` 事件（Task 3）。
- Produces: `core::server::start(&app_handle)`；`GET http://127.0.0.1:<port>/say?text=&duration=&token=`。

- [x] **Step 1: 加 Rust 依赖**

修改 `src-tauri/Cargo.toml`，在 `[dependencies]` 段内（`fs_extra = "1"` 附近）加两行：

```toml
tiny_http = "0.12"
form_urlencoded = "1"
```

- [x] **Step 2: 写 server.rs**

Create `src-tauri/src/core/server.rs`：

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_pinia::ManagerExt;
use tiny_http::{Method, Response, Server};

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AiConfig {
    http_enabled: bool,
    http_port: u16,
    http_token: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            http_enabled: false,
            http_port: 7800,
            http_token: String::new(),
        }
    }
}

#[derive(Serialize, Clone)]
struct ShowChatPayload {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
}

// ponytail: 改端口/开关/token 后需重启 app 生效（不做热重启）
pub fn start(app_handle: &AppHandle) {
    // 读持久化的 ai 配置（store id 与 key 均为 "ai"）；无文件时取默认（关闭）
    let config: AiConfig = app_handle
        .with_store("ai", |store| store.try_get_or_default::<AiConfig>("ai"))
        .unwrap_or_default();

    if !config.http_enabled {
        return;
    }

    let handle = app_handle.clone();
    let addr = format!("127.0.0.1:{}", config.http_port);
    let token = config.http_token;

    // ponytail: tiny_http 单线程阻塞循环，够用；不引入 axum/tokio
    std::thread::spawn(move || {
        let server = match Server::http(&addr) {
            Ok(server) => server,
            Err(err) => {
                log::error!("chat http server failed to bind {addr}: {err}");
                return;
            }
        };

        log::info!("chat http server listening on {addr}");

        for request in server.incoming_requests() {
            handle_request(&handle, &token, request);
        }
    });
}

fn handle_request(app_handle: &AppHandle, token: &str, request: tiny_http::Request) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));

    // 只支持 GET /say
    if request.method() != &Method::Get || path != "/say" {
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return;
    }

    let params: HashMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

    // token 非空时校验
    if !token.is_empty() && params.get("token").map(String::as_str) != Some(token) {
        let _ = request.respond(Response::from_string("unauthorized").with_status_code(401));
        return;
    }

    let text = match params.get("text") {
        Some(text) if !text.is_empty() => text.clone(),
        _ => {
            let _ = request.respond(Response::from_string("missing text").with_status_code(400));
            return;
        }
    };

    // duration：秒 → 毫秒；没给则不带（chat 页用默认）；0 表示常驻
    let duration = params
        .get("duration")
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds * 1000);

    let _ = app_handle.emit("show-chat", ShowChatPayload { text, duration });

    let _ = request.respond(Response::from_string("ok"));
}
```

- [x] **Step 3: 注册模块 + setup 启动**

修改 `src-tauri/src/core/mod.rs`，加一行：

```rust
pub mod server;
```

修改 `src-tauri/src/lib.rs`，在 `setup` 闭包里 `setup::default(...)` 调用之后、`Ok(())` 之前加：

```rust
            core::server::start(&app_handle);
```

> `app_handle` 在闭包里类型为 `&AppHandle`，与 `server::start(&AppHandle)` 匹配。`core` 模块已在文件顶部 `mod core;`。

- [x] **Step 4: 加 currentMonitor 权限（防御性）**

修改 `src-tauri/capabilities/default.json`，在 `permissions` 数组里 `"core:window:allow-set-position",` 之后加一行：

```json
    "core:window:allow-current-monitor",
```

> chat 页 `currentMonitor()` 需要。`windows: ["*"]` 已让 chat 继承全部权限。

- [x] **Step 5: 编译**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat/src-tauri && cargo build`
Expected: 编译通过。

- [x] **Step 6: 端到端验证 HTTP 接口**

Run: `cd /Users/xuebaoku/GolandProjects/BongoCat && pnpm tauri dev`

先确认默认关闭：

Run: `curl -s -o /dev/null -w "%{http_code}\n" "http://127.0.0.1:7800/say?text=hi"`
Expected: 连接失败（端口未监听）。

在 AI 设置里打开 HTTP 开关 → **重启** app（`Ctrl-C` 后再 `pnpm tauri dev`），然后：

```bash
# 正常推送 → 200 且猫头顶冒泡
curl -s "http://127.0.0.1:7800/say?text=%E4%BD%A0%E5%A5%BD%E5%91%80"; echo
# 缺 text → 400
curl -s -o /dev/null -w "%{http_code}\n" "http://127.0.0.1:7800/say"
# 指定 5 秒 → 5 秒后消失
curl -G -s "http://127.0.0.1:7800/say" --data-urlencode "text=五秒后消失" --data "duration=5"; echo
# duration=0 → 常驻
curl -G -s "http://127.0.0.1:7800/say" --data-urlencode "text=常驻" --data "duration=0"; echo
```
Expected: 第一/三/四条返回 `ok` 且猫头顶冒泡；第二条返回 `400`。

（可选）设置里填一个 `httpToken` 后重启，验证不带/错 token → `401`。

确认后 `Ctrl-C` 退出。

- [x] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/core/server.rs src-tauri/src/core/mod.rs src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat(ai): local http push server (tiny_http) reusing show-chat"
```

---

## 自检结果（Self-Review）

**1. Spec coverage**

| Spec 节 | 对应 Task |
|---|---|
| 3 新增 chat 窗口 | Task 4 |
| 4 chat 页面（穿透/UI/样式/监听/测量/setSize/定位/计时/几何跟随/字号重测） | Task 4 + Task 5 |
| 5 定位纯函数 + DPI/多屏 | Task 2（纯函数）+ Task 5（currentMonitor 取数 + 物理换算） |
| 6 推送 API say() | Task 3 |
| 6.1 HTTP server | Task 9 |
| 7 macOS NSPanel + 广播 | Task 6 |
| 8 AI 设置 tab + store + i18n | Task 1（store）+ Task 7（页面 + i18n） |
| 9(a) 纯函数 8 分支单测 | Task 2（8 条 assert）|
| 9(b) DPI/多屏数据来源 | Task 5 reposition() |
| 9(c) 手动验证清单 | Task 5/6/7 的手动验证步骤 |
| 9(d) 启动招呼 | Task 8 |
| 9(e) HTTP 验证 | Task 9 Step 6 |
| 10 改动文件一览 | 全部覆盖（tiny_http 放 src-tauri/Cargo.toml 而非 workspace，等价） |
| 11 ponytail 简化 | macos.rs / server.rs 内 `// ponytail` 注释 |

**2. Placeholder scan**：无 TODO/TBD；每个写代码的 step 都给了完整代码。

**3. Type consistency**：`computeBubblePosition(main, bubble, screen, gap)` 签名在 Task 2 定义、Task 5 调用一致；事件名 `show-chat` 在 `LISTEN_KEY.SHOW_CHAT`（前端）与 Rust `app_handle.emit("show-chat")` 一致；payload `{ text, duration? }` 前后端一致；store id/key `"ai"`/`"ai"` 在前端 `defineStore('ai', …{ ai })` 与 Rust `with_store("ai", … try_get("ai"))` 一致；camelCase 字段（`httpEnabled`/`httpPort`/`httpToken`）经 `#[serde(rename_all = "camelCase")]` 对齐。

**说明（偏离 spec 的两处，均更稳妥）**：
- `tiny_http` 加到 `src-tauri/Cargo.toml` 而非 workspace（它只被 src-tauri 用），等价。
- 额外引入 `form_urlencoded`（标准的 query 解码，避免手写 percent-decode 在信任边界出错）——spec 未列，但属正确性必需。

---

**Plan complete and saved to `docs/superpowers/plans/2026-06-29-ai-chat-bubble.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — 我为每个 task 派一个全新 subagent，task 间做两段式 review，迭代快。

**2. Inline Execution** — 在当前 session 按 executing-plans 批量执行，带 checkpoint 供你 review。

**Which approach?**
