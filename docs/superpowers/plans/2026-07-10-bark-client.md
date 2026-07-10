# Bark 客户端支持 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** BongoCat 注册到自托管 htnanako/bark-server（SSE fork），实时接收 Bark 推送并以聊天气泡展示、记入 chat-history。

**Architecture:** 逻辑全部在前端（Vue 3）实现：`fetch POST /register` 注册换 device_key/stream_token；`fetch` + ReadableStream 手写极简 SSE 解析连接 `/events/{key}`（不发 Last-Event-ID → 服务端不回放）；Web Crypto 解密 AES-CBC/GCM；消息 emit 现有 `show-chat` 事件（`source: 'bark'`），复用 chat 窗口的展示与历史记录路径。**fetch 必须用 `@tauri-apps/plugin-http` 的实现**（fork 服务端无 CORS 中间件，webview 原生 fetch 跨域被拦截；plugin-http 走 Rust 侧发请求不受 CORS 限制，响应体是真 ReadableStream，SSE 可用）。Rust 端改动仅限该插件的注册与权限。

**Tech Stack:** Vue 3 + Pinia（tauri-plugin-pinia 持久化/跨窗口同步）、antdv-next、@tauri-apps/api event、@tauri-apps/plugin-http、Web Crypto、tsx（自检脚本）。

**Spec:** `docs/superpowers/specs/2026-07-10-bark-client-design.md`

## Global Constraints

- 包管理器：pnpm（仓库 preinstall 强制）；新依赖**仅限**官方 `@tauri-apps/plugin-http`（npm）+ `tauri-plugin-http`（crate）——CORS 所迫；不加 vitest，纯逻辑用 `npx tsx` 跑 assert 自检脚本
- **Rust 端（src-tauri）改动仅限 plugin-http 注册**：`Cargo.toml` 依赖、`lib.rs` 一行 `.plugin(...)`、`capabilities/default.json` 权限；不写任何业务 Rust 代码
- 不支持：消息回放/Last-Event-ID、AES-ECB、markdown/url/icon/level 等富字段、系统通知、多实例
- locales 五个文件（en-US、pt-BR、vi-VN、zh-CN、zh-TW）必须同步补齐，模板里不出现硬编码文案
- 代码注释风格跟随仓库现状（中文、说明约束而非复述代码）；刻意简化标 `// ponytail:`
- 提交信息用 conventional commits，scope 用 `chat`（与现有 `feat(chat):` 一致）
- 每个任务完成后运行 `pnpm lint`（仓库无 typecheck 脚本，eslint 是唯一静态门禁）

---

### Task 1: 纯逻辑工具 `src/utils/bark.ts`（SSE 解析 + 解密 + 文本映射）

**Files:**
- Create: `src/utils/bark.ts`
- Test: `scripts/barkSelfCheck.ts`（assert 自检，`npx tsx` 运行）

**Interfaces:**
- Consumes: 无（纯函数，零依赖）
- Produces（Task 3 依赖）:
  - `interface BarkCryptoConfig { mode: 'cbc' | 'gcm', key: string, iv: string }`
  - `interface SSEEvent { event: string, data: string, id?: string }`
  - `interface BarkNotification { title?: string, subtitle?: string, body?: string, payload?: Record<string, unknown> }`
  - `createSSEParser(): (chunk: string) => SSEEvent[]`（增量解析，跨 chunk 缓冲）
  - `decryptBark(ciphertext: string, config: BarkCryptoConfig, ivOverride?: string): Promise<Record<string, unknown>>`
  - `resolveBarkText(notification: BarkNotification, cryptoConfig?: BarkCryptoConfig): Promise<string>`

- [ ] **Step 1: 写自检脚本（此时必然失败）**

创建 `scripts/barkSelfCheck.ts`：

