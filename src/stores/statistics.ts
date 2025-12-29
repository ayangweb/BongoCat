import { defineStore } from 'pinia'
import { reactive } from 'vue'

export interface StatisticsStore {
  keyboard: {
    total: number
    keys: Record<string, number>
  }
  mouse: {
    total: number
    left: number
    right: number
  }
  settings: {
    enabled: boolean
    mouseClickEnabled: boolean
  }
}

export const useStatisticsStore = defineStore('statistics', () => {
  const keyboard = reactive<StatisticsStore['keyboard']>({
    total: 0,
    keys: {},
  })

  const mouse = reactive<StatisticsStore['mouse']>({
    total: 0,
    left: 0,
    right: 0,
  })

  const settings = reactive<StatisticsStore['settings']>({
    enabled: true,
    mouseClickEnabled: true,
  })

  const recordKeyPress = (key: string) => {
    if (!settings.enabled) return
    keyboard.total++
    keyboard.keys[key] = (keyboard.keys[key] || 0) + 1
  }

  const recordMouseClick = (button: string) => {
    if (!settings.enabled || !settings.mouseClickEnabled) return
    mouse.total++
    if (button === 'Left') mouse.left++
    else if (button === 'Right') mouse.right++
  }

  const reset = () => {
    keyboard.total = 0
    keyboard.keys = {}
    mouse.total = 0
    mouse.left = 0
    mouse.right = 0
  }

  return {
    keyboard,
    mouse,
    settings,
    // actions
    recordKeyPress,
    recordMouseClick,
    reset,
  }
}, {
  tauri: {
    autoStart: true,
    saveOnChange: true,
  },
})
