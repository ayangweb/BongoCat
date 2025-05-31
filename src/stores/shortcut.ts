import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useShortcutStore = defineStore('shortcut', () => {
  const disabled = ref(false)
  const hosKeys = ref({ visibleCat: '' })

  return {
    disabled,
    hosKeys,
  }
})