```ts
import assert from 'node:assert'

import { createSSEParser, decryptBark, resolveBarkText } from '../src/utils/bark'

async function main() {
  // --- SSE 解析：心跳注释行、事件跨 chunk、多行 data、\r\n 归一化 ---
  const feed = createSSEParser()

  assert.deepStrictEqual(feed(': ping\n\nevent: noti'), [], '心跳行应被忽略，半个事件应留在缓冲区')

  const events = feed('fication\nid: 1\ndata: {"a":\ndata: 1}\n\n')
  assert.strictEqual(events.length, 1)
  assert.strictEqual(events[0].event, 'notification')
  assert.strictEqual(events[0].id, '1')
  assert.strictEqual(events[0].data, '{"a":\n1}', '多行 data 用 \\n 拼接')

  const crlf = feed('event: ready\r\ndata: {}\r\n\r\n')
  assert.strictEqual(crlf.length, 1)
  assert.strictEqual(crlf[0].event, 'ready')

  // --- 解密：用 Node webcrypto 加密再解回（CBC / GCM），坏密文拒绝 ---
  const encoder = new TextEncoder()
  const keyStr = '0123456789abcdef' // 16 字符 = AES-128
  const ivCbc = 'abcdefghijklmnop' // CBC 16 字符
  const ivGcm = 'abcdefghijkl' // GCM 12 字符
  const plain = JSON.stringify({ title: 'hi', body: 'there' })
  const toB64 = (buf: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(buf)))

  const cbcKey = await crypto.subtle.importKey('raw', encoder.encode(keyStr), 'AES-CBC', false, ['encrypt'])
  const cbcB64 = toB64(await crypto.subtle.encrypt({ name: 'AES-CBC', iv: encoder.encode(ivCbc) }, cbcKey, encoder.encode(plain)))
  const cbcOut = await decryptBark(cbcB64, { mode: 'cbc', key: keyStr, iv: ivCbc })
  assert.strictEqual(cbcOut.title, 'hi')

  const gcmKey = await crypto.subtle.importKey('raw', encoder.encode(keyStr), 'AES-GCM', false, ['encrypt'])
  const gcmB64 = toB64(await crypto.subtle.encrypt({ name: 'AES-GCM', iv: encoder.encode(ivGcm) }, gcmKey, encoder.encode(plain)))
  const gcmOut = await decryptBark(gcmB64, { mode: 'gcm', key: keyStr, iv: ivGcm })
  assert.strictEqual(gcmOut.body, 'there')

  await assert.rejects(decryptBark('AAAAAAAA', { mode: 'gcm', key: keyStr, iv: ivGcm }), '坏密文必须抛错')

  // --- 文本映射：payload 优先、key 不区分大小写、title+body 换行拼接 ---
  assert.strictEqual(await resolveBarkText({ title: 'a', body: 'b' }), 'a\nb')
  assert.strictEqual(await resolveBarkText({ title: 'x', payload: { Title: 'a', Body: 'b' } }), 'a\nb', 'payload 覆盖顶层字段')
  assert.strictEqual(await resolveBarkText({ body: 'only' }), 'only', '无 title 时不出现多余换行')
  assert.strictEqual(await resolveBarkText({}), '', '空消息返回空串')

  // 加密 payload 端到端；payload 内 iv 覆盖本地配置
  const encText = await resolveBarkText({ payload: { ciphertext: cbcB64 } }, { mode: 'cbc', key: keyStr, iv: ivCbc })
  assert.strictEqual(encText, 'hi\nthere')
  const encIvOverride = await resolveBarkText({ payload: { ciphertext: cbcB64, iv: ivCbc } }, { mode: 'cbc', key: keyStr, iv: 'wrongwrongwrongw' })
  assert.strictEqual(encIvOverride, 'hi\nthere', 'payload.iv 应覆盖本地 iv')

  // 有 ciphertext 但未配密钥 → 抛错（上层丢弃该条）
  await assert.rejects(resolveBarkText({ payload: { ciphertext: cbcB64 } }))

  console.log('bark self-check OK')
}

main()
```

- [ ] **Step 2: 运行自检确认失败**

Run: `npx tsx scripts/barkSelfCheck.ts`
Expected: FAIL — `Cannot find module '../src/utils/bark'`

- [ ] **Step 3: 实现 `src/utils/bark.ts`**

