# Chat 历史消息 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 记录每条 Chat 气泡消息(时间/状态/来源),并在偏好设置 Chat 标签页提供带筛选的历史消息弹窗。

**Architecture:** 所有消息(前端 `say()` 与 Rust HTTP `/say`)都汇聚到气泡窗口 `src/pages/chat/index.vue` 的 `showChat` 入口,在此单点记录到新的 `chat-history` pinia store(`@tauri-store/pinia` 自动落盘 + 跨窗口同步)。设置窗口读同一 store,用 Modal + Table 展示。

**Tech Stack:** Vue 3 + pinia + @tauri-store/pinia、antdv-next(Modal/Table/Select/DateRangePicker/Tag)、dayjs、Rust(serde 加一个字段)。测试:纯函数用 node assert 脚本(`tsx` 运行,参照 `src/utils/chatPosition.test.ts`);Rust 用 `cargo test`。

**Spec:** `docs/superpowers/specs/2026-07-08-chat-history-design.md`

## Global Constraints

- 历史上限 500 条,超出淘汰最旧。
- 状态取值:`'shown' | 'skipped'`(skipped = 总开关关闭时收到);来源取值:`'http' | 'internal'`。
- 记录不得改变现有气泡展示行为(记录在 enabled 判断之前,展示逻辑原样)。
- 日期筛选为闭区间:开始日 00:00:00.000 至 结束日 23:59:59.999。
- 所有界面文案走 i18n,5 个语言文件(zh-CN、zh-TW、en-US、pt-BR、vi-VN)都要补齐。
- 提交前 lint 由 git hook 自动跑(lint-staged eslint --fix),无需手动。
- 包管理器是 pnpm;前端测试运行方式:`pnpm exec tsx <test-file>`。

---

### Task 1: 纯函数工具(裁剪 + 过滤)

**Files:**

- Create: `src/utils/chatHistory.ts`
- Test: `src/utils/chatHistory.test.ts`

**Interfaces:**

- Consumes: 无
- Produces:
  - `interface ChatMessage { time: number, text: string, status: 'shown' | 'skipped', source: 'http' | 'internal' }`
  - `const MAX_HISTORY = 500`
  - `function appendCapped(history: ChatMessage[], message: ChatMessage, limit?: number): void`(原地 push,超限移除最旧)
  - `interface HistoryFilter { status?: ChatMessage['status'], source?: ChatMessage['source'], range?: [number, number] }`(range 为毫秒时间戳闭区间)
  - `function filterHistory(history: ChatMessage[], filter: HistoryFilter): ChatMessage[]`

- [ ] **Step 1: 写失败的测试**

创建 `src/utils/chatHistory.test.ts`(node assert 风格,参照 `chatPosition.test.ts`):

```ts
import assert from 'node:assert'

import type { ChatMessage } from './chatHistory'

import { appendCapped, filterHistory } from './chatHistory'

function msg(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return { time: 1000, text: 'hi', status: 'shown', source: 'internal', ...overrides }
}

// 1. appendCapped：未超限时直接追加
{
  const history: ChatMessage[] = []
  appendCapped(history, msg(), 3)
  appendCapped(history, msg({ time: 2000 }), 3)
  assert.strictEqual(history.length, 2)
  assert.strictEqual(history[1].time, 2000)
}

// 2. appendCapped：超限时移除最旧，保留最新
{
  const history: ChatMessage[] = []
  for (let i = 1; i <= 5; i++) {
    appendCapped(history, msg({ time: i }), 3)
  }
  assert.strictEqual(history.length, 3)
  assert.deepStrictEqual(history.map(m => m.time), [3, 4, 5])
}

// 3. filterHistory：空条件返回全部
{
  const history = [msg(), msg({ status: 'skipped' })]
  assert.strictEqual(filterHistory(history, {}).length, 2)
}

// 4. filterHistory：按状态过滤
{
  const history = [msg(), msg({ status: 'skipped' })]
  const out = filterHistory(history, { status: 'skipped' })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].status, 'skipped')
}

// 5. filterHistory：按来源过滤
{
  const history = [msg(), msg({ source: 'http' })]
  const out = filterHistory(history, { source: 'http' })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].source, 'http')
}

// 6. filterHistory：日期范围为闭区间
{
  const history = [msg({ time: 100 }), msg({ time: 200 }), msg({ time: 300 })]
  const out = filterHistory(history, { range: [100, 200] })
  assert.deepStrictEqual(out.map(m => m.time), [100, 200])
}

// 7. filterHistory：多条件同时生效
{
  const history = [
    msg({ time: 100, status: 'shown', source: 'http' }),
    msg({ time: 150, status: 'skipped', source: 'http' }),
    msg({ time: 200, status: 'skipped', source: 'internal' }),
  ]
  const out = filterHistory(history, { status: 'skipped', source: 'http', range: [100, 200] })
  assert.strictEqual(out.length, 1)
  assert.strictEqual(out[0].time, 150)
}

console.log('chatHistory tests passed')
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec tsx src/utils/chatHistory.test.ts`
Expected: FAIL(Cannot find module './chatHistory')

