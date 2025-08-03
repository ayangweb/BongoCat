import type { Theme } from '@tauri-apps/api/window'

import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useGeneralStore = defineStore('general', () => {
  const autoCheckUpdate = ref(false)
  const autostart = ref(false)
  const taskbarVisibility = ref(false)
  const theme = ref<'auto' | Theme>('auto')
  const isDark = ref(false)
  // Stores the selected language for i18n
  const locale = ref<string>('')

  return {
    autoCheckUpdate,
    autostart,
    taskbarVisibility,
    theme,
    isDark,
    locale,
  }
})
