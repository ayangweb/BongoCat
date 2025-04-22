import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useCatStore = defineStore('cat', () => {
  const mode = ref<'standard' | 'keyboard'>('standard')
  const penetrable = ref(false)
  const opacity = ref(100)
  const mirrorMode = ref(false)

  return {
    mode,
    penetrable,
    opacity,
    mirrorMode,
  }
})
