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
