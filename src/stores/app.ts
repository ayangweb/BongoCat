import type { WindowState } from '@/composables/useWindowState'

import { getName, getVersion } from '@tauri-apps/api/app'
import { defineStore } from 'pinia'
import { onMounted, reactive, ref } from 'vue'

export const useAppStore = defineStore('app', () => {
  const name = ref('')
  const version = ref('')
  const windowState = reactive<WindowState>({})
  const visibleCat = ref(true)
  const visiblePreference = ref(false)

  onMounted(async () => {
    name.value = await getName()
    version.value = await getVersion()
    visibleCat.value = true
    visiblePreference.value = false
  })

  return {
    name,
    version,
    windowState,
    visibleCat,
    visiblePreference,
  }
})