```ts
export interface BarkCryptoConfig {
  mode: 'cbc' | 'gcm'
  key: string // ASCII，16/24/32 字符对应 AES-128/192/256
  iv: string // ASCII，CBC 16 字符 / GCM 12 字符
}

export interface SSEEvent {
  event: string
  data: string
  id?: string
}

export interface BarkNotification {
  title?: string
  subtitle?: string
  body?: string
  payload?: Record<string, unknown>
}

// 极简增量 SSE 解析器：feed(chunk) 返回本次凑齐的完整事件，未凑齐的留在缓冲区。
// ponytail: 只实现本项目用到的子集——event/data/id 字段、`:` 心跳注释行、\r\n 归一化；不做 retry 字段
export function createSSEParser() {
  let buffer = ''

  return function feed(chunk: string): SSEEvent[] {
    buffer += chunk.replace(/\r\n/g, '\n')

    // 事件以空行分隔；split 后最后一段是未完成的残帧，放回缓冲区
    const frames = buffer.split('\n\n')
    buffer = frames.pop() ?? ''

    const events: SSEEvent[] = []

    for (const frame of frames) {
      const event: SSEEvent = { event: 'message', data: '' }
      const dataLines: string[] = []

      for (const line of frame.split('\n')) {
        if (!line || line.startsWith(':')) continue

        const colon = line.indexOf(':')
        if (colon === -1) continue

        const field = line.slice(0, colon)
        // SSE 规范：冒号后紧跟的单个空格不属于值
        const value = line.startsWith(' ', colon + 1) ? line.slice(colon + 2) : line.slice(colon + 1)

        if (field === 'event') event.event = value
        else if (field === 'data') dataLines.push(value)
        else if (field === 'id') event.id = value
      }

      event.data = dataLines.join('\n')

      if (event.data || event.event !== 'message') {
        events.push(event)
      }
    }

    return events
  }
}

// Bark AES 解密：ciphertext 为 base64，key/iv 为 ASCII 字符串（与 Bark 客户端约定一致）；
// GCM 为 combined 模式（tag 附在密文尾部，正好是 Web Crypto 的默认行为）。结果必须是 JSON。
export async function decryptBark(ciphertext: string, config: BarkCryptoConfig, ivOverride?: string): Promise<Record<string, unknown>> {
  const encoder = new TextEncoder()
  const algorithm = config.mode === 'gcm' ? 'AES-GCM' : 'AES-CBC'
  const data = Uint8Array.from(atob(ciphertext), char => char.charCodeAt(0))
  const iv = encoder.encode(ivOverride ?? config.iv)
  const key = await crypto.subtle.importKey('raw', encoder.encode(config.key), algorithm, false, ['decrypt'])
  const plain = await crypto.subtle.decrypt({ name: algorithm, iv }, key, data)

  return JSON.parse(new TextDecoder().decode(plain))
}

// notification 事件 → 气泡文本。payload 字段优先于顶层字段，key 不区分大小写（Bark 约定）；
// payload 含 ciphertext 时先解密——未配置密钥则抛错，由调用方丢弃该条。
export async function resolveBarkText(notification: BarkNotification, cryptoConfig?: BarkCryptoConfig): Promise<string> {
  const lower = (obj: Record<string, unknown>) =>
    Object.fromEntries(Object.entries(obj).map(([key, value]) => [key.toLowerCase(), value]))

  let payload = lower(notification.payload ?? {})

  if (typeof payload.ciphertext === 'string') {
    if (!cryptoConfig?.key) {
      throw new Error('encrypted message but no crypto key configured')
    }

    const ivOverride = typeof payload.iv === 'string' ? payload.iv : undefined
    payload = lower(await decryptBark(payload.ciphertext, cryptoConfig, ivOverride))
  }

  const title = typeof payload.title === 'string' ? payload.title : notification.title ?? ''
  const body = typeof payload.body === 'string' ? payload.body : notification.body ?? ''

  return [title, body].filter(Boolean).join('\n')
}
```

- [ ] **Step 4: 运行自检确认通过**

Run: `npx tsx scripts/barkSelfCheck.ts`
Expected: 输出 `bark self-check OK`，退出码 0

- [ ] **Step 5: lint 并提交**

```bash
pnpm lint
git add src/utils/bark.ts scripts/barkSelfCheck.ts
git commit -m "feat(chat): add bark SSE parser, AES decrypt and text mapping utils"
```

---

### Task 2: store 扩展 + 历史 source 类型 + 筛选项 + locales

**Files:**
- Modify: `src/stores/chat.ts`
- Modify: `src/utils/chatHistory.ts:7`
- Modify: `src/pages/preference/components/chat/components/history-modal/index.vue:26-29`
- Modify: `src/locales/en-US.json`、`src/locales/pt-BR.json`、`src/locales/vi-VN.json`、`src/locales/zh-CN.json`、`src/locales/zh-TW.json`

**Interfaces:**
- Consumes: 无
- Produces（Task 3/4 依赖）:
  - `chatStore.bark: { enabled: boolean, serverUrl: string, deviceKey: string, streamToken: string, cryptoMode: 'cbc' | 'gcm', cryptoKey: string, cryptoIv: string }`
  - `chatStore.barkStatus: BarkStatus`，`export type BarkStatus = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'unauthorized'`（从 `@/stores/chat` 导出）
  - `ChatMessage['source']` 联合类型含 `'bark'`
  - locale key：`pages.preference.chat.labels.bark*`、`hints.bark*`、`barkStatus.*`、`history.bark`

- [ ] **Step 1: 扩展 `src/stores/chat.ts`**

在 `ChatStore` 接口 `ai` 字段后新增（`barkStatus` 独立于 `bark` 对象——它由 chat 窗口高频写入，若放进 `bark` 会触发 Task 4 的配置 watch 造成重连死循环）：

```ts
export type BarkStatus = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'unauthorized'

export interface ChatStore {
  ai: {
    // …原有字段不动…
  }
  bark: {
    enabled: boolean
    serverUrl: string
    deviceKey: string
    streamToken: string
    cryptoMode: 'cbc' | 'gcm'
    cryptoKey: string
    cryptoIv: string
  }
  barkStatus: BarkStatus
}
```

在 `useChatStore` 的 setup 里（`ai` 的 reactive 之后）新增，并加入返回值：

