import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useGeneralStore = defineStore('general', () => {
  const autoCheckUpdate = ref(false)
  const autoStart = ref(false)

  return {
    autoCheckUpdate,
    autoStart,
  }
})
