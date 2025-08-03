<script setup lang="ts">
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Switch } from 'ant-design-vue'
import { watch } from 'vue'
import { useI18n } from 'vue-i18n'

import Languages from './components/languages/index.vue'
import MacosPermissions from './components/macos-permissions/index.vue'
import ThemeMode from './components/theme-mode/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

const { t } = useI18n()
const generalStore = useGeneralStore()

watch(() => generalStore.autostart, async (value) => {
  const enabled = await isEnabled()

  if (value && !enabled) {
    return enable()
  }

  if (!value && enabled) {
    disable()
  }
}, { immediate: true })
</script>

<template>
  <MacosPermissions />

  <ProList :title="t('general.settings.app')">
    <ProListItem :title="t('general.settings.launchStartup')">
      <Switch v-model:checked="generalStore.autostart" />
    </ProListItem>

    <ProListItem
      :description="t('general.settings.taskbar.description')"
      :title="t('general.settings.taskbar.title')"
    >
      <Switch v-model:checked="generalStore.taskbarVisibility" />
    </ProListItem>
  </ProList>

  <ProList :title="t('general.settings.appearance')">
    <ThemeMode />
    <Languages />
  </ProList>

  <ProList :title="t('general.settings.update')">
    <ProListItem :title="t('general.settings.autoUpdate')">
      <Switch v-model:checked="generalStore.autoCheckUpdate" />
    </ProListItem>
  </ProList>
</template>
