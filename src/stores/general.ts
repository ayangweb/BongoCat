import type { Theme } from '@tauri-apps/api/window'

import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

interface Appearance {
  theme: 'auto' | Theme
  isDark: boolean
}

export const useGeneralStore = defineStore('general', () => {
  const autoCheckUpdate = ref(false)
  const autostart = ref(false)
  const taskbarVisibility = ref(false)
  const appearance = reactive<Appearance>({
    theme: 'auto',
    isDark: false,
  })

  return {
    autoCheckUpdate,
    autostart,
    taskbarVisibility,
    appearance,
  }
})
