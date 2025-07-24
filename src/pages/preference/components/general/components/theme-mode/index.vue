<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Select, SelectOption } from 'ant-design-vue'
import { onMounted, watch } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const generalStore = useGeneralStore()
const appWindow = getCurrentWebviewWindow()

onMounted(() => {
  appWindow.onThemeChanged(async ({ payload }) => {
    if (generalStore.theme !== 'auto') return

    generalStore.isDark = payload === 'dark'
  })
})

watch(() => generalStore.theme, async (value) => {
  let nextTheme = value === 'auto' ? null : value

  await appWindow.setTheme(nextTheme)

  nextTheme = nextTheme ?? (await appWindow.theme())

  generalStore.isDark = nextTheme === 'dark'
}, { immediate: true })

watch(() => generalStore.isDark, (value) => {
  if (value) {
    document.documentElement.classList.add('dark')
  } else {
    document.documentElement.classList.remove('dark')
  }
}, { immediate: true })
</script>

<template>
  <ProListItem title="主题模式">
    <Select v-model:value="generalStore.theme">
      <SelectOption value="auto">
        跟随系统
      </SelectOption>
      <SelectOption value="light">
        亮色模式
      </SelectOption>
      <SelectOption value="dark">
        暗色模式
      </SelectOption>
    </Select>
  </ProListItem>
</template>
