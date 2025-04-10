import { getName, getVersion } from '@tauri-apps/api/app'
import { defineStore } from 'pinia'
import { onMounted, ref } from 'vue'

export const useAppStore = defineStore('app', () => {
  const name = ref('')
  const version = ref('')

  onMounted(async () => {
    name.value = await getName()
    version.value = await getVersion()
  })

  return {
    name,
    version,
  }
})
