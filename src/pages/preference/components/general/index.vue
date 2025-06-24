<script setup lang="ts">
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'
import { Switch } from 'ant-design-vue'
import { watch } from 'vue'

import MacosPermissions from './components/macos-permissions/index.vue'

import ProList from '@/components/pro-list/index.vue'
import ProListItem from '@/components/pro-list-item/index.vue'
import { useGeneralStore } from '@/stores/general'

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

  <ProList title="Application Settings">
    <ProListItem title="Startup on Boot">
      <Switch v-model:checked="generalStore.autostart" />
    </ProListItem>
  </ProList>

  <ProList title="Update Settings">
    <ProListItem title="Automatically Check for Updates">
      <Switch v-model:checked="generalStore.autoCheckUpdate" />
    </ProListItem>
  </ProList>
</template>
