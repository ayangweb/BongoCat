import { defineStore } from 'pinia'
import { reactive, watch } from 'vue'

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

  return {
    ai,
  }
})
