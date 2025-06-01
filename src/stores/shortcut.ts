import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface HotKeys {
  visibleCat: string
}

export const useShortcutStore = defineStore('shortcut', () => {
  const enabled = ref(false)
  const hotKeys = ref<HotKeys>({ visibleCat: '' })

  return {
    enabled,
    hotKeys,
  }
})
