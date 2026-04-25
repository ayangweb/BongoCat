<script setup lang="ts">
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Switch } from 'antdv-next'
import { watch } from 'vue'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { useGeneralStore } from '@/stores/general'
import { isMac, isWindows } from '@/utils/platform'

import Language from './components/language/index.vue'
import MacosPermissions from './components/macos-permissions/index.vue'
import ThemeMode from './components/theme-mode/index.vue'
import WindowsPermissions from './components/windows-permissions/index.vue'

const generalStore = useGeneralStore()

watch(() => generalStore.app.autostart, async (value) => {
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
  <MacosPermissions v-if="isMac" />

  <WindowsPermissions v-if="isWindows" />

  <ProList :title="$t('pages.preference.general.labels.appSettings')">
    <ProListItem :title="$t('pages.preference.general.labels.launchOnStartup')">
      <Switch v-model:checked="generalStore.app.autostart" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.general.hints.showTaskbarIcon')"
      :title="$t('pages.preference.general.labels.showTaskbarIcon')"
    >
      <Switch v-model:checked="generalStore.app.taskbarVisible" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.general.hints.showTrayIcon')"
      :title="$t('pages.preference.general.labels.showTrayIcon')"
    >
      <Switch v-model:checked="generalStore.app.trayVisible" />
    </ProListItem>
  </ProList>

  <ProList :title="$t('pages.preference.general.labels.appearanceSettings')">
    <ThemeMode />

    <Language />
  </ProList>

  <ProList :title="$t('pages.preference.general.labels.updateSettings')">
    <ProListItem :title="$t('pages.preference.general.labels.autoCheckUpdate')">
      <Switch v-model:checked="generalStore.update.autoCheck" />
    </ProListItem>
  </ProList>
</template>
