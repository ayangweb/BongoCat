import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useCatStore = defineStore('cat', () => {
  const visible = ref(true)
  const mirrorMode = ref(false)
  const singleMode = ref(false)
  const penetrable = ref(false)
  const scale = ref(100)
  const opacity = ref(100)

  return {
    visible,
    mirrorMode,
    singleMode,
    penetrable,
    scale,
    opacity,
  }
})
