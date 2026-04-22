<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Select } from 'antdv-next'
import { computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type { Theme } from '@/stores/general'

import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const generalStore = useGeneralStore()
const appWindow = getCurrentWebviewWindow()
const { t } = useI18n()

const options = computed<Array<{ label: string, value: Theme }>>(() => [
  { label: t('pages.preference.general.options.auto'), value: 'auto' },
  { label: t('pages.preference.general.options.lightMode'), value: 'light' },
  { label: t('pages.preference.general.options.darkMode'), value: 'dark' },
])

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
  <ProListItem :title="$t('pages.preference.general.labels.themeMode')">
    <Select
      v-model:value="generalStore.appearance.theme"
      :options="options"
    />
  </ProListItem>
</template>