- [ ] **Step 3: 实现**

创建 `src/utils/chatHistory.ts`:

```ts
export const MAX_HISTORY = 500

export interface ChatMessage {
  time: number // 毫秒时间戳，展示时格式化到秒
  text: string
  status: 'shown' | 'skipped' // skipped = 总开关关闭时收到
  source: 'http' | 'internal'
}

export interface HistoryFilter {
  status?: ChatMessage['status']
  source?: ChatMessage['source']
  range?: [number, number] // 闭区间，毫秒时间戳
}

// 原地追加并裁剪到上限（移除最旧）
export function appendCapped(history: ChatMessage[], message: ChatMessage, limit = MAX_HISTORY) {
  history.push(message)

  if (history.length > limit) {
    history.splice(0, history.length - limit)
  }
}

export function filterHistory(history: ChatMessage[], filter: HistoryFilter): ChatMessage[] {
  return history.filter(({ time, status, source }) => {
    if (filter.status && status !== filter.status) return false
    if (filter.source && source !== filter.source) return false
    if (filter.range && (time < filter.range[0] || time > filter.range[1])) return false
    return true
  })
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm exec tsx src/utils/chatHistory.test.ts`
Expected: `chatHistory tests passed`

- [ ] **Step 5: Commit**

```bash
git add src/utils/chatHistory.ts src/utils/chatHistory.test.ts
git commit -m "feat(chat): add chat history append/filter helpers"
```

---

### Task 2: chatHistory store + 各窗口启动

**Files:**

- Create: `src/stores/chatHistory.ts`
- Modify: `src/App.vue`(onMounted 里与其他 store 一致地 `$tauri.start()`)

**Interfaces:**

- Consumes: Task 1 的 `ChatMessage`、`appendCapped`
- Produces: `useChatHistoryStore()` → `{ history: ChatMessage[], record(message: ChatMessage): void }`(store id 为 `'chat-history'`,由 @tauri-store/pinia 自动落盘并跨窗口同步)

- [ ] **Step 1: 创建 store**

创建 `src/stores/chatHistory.ts`:

```ts
import { defineStore } from 'pinia'
import { ref } from 'vue'

import type { ChatMessage } from '@/utils/chatHistory'

import { appendCapped } from '@/utils/chatHistory'

export const useChatHistoryStore = defineStore('chat-history', () => {
  const history = ref<ChatMessage[]>([])

  function record(message: ChatMessage) {
    appendCapped(history.value, message)
  }

  return {
    history,
    record,
  }
})
```

- [ ] **Step 2: App.vue 启动 store**

修改 `src/App.vue`:

在 imports 区(`useChatStore` 之后)加:

```txt
import { useChatHistoryStore } from './stores/chatHistory'
```

在 `const chatStore = useChatStore()` 之后加:

```txt
const chatHistoryStore = useChatHistoryStore()
```

在 `onMounted` 中 `await chatStore.$tauri.start()` 之后加:

```txt
await chatHistoryStore.$tauri.start()
```

- [ ] **Step 3: 类型检查**

Run: `pnpm exec vue-tsc --noEmit -p tsconfig.json 2>&1 | head -20`(若项目无 vue-tsc,改跑 `pnpm build:vite`,预期无类型错误)
Expected: 无新增错误

- [ ] **Step 4: Commit**

```bash
git add src/stores/chatHistory.ts src/App.vue
git commit -m "feat(chat): add persisted chat-history store"
```

---

### Task 3: Rust /say 标记来源

**Files:**

- Modify: `src-tauri/src/core/server.rs`(`ShowChatPayload` 结构体 ~L68-84、`handle_say` ~L216、`mod tests`)

**Interfaces:**

