<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Select, SelectOption } from 'ant-design-vue'
import { onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const { t } = useI18n()
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
  <ProListItem :title="t('general.theme.title')">
    <Select
      v-model:value="generalStore.theme"
      style="width: 150px"
    >
      <SelectOption value="auto">
        {{ t('general.theme.auto') }}
      </SelectOption>
      <SelectOption value="light">
        {{ t('general.theme.light') }}
      </SelectOption>
      <SelectOption value="dark">
        {{ t('general.theme.dark') }}
      </SelectOption>
    </Select>
  </ProListItem>
</template>
