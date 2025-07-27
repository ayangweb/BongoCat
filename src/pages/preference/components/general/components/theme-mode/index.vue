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
  <!-- 主题模式 Theme mode -->
  <ProListItem title="Chế độ chủ đề">
    <Select v-model:value="generalStore.theme">
      <SelectOption value="auto">
        <!--   Follow the system     跟随系统 -->
        Theo hệ thống
      </SelectOption>
      <SelectOption value="light">
        <!--   Bright color mode     亮色模式 -->
        Chế độ màu sáng
      </SelectOption>
      <SelectOption value="dark">
        <!--    Dark Mode    暗色模式 -->
        Chế độ tối
      </SelectOption>
    </Select>
  </ProListItem>
</template>