```ts
const bark = reactive<ChatStore['bark']>({
  enabled: false,
  serverUrl: '',
  deviceKey: '',
  streamToken: '',
  cryptoMode: 'cbc',
  cryptoKey: '',
  cryptoIv: '',
})

// 连接状态由 chat 窗口写入、设置页只读展示；持久化的旧值无意义，chat 窗口挂载时会重置
const barkStatus = ref<BarkStatus>('idle')
```

```ts
return {
  ai,
  bark,
  barkStatus,
}
```

同时把文件顶部 `import { reactive, watch } from 'vue'` 改为 `import { reactive, ref, watch } from 'vue'`。

- [ ] **Step 2: `src/utils/chatHistory.ts` source 类型加 `'bark'`**

```ts
  source: 'http' | 'internal' | 'bark'
```

- [ ] **Step 3: history-modal 筛选项加 Bark**

`src/pages/preference/components/chat/components/history-modal/index.vue` 的 `sourceOptions`：

```ts
const sourceOptions = computed(() => [
  { value: 'http', label: t('pages.preference.chat.history.http') },
  { value: 'internal', label: t('pages.preference.chat.history.internal') },
  { value: 'bark', label: t('pages.preference.chat.history.bark') },
])
```

（表格 source 列用的是 `$t(\`…history.${record.source}\`)` 动态 key，加 locale 即自动生效，无需改模板。）

- [ ] **Step 4: 五个 locale 文件补文案**

每个文件的 `pages.preference.chat` 下：`labels` 与 `hints` 追加键、新增 `barkStatus` 同级对象、`history` 追加 `bark` 键。

`src/locales/zh-CN.json`：

```json
"labels": {
  "bark": "Bark 推送",
  "barkEnabled": "启用 Bark",
  "barkServerUrl": "服务器地址",
  "barkRegister": "注册设备",
  "barkDeviceKey": "设备 Key",
  "barkStatus": "连接状态",
  "barkCryptoKey": "解密密钥",
  "barkCryptoIv": "IV",
  "barkCryptoMode": "加密模式"
},
"hints": {
  "bark": "连接自托管 htnanako/bark-server（SSE），实时接收 Bark 推送并弹出气泡。断线期间的消息不补收。",
  "barkRegister": "首次使用先注册获取设备 Key；用它向服务器推送消息。",
  "barkCrypto": "留空则不解密；收到加密消息且未配置密钥时丢弃该条。"
},
"barkStatus": {
  "idle": "未启用",
  "connecting": "连接中",
  "connected": "已连接",
  "reconnecting": "重连中",
  "unauthorized": "需要重新注册"
},
"history": { "bark": "Bark" }
```

`src/locales/zh-TW.json`：

```json
"labels": {
  "bark": "Bark 推送",
  "barkEnabled": "啟用 Bark",
  "barkServerUrl": "伺服器位址",
  "barkRegister": "註冊裝置",
  "barkDeviceKey": "裝置 Key",
  "barkStatus": "連線狀態",
  "barkCryptoKey": "解密金鑰",
  "barkCryptoIv": "IV",
  "barkCryptoMode": "加密模式"
},
"hints": {
  "bark": "連接自架 htnanako/bark-server（SSE），即時接收 Bark 推送並彈出氣泡。斷線期間的訊息不補收。",
  "barkRegister": "首次使用先註冊取得裝置 Key；用它向伺服器推送訊息。",
  "barkCrypto": "留空則不解密；收到加密訊息且未設定金鑰時捨棄該條。"
},
"barkStatus": {
  "idle": "未啟用",
  "connecting": "連線中",
  "connected": "已連線",
  "reconnecting": "重連中",
  "unauthorized": "需要重新註冊"
},
"history": { "bark": "Bark" }
```

`src/locales/en-US.json`：

```json
"labels": {
  "bark": "Bark Push",
  "barkEnabled": "Enable Bark",
  "barkServerUrl": "Server URL",
  "barkRegister": "Register Device",
  "barkDeviceKey": "Device Key",
  "barkStatus": "Connection Status",
  "barkCryptoKey": "Decryption Key",
  "barkCryptoIv": "IV",
  "barkCryptoMode": "Cipher Mode"
},
"hints": {
  "bark": "Connect to a self-hosted htnanako/bark-server via SSE to receive Bark pushes as chat bubbles. Messages sent while offline are not replayed.",
  "barkRegister": "Register first to get a device key, then use it to push messages to the server.",
  "barkCrypto": "Leave empty to skip decryption; encrypted messages are dropped when no key is configured."
},
"barkStatus": {
  "idle": "Idle",
  "connecting": "Connecting",
  "connected": "Connected",
  "reconnecting": "Reconnecting",
  "unauthorized": "Re-register required"
},
"history": { "bark": "Bark" }
```

`src/locales/pt-BR.json`：

