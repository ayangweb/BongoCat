import { defineStore } from 'pinia'
import { ref } from 'vue'

export type CatMode = 'standard' | 'keyboard'

export const useCatStore = defineStore('cat', () => {
  const mode = ref<CatMode>('standard')
  const visible = ref(true)
  const penetrable = ref(false)
  const opacity = ref(100)
  const mirrorMode = ref(false)

  const size = ref(100) // 新增：模型大小，初始为 100%

  return {
    mode,
    visible,
    penetrable,
    opacity,
    mirrorMode,
    size,
  }
})
