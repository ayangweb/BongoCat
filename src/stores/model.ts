import type { ModelType } from '../types/model'

import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useModelStore = defineStore('model', () => {
  const mode = ref<ModelType>('STANDARD')

  return {
    mode,
  }
}, {
  tauri: {
    saveOnChange: true,
  },
})