```json
"labels": {
  "bark": "Push Bark",
  "barkEnabled": "Ativar Bark",
  "barkServerUrl": "URL do servidor",
  "barkRegister": "Registrar dispositivo",
  "barkDeviceKey": "Chave do dispositivo",
  "barkStatus": "Status da conexão",
  "barkCryptoKey": "Chave de descriptografia",
  "barkCryptoIv": "IV",
  "barkCryptoMode": "Modo de cifra"
},
"hints": {
  "bark": "Conecte-se a um htnanako/bark-server auto-hospedado via SSE para receber pushes do Bark como balões. Mensagens enviadas offline não são reenviadas.",
  "barkRegister": "Registre primeiro para obter a chave do dispositivo; use-a para enviar mensagens ao servidor.",
  "barkCrypto": "Deixe vazio para não descriptografar; mensagens criptografadas são descartadas sem chave configurada."
},
"barkStatus": {
  "idle": "Inativo",
  "connecting": "Conectando",
  "connected": "Conectado",
  "reconnecting": "Reconectando",
  "unauthorized": "Necessário registrar novamente"
},
"history": { "bark": "Bark" }
```

`src/locales/vi-VN.json`：

```json
"labels": {
  "bark": "Đẩy Bark",
  "barkEnabled": "Bật Bark",
  "barkServerUrl": "Địa chỉ máy chủ",
  "barkRegister": "Đăng ký thiết bị",
  "barkDeviceKey": "Khóa thiết bị",
  "barkStatus": "Trạng thái kết nối",
  "barkCryptoKey": "Khóa giải mã",
  "barkCryptoIv": "IV",
  "barkCryptoMode": "Chế độ mã hóa"
},
"hints": {
  "bark": "Kết nối tới htnanako/bark-server tự triển khai qua SSE để nhận thông báo Bark dưới dạng bong bóng. Tin nhắn gửi khi ngoại tuyến sẽ không được phát lại.",
  "barkRegister": "Đăng ký trước để nhận khóa thiết bị; dùng nó để gửi tin nhắn tới máy chủ.",
  "barkCrypto": "Để trống nếu không giải mã; tin nhắn mã hóa sẽ bị bỏ qua khi chưa cấu hình khóa."
},
"barkStatus": {
  "idle": "Chưa bật",
  "connecting": "Đang kết nối",
  "connected": "Đã kết nối",
  "reconnecting": "Đang kết nối lại",
  "unauthorized": "Cần đăng ký lại"
},
"history": { "bark": "Bark" }
```

（以上均为**追加**到现有对象，不动已有键；JSON 里注意逗号。）

- [ ] **Step 5: lint 并提交**

```bash
pnpm lint
git add src/stores/chat.ts src/utils/chatHistory.ts src/pages/preference/components/chat/components/history-modal/index.vue src/locales/
git commit -m "feat(chat): add bark config store, history source type and locales"
```

---

### Task 3: 接入 plugin-http + `src/composables/useBark.ts`（注册 + SSE 连接循环）

**Files:**
- Modify: `package.json`（pnpm add）、`src-tauri/Cargo.toml`（cargo add）、`src-tauri/src/lib.rs`、`src-tauri/capabilities/default.json`
- Create: `src/composables/useBark.ts`

**Interfaces:**
- Consumes: Task 1 的 `createSSEParser`/`resolveBarkText`/`BarkCryptoConfig`/`SSEEvent`；Task 2 的 `chatStore.bark`/`chatStore.barkStatus`；现有 `LISTEN_KEY.SHOW_CHAT`（`src/constants/index.ts`）
- Produces（Task 4 依赖）: `useBark(): { register: () => Promise<void>, connect: () => void, disconnect: () => void }`
  - `register()` 失败时 throw（Error.message 给 UI 展示），成功时把 `deviceKey`/`streamToken` 写入 store
  - `connect()` 幂等：先断开旧连接；未启用或未注册时只归位状态不连接
  - 连接状态通过 `chatStore.barkStatus` 对外暴露（跨窗口同步到设置页）

- [ ] **Step 1: 安装并注册 plugin-http**

```bash
pnpm add @tauri-apps/plugin-http
cargo add tauri-plugin-http --manifest-path src-tauri/Cargo.toml
```

`src-tauri/src/lib.rs`：在现有 `.plugin(tauri_plugin_pinia::init())` 一行之后插入：

```rust
        .plugin(tauri_plugin_http::init())
```

`src-tauri/capabilities/default.json` 的 `permissions` 数组追加（bark 服务器地址由用户配置，scope 放开 http/https）：

```json
{
  "identifier": "http:default",
  "allow": [
    { "url": "http://**" },
    { "url": "https://**" }
  ]
}
```

- [ ] **Step 2: 编译确认插件接入无误**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过，无错误。

- [ ] **Step 3: 实现 `src/composables/useBark.ts`**

