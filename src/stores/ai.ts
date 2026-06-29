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
