import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useGeneralStore = defineStore('general', () => {
  const autoCheckUpdate = ref(false)
  const autostart = ref(false)
  const skipTaskbar = ref(true)

  return {
    autoCheckUpdate,
    autostart,
    skipTaskbar,
  }
})