注意首行 import：**必须**用 plugin-http 的 fetch（fork 服务端无 CORS 头，webview 原生 fetch 会被拦截）。

```ts
import { emit } from '@tauri-apps/api/event'
import { fetch } from '@tauri-apps/plugin-http'

import type { BarkCryptoConfig, SSEEvent } from '@/utils/bark'

import { LISTEN_KEY } from '@/constants'
import { useChatStore } from '@/stores/chat'
import { createSSEParser, resolveBarkText } from '@/utils/bark'

// 与 bark-macOS 客户端一致的设备身份；服务端内置的 macos_sse provider 按这些值匹配
const REGISTER_IDENTITY = {
  device_token: null,
  platform: 'macos',
  app_id: 'me.fin.bark.macos',
  provider_id: 'macos_sse',
  topic: 'me.fin.bark.macos',
}

const BACKOFF_SECONDS = [1, 2, 5, 10, 20, 30]

// 模块级单例：SSE 连接只存在于 chat 窗口（应用生命周期内常驻）；设置页仅调用 register()
let controller: AbortController | undefined
let attempt = 0

export function useBark() {
  const chatStore = useChatStore()

  function base() {
    return chatStore.bark.serverUrl.replace(/\/+$/, '')
  }

  /** 注册设备：首次 device_key 留空由服务端签发；重复注册同 key 复用 stream_token */
  async function register() {
    const res = await fetch(`${base()}/register`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ...REGISTER_IDENTITY,
        device_key: chatStore.bark.deviceKey || undefined,
      }),
    })

    const json = await res.json().catch(() => ({}))

    if (!res.ok) {
      // 409 = 设备身份冲突；清空本地 key 重新注册可恢复
      throw new Error(json.message ?? `HTTP ${res.status}`)
    }

    const data = json.data ?? json
    const key = data.device_key ?? data.key

    if (!key || !data.stream_token) {
      throw new Error('response missing device_key / stream_token')
    }

    chatStore.bark.deviceKey = key
    chatStore.bark.streamToken = data.stream_token
  }

  function disconnect() {
    controller?.abort()
    controller = undefined
    chatStore.barkStatus = 'idle'
  }

  function connect() {
    disconnect()

    const { enabled, deviceKey, streamToken } = chatStore.bark
    if (!enabled || !deviceKey || !streamToken) return

    attempt = 0
    controller = new AbortController()
    void runLoop(controller)
  }

  async function runLoop(ctrl: AbortController) {
    while (!ctrl.signal.aborted) {
      try {
        chatStore.barkStatus = attempt > 0 ? 'reconnecting' : 'connecting'

        // 不发 Last-Event-ID：服务端不回放，只收实时消息（设计决策：不补收离线消息）
        const url = `${base()}/events/${chatStore.bark.deviceKey}?stream_token=${encodeURIComponent(chatStore.bark.streamToken)}`
        const res = await fetch(url, {
          headers: { Accept: 'text/event-stream' },
          signal: ctrl.signal,
        })

        if (res.status === 401 || res.status === 403) {
          // token 失效：停止重连，避免无效循环打服务器；用户重新注册后 watch 会重建连接
          chatStore.barkStatus = 'unauthorized'
          return
        }

        if (!res.ok || !res.body) {
          throw new Error(`http ${res.status}`)
        }

        chatStore.barkStatus = 'connected'
        attempt = 0

        const feed = createSSEParser()
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()

        while (true) {
          const { done, value } = await reader.read()
          if (done) break

          for (const event of feed(value)) {
            await handleEvent(event)
          }
        }

        // 服务端关闭连接（含同 key 被新连接踢下线）→ 走重连
        throw new Error('stream closed')
      } catch {
        if (ctrl.signal.aborted) return

        chatStore.barkStatus = 'reconnecting'

        // 指数退避 + 随机抖动，避免服务端重启时惊群
        const seconds = BACKOFF_SECONDS[Math.min(attempt, BACKOFF_SECONDS.length - 1)]
        attempt += 1
        await new Promise(resolve => setTimeout(resolve, seconds * 1000 + Math.random() * 800))
      }
    }
  }

  async function handleEvent(event: SSEEvent) {
    if (event.event !== 'notification') return

    try {
      const notification = JSON.parse(event.data)
      // 解密配置每条消息实时读取，改密钥无需重连
      const cryptoConfig: BarkCryptoConfig | undefined = chatStore.bark.cryptoKey
        ? { mode: chatStore.bark.cryptoMode, key: chatStore.bark.cryptoKey, iv: chatStore.bark.cryptoIv }
        : undefined
      const text = await resolveBarkText(notification, cryptoConfig)

      if (!text) return

      await emit(LISTEN_KEY.SHOW_CHAT, { text, source: 'bark' })
    } catch (error) {
      // 解密失败 / 非法 JSON：丢弃该条，不影响连接
      console.warn('[bark] drop message:', error)
    }
  }

  return { register, connect, disconnect }
}
```