- Consumes: 无
- Produces: `show-chat` 事件 payload 新增 `source: "http"` 字段(camelCase JSON,始终存在)

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/core/server.rs` 的 `mod tests` 末尾(`rejects_out_of_range_and_bad_values` 之后)加:

```rust
    #[test]
    fn say_payload_serializes_http_source() {
        let payload = ShowChatPayload {
            text: "hi".into(),
            duration: None,
            text_color: None,
            font_size: None,
            bg_color: None,
            bg_opacity: None,
            source: "http".into(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""source":"http""#));
    }
```

- [ ] **Step 2: 运行确认编译失败**

Run: `cd src-tauri && cargo test say_payload 2>&1 | tail -5`
Expected: FAIL(struct `ShowChatPayload` 没有 `source` 字段)

- [ ] **Step 3: 实现**

`ShowChatPayload` 结构体末尾(`bg_opacity` 字段之后)加:

```rust
    // 消息来源；前端 say() 不带此字段，气泡窗口按 internal 兜底
    source: String,
```

`handle_say` 中构造 payload 处(`bg_opacity: overrides.bg_opacity,` 之后)加:

```rust
        source: "http".into(),
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: 全部测试 PASS(含既有测试)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/server.rs
git commit -m "feat(chat): tag http /say messages with source field"
```

---

### Task 4: 气泡窗口单点记录

**Files:**

- Modify: `src/pages/chat/index.vue`(`ShowChatPayload` 接口 ~L14-21、`showChat` 函数 ~L117)

**Interfaces:**

- Consumes: Task 2 的 `useChatHistoryStore().record()`;Task 3 的 payload `source` 字段
- Produces: 每条 `show-chat` 事件都被记录(含总开关关闭时的 skipped)

- [ ] **Step 1: 实现记录**

修改 `src/pages/chat/index.vue`:

imports 区(`useChatStore` 之后)加:

```txt
import { useChatHistoryStore } from '@/stores/chatHistory'
```

`const chatStore = useChatStore()` 之后加:

```txt
const chatHistoryStore = useChatHistoryStore()
```

`ShowChatPayload` 接口末尾加一个字段:

```txt
  source?: 'http'
```

`showChat` 函数改为(仅开头新增记录,其余逻辑不动):

```txt
async function showChat({ text: nextText, duration, textColor, fontSize, bgColor, bgOpacity, source }: ShowChatPayload) {
  // 单点记录：所有来源的消息都经过这里，enabled 决定展示与否
  chatHistoryStore.record({
    time: Date.now(),
    text: nextText,
    status: chatStore.ai.enabled ? 'shown' : 'skipped',
    source: source ?? 'internal',
  })

  // 总开关唯一生效点
  if (!chatStore.ai.enabled) return
  // …… 以下原有逻辑全部保持不变
```

- [ ] **Step 2: 手动验证记录**

Run: `pnpm tauri dev`(或复用已在跑的 dev 实例),打开设置 → Chat → 调试模式 → 点「展示」发一条测试消息。
然后检查落盘文件:`cat ~/Library/Application\ Support/com.ayangweb.BongoCat/tauri-store/chat-history.json 2>/dev/null || ls ~/Library/Application\ Support/*BongoCat*/`(store 文件名以实际为准,内容应包含刚才的消息、`"status":"shown"`、`"source":"internal"`)
Expected: 能看到记录的消息 JSON

- [ ] **Step 3: Commit**

```bash
git add src/pages/chat/index.vue
git commit -m "feat(chat): record every bubble message to chat-history store"
```

---

### Task 5: 历史消息弹窗 UI + i18n

**Files:**

- Create: `src/pages/preference/components/chat/components/history-modal/index.vue`
- Modify: `src/pages/preference/components/chat/index.vue`(新增「历史消息」入口)
- Modify: `src/locales/zh-CN.json`、`src/locales/zh-TW.json`、`src/locales/en-US.json`、`src/locales/pt-BR.json`、`src/locales/vi-VN.json`

**Interfaces:**

- Consumes: Task 1 的 `filterHistory`/`ChatMessage`;Task 2 的 `useChatHistoryStore()`
- Produces: `<HistoryModal v-model="visible" />` 组件(`defineModel<boolean>` 控制显隐,参照 `behavior-modal` 的用法)

- [ ] **Step 1: 补齐 i18n**

在 5 个语言文件 `pages.preference.chat` 下操作:`labels` 内加 `history` 键,`hints` 内加 `history` 键,并在 `chat` 对象内新增 `history` 子对象(与 `labels`/`hints` 平级)。

`zh-CN.json`:

```txt
"labels": { "history": "历史消息" },
"hints": { "history": "记录最近 500 条气泡消息，重启后保留。" },
"history": {
  "view": "查看",
  "time": "时间",
  "status": "状态",
  "source": "来源",
  "content": "内容",
  "action": "操作",
  "detail": "详情",
  "shown": "已展示",
  "skipped": "未展示",
  "http": "HTTP",
  "internal": "内部",
  "filterStatus": "全部状态",
  "filterSource": "全部来源"
}
```

`zh-TW.json`:

```txt
"labels": { "history": "歷史訊息" },
"hints": { "history": "記錄最近 500 則氣泡訊息，重啟後保留。" },
"history": {
  "view": "檢視",
  "time": "時間",
  "status": "狀態",
  "source": "來源",
  "content": "內容",
  "action": "操作",
  "detail": "詳情",
  "shown": "已展示",
  "skipped": "未展示",
  "http": "HTTP",
  "internal": "內部",
  "filterStatus": "全部狀態",
  "filterSource": "全部來源"
}
```

`en-US.json`:

```txt
"labels": { "history": "Message history" },
"hints": { "history": "Keeps the latest 500 bubble messages, preserved across restarts." },
"history": {
  "view": "View",
  "time": "Time",
  "status": "Status",
  "source": "Source",
  "content": "Content",
  "action": "Action",
  "detail": "Detail",
  "shown": "Shown",
  "skipped": "Skipped",
  "http": "HTTP",
  "internal": "Internal",
  "filterStatus": "All statuses",
  "filterSource": "All sources"
}
```

`pt-BR.json`:

```txt
"labels": { "history": "Histórico de mensagens" },
"hints": { "history": "Mantém as últimas 500 mensagens de balão, preservadas entre reinicializações." },
"history": {
  "view": "Ver",
  "time": "Hora",
  "status": "Status",
  "source": "Origem",
  "content": "Conteúdo",
  "action": "Ação",
  "detail": "Detalhes",
  "shown": "Exibida",
  "skipped": "Ignorada",
  "http": "HTTP",
  "internal": "Interno",
  "filterStatus": "Todos os status",
  "filterSource": "Todas as origens"
}
```

`vi-VN.json`:

```txt
"labels": { "history": "Lịch sử tin nhắn" },
"hints": { "history": "Lưu 500 tin nhắn bong bóng gần nhất, giữ lại sau khi khởi động lại." },
"history": {
  "view": "Xem",
  "time": "Thời gian",
  "status": "Trạng thái",
  "source": "Nguồn",
  "content": "Nội dung",
  "action": "Thao tác",
  "detail": "Chi tiết",
  "shown": "Đã hiển thị",
  "skipped": "Đã bỏ qua",
  "http": "HTTP",
  "internal": "Nội bộ",
  "filterStatus": "Tất cả trạng thái",
  "filterSource": "Tất cả nguồn"
}
```

(注意:`labels`/`hints` 是往既有对象里加键,`history` 是新对象;保持 JSON 逗号合法。)

- [ ] **Step 2: 创建 HistoryModal 组件**

创建 `src/pages/preference/components/chat/components/history-modal/index.vue`:

```vue
<script setup lang="ts">
import { DateRangePicker, Flex, Modal, Select, Table, Tag } from 'antdv-next'
import dayjs from 'dayjs'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { ChatMessage } from '@/utils/chatHistory'

import { useChatHistoryStore } from '@/stores/chatHistory'
import { filterHistory } from '@/utils/chatHistory'

const modelValue = defineModel<boolean>()
const { t } = useI18n()
const chatHistoryStore = useChatHistoryStore()

const status = ref<ChatMessage['status']>()
const source = ref<ChatMessage['source']>()
const range = ref<[unknown, unknown]>()
const detailText = ref<string>()

const statusOptions = computed(() => [
  { value: 'shown', label: t('pages.preference.chat.history.shown') },
  { value: 'skipped', label: t('pages.preference.chat.history.skipped') },
])

const sourceOptions = computed(() => [
  { value: 'http', label: t('pages.preference.chat.history.http') },
  { value: 'internal', label: t('pages.preference.chat.history.internal') },
])

// 日期闭区间：开始日 00:00:00.000 → 结束日 23:59:59.999；dayjs() 对字符串/Dayjs 输入都适用
const rows = computed(() => {
  const [start, end] = range.value ?? []
  const ms: [number, number] | undefined = start && end
    ? [dayjs(start as never).startOf('day').valueOf(), dayjs(end as never).endOf('day').valueOf()]
    : undefined

  return filterHistory(chatHistoryStore.history, {
    status: status.value,
    source: source.value,
    range: ms,
  }).slice().reverse()
})

const columns = computed(() => [
  { title: t('pages.preference.chat.history.time'), key: 'time', width: 170 },
  { title: t('pages.preference.chat.history.status'), key: 'status', width: 90 },
  { title: t('pages.preference.chat.history.source'), key: 'source', width: 90 },
  { title: t('pages.preference.chat.history.content'), dataIndex: 'text', key: 'text', ellipsis: true },
  { title: t('pages.preference.chat.history.action'), key: 'action', width: 80 },
])
</script>

<template>
  <Modal
    v-model:open="modelValue"
    :footer="null"
    :title="$t('pages.preference.chat.labels.history')"
    width="720px"
  >
    <Flex
      class="mb-3"
      :gap="8"
    >
      <Select
        v-model:value="status"
        allow-clear
        class="w-30"
        :options="statusOptions"
        :placeholder="$t('pages.preference.chat.history.filterStatus')"
      />

      <Select
        v-model:value="source"
        allow-clear
        class="w-30"
        :options="sourceOptions"
        :placeholder="$t('pages.preference.chat.history.filterSource')"
      />

      <DateRangePicker
        v-model:value="range"
        allow-clear
      />
    </Flex>

    <Table
      :columns="columns"
      :data-source="rows"
      :pagination="{ pageSize: 20 }"
      :row-key="(_: ChatMessage, index: number) => index"
      size="small"
    >
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'time'">
          {{ dayjs(record.time).format('YYYY-MM-DD HH:mm:ss') }}
        </template>

        <template v-else-if="column.key === 'status'">
          <Tag :color="record.status === 'shown' ? 'green' : 'default'">
            {{ $t(`pages.preference.chat.history.${record.status}`) }}
          </Tag>
        </template>

        <template v-else-if="column.key === 'source'">
          {{ $t(`pages.preference.chat.history.${record.source}`) }}
        </template>

        <template v-else-if="column.key === 'action'">
          <a @click="detailText = record.text">
            {{ $t('pages.preference.chat.history.detail') }}
          </a>
        </template>
      </template>
    </Table>

    <Modal
      :footer="null"
      :open="detailText !== undefined"
      :title="$t('pages.preference.chat.history.detail')"
      @cancel="detailText = undefined"
    >
      <div class="max-h-80 overflow-auto whitespace-pre-wrap break-words">
        {{ detailText }}
      </div>
    </Modal>
  </Modal>
</template>
```

实现时若 `DateRangePicker` 的 v-model 值类型报错,以组件实际 Props 类型为准调整 `range` 的类型标注(值可能是 Dayjs 或字符串,`dayjs()` 二者都能包)。

- [ ] **Step 3: 在 Chat 标签页加入口**

修改 `src/pages/preference/components/chat/index.vue`:

imports 区加:

```txt
import HistoryModal from './components/history-modal/index.vue'
```

`const testText = ref('你好呀~')` 之后加:

```txt
const historyVisible = ref(false)
```

第一个 `<ProList :title="$t('pages.preference.chat.labels.basic')">` 内、`enabled` 那个 ProListItem 之后加:

```txt
    <ProListItem
      :description="$t('pages.preference.chat.hints.history')"
      :title="$t('pages.preference.chat.labels.history')"
    >
      <Button @click="historyVisible = true">
        {{ $t('pages.preference.chat.history.view') }}
      </Button>
    </ProListItem>
```

模板最末尾(最后一个 `</ProList>` 之后)加:

```txt
  <HistoryModal v-model="historyVisible" />
```

(`Button` 已在该文件的 antdv-next import 里。)

- [ ] **Step 4: 手动验证(spec 的验收路径)**

Run: `pnpm tauri dev`,依次验证:

1. 设置 → Chat → 「历史消息」→「查看」打开弹窗,能看到 Task 4 验证时的记录,时间格式 `YYYY-MM-DD HH:mm:ss`,最新在前。
2. 调试「展示」发一条 → 列表出现 `已展示 / 内部`。
3. 关闭总开关再发一条 → 气泡不弹,列表出现 `未展示 / 内部`。
4. 开启 HTTP 接口(需重启生效)后 `curl "http://127.0.0.1:7800/say?text=hello-from-http"` → 列表出现 `已展示 / HTTP`。
5. 状态/来源/日期筛选各试一次,结果正确;选“今天到今天”应包含今天的消息(闭区间)。
6. 发一条长文本(如 200 字),列表单行省略,点「详情」二级弹窗显示完整文本且换行保留。
7. 重启应用,历史仍在。

Expected: 全部通过

- [ ] **Step 5: Commit**

```bash
git add src/pages/preference/components/chat src/locales
git commit -m "feat(chat): add message history modal with status/source/date filters"
```
