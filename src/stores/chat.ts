import { defineStore } from 'pinia'
import { reactive, ref, watch } from 'vue'

// ColorPicker 早期把 AggregationColor 对象写进了 store，经 tauri-store 序列化后变成
// { metaColor: { r,g,b } } 残留，回灌给 ColorPicker 会触发 fast-color "unsupported input"。
// 把任何非字符串颜色还原成 hex；保留用户原色，无法识别时回退默认值。
function toHex(value: unknown, fallback: string): string {
  if (typeof value === 'string') return value
  const meta = (value as any)?.metaColor
  if (meta && typeof meta.r === 'number') {
    const h = (n: number) => Math.round(n).toString(16).padStart(2, '0')
    return `#${h(meta.r)}${h(meta.g)}${h(meta.b)}`
  }
  return fallback
}

export type BarkStatus = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'unauthorized'

export interface ChatStore {
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

export const useChatStore = defineStore('ai', () => {
  const ai = reactive<ChatStore['ai']>({
    enabled: true,
    duration: 3,
    textColor: '#333',
    fontSize: 14,
    bgColor: '#ffffff',
    bgOpacity: 90,
    debug: false,
    httpEnabled: false,
    httpPort: 7800,
    httpToken: '',
  })

  // ponytail: 仅为修复历史脏数据；value-format="hex" 之后正常使用不会再写入对象
  watch(() => [ai.textColor, ai.bgColor], () => {
    ai.textColor = toHex(ai.textColor, '#333')
    ai.bgColor = toHex(ai.bgColor, '#ffffff')
  }, { immediate: true })

  const bark = reactive<ChatStore['bark']>({
    enabled: false,
    serverUrl: '',
    deviceKey: '',
    streamToken: '',
    cryptoMode: 'cbc',
    cryptoKey: '',
    cryptoIv: '',
  })

  // 连接状态由 chat 窗口写入、设置页只读展示；持久化的旧值无意义，chat 窗口挂载时会重置。
  // 独立于 bark 对象：它被高频写入，放进 bark 会触发 chat 窗口的配置 watch 造成重连循环。
  const barkStatus = ref<BarkStatus>('idle')

  return {
    ai,
    bark,
    barkStatus,
  }
})
