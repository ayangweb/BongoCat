import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useShortcutStore = defineStore('shortcut', () => {
  const enabled = ref(false)
  const hosKeys = ref({ visibleCat: '' })

  return {
    enabled,
    hosKeys,
  }
})
