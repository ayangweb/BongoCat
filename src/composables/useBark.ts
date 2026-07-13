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
          // token 失效：停止重连，避免无效循环打服务器；用户重新注册后配置 watch 会重建连接
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
