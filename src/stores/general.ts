import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useGeneralStore = defineStore('cat', () => {
  const autoCheckUpdate = ref(false)

  return {
    autoCheckUpdate,
  }
})