- [ ] **Step 4: lint 并提交**

```bash
pnpm lint
git add package.json pnpm-lock.yaml src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/capabilities/default.json src/composables/useBark.ts
git commit -m "feat(chat): add useBark composable with register and SSE connect loop via plugin-http"
```

---

### Task 4: chat 窗口接线 + 设置页 Bark 卡片

**Files:**
- Modify: `src/pages/chat/index.vue`（payload 类型 + 挂载连接 + 配置 watch）
- Create: `src/pages/preference/components/chat/components/bark-settings/index.vue`
- Modify: `src/pages/preference/components/chat/index.vue`（引入卡片）

**Interfaces:**
- Consumes: Task 3 的 `useBark()`；Task 2 的 store 字段与 locale key
- Produces: 完整可用的功能面（消息展示自动复用现有 `showChat` 单点路径，无新分支）

- [ ] **Step 1: `src/pages/chat/index.vue` 接线**

`ShowChatPayload` 的 source 类型（第 22 行）改为：

```ts
  source?: 'http' | 'bark'
```

script 顶部 import 区新增：

```ts
import { useBark } from '@/composables/useBark'
```

`const chatHistoryStore = …` 之后新增：

```ts
const { connect: connectBark, disconnect: disconnectBark } = useBark()
```

现有 `onMounted` 末尾追加一行：

```ts
  connectBark()
```

现有 `onUnmounted` 内追加一行：

```ts
  disconnectBark()
```

文件末尾（现有 fontSize watch 之后）新增配置 watch——只 watch 连接参数，解密配置逐条实时读不需重连；500ms 防抖避免输入服务器地址时逐键重连：

```ts
// bark 连接参数变更后重建连接；防抖吸收设置页逐键输入
let barkReconnectTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => [chatStore.bark.enabled, chatStore.bark.serverUrl, chatStore.bark.deviceKey, chatStore.bark.streamToken],
  () => {
    clearTimeout(barkReconnectTimer)
    barkReconnectTimer = setTimeout(connectBark, 500)
  },
)
```

- [ ] **Step 2: 创建 `src/pages/preference/components/chat/components/bark-settings/index.vue`**

```vue
<script setup lang="ts">
import { Button, Flex, Input, InputPassword, Select, Switch } from 'antdv-next'
import { ref } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { useBark } from '@/composables/useBark'
import { useChatStore } from '@/stores/chat'

const chatStore = useChatStore()
const { register } = useBark()
const registering = ref(false)
const registerError = ref('')

const modeOptions = [
  { value: 'cbc', label: 'AES-CBC' },
  { value: 'gcm', label: 'AES-GCM' },
]

async function handleRegister() {
  registering.value = true
  registerError.value = ''

  try {
    await register()
  } catch (error) {
    registerError.value = (error as Error).message ?? String(error)
  } finally {
    registering.value = false
  }
}
</script>

<template>
  <ProList :title="$t('pages.preference.chat.labels.bark')">
    <ProListItem
      :description="$t('pages.preference.chat.hints.bark')"
      :title="$t('pages.preference.chat.labels.barkEnabled')"
    >
      <Switch v-model:checked="chatStore.bark.enabled" />
    </ProListItem>

    <template v-if="chatStore.bark.enabled">
      <ProListItem :title="$t('pages.preference.chat.labels.barkServerUrl')">
        <Input
          v-model:value="chatStore.bark.serverUrl"
          class="w-64"
          placeholder="https://bark.example.com"
        />
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.chat.hints.barkRegister')"
        :title="$t('pages.preference.chat.labels.barkRegister')"
      >
        <Flex
          align="center"
          :gap="8"
        >
          <span
            v-if="registerError"
            class="text-3 color-red"
          >{{ registerError }}</span>

          <Button
            :disabled="!chatStore.bark.serverUrl"
            :loading="registering"
            @click="handleRegister"
          >
            {{ $t('pages.preference.chat.labels.barkRegister') }}
          </Button>
        </Flex>
      </ProListItem>

      <ProListItem
        v-if="chatStore.bark.deviceKey"
        :title="$t('pages.preference.chat.labels.barkDeviceKey')"
      >
        <code class="select-all break-all text-3">{{ chatStore.bark.deviceKey }}</code>
      </ProListItem>

      <ProListItem :title="$t('pages.preference.chat.labels.barkStatus')">
        {{ $t(`pages.preference.chat.barkStatus.${chatStore.barkStatus}`) }}
      </ProListItem>

      <ProListItem
        :description="$t('pages.preference.chat.hints.barkCrypto')"
        :title="$t('pages.preference.chat.labels.barkCryptoKey')"
      >
        <Flex :gap="8">
          <Select
            v-model:value="chatStore.bark.cryptoMode"
            class="w-30"
            :options="modeOptions"
          />

          <InputPassword
            v-model:value="chatStore.bark.cryptoKey"
            class="w-40"
          />

          <Input
            v-model:value="chatStore.bark.cryptoIv"
            class="w-36"
            :placeholder="$t('pages.preference.chat.labels.barkCryptoIv')"
          />
        </Flex>
      </ProListItem>
    </template>
  </ProList>
</template>
```

