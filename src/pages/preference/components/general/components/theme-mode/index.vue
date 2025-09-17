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
    if (generalStore.appearance.theme !== 'auto') return

    generalStore.appearance.isDark = payload === 'dark'
  })
})

watch(() => generalStore.appearance.theme, async (value) => {
  let nextTheme = value === 'auto' ? null : value

  await appWindow.setTheme(nextTheme)

  nextTheme = nextTheme ?? (await appWindow.theme())

  generalStore.appearance.isDark = nextTheme === 'dark'
}, { immediate: true })

watch(() => generalStore.appearance.isDark, (value) => {
  if (value) {
    document.documentElement.classList.add('dark')
  } else {
    document.documentElement.classList.remove('dark')
  }
}, { immediate: true })
</script>

<template>
  <ProListItem title="主题模式">
    <Select v-model:value="generalStore.appearance.theme">
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
