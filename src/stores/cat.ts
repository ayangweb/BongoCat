import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useCatStore = defineStore('cat', () => {
  const visible = ref(false)
  const mirrorMode = ref(false)
  const singleMode = ref(false)
  const mouseMirror = ref(false)
  const penetrable = ref(false)
  const alwaysOnTop = ref(true)
  const scale = ref(100)
  const opacity = ref(100)
  const autoAdsorb = ref(false)
  const adsorbRange = ref(5)

  return {
    visible,
    mirrorMode,
    singleMode,
    mouseMirror,
    penetrable,
    alwaysOnTop,
    scale,
    opacity,
    autoAdsorb,
    adsorbRange,
  }
})