- [ ] **Step 3: 设置页引入卡片**

`src/pages/preference/components/chat/index.vue`：import 区加

```ts
import BarkSettings from './components/bark-settings/index.vue'
```

模板中 HTTP 的 `</ProList>`（调试 ProList 之前）后插入：

```html
  <BarkSettings />
```

- [ ] **Step 4: 启动应用冒烟**

Run: `pnpm tauri dev`
Expected: 设置页 Chat 标签出现"Bark 推送"卡片；开关默认关；打开后显示服务器地址/注册/状态（未启用→连接中/重连中，因为还没注册成功，无崩溃报错即可）。

- [ ] **Step 5: lint 并提交**

```bash
pnpm lint
git add src/pages/chat/index.vue src/pages/preference/components/chat/
git commit -m "feat(chat): wire bark SSE client into chat window and preference page"
```

---

### Task 5: 端到端手动验证 + 手动测试脚本

**Files:**
- Create: `scripts/bark-manual.sh`（参照 `scripts/bubble-http.sh` 先例）

**Interfaces:**
- Consumes: 用户已部署的 htnanako/bark-server 地址；Task 4 完成的 UI
- Produces: 可重复执行的手动验证脚本与验证结论

- [ ] **Step 1: 创建 `scripts/bark-manual.sh`**

```bash
#!/usr/bin/env bash
# BongoCat Bark 客户端手动验证：向 htnanako/bark-server 推送明文/加密消息
#
# 用法:
#   ./scripts/bark-manual.sh <server> <device_key>                 # 明文
#   ./scripts/bark-manual.sh <server> <device_key> <key16> <iv16>  # 追加 AES-128-CBC 加密消息
#
# device_key 在 BongoCat 设置页 Chat → Bark 推送里注册后显示。
set -euo pipefail

SERVER=${1:?usage: bark-manual.sh <server> <device_key> [aes128key(16char)] [iv(16char)]}
DEVICE_KEY=${2:?missing device_key}

echo "--- 明文消息 ---"
curl -fsS "$SERVER/$DEVICE_KEY/测试标题/来自 bark-manual 的正文" && echo " <- ok（气泡应显示：测试标题 换行 正文）"

if [[ $# -ge 4 ]]; then
  KEY=$3
  IV=$4
  [[ ${#KEY} -eq 16 && ${#IV} -eq 16 ]] || { echo "key/iv 必须都是 16 字符（AES-128-CBC）"; exit 1; }

  echo "--- 加密消息（AES-128-CBC）---"
  PLAIN='{"title":"加密标题","body":"加密正文"}'
  CIPHER=$(printf %s "$PLAIN" | openssl enc -aes-128-cbc -K "$(printf %s "$KEY" | xxd -p)" -iv "$(printf %s "$IV" | xxd -p)" | base64)
  curl -fsS -G "$SERVER/$DEVICE_KEY" --data-urlencode "ciphertext=$CIPHER" --data-urlencode "iv=$IV" && echo " <- ok（需在设置页配置相同的 CBC 密钥/IV）"
fi
```

之后 `chmod +x scripts/bark-manual.sh`。

- [ ] **Step 2: 端到端验证（对用户已部署的服务器）**

依次执行并记录结果：

1. `pnpm tauri dev` 启动应用 → 设置页填服务器地址 → 点"注册设备" → 出现设备 Key，状态变"已连接"
2. `./scripts/bark-manual.sh <server> <device_key>` → 气泡弹出"测试标题\n来自 bark-manual 的正文"，历史 Modal 中该条 source 为 Bark
3. 设置页配置 CBC 密钥（16 字符）+ IV（16 字符）→ `./scripts/bark-manual.sh <server> <device_key> <key> <iv>` → 弹出解密后的"加密标题\n加密正文"
4. 断网/杀掉与服务器的连接（或重启服务器）→ 状态变"重连中"，恢复后回"已连接"；断线期间推的消息**不**弹出（无回放）
5. 关闭"启用气泡"总开关 → 推消息不弹泡但历史里记为"未展示"；关闭"启用 Bark"→ 状态回"未启用"

Expected: 全部符合。任何一步不符 → 用 superpowers:systematic-debugging 排查后修复再验。

- [ ] **Step 3: 提交**

```bash
git add scripts/bark-manual.sh
git commit -m "chore(chat): add bark manual test script"
```
